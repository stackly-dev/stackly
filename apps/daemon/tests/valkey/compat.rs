//! The compatibility evidence of the Valkey package (spec 0040 AC-15, spec
//! 0018 *Compatibility record*): one test runs the final package on Valkey
//! 9.1.2 through the `stacklyd` binary, from init to stop with each
//! persistence mode, a crash of `valkey-server`, and adoption after a
//! `SIGKILL` of the daemon, and writes `compatibility.json` with the checks
//! that passed. The evidence comes from the final `provider.yaml`, so a
//! change of the package needs a new run.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::{
    InstanceHealthState, InstanceLifecycle, InstanceReadinessState, InstanceSummary,
};
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::harness::{
    Made, PROVIDER, Reply, Site, Valkey, alive, create_instance, eventually, files_under, pid_of,
    port_of, signal, stop_instance,
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
    /// the final package revision of the run, the checks in alphabetical
    /// order, and the one limitation the run cannot cover.
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
            "knownLimitations": ["power_loss_not_simulated"],
        });
        let path = evidence.artifact_dir().join("compatibility.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&file).expect("the file is JSON"),
        )
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    }
}

/// The value of `key`, or `None` when the server has none.
fn get(valkey: &mut Valkey, key: &str) -> Option<String> {
    match valkey.cmd(&["GET", key]) {
        Reply::Bulk(value) => value,
        other => panic!("GET {key}: {other:?}"),
    }
}

/// Sets `key` to `value` and expects `+OK`.
fn set(valkey: &mut Valkey, key: &str, value: &str) {
    assert_eq!(
        valkey.cmd(&["SET", key, value]),
        Reply::Simple("OK".to_owned()),
        "SET {key}"
    );
}

/// Starts `made` and signs in on its client port.
async fn start_signed_in(api: &Api, made: &Made) -> (InstanceSummary, Valkey) {
    let running = api.start_ready(made.id).await;
    let valkey = Valkey::signed_in(port_of(&running), &made.password);
    (running, valkey)
}

/// Sends `SIGKILL` to the `valkey-server` of `running` and waits until the
/// supervisor respawned it and the new process is ready.
async fn crash_and_respawn(api: &Api, made: &Made, running: &InstanceSummary) -> InstanceSummary {
    let pid = pid_of(running);
    signal(pid, "KILL");
    let respawned = api
        .wait_instance(made.id, "respawned and ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != pid)
        })
        .await;
    assert!(!alive(pid), "the killed valkey-server {pid} is still there");
    respawned
}

