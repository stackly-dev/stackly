//! The compatibility evidence of the MySQL package (spec 0042 AC-16,
//! BR-025, spec 0018 *Compatibility record*): one test per artifact runs
//! the final package from init to stop through the `stacklyd` binary, with
//! two instances side by side, a restart, and adoption after a `SIGKILL` of
//! the daemon, then refuses its data on the runtime of the other line, and
//! writes `compatibility.json` with the checks that passed. The evidence
//! comes from the final `provider.yaml`, so a change of the package needs a
//! new run.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::{
    InstanceHealthState, InstanceLifecycle, InstanceReadinessState, InstanceSummary,
};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::harness::{
    MY84, MY97, Made, My, Mysql, PROVIDER, Site, create_instance, eventually, mysqld_serving,
    refused_start,
};
use crate::support::Api;

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

/// The PID of the main process of `summary`.
fn pid_of(summary: &InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
}

/// Writes `value` as the one row of `stackly_compat.marker`.
fn write_marker(mysql: &Mysql, made: &Made, value: &str) {
    let ran = mysql.admin(
        &made.password,
        &format!(
            "CREATE DATABASE IF NOT EXISTS stackly_compat; \
             CREATE TABLE IF NOT EXISTS stackly_compat.marker (value varchar(64) NOT NULL); \
             INSERT INTO stackly_compat.marker VALUES ('{value}')"
        ),
    );
    assert!(ran.ok, "the marker is written: {}", ran.stderr);
}

/// The rows of `stackly_compat.marker`, one per line.
fn read_marker(mysql: &Mysql, made: &Made) -> String {
    mysql.scalar(&made.password, "SELECT value FROM stackly_compat.marker")
}

