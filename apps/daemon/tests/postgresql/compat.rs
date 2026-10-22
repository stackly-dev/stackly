//! The compatibility evidence of the PostgreSQL package (spec 0019 AC-14,
//! spec 0018 *Compatibility record*): one test per major runs the package
//! against its artifact from init to a refused major, and writes
//! `compatibility.json` with the checks that passed. The evidence comes
//! from the final `provider.yaml`, so a change of the package needs a new
//! run.

use serde_json::json;
use stackly_protocol::instance::{InstanceHealthState, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, PROVIDER, Pg, Site, create_instance, output_of};
use crate::phases::{readopt, refuse_other_major, reload_log_statement};

/// The checks a run proved, in the order they passed. A name is added only
/// after its assertions passed.
#[derive(Debug, Default)]
struct Checks(Vec<&'static str>);

impl Checks {
    fn passed(&mut self, name: &'static str) {
        assert!(!self.0.contains(&name), "check {name} passed twice");
        self.0.push(name);
    }

    /// Writes `compatibility.json` into the artifact folder of `evidence`:
    /// the final package revision of the run and the checks in alphabetical
    /// order.
    fn write(&self, evidence: &Evidence, site: &Site) {
        let mut checks = self.0.clone();
        checks.sort_unstable();
        let file = json!({
            "schema": "stackly.compatibility-evidence/v1",
            "providerRevision": {
                "id": PROVIDER,
                "revision": site.revision,
                "manifestSha256": site.manifest_sha256,
            },
            "checks": checks,
            "knownLimitations": [],
        });
        let path = evidence.artifact_dir().join("compatibility.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&file).expect("the file is JSON"),
        )
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    }
}

/// Whether the process `pid` exists.
fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .expect("kill runs")
        .success()
}

/// Runs the package on `pg` from init to stop, and refuses its data on the
/// runtime of `other`.
async fn runs_the_package_and_records_what_it_proved(pg: Pg, other: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let Some(other_runtime) = site.install_other(&ev, other) else {
        return;
    };
    let mut checks = Checks::default();

    // init, start, readiness
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "compat").await;
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&site.instance_paths(made.id).manifest)
            .expect("the manifest is written"),
    )
    .expect("the manifest is JSON");
    assert_eq!(manifest["data"]["initializedLine"], pg.major);
    let created = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));
    checks.passed("init");

    let running = api.start_ready(made.id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    let process = running.process.clone().expect("a running process");
    assert!(!process.adopted, "{running:?}");
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    assert_eq!(version, pg.version);
    site.subject(&ev, &version);
    checks.passed("start");

    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    api.wait_instance(made.id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await;
    checks.passed("readiness");

    // reload, while no daemon runs and the postmaster keeps running
    let pid = process.pid;
    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "none");
    let started = psql.scalar(&made.password, "SELECT pg_postmaster_start_time()");
    drop(api);
    daemon.shutdown().await;
    reload_log_statement(&site, &made, pid, &started).await;
    let (daemon, api) = readopt(&site, &made, pid).await;
    checks.passed("reload");

    // adoption: the postmaster outlives a daemon killed with SIGKILL
    drop(api);
    daemon.shutdown().await;
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    api.wait_instance(made.id, "adopted by the first binary", |summary| {
        summary
            .process
            .as_ref()
            .is_some_and(|process| process.adopted)
    })
    .await;
    drop(api);
    first.kill();
    assert!(alive(pid), "the postmaster outlives the killed daemon");
    let second = site.binary(&[]);
    let api = site.binary_api(&second).await;
    let adopted = api
        .wait_instance(made.id, "adopted and ready", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.adopted)
                && summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    let adopted_process = adopted.process.expect("the process is adopted");
    assert_eq!(adopted_process.pid, pid);
    assert_eq!(adopted_process.started_at, process.started_at);
    assert_eq!(
        psql.scalar(&made.password, "SELECT pg_postmaster_start_time()"),
        started,
        "the postmaster was not restarted"
    );
    checks.passed("adoption");

    // stop
    let accepted = api
        .stop_when_free(made.id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());
    assert!(!alive(pid), "the postmaster {pid} is still there");
    let data = site.instance_paths(made.id).data;
    assert!(
        !output_of("/bin/ps", &["-axww", "-o", "command="]).contains(&data.display().to_string()),
        "a process still serves the data folder"
    );
    drop(api);
    second.stop();
    checks.passed("stop");

    // the data of this major on the runtime of the other
    refuse_other_major(&site, &made, pg, other, &other_runtime).await;
    checks.passed("major_mismatch_refusal");

    checks.write(&ev, &site);
    ev.pass_with(&format!(
        "PostgreSQL {version} passed {}",
        checks.0.join(", ")
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]
async fn postgresql_17_runs_the_package_and_records_what_it_proved() {
    let pg = PG17;
    let other = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009", "BR-025"],
        kind = Kind::Native,
        expected = &format!(
            "a PostgreSQL {} instance of the final package is created and initialized, started, ready, reloaded without a restart, adopted after a SIGKILL of the daemon, stopped, and its data is refused by the runtime of {}, and compatibility.json lists those seven checks",
            pg.major, other.major
        ),
    );
    runs_the_package_and_records_what_it_proved(pg, other, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]
async fn postgresql_18_runs_the_package_and_records_what_it_proved() {
    let pg = PG18;
    let other = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009", "BR-025"],
        kind = Kind::Native,
        expected = &format!(
            "a PostgreSQL {} instance of the final package is created and initialized, started, ready, reloaded without a restart, adopted after a SIGKILL of the daemon, stopped, and its data is refused by the runtime of {}, and compatibility.json lists those seven checks",
            pg.major, other.major
        ),
    );
    runs_the_package_and_records_what_it_proved(pg, other, ev).await;
}
