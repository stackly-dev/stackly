//! What an actor decides from the registry when a process exits (spec 0010,
//! *Commit*, AC-22): the event comes from the aggregate the commit reads,
//! never from an earlier read, and a failed read is tried again instead of
//! being taken for an answer.

use std::time::Duration;

use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, OperationKind, OperationState, RegistryError,
};
use stackly_platform::{ChildRole, ExitStatus};

use stackly_core::supervisor::RegistryMode;

use crate::common::{Rig, Spec};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Starts the instance and waits until it is `ready`, with the port free
/// again for a respawn.
async fn running(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    rig.kill_servers();
    spawn
}

#[tokio::test]
async fn a_stop_that_lands_in_the_middle_of_a_crash_commit_ends_stopped_not_stopping() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    // The crash commit read the aggregate as `running` and waits at the door.
    let door = rig.registry.hold_crash_commit();
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the crash commit waits", || door.entered()).await;
    // The stop lands now: it journals `stopping`, finds the process gone,
    // and waits for the exit handler.
    let stop = rig.stop(id).await;
    wait_for("the stop journals", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopping
    })
    .await;
    door.release();
    let detail = rig.wait_final(stop).await;

    assert_eq!(detail.view.operation.state(), OperationState::Succeeded);
    let instance = rig.instance_row(id);
    assert_eq!(
        instance.lifecycle(),
        InstanceLifecycle::Stopped,
        "the exit ended the stop, whichever commit came first"
    );
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert_eq!(rig.sup.status(id).process, None);
}

#[tokio::test]
async fn a_registry_read_that_fails_when_a_process_exits_is_tried_again_and_the_crash_respawns() {
    let rig = Rig::with_config(|config| {
        config.registry_retry_interval = Duration::from_millis(20);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.registry
        .fail_instance_reads(vec![RegistryError::Unavailable, RegistryError::Unavailable]);
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the subsystem degrades", || {
        rig.events.degraded.lock().unwrap().contains(&true)
    })
    .await;
    let second = rig.main_spawn(2).await;

    assert_ne!(second.pid, first.pid);
    let names = rig.events.lifecycle_names(id);
    assert!(
        names.contains(&"recovering"),
        "a failed read is no verdict: the crash retries, it is not `failed`: {names:?}"
    );
    assert!(!names.contains(&"failed"), "{names:?}");
    assert_eq!(rig.events.degraded.lock().unwrap().last(), Some(&false));
}

#[tokio::test]
async fn a_registry_read_that_fails_at_the_end_of_a_pause_does_not_drop_the_respawn() {
    let rig = Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_millis(300);
        config.restart_max_delay = Duration::from_millis(300);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));
    wait_for("the instance recovers", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;
    // The pause is running; its closing read is the next read.
    rig.registry
        .fail_instance_reads(vec![RegistryError::Unavailable]);
    let second = rig.main_spawn(2).await;

    assert_ne!(second.pid, first.pid, "the pause tried the read again");
    wait_for("the subsystem recovers", || {
        rig.events.degraded.lock().unwrap().last() == Some(&false)
    })
    .await;
}

#[tokio::test]
async fn a_stop_whose_process_exit_was_handled_before_its_journal_still_ends_stopped() {
    let rig = Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_secs(30);
        config.restart_max_delay = Duration::from_secs(30);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    // The stop has seen the process; it waits before its `journal` stage.
    let door = rig
        .registry
        .hold_stage(OperationKind::StopInstance, "journal");
    let stop = rig.stop(id).await;
    wait_for("the stop waits before the journal", || door.entered()).await;
    // The process exits and the exit handler finishes completely: the crash
    // is recorded as `recovering` and the actor's slot is empty.
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the exit is handled", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
            && rig.sup.status(id).process.is_none()
    })
    .await;
    door.release();
    let detail = rig.wait_final(stop).await;

    assert_eq!(
        detail.view.operation.state(),
        OperationState::Succeeded,
        "a process that is already gone is a stopped process"
    );
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
}

#[tokio::test]
async fn a_crash_between_the_validate_and_the_lock_of_a_stop_cancels_the_respawn_and_ends_stopped()
{
    let rig = Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_millis(500);
        config.restart_max_delay = Duration::from_millis(500);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    // The stop has read the instance `running`; it waits before its `lock`
    // stage while the process crashes on its own and the crash is handled:
    // `recovering`, the actor's slot empty, a respawn pending.
    let door = rig.registry.hold_stage(OperationKind::StopInstance, "lock");
    let stop = rig.stop(id).await;
    wait_for("the stop waits before the lock", || door.entered()).await;
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the crash is handled", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
            && rig.sup.status(id).process.is_none()
    })
    .await;
    door.release();
    let detail = rig.wait_final(stop).await;

    assert_eq!(
        detail.view.operation.state(),
        OperationState::Succeeded,
        "a crash that left no process is a stopped process"
    );
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1, "the pending respawn was cancelled by the stop");
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
}

/// Starts a rig whose recheck runs every second, with one running instance.
async fn rechecked_running() -> (Rig, InstanceId, stackly_testkit::FakeSpawn) {
    let rig = Rig::with_config(|config| {
        config.unknown_recheck_interval = Duration::from_secs(1);
    })
    .await;
    let id = rig.instance(&Spec::default());
    // Reconciliation starts the recheck task.
    rig.sup
        .reconcile(RegistryMode::Ready)
        .await
        .expect("reconciliation of a stopped instance succeeds");
    let first = running(&rig, id).await;
    (rig, id, first)
}

/// Checks what an exit the registry never took leaves (audit note 100):
/// `unknown` until the recheck settles it, then `stopped`, and no respawn.
async fn an_unrecorded_exit_reads_unknown_until_the_recheck_settles_it(rig: &Rig, id: InstanceId) {
    wait_for("the exit is marked unrecorded", || {
        let status = rig.sup.status(id);
        status.exit_unrecorded && status.process.is_none()
    })
    .await;
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Running,
        "the registry still holds the state from before the exit"
    );

    wait_for("the recheck settles the instance", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;
    assert!(!rig.sup.status(id).exit_unrecorded);
    let names = rig.events.lifecycle_names(id);
    assert!(names.contains(&"unknown"), "{names:?}");
    let spawns = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(spawns, 1, "nothing starts on its own after a lost exit");
}

#[tokio::test]
async fn an_exit_commit_the_registry_refuses_for_good_shows_unknown_until_the_recheck_confirms_it_stopped()
 {
    let (rig, id, first) = rechecked_running().await;

    rig.registry.fail_commits(vec![RegistryError::Integrity {
        detail: "a constraint the test breaks".to_owned(),
    }]);
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));

    an_unrecorded_exit_reads_unknown_until_the_recheck_settles_it(&rig, id).await;
}

#[tokio::test]
async fn an_exit_whose_registry_read_fails_for_good_shows_unknown_until_the_recheck_confirms_it_stopped()
 {
    let (rig, id, first) = rechecked_running().await;

    rig.registry
        .fail_instance_reads(vec![RegistryError::Corrupt {
            table: "instances",
            column: "lifecycle",
            detail: "a value the test breaks".to_owned(),
        }]);
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));

    an_unrecorded_exit_reads_unknown_until_the_recheck_settles_it(&rig, id).await;
}