/// Stops `id`, waits until its process is gone, and checks nothing failed.
async fn stop_gone(api: &Api, id: Uuid, pid: u32) {
    let accepted = api
        .stop_when_free(id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    eventually("mysqld to be gone", Duration::from_secs(60), || !alive(pid)).await;
    let stopped = api.get(id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none(), "{stopped:#?}");
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
}

async fn runs_the_package_and_records_what_it_proved(my: My, other: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let Some(other_runtime) = site.install_other(&ev, other) else {
        return;
    };
    let mut checks = Checks::default();

    // init
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    let a = create_instance(&site, &api, "compat-a").await;
    let created = api.get(a.id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true), "{created:#?}");
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(site.instance_paths(a.id).manifest)
            .expect("the manifest is written"),
    )
    .expect("the manifest is JSON");
    assert_eq!(manifest["data"]["initializedLine"], my.line);
    checks.passed("init");

    // start and readiness
    let running = api.start_ready(a.id).await;
    let process = running.process.clone().expect("a running process");
    assert!(!process.adopted, "{running:#?}");
    let mysql_a = site.mysql(a.id, a.port);
    let version = mysql_a.scalar(&a.password, "SELECT VERSION()");
    assert_eq!(version, my.version);
    site.subject(&ev, &version);
    checks.passed("start");
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    api.wait_instance(a.id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await;
    checks.passed("readiness");

    // isolation: two instances of one line, each with its own row
    let b = create_instance(&site, &api, "compat-b").await;
    let running_b = api.start_ready(b.id).await;
    let mysql_b = site.mysql(b.id, b.port);
    write_marker(&mysql_a, &a, "alpha");
    write_marker(&mysql_b, &b, "beta");
    assert_eq!(read_marker(&mysql_a, &a), "alpha");
    assert_eq!(read_marker(&mysql_b, &b), "beta");
    assert!(
        !mysql_a
            .run(
                crate::harness::Via::Tcp,
                "root",
                Some(&b.password),
                "SELECT 1"
            )
            .ok,
        "the password of b does not open a"
    );
    assert_ne!(
        site.instance_paths(a.id).data,
        site.instance_paths(b.id).data
    );
    checks.passed("isolation");

    // restart: a new mysqld on the same data
    let accepted = api
        .restart(a.id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let restarted = api
        .wait_instance(a.id, "ready after the restart", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|running| running.pid != process.pid)
        })
        .await;
    assert!(!alive(process.pid), "the old mysqld is gone");
    assert_eq!(read_marker(&mysql_a, &a), "alpha");
    assert_eq!(read_marker(&mysql_b, &b), "beta");
    checks.passed("restart");

    // adoption: both mysqld outlive a daemon killed with SIGKILL
    let kept_a = restarted.process.clone().expect("a running process");
    let kept_b = running_b.process.clone().expect("a running process");
    drop(api);
    first.kill();
    assert!(alive(kept_a.pid) && alive(kept_b.pid), "mysqld outlives it");
    let second = site.binary(&[]);
    let api = site.binary_api(&second).await;
    for kept in [(&a, &kept_a), (&b, &kept_b)] {
        let (made, process) = kept;
        let adopted = api
            .wait_instance(made.id, "adopted and ready", |summary| {
                summary.readiness.state == InstanceReadinessState::Ready
                    && summary
                        .process
                        .as_ref()
                        .is_some_and(|process| process.adopted)
            })
            .await;
        let adopted_process = adopted.process.clone().expect("the process is adopted");
        assert_eq!(adopted_process.pid, process.pid);
        assert_eq!(adopted_process.started_at, process.started_at);
        assert_eq!(adopted.lifecycle, InstanceLifecycle::Running);
    }
    assert_eq!(read_marker(&mysql_a, &a), "alpha");
    checks.passed("adoption");

    // stop
    for made in [&a, &b] {
        let summary = api.get(made.id).await.expect("the instance can be read");
        stop_gone(&api, made.id, pid_of(&summary)).await;
    }
    drop(api);
    second.stop();
    checks.passed("stop");

    // the data of this line on the runtime of the other
    let data = site.instance_paths(a.id).data;
    site.rebind_instance(a.id, &other_runtime);
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let error = refused_start(&api, a.id).await;
    assert_eq!(error.code(), "supervisor.data_incompatible", "{error:?}");
    assert_eq!(error.detail("outcome"), Some("incompatible"));
    assert_eq!(error.detail("data"), Some(my.line));
    assert_eq!(error.detail("runtime"), Some(other.line));
    assert!(mysqld_serving(&data).is_empty(), "a mysqld was spawned");
    drop(api);
    daemon.shutdown().await;
    checks.passed("line_mismatch_refusal");

    checks.write(&ev, &site);
    ev.pass_with(&format!("MySQL {version} passed {}", checks.0.join(", ")));
}

// covers: AC-16 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed MySQL artifacts in the runtime cache"]
async fn mysql_8_4_runs_the_package_and_records_what_it_proved() {
    let ev = evidence!(
        covers = ["REQ-055", "AC-081", "BR-009", "BR-025"],
        kind = Kind::Native,
        expected = "a MySQL 8.4 instance of the final package is created and initialized, started, ready and healthy, isolated from a second instance of its line, restarted with a new mysqld on the same data, adopted with a second one after a SIGKILL of stacklyd, stopped, and its data is refused by the runtime of 9.7, and compatibility.json lists those eight checks",
    );
    runs_the_package_and_records_what_it_proved(MY84, MY97, ev).await;
}

// covers: AC-16 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed MySQL artifacts in the runtime cache"]
async fn mysql_9_7_runs_the_package_and_records_what_it_proved() {
    let ev = evidence!(
        covers = ["REQ-055", "AC-081", "BR-009", "BR-025"],
        kind = Kind::Native,
        expected = "a MySQL 9.7 instance of the final package is created and initialized, started, ready and healthy, isolated from a second instance of its line, restarted with a new mysqld on the same data, adopted with a second one after a SIGKILL of stacklyd, stopped, and its data is refused by the runtime of 8.4, and compatibility.json lists those eight checks",
    );
    runs_the_package_and_records_what_it_proved(MY97, MY84, ev).await;
}
