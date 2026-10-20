//! The instance process outliving the daemon (spec 0010 AC-9, AC-11,
//! AC-21): the real `stacklyd` binary is killed with SIGKILL or stopped
//! with SIGTERM, and the next one adopts, restarts, or closes what the
//! first left.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_protocol::instance::{
    InstanceBlocker, InstanceDesired, InstanceLifecycle, InstanceReadinessState, InstanceSummary,
};
use stackly_protocol::operation::OperationState;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::{client, daemon_paths, runtime, start_daemon};
use crate::support::{Api, alive, reap_guard, seed_root, serving};

/// Name and SHA-256 of every file under `data`, so a restart can be shown
/// to have deleted nothing.
fn data_digest(data: &Path) -> BTreeMap<String, String> {
    let mut digest = BTreeMap::new();
    let mut pending = vec![data.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("the data folder can be read") {
            let path = entry.expect("a data folder entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("a data file can be read");
                let relative = path
                    .strip_prefix(data)
                    .expect("the file is under the data folder")
                    .display()
                    .to_string();
                digest.insert(
                    relative,
                    Sha256::digest(&bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                );
            }
        }
    }
    digest
}

fn kill_signal(signal: &str, pid: u32) {
    let status = Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "kill {signal} {pid}");
}

async fn api_of(daemon: &stackly_testkit::SpawnedDaemon, runtime_id: Uuid) -> Api {
    let (connected, _) = client(daemon.socket()).await;
    Api::new(connected, runtime_id)
}

/// Waits until the instance is adopted and `ready`.
async fn adopted_and_ready(api: &Api, id: Uuid) -> InstanceSummary {
    api.wait_instance(id, "adopted and ready", |summary| {
        summary
            .process
            .as_ref()
            .is_some_and(|process| process.adopted)
            && summary.readiness.state == InstanceReadinessState::Ready
    })
    .await
}

