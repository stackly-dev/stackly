//! Force stop, restart, and how a process ended (spec 0026, AC-2 to AC-9):
//! `SIGKILL` only through `force_kill`, only while `stopping`, only after
//! the identity check; `last_exit` from the journal and the exit; a restart
//! that stops with the provider's strategy, waits for the lock, and starts
//! again under one operation.

use std::time::Duration;

use stackly_core::supervisor::codes;
use stackly_domain::{
    DesiredState, ExitKind, InstanceBlocker, InstanceId, InstanceLifecycle, OperationKind,
    OperationState,
};
use stackly_platform::{ExitStatus, ProcessError, ProviderSignal};
use stackly_testkit::ProcessOp;

use crate::common::{Rig, Spec, replace};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Starts the instance on the dummy server and returns its fake spawn.
async fn running(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

/// A rig whose stop gives up after one second.
async fn short_stop() -> Rig {
    Rig::patched(|text| replace(text, "stopSeconds: 10", "stopSeconds: 1")).await
}

/// Starts the instance and lets a stop time out, so it is `stopping`.
async fn stuck_stopping(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let spawn = running(rig, id).await;
    let stop = rig.stop(id).await;
    let detail = rig.wait_state(stop, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), codes::STOP_TIMEOUT.to_string());
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Stopping
    );
    spawn
}

fn force_kills(rig: &Rig) -> Vec<stackly_testkit::ProcessCall> {
    rig.processes()
        .calls()
        .into_iter()
        .filter(|call| call.op == ProcessOp::ForceKill)
        .collect()
}

#[tokio::test]
async fn ac_1_ac_5_a_clean_stop_records_last_exit_clean_and_kills_nothing() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;
    assert_eq!(rig.instance_row(id).last_exit(), None);

    let stop = rig.stop(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(spawn.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    // A process the stop signal itself ended reads clean too.
    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Signaled(15)));
    rig.wait_state(stop, OperationState::Succeeded).await;

    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.last_exit(), Some(ExitKind::Clean));
    assert!(force_kills(&rig).is_empty());
}

#[tokio::test]
async fn ac_5_a_child_a_foreign_signal_ends_while_stopping_is_crashed() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    let stop = rig.stop(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(spawn.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Signaled(11)));
    rig.wait_state(stop, OperationState::Succeeded).await;

    assert_eq!(rig.instance_row(id).last_exit(), Some(ExitKind::Crashed));
}

#[tokio::test]
async fn ac_5_a_crash_while_running_records_crashed() {
    let rig = Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_secs(30);
        config.restart_max_delay = Duration::from_secs(30);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    rig.kill_servers();
    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Exited(70)));
    wait_for("the crash is recorded", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;

    assert_eq!(rig.instance_row(id).last_exit(), Some(ExitKind::Crashed));
}

#[tokio::test]
async fn ac_2_ac_4_a_force_stop_after_a_stop_timeout_kills_the_main_process_once_and_ends_forced() {
    let rig = short_stop().await;
    let id = rig.instance(&Spec::default());
    let spawn = stuck_stopping(&rig, id).await;
    rig.kill_servers();

    let force = rig.force_stop(id).await;
    let detail = rig.wait_state(force, OperationState::Succeeded).await;

    let stages: Vec<_> = detail
        .steps
        .iter()
        .map(|step| step.name().as_str())
        .collect();
    assert_eq!(
        stages,
        [
            "validate",
            "lock",
            "journal",
            "kill",
            "await_exit",
            "complete"
        ]
    );
    let kills = force_kills(&rig);
    assert_eq!(kills.len(), 1, "{kills:?}");
    assert_eq!(kills[0].pid, Some(spawn.pid));
    assert_eq!(kills[0].error, None);
    // The normal stop sent its term; the force sent no other signal.
    assert_eq!(rig.processes().signals(spawn.pid), [ProviderSignal::Term]);
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert_eq!(instance.last_exit(), Some(ExitKind::Forced));
}

#[tokio::test]
async fn ac_2_a_force_stop_of_a_running_instance_is_refused_without_a_signal() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    let force = rig.force_stop(id).await;
    let detail = rig.wait_state(force, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::FORCE_REQUIRES_STOPPING.to_string()
    );
    assert!(force_kills(&rig).is_empty());
    assert!(rig.processes().signals(spawn.pid).is_empty());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
}

#[tokio::test]
async fn ac_2_a_force_stop_of_a_stopped_instance_does_nothing() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let force = rig.force_stop(id).await;
    rig.wait_state(force, OperationState::Succeeded).await;

    assert!(force_kills(&rig).is_empty());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
}

#[tokio::test]
async fn ac_3_a_force_stop_never_kills_a_pid_whose_identity_changed() {
    let rig = short_stop().await;
    let id = rig.instance(&Spec::default());
    let spawn = stuck_stopping(&rig, id).await;
    // The PID now belongs to a process that started later.
    rig.processes().update_facts(spawn.pid, |facts| {
        facts.start = "9999999999.000001".parse().unwrap();
    });

    let force = rig.force_stop(id).await;
    let detail = rig.wait_state(force, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::SIGNAL_REFUSED.to_string()
    );
    let kills = force_kills(&rig);
    assert_eq!(kills.len(), 1);
    assert_eq!(
        kills[0].error,
        Some(ProcessError::IdentityMismatch { pid: spawn.pid })
    );
    assert!(
        rig.processes().process_state(spawn.pid) != Some(stackly_testkit::FakeProcessState::Exited),
        "the process that now holds the PID is alive"
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);
    assert!(matches!(
        rig.sup.status(id).blocker,
        Some(InstanceBlocker::IdentityUnproven { .. })
    ));
}

