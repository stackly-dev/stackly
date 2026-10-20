//! What an actor decides from the registry when a process exits (spec 0010,
//! *Commit*, AC-22): the event comes from the aggregate the commit reads,
//! never from an earlier read, and a failed read is tried again instead of
//! being taken for an answer.

use std::time::Duration;

use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, OperationKind, OperationState, RegistryError,
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