#[test]
fn a_daemon_killed_with_sigkill_leaves_the_process_and_the_next_daemon_adopts_it() {
    let ev = evidence!(
        covers = ["AC-013", "REQ-013"],
        kind = Kind::Fixture,
        expected = "after stacklyd is killed with SIGKILL the fake keeps running, and the next stacklyd adopts the same pid and start time without a spawn, shows it ready again on its stored port, deletes no data file, and lists one instance",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let runtime_id = seed_root(&root);
    let paths = daemon_paths(&root);
    let rt = runtime();

    let first = start_daemon(&root);
    let (id, before, port) = rt.block_on(async {
        let api = api_of(&first, runtime_id).await;
        let id = api.create_ready("survivor", json!({})).await;
        let running = api.start_ready(id).await;
        let port = running.endpoints[0].port.expect("the bound port is shown");
        (id, running, port)
    });
    let data = paths
        .layout
        .instance_paths(id.to_string().parse().expect("an instance id"))
        .data;
    let digests = data_digest(&data);
    let process = before
        .process
        .clone()
        .expect("a running instance has a process");

    kill_signal("-9", first.pid());
    drop(first);
    assert!(
        alive(process.pid),
        "the instance process outlives the daemon"
    );

    let second = start_daemon(&root);
    rt.block_on(async {
        let api = api_of(&second, runtime_id).await;
        let adopted = adopted_and_ready(&api, id).await;
        let adopted_process = adopted.process.clone().expect("the process is adopted");
        assert_eq!(adopted_process.pid, process.pid);
        assert_eq!(adopted_process.started_at, process.started_at);
        assert_eq!(adopted.lifecycle, InstanceLifecycle::Running);
        assert_eq!(adopted.desired, InstanceDesired::Running);
        assert_eq!(adopted.endpoints[0].port, Some(port));
        assert!(adopted.endpoints[0].bound);
        assert_eq!(api.list(Default::default()).await.items.len(), 1);

        // A start of an instance that runs is a no-op, not a second spawn.
        let again = api.start(id).await.expect("a start on a running instance");
        assert!(again.already_in_state);
        assert_eq!(serving(&data).len(), 1);

        let accepted = api
            .stop(id)
            .await
            .expect("the stop is accepted")
            .accepted
            .expect("a stop begins an operation");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
        api.wait_instance(id, "stopped", |summary| {
            summary.lifecycle == InstanceLifecycle::Stopped
        })
        .await;
    });
    assert_eq!(
        data_digest(&data).keys().collect::<Vec<_>>(),
        digests.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        digests.get("FAKE_VERSION"),
        data_digest(&data).get("FAKE_VERSION"),
        "the data of the instance was not touched"
    );
    assert!(serving(&data).is_empty());
    assert_eq!(second.stop().expect("the daemon stops").code(), Some(0));
    ev.pass();
}

#[test]
fn sigterm_leaves_the_instance_running_and_the_next_daemon_closes_the_interrupted_start_and_adopts()
{
    let ev = evidence!(
        covers = ["AC-013", "REQ-013"],
        kind = Kind::Fixture,
        expected = "SIGTERM during a start leaves the fake running, the next stacklyd closes the start operation failed with reason interrupted and adopts the process, and the instance becomes ready",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let runtime_id = seed_root(&root);
    let rt = runtime();

    let first = start_daemon(&root);
    let (id, operation, pid) = rt.block_on(async {
        let api = api_of(&first, runtime_id).await;
        let id = api
            .create_ready("interrupted", json!({ "readyDelayMs": 3000 }))
            .await;
        let accepted = api
            .start(id)
            .await
            .expect("the start is accepted")
            .accepted
            .expect("a start begins an operation");
        let waiting = api
            .wait_instance(id, "running and not ready", |summary| {
                summary.process.is_some()
                    && summary.readiness.state == InstanceReadinessState::NotReady
            })
            .await;
        (
            id,
            accepted.operation.id,
            waiting.process.expect("a process runs").pid,
        )
    });
    assert_eq!(first.stop().expect("the daemon stops").code(), Some(0));
    assert!(
        alive(pid),
        "SIGTERM to the daemon does not signal the instance"
    );

    let second = start_daemon(&root);
    rt.block_on(async {
        let api = api_of(&second, runtime_id).await;
        let closed = api.wait_final(operation).await;
        assert_eq!(closed.operation.state, OperationState::Failed);
        assert_eq!(
            closed
                .operation
                .failure
                .as_ref()
                .map(|failure| failure.reason.as_str()),
            Some("interrupted")
        );
        let adopted = adopted_and_ready(&api, id).await;
        assert_eq!(adopted.process.expect("a process runs").pid, pid);
        assert_eq!(adopted.lifecycle, InstanceLifecycle::Running);
        let accepted = api
            .stop_when_free(id)
            .await
            .accepted
            .expect("a stop begins an operation");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    });
    assert_eq!(second.stop().expect("the daemon stops").code(), Some(0));
    ev.pass();
}

#[test]
fn a_process_lost_while_the_daemon_was_away_is_started_again_in_the_same_boot() {
    let ev = evidence!(
        covers = ["AC-013", "REQ-013"],
        kind = Kind::Fixture,
        expected = "an instance that should run whose process died while the daemon was away is started again by the next stacklyd in the same boot, and it ends running and ready with a new process",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let runtime_id = seed_root(&root);
    let rt = runtime();

    let first = start_daemon(&root);
    let (id, pid) = rt.block_on(async {
        let api = api_of(&first, runtime_id).await;
        let id = api.create_ready("lost", json!({})).await;
        let running = api.start_ready(id).await;
        (id, running.process.expect("a process runs").pid)
    });
    kill_signal("-9", first.pid());
    drop(first);
    kill_signal("-9", pid);
    let waited = std::time::Instant::now();
    while alive(pid) && waited.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }

    let second = start_daemon(&root);
    rt.block_on(async {
        let api = api_of(&second, runtime_id).await;
        let restarted = api
            .wait_instance(id, "running with another process", |summary| {
                summary.lifecycle == InstanceLifecycle::Running
                    && summary.readiness.state == InstanceReadinessState::Ready
                    && summary
                        .process
                        .as_ref()
                        .is_some_and(|process| process.pid != pid)
            })
            .await;
        assert!(!restarted.process.expect("a process runs").adopted);
        let accepted = api
            .stop_when_free(id)
            .await
            .accepted
            .expect("a stop begins an operation");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    });
    assert_eq!(second.stop().expect("the daemon stops").code(), Some(0));
    ev.pass();
}

/// A process of the test that the registry is made to remember as the main
/// process of an instance, with a start time that is not its own.
struct Impostor {
    child: std::process::Child,
}

impl Impostor {
    fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Impostor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// With the daemon gone, writes an observation into the registry of `root`
/// that names a `sleep` of this test as the last process of `id`, with a
/// start time one second later than the process has.
fn plant_impostor(root: &TestRoot, id: Uuid) -> Impostor {
    use stackly_domain::{
        Pid, ProcessIdentity, ProcessObservation, ProcessStart, WriteBatch, WriteOp,
    };
    use stackly_storage::{OpenOptions, RegistryOpen};

    let child = Command::new("/bin/sleep")
        .arg("600")
        .spawn()
        .expect("sleep starts");
    let platform = stackly_platform_macos::MacosPlatform::new(Some(root.path().into()))
        .expect("a test root is a valid root override");
    let prepared = stackly_core::prepare_layout(&platform).expect("the layout is prepared");
    let pid = Pid::new(child.id()).expect("a pid is not 0");
    let facts = platform
        .processes()
        .inspect(pid)
        .expect("the process can be inspected")
        .expect("the process exists");
    let boot = platform.processes().boot_id().expect("the boot id is read");
    let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
        .open(
            &platform,
            prepared.layout(),
            stackly_core::daemon_info().version,
        )
        .expect("the registry opens")
    else {
        panic!("the registry opens ready");
    };
    use stackly_domain::Registry as _;
    registry
        .commit(WriteBatch::new(vec![WriteOp::RecordProcessObservation(
            ProcessObservation {
                instance: id.to_string().parse().expect("an instance id"),
                identity: ProcessIdentity {
                    pid,
                    start: ProcessStart::new(facts.start.seconds() + 1, facts.start.micros())
                        .expect("a start time"),
                    executable: facts.executable,
                    session: facts.session,
                    boot,
                },
                observed_at: platform.clock().now(),
            },
        )]))
        .expect("the observation is stored");
    drop(registry);
    Impostor { child }
}

/// Starts an instance on a daemon, kills the daemon and the instance's
/// process, and returns the ID with the runtime ID.
fn leave_a_dead_instance(root: &TestRoot) -> (Uuid, Uuid) {
    let runtime_id = seed_root(root);
    let rt = runtime();
    let first = start_daemon(root);
    let (id, pid) = rt.block_on(async {
        let api = api_of(&first, runtime_id).await;
        let id = api.create_ready("reused", json!({})).await;
        let running = api.start_ready(id).await;
        (id, running.process.expect("a process runs").pid)
    });
    kill_signal("-9", first.pid());
    drop(first);
    kill_signal("-9", pid);
    let waited = std::time::Instant::now();
    while alive(pid) && waited.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    (id, runtime_id)
}

#[test]
fn a_recorded_pid_that_another_program_owns_now_is_never_signalled_and_the_instance_runs_again() {
    let ev = evidence!(
        covers = ["AC-067", "REQ-011", "REQ-013"],
        kind = Kind::Fixture,
        expected = "RISK-14 (status from an old PID): when the last recorded pid belongs to another program with another start time and the instance lock is free, the next stacklyd takes the old process for gone, never signals that program, and starts the instance again",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let (id, runtime_id) = leave_a_dead_instance(&root);
    let impostor = plant_impostor(&root, id);
    let rt = runtime();

    let second = start_daemon(&root);
    rt.block_on(async {
        let api = api_of(&second, runtime_id).await;
        let restarted = api
            .wait_instance(id, "running again", |summary| {
                summary.lifecycle == InstanceLifecycle::Running
                    && summary.readiness.state == InstanceReadinessState::Ready
            })
            .await;
        let process = restarted.process.expect("a process runs");
        assert_ne!(process.pid, impostor.pid());
        assert!(!process.adopted);
        let accepted = api
            .stop_when_free(id)
            .await
            .accepted
            .expect("a stop begins an operation");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    });
    assert!(alive(impostor.pid()), "the other program was not signalled");
    assert_eq!(second.stop().expect("the daemon stops").code(), Some(0));
    ev.pass();
}

#[test]
fn a_recorded_pid_with_the_lock_held_by_someone_makes_the_instance_unknown_and_nothing_is_signalled()
 {
    let ev = evidence!(
        covers = ["AC-067", "REQ-013"],
        kind = Kind::Fixture,
        expected = "RISK-14 (status from an old PID): when the last recorded process cannot be proven and instance.lock is held by another process, the next stacklyd makes the instance unknown with a dataInUse blocker, refuses start and stop with supervisor.identity_unproven, and signals nothing",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let (id, runtime_id) = leave_a_dead_instance(&root);
    let impostor = plant_impostor(&root, id);
    let paths = daemon_paths(&root);
    let platform = stackly_platform_macos::MacosPlatform::new(Some(root.path().into()))
        .expect("a test root is a valid root override");
    let held = match platform
        .processes()
        .lock_instance(
            &paths
                .layout
                .instance_paths(id.to_string().parse().expect("an instance id"))
                .lock,
        )
        .expect("the lock file can be opened")
    {
        stackly_platform::LockOutcome::Acquired(lock) => lock,
        stackly_platform::LockOutcome::Held => panic!("nobody holds the lock"),
    };
    let rt = runtime();

    let second = start_daemon(&root);
    rt.block_on(async {
        let api = api_of(&second, runtime_id).await;
        let unknown = api
            .wait_instance(id, "unknown with a dataInUse blocker", |summary| {
                summary.lifecycle == InstanceLifecycle::Unknown
                    && matches!(summary.blocker, Some(InstanceBlocker::DataInUse(_)))
            })
            .await;
        assert!(unknown.process.is_none());
        let start = api.start(id).await.unwrap_err();
        assert_eq!(start.code(), "supervisor.identity_unproven", "{start:?}");
        let stop = api.stop(id).await.unwrap_err();
        assert_eq!(stop.code(), "supervisor.identity_unproven", "{stop:?}");
    });
    assert!(alive(impostor.pid()), "the other program was not signalled");
    drop(held);
    assert_eq!(second.stop().expect("the daemon stops").code(), Some(0));
    ev.pass();
}