#[tokio::test]
async fn ac_4_a_process_that_left_between_the_check_and_the_kill_still_reads_forced() {
    let rig = short_stop().await;
    let id = rig.instance(&Spec::default());
    let spawn = stuck_stopping(&rig, id).await;
    rig.kill_servers();
    rig.processes().fail_next(
        ProcessOp::ForceKill,
        ProcessError::NotFound { pid: spawn.pid },
    );

    let force = rig.force_stop(id).await;
    wait_for("the kill reaches the gone process", || {
        !force_kills(&rig).is_empty()
    })
    .await;
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(force, OperationState::Succeeded).await;

    assert_eq!(rig.instance_row(id).last_exit(), Some(ExitKind::Forced));
}

#[tokio::test]
async fn ac_4_a_process_that_outlives_the_force_wait_fails_force_timeout_and_its_exit_reads_forced()
{
    let rig = Rig::build(
        |config| config.force_exit_wait = Duration::from_millis(300),
        |text| replace(text, "stopSeconds: 10", "stopSeconds: 1"),
    )
    .await;
    let id = rig.instance(&Spec::default());
    let spawn = stuck_stopping(&rig, id).await;
    rig.kill_servers();
    rig.processes().survive_force_kill(spawn.pid);

    let force = rig.force_stop(id).await;
    let detail = rig.wait_state(force, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), codes::FORCE_TIMEOUT.to_string());
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Stopping
    );
    assert!(rig.sup.status(id).process.is_some(), "still watched");

    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Signaled(9)));
    wait_for("the late exit is recorded", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;
    assert_eq!(rig.instance_row(id).last_exit(), Some(ExitKind::Forced));
}

#[tokio::test]
async fn ac_5_a_start_after_a_force_stop_makes_the_next_exit_clean_again() {
    let rig = short_stop().await;
    let id = rig.instance(&Spec::default());
    stuck_stopping(&rig, id).await;
    rig.kill_servers();
    let force = rig.force_stop(id).await;
    rig.wait_state(force, OperationState::Succeeded).await;

    let start = rig.start(id).await;
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(start, OperationState::Succeeded).await;
    let stop = rig.stop(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(second.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes()
        .exit(second.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(stop, OperationState::Succeeded).await;

    assert_eq!(rig.instance_row(id).last_exit(), Some(ExitKind::Clean));
}

#[tokio::test]
async fn ac_6_a_restart_stops_with_the_provider_strategy_and_runs_a_new_process() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    let restart = rig.restart(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(first.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    let detail = rig.wait_state(restart, OperationState::Succeeded).await;

    assert_ne!(second.pid, first.pid);
    assert_eq!(rig.processes().signals(first.pid), [ProviderSignal::Term]);
    assert!(force_kills(&rig).is_empty(), "a restart never forces");
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(instance.desired(), DesiredState::Running);
    assert_eq!(instance.last_exit(), Some(ExitKind::Clean));
    let status = rig.sup.status(id);
    assert_eq!(status.process.map(|process| process.pid), Some(second.pid));
    assert_eq!(
        status.restart.attempts, 0,
        "the restart window starts at zero"
    );
    let stages: Vec<_> = detail
        .steps
        .iter()
        .map(|step| step.name().as_str())
        .collect();
    assert!(
        stages.starts_with(&[
            "validate",
            "lock",
            "journal",
            "stop",
            "await_exit",
            "lock_wait",
            "start"
        ]),
        "{stages:?}"
    );
    assert!(
        stages.contains(&"spawn") && stages.contains(&"readiness"),
        "{stages:?}"
    );
}

#[tokio::test]
async fn ac_7_a_restart_of_a_stopped_instance_is_refused() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let restart = rig.restart(id).await;
    let detail = rig.wait_state(restart, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::INSTANCE_NOT_RUNNING.to_string()
    );
    assert!(rig.processes().spawns().is_empty());
}

#[tokio::test]
async fn ac_8_a_restart_whose_stop_times_out_fails_and_the_instance_stays_stopped() {
    let rig = short_stop().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    let restart = rig.restart(id).await;
    let detail = rig.wait_state(restart, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), codes::STOP_TIMEOUT.to_string());
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopping);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    wait_for("the late exit ends it stopped", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        rig.processes().spawns().len(),
        1,
        "nothing started it again"
    );
}

#[tokio::test]
async fn ac_8_a_restart_whose_lock_stays_held_fails_data_in_use_and_spawns_nothing() {
    let rig = Rig::with_config(|config| {
        config.restart_lock_wait = Duration::from_millis(300);
        config.restart_lock_poll = Duration::from_millis(20);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;
    let door = rig
        .registry
        .hold_stage(OperationKind::RestartInstance, "lock_wait");

    let restart = rig.restart(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(first.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    wait_for("the restart waits before lock_wait", || door.entered()).await;
    // A process the old one left behind holds the lock.
    let held = rig.take_lock(id);
    door.release();
    let detail = rig.wait_state(restart, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), codes::DATA_IN_USE.to_string());
    assert_eq!(rig.processes().spawns().len(), 1, "no second process");
    assert!(matches!(
        rig.sup.status(id).blocker,
        Some(InstanceBlocker::DataInUse { .. })
    ));
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Stopped);
    drop(held);
}

#[tokio::test]
async fn ac_6_a_restart_of_a_recovering_instance_ends_the_pause_and_starts_at_once() {
    let rig = Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_secs(30);
        config.restart_max_delay = Duration::from_secs(30);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;
    rig.kill_servers();
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the crash pauses", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
            && rig.sup.status(id).process.is_none()
    })
    .await;

    let restart = rig.restart(id).await;
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(restart, OperationState::Succeeded).await;

    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(instance.desired(), DesiredState::Running);
    assert_eq!(rig.sup.status(id).restart.attempts, 0);
}