/// Stops `id` and asserts that its process is gone and nothing failed.
async fn assert_stopped_clean(api: &Api, id: Uuid, pid: u32) {
    stop_instance(api, id).await;
    eventually("the process to be gone", Duration::from_secs(10), || {
        !alive(pid)
    })
    .await;
    let stopped = api.get(id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none(), "{stopped:#?}");
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
    assert!(stopped.leftover_processes.is_empty(), "{stopped:#?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_compatibility() {
    let ev = evidence!(
        covers = ["REQ-038", "AC-038", "AC-077", "BR-025"],
        kind = Kind::Native,
        expected = "Valkey 9.1.2 instances of the final package in the modes rdb, aof, and volatile are created and initialized, started, ready and healthy, adopted after a SIGKILL of stacklyd, keep a key over a stop in rdb and aof and over a crash of valkey-server in aof and respawn after it, lose it in volatile, and stop with no process left, and compatibility.json lists those nine checks with the known limitation power_loss_not_simulated",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let mut checks = Checks::default();

    // init: one instance per persistence mode
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    let rdb = create_instance(&site, &api, "rdb", json!({ "persistence": "rdb" })).await;
    let aof = create_instance(&site, &api, "aof", json!({ "persistence": "aof" })).await;
    let volatile = create_instance(
        &site,
        &api,
        "volatile",
        json!({ "persistence": "volatile" }),
    )
    .await;
    for made in [&rdb, &aof, &volatile] {
        let created = api.get(made.id).await.expect("the instance can be read");
        assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
        assert_eq!(created.initialized, Some(true), "{created:#?}");
    }
    checks.passed("init");

    // start and readiness
    let (running, mut valkey) = start_signed_in(&api, &rdb).await;
    let process = running.process.clone().expect("a running process");
    assert!(!process.adopted, "{running:#?}");
    let version = valkey.info_server("valkey_version");
    assert_eq!(version, crate::harness::VERSION);
    site.subject(&ev, &version);
    checks.passed("start");
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    api.wait_instance(rdb.id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await;
    checks.passed("readiness");

    // adoption: valkey-server outlives a daemon killed with SIGKILL
    drop(api);
    first.kill();
    assert!(alive(process.pid), "the instance outlives the daemon");
    let second = site.binary(&[]);
    let api = site.binary_api(&second).await;
    let adopted = api
        .wait_instance(rdb.id, "adopted and ready", |summary| {
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
    assert_eq!(
        valkey.cmd(&["PING"]),
        Reply::Simple("PONG".to_owned()),
        "the same server answers on the same connection"
    );
    checks.passed("adoption");

    // rdb: a key survives a normal stop and start through dump.rdb
    set(&mut valkey, "before-stop", "one");
    drop(valkey);
    stop_instance(&api, rdb.id).await;
    assert_eq!(
        files_under(&site.instance_paths(rdb.id).data),
        ["dump.rdb"],
        "the data folder of rdb"
    );
    let (running, mut valkey) = start_signed_in(&api, &rdb).await;
    assert_eq!(get(&mut valkey, "before-stop").as_deref(), Some("one"));
    checks.passed("persistence_rdb");

    // a crash of valkey-server: the supervisor respawns it on its data
    drop(valkey);
    let respawned = crash_and_respawn(&api, &rdb, &running).await;
    let mut valkey = Valkey::signed_in(port_of(&respawned), &rdb.password);
    assert_eq!(
        get(&mut valkey, "before-stop").as_deref(),
        Some("one"),
        "the snapshot is read again after the respawn"
    );
    drop(valkey);
    checks.passed("crash_recovery");

    // aof: a key survives a crash and a normal stop
    let (running, mut valkey) = start_signed_in(&api, &aof).await;
    set(&mut valkey, "before-crash", "two");
    drop(valkey);
    // More than the one second of appendfsync everysec.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let respawned = crash_and_respawn(&api, &aof, &running).await;
    let mut valkey = Valkey::signed_in(port_of(&respawned), &aof.password);
    assert_eq!(get(&mut valkey, "before-crash").as_deref(), Some("two"));
    drop(valkey);
    stop_instance(&api, aof.id).await;
    let files = files_under(&site.instance_paths(aof.id).data);
    assert!(
        files.iter().all(|file| file.starts_with("appendonlydir/")),
        "{files:?}"
    );
    let (_, mut valkey) = start_signed_in(&api, &aof).await;
    assert_eq!(get(&mut valkey, "before-crash").as_deref(), Some("two"));
    drop(valkey);
    checks.passed("persistence_aof");

    // volatile: nothing on disk, nothing over a stop
    let (_, mut valkey) = start_signed_in(&api, &volatile).await;
    set(&mut valkey, "before-stop", "three");
    drop(valkey);
    stop_instance(&api, volatile.id).await;
    assert!(
        files_under(&site.instance_paths(volatile.id).data).is_empty(),
        "volatile writes nothing"
    );
    let (_, mut valkey) = start_signed_in(&api, &volatile).await;
    assert_eq!(get(&mut valkey, "before-stop"), None);
    drop(valkey);
    checks.passed("volatile");

    // stop
    for made in [&rdb, &aof, &volatile] {
        let summary = api.get(made.id).await.expect("the instance can be read");
        assert_stopped_clean(&api, made.id, pid_of(&summary)).await;
    }
    drop(api);
    second.stop();
    checks.passed("stop");

    checks.write(&ev, &site);
    ev.pass_with(&format!("Valkey {version} passed {}", checks.0.join(", ")));
}
