//! The restart policy of a crash outside any operation (spec 0010 AC-7,
//! AC-8, AC-17): the pause belongs to the actor's timer, the respawn is a
//! system `start_instance` operation with no `start_begin`, and a stop
//! during the pause runs at once.

use std::time::Duration;

use stackly_core::supervisor::{BindOutcome, codes};
use stackly_domain::{DesiredState, InstanceId, InstanceLifecycle, OperationKind, OperationState};
use stackly_platform::{ChildRole, ExitStatus};
use stackly_testkit::FakeProcessState;

use crate::common::{Rig, Spec};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn main_spawns(rig: &Rig) -> usize {
    rig.processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count()
}

/// Starts the instance and waits until it is `ready`. The real server of
/// the first process is stopped again, so the port is free when the fake
/// process dies.
async fn running(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    rig.kill_servers();
    spawn
}

#[tokio::test]
async fn a_crash_outside_any_operation_respawns_after_the_pause_without_start_begin() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the instance recovers", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;
    let second = rig.main_spawn(2).await;

    assert_ne!(second.pid, first.pid);
    assert_eq!(
        rig.processes().process_state(first.pid),
        Some(FakeProcessState::Exited),
        "never two main processes at once"
    );
    let names = rig.events.lifecycle_names(id);
    assert_eq!(
        names.iter().filter(|name| **name == "starting").count(),
        1,
        "the respawn has no start_begin: {names:?}"
    );
    let respawns: Vec<_> = rig
        .operations_of(id, OperationKind::StartInstance)
        .await
        .into_iter()
        .filter(|view| view.journal.input().as_str().contains("restart_policy"))
        .collect();
    assert_eq!(respawns.len(), 1, "one system start for the respawn");
    let stages: Vec<_> = rig
        .engine
        .get(respawns[0].operation.id())
        .await
        .unwrap()
        .unwrap()
        .steps
        .iter()
        .map(|step| step.name().as_str().to_owned())
        .collect();
    assert!(!stages.contains(&"journal".to_owned()), "{stages:?}");
}

#[tokio::test]
async fn the_respawn_ends_ready_when_the_new_process_answers() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.processes()
        .exit(first.pid, Some(ExitStatus::Signaled(11)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    let respawn = loop {
        let found = rig
            .operations_of(id, OperationKind::StartInstance)
            .await
            .into_iter()
            .find(|view| view.journal.input().as_str().contains("restart_policy"));
        if let Some(found) = found {
            break found;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let detail = rig
        .wait_state(respawn.operation.id(), OperationState::Succeeded)
        .await;

    assert_eq!(detail.view.operation.state(), OperationState::Succeeded);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert!(rig.sup.status(id).ports_final);
}

#[tokio::test]
async fn the_pause_is_shown_and_a_stop_during_it_runs_at_once_and_cancels_the_respawn() {
    let rig = Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_secs(30);
        config.restart_max_delay = Duration::from_secs(30);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));
    wait_for("the instance recovers", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;
    wait_for("the pause is shown", || {
        rig.sup.status(id).restart.next_attempt_at.is_some()
    })
    .await;
    assert_eq!(rig.sup.status(id).restart.attempts, 1);
    let stop = rig.stop(id).await;
    let detail = rig.wait_final(stop).await;

    assert_eq!(
        detail.view.operation.state(),
        OperationState::Succeeded,
        "no resource.busy: nothing held the lock during the pause"
    );
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert_eq!(rig.sup.status(id).restart.next_attempt_at, None);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(main_spawns(&rig), 1, "no spawn after the stop");
}

#[tokio::test]
async fn a_crash_loop_after_a_ready_run_spawns_six_times_in_all_then_fails_crash_loop() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;

    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    for attempt in 2..=6 {
        let spawn = rig.main_spawn(attempt).await;
        let earlier = rig.main_spawn(attempt - 1).await;
        assert_eq!(
            rig.processes().process_state(earlier.pid),
            Some(FakeProcessState::Exited)
        );
        rig.processes()
            .exit(spawn.pid, Some(ExitStatus::Exited(70)));
    }
    wait_for("the instance fails with its budget shown as spent", || {
        let restart = rig.sup.status(id).restart;
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Failed
            && restart.exhausted
            && restart.attempts == 6
    })
    .await;

    let instance = rig.instance_row(id);
    assert_eq!(
        instance.last_failure().map(|code| code.as_str()),
        Some("crash_loop")
    );
    assert_eq!(main_spawns(&rig), 6, "six spawns for one crash loop");
    let restart = rig.sup.status(id).restart;
    assert!(restart.exhausted);
    assert_eq!(restart.attempts, 6);
    // The operation ends a moment after the lifecycle: wait for it.
    let failed = loop {
        let respawns = rig.operations_of(id, OperationKind::StartInstance).await;
        if let Some(view) = respawns
            .into_iter()
            .find(|view| view.operation.state() == OperationState::Failed)
        {
            break view;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let detail = rig
        .engine
        .get(failed.operation.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        Rig::failure_code(&detail),
        codes::RETRY_BUDGET_EXHAUSTED.to_string()
    );
}

#[tokio::test]
async fn a_port_taken_by_someone_else_after_a_crash_outside_an_operation_is_not_retried() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;
    let port: u16 = first
        .args
        .iter()
        .find_map(|arg| arg.strip_prefix("--port="))
        .unwrap()
        .parse()
        .unwrap();

    rig.ports.answer(port, BindOutcome::InUse);
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));
    wait_for("the instance fails", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Failed
    })
    .await;

    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("endpoint_in_use")
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(main_spawns(&rig), 1, "no respawn and no budget used");
    assert_eq!(rig.sup.status(id).restart.attempts, 0);
}

#[tokio::test]
async fn sixty_seconds_of_ready_clears_the_history_so_the_next_crash_is_the_first_again() {
    let rig = Rig::with_config(|config| {
        config.restart_healthy_reset = Duration::from_millis(300);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    wait_for("the history is cleared after a healthy period", || {
        let status = rig.sup.status(id);
        status.restart.attempts == 0
            && rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;
    rig.kill_servers();

    rig.processes()
        .exit(second.pid, Some(ExitStatus::Exited(1)));
    wait_for("the instance recovers with one attempt counted", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
            && rig.sup.status(id).restart.attempts == 1
    })
    .await;

    assert_eq!(
        rig.sup.status(id).restart.attempts,
        1,
        "the budget starts again"
    );
}
