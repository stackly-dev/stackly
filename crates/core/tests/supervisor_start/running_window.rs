//! A reader never joins `running` with the status of an instance without a
//! process (spec 0010 AC-3, the lifecycle fix found by spec 0042 verify):
//! a restart and the respawn after a crash take the same path as a start,
//! so the actor learns the new process and its `not_ready` before the
//! `spawned` commit, and a failed commit of the identity of a respawn
//! leaves the slot empty with readiness `unknown` `not_running`.

use std::time::Duration;

use stackly_core::supervisor::InstanceStatus;
use stackly_domain::{
    InstanceId, InstanceLifecycle, OperationId, OperationKind, OperationState, Pid, ReadinessState,
    ResourceRevision,
};
use stackly_platform::ExitStatus;

use crate::common::{Rig, Spec};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Starts the instance and waits until it is `ready`; the real server is
/// stopped again so the port is free for the next process.
async fn running(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    rig.kill_servers();
    spawn
}

fn lifecycles(rig: &Rig, id: InstanceId) -> Vec<(InstanceLifecycle, ResourceRevision)> {
    let mut found: Vec<_> = rig
        .events
        .lifecycles
        .lock()
        .expect("the recorder is not poisoned")
        .iter()
        .filter(|(instance, _, _)| *instance == id)
        .map(|(_, lifecycle, revision)| (*lifecycle, *revision))
        .collect();
    found.sort_by_key(|(_, revision)| *revision);
    found
}

fn statuses(rig: &Rig, id: InstanceId) -> Vec<(InstanceStatus, ResourceRevision)> {
    rig.events
        .statuses
        .lock()
        .expect("the recorder is not poisoned")
        .iter()
        .filter(|(instance, _, _)| *instance == id)
        .map(|(_, status, revision)| (status.clone(), *revision))
        .collect()
}

/// The revisions at which the registry turned the instance `running` from
/// any other lifecycle.
fn became_running(rig: &Rig, id: InstanceId) -> Vec<ResourceRevision> {
    let mut previous = None;
    let mut found = Vec::new();
    for (lifecycle, revision) in lifecycles(rig, id) {
        if lifecycle == InstanceLifecycle::Running && previous != Some(InstanceLifecycle::Running) {
            found.push(revision);
        }
        previous = Some(lifecycle);
    }
    found
}

/// Every status published at a revision whose registry lifecycle is
/// `running` names a process and a readiness other than `unknown`.
fn assert_running_always_has_a_process(rig: &Rig, id: InstanceId) {
    let lifecycles = lifecycles(rig, id);
    for (status, revision) in statuses(rig, id) {
        let governing = lifecycles
            .iter()
            .rev()
            .find(|(_, at)| *at <= revision)
            .map(|(lifecycle, _)| *lifecycle);
        if governing != Some(InstanceLifecycle::Running) {
            continue;
        }
        assert!(
            status.process.is_some(),
            "a status at {revision:?}, where the registry says running, names no process: {status:?}"
        );
        assert_ne!(
            status.readiness.state(),
            ReadinessState::Unknown,
            "a status at {revision:?}, where the registry says running, reads unknown: {status:?}"
        );
    }
}

/// Before the commit that made the instance `running` at `running_at`, a
/// status already named `pid` as `not_ready`.
fn assert_not_ready_before(rig: &Rig, id: InstanceId, pid: Pid, running_at: ResourceRevision) {
    let earlier = statuses(rig, id).into_iter().find(|(status, revision)| {
        *revision < running_at
            && status.process.as_ref().map(|process| process.pid) == Some(pid)
            && status.readiness.state() == ReadinessState::NotReady
    });
    assert!(
        earlier.is_some(),
        "no not_ready status named process {pid} before the running commit at {running_at:?}: {:?}",
        statuses(rig, id)
    );
}

async fn respawn_of(rig: &Rig, id: InstanceId) -> OperationId {
    loop {
        let found = rig
            .operations_of(id, OperationKind::StartInstance)
            .await
            .into_iter()
            .find(|view| view.journal.input().as_str().contains("restart_policy"));
        if let Some(found) = found {
            return found.operation.id();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_restart_names_the_new_process_as_not_ready_before_the_registry_says_running_again() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    let restart = rig.restart(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(first.pid).is_empty()
    })
    .await;
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(restart, OperationState::Succeeded).await;

    let running_at = became_running(&rig, id);
    assert_eq!(
        running_at.len(),
        2,
        "the start and the restart each turned the instance running: {:?}",
        lifecycles(&rig, id)
    );
    assert_not_ready_before(&rig, id, first.pid, running_at[0]);
    assert_not_ready_before(&rig, id, second.pid, running_at[1]);
    assert_running_always_has_a_process(&rig, id);
    assert_eq!(
        rig.sup.status(id).process.map(|process| process.pid),
        Some(second.pid)
    );
}

#[tokio::test]
async fn the_respawn_after_a_crash_names_the_new_process_as_not_ready_before_the_registry_says_running()
 {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    let respawn = respawn_of(&rig, id).await;
    rig.wait_state(respawn, OperationState::Succeeded).await;

    let running_at = became_running(&rig, id);
    assert_eq!(
        running_at.len(),
        2,
        "the start and the respawn each turned the instance running: {:?}",
        lifecycles(&rig, id)
    );
    assert_not_ready_before(&rig, id, second.pid, running_at[1]);
    assert_running_always_has_a_process(&rig, id);
}

#[tokio::test]
async fn a_failed_commit_of_the_identity_of_a_respawn_leaves_no_process_and_reads_not_running() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.registry.refuse_process_observations(true);
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    let respawn = respawn_of(&rig, id).await;
    let detail = rig.wait_final(respawn).await;

    assert_ne!(
        detail.view.operation.state(),
        OperationState::Succeeded,
        "a respawn whose identity was not recorded never succeeds"
    );
    assert_eq!(
        Rig::failure_code(&detail),
        stackly_core::supervisor::codes::IDENTITY_CAPTURE_FAILED.to_string()
    );
    assert_ne!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    let status = rig.sup.status(id);
    assert!(status.process.is_none(), "no process is left to show");
    assert_eq!(status.readiness.state(), ReadinessState::Unknown);
    assert_eq!(status.readiness.reason().to_string(), "not_running");
    assert_eq!(
        became_running(&rig, id).len(),
        1,
        "only the first start turned the instance running"
    );
    assert_running_always_has_a_process(&rig, id);
}
