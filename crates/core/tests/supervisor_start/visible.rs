//! What a failing start shows on fakes (spec 0033, milestone 1): the pause
//! between two attempts of one start is the stage `backoff` once the next
//! attempt's time is shown (AC-2), every recorded failure carries its time
//! and exit in the status (AC-3), and a start that cannot use a fixed port
//! names the first free one after it without changing anything (AC-4,
//! AC-14).

use std::net::TcpListener;
use std::time::Duration;

use jiff::SignedDuration;
use stackly_core::supervisor::{BindOutcome, FailureFacts, InitRequest, codes};
use stackly_domain::{
    FailureCode, InstanceLifecycle, OperationKind, OperationState, OperationTrigger, RegistryError,
};
use stackly_platform::{ChildRole, ExitStatus};

use crate::common::{LOOPBACK, Rig, Spec};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A rig whose pause between attempts lasts long enough to look at.
async fn slow_rig() -> Rig {
    Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_secs(30);
        config.restart_max_delay = Duration::from_secs(30);
    })
    .await
}

/// The stage the operation shows now.
async fn stage_of(rig: &Rig, op: stackly_domain::OperationId) -> Option<String> {
    rig.engine
        .get(op)
        .await
        .expect("the registry reads")
        .and_then(|detail| {
            detail
                .view
                .operation
                .stage()
                .map(|stage| stage.as_str().to_owned())
        })
}

/// The details of the failed operation's error, as text pairs.
fn failure_details(detail: &stackly_core::operation::OperationDetailView) -> Vec<(String, String)> {
    detail
        .view
        .journal
        .failure_error()
        .expect("the failed operation stored its error")
        .details()
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect()
}

fn suggested(details: &[(String, String)]) -> Option<String> {
    details
        .iter()
        .find(|(key, _)| key == "suggestedPort")
        .map(|(_, value)| value.clone())
}

/// The port the fake was told to listen on.
fn port_of(spawn: &stackly_testkit::FakeSpawn) -> u16 {
    spawn
        .args
        .iter()
        .find_map(|arg| arg.strip_prefix("--port="))
        .expect("the fake gets its port")
        .parse()
        .expect("a port number")
}

#[tokio::test]
async fn a_pause_inside_a_start_is_the_backoff_stage_after_the_next_attempt_time_is_shown() {
    // covers: spec 0033 AC-2
    let rig = slow_rig().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Exited(70)));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while stage_of(&rig, op).await.as_deref() != Some("backoff") {
        assert!(tokio::time::Instant::now() < deadline, "timed out: backoff");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let restart = rig.sup.status(id).restart;
    let next = restart
        .next_attempt_at
        .expect("the stage follows the shown time of the next attempt");
    assert!(next > rig.fake_now(), "the next attempt is in the future");
    assert_eq!(
        restart.attempts, 1,
        "the crash that began the pause counts, so the respawn waited for is 1 of 5"
    );
    let steps: Vec<_> = rig
        .engine
        .get(op)
        .await
        .unwrap()
        .unwrap()
        .steps
        .iter()
        .map(|step| step.name().as_str().to_owned())
        .collect();
    assert!(
        !steps.contains(&"backoff".to_owned()),
        "the pause writes no journal step: {steps:?}"
    );

    rig.engine
        .cancel(op, OperationTrigger::background())
        .await
        .unwrap();
    let detail = rig.wait_final(op).await;
    assert_eq!(detail.view.operation.state(), OperationState::Cancelled);
    assert_eq!(rig.sup.status(id).restart.next_attempt_at, None);
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(
        mains, 1,
        "a cancel in the backoff stage spawns nothing more"
    );
}

#[tokio::test]
async fn a_crash_records_its_time_and_exit_a_stop_in_the_pause_keeps_them_and_a_spawn_clears_them()
{
    // covers: spec 0033 AC-3 (crashed with a retry, kept by a stop, cleared by spawned)
    let rig = slow_rig().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    let crashed_at = rig.fake_now();
    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Exited(70)));
    wait_for("the crash is recorded", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;

    assert_eq!(
        rig.sup.status(id).last_failure,
        Some(FailureFacts::new(
            FailureCode::PROCESS_EXITED,
            crashed_at,
            Some(ExitStatus::Exited(70))
        ))
    );
    let published = rig.events.statuses_of(id);
    assert!(
        published.iter().any(
            |status| status.last_failure.as_ref().map(|facts| facts.exit_code) == Some(Some(70))
        ),
        "the facts went out as a status change"
    );

    rig.engine
        .cancel(op, OperationTrigger::background())
        .await
        .unwrap();
    rig.wait_final(op).await;
    let stopped = rig.instance_row(id);
    assert_eq!(stopped.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(
        stopped.last_failure().map(FailureCode::as_str),
        Some("process_exited")
    );
    assert_eq!(
        rig.sup.status(id).last_failure.map(|facts| facts.code),
        Some(FailureCode::PROCESS_EXITED),
        "a stop during the pause keeps the facts with the stored failure"
    );

    let again = rig.start(id).await;
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(again, OperationState::Succeeded).await;
    assert_eq!(rig.instance_row(id).last_failure(), None);
    assert_eq!(
        rig.sup.status(id).last_failure,
        None,
        "spawned clears the facts with the stored failure"
    );
    rig.kill_servers();
}

#[tokio::test]
async fn a_crash_loop_records_the_time_and_exit_of_the_sixth_crash() {
    // covers: spec 0033 AC-3 (each failure commit replaces the facts)
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    for attempt in 1..=5 {
        let spawn = rig.main_spawn(attempt).await;
        rig.processes()
            .exit(spawn.pid, Some(ExitStatus::Exited(70)));
    }
    let sixth = rig.main_spawn(6).await;
    rig.fake.clock().advance(SignedDuration::from_hours(1));
    let last_at = rig.fake_now();
    rig.processes().exit(sixth.pid, Some(ExitStatus::Exited(3)));
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::RETRY_BUDGET_EXHAUSTED.to_string()
    );
    assert!(
        failure_details(&detail).contains(&("lastExitCode".to_owned(), "3".to_owned())),
        "{:?}",
        failure_details(&detail)
    );
    assert_eq!(
        rig.sup.status(id).last_failure,
        Some(FailureFacts::new(
            FailureCode::CRASH_LOOP,
            last_at,
            Some(ExitStatus::Exited(3))
        ))
    );
}

#[tokio::test]
async fn a_signal_that_ends_the_process_is_its_signal_number() {
    // covers: spec 0033 AC-3 (signal, not exit code)
    let rig = slow_rig().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Signaled(11)));
    wait_for("the crash is recorded", || {
        rig.sup.status(id).last_failure.is_some()
    })
    .await;

    let facts = rig.sup.status(id).last_failure.unwrap();
    assert_eq!(facts.code, FailureCode::PROCESS_SIGNALED);
    assert_eq!((facts.exit_code, facts.signal), (None, Some(11)));
    rig.engine
        .cancel(op, OperationTrigger::background())
        .await
        .unwrap();
    rig.wait_final(op).await;
}

#[tokio::test]
async fn a_start_that_fails_after_its_journal_records_its_time_without_an_exit() {
    // covers: spec 0033 AC-3 (start_failed with no process that exited)
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    rig.fail_spawn();

    let op = rig.start(id).await;
    rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        rig.sup.status(id).last_failure,
        Some(FailureFacts::new(
            FailureCode::SPAWN_FAILED,
            rig.fake_now(),
            None
        ))
    );
}

#[tokio::test]
async fn an_init_step_that_fails_records_the_exit_code_of_the_step() {
    // covers: spec 0033 AC-3 (init_failed)
    let rig = std::sync::Arc::new(Rig::new().await);
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Uninitialized,
        ..Spec::default()
    });
    let init = {
        let rig = std::sync::Arc::clone(&rig);
        tokio::spawn(async move {
            rig.sup
                .execute_init(InitRequest::new(id, Rig::user()))
                .await
        })
    };
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(3)));
    init.await.unwrap().expect_err("the step failed");

    let facts = rig
        .sup
        .status(id)
        .last_failure
        .expect("the init failure is known");
    assert_eq!(facts.code.as_str(), "init_step_failed");
    assert_eq!((facts.exit_code, facts.signal), (Some(3), None));
    assert_eq!(facts.at, rig.fake_now());
}

#[tokio::test]
async fn a_fixed_port_in_use_suggests_the_first_free_port_after_it_and_changes_nothing() {
    // covers: spec 0033 AC-4, AC-14 (the endpoints stage, before the journal)
    let rig = Rig::new().await;
    let port = Rig::free_port();
    rig.ports.answer(port, BindOutcome::InUse);
    rig.ports.answer(port + 1, BindOutcome::InUse);
    rig.ports.answer(port + 2, BindOutcome::Free);
    let id = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });
    let before = rig.instance_row(id);

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_IN_USE.to_string()
    );
    let details = failure_details(&detail);
    assert_eq!(
        suggested(&details),
        Some((port + 2).to_string()),
        "{details:?}"
    );
    let keys: Vec<_> = details.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        ["endpoint", "address", "port", "suggestedPort", "omitted"]
    );
    let after = rig.instance_row(id);
    assert_eq!(after.revision(), before.revision(), "nothing was written");
    assert_eq!(after.endpoints(), before.endpoints(), "the port stays");
    assert!(rig.processes().spawns().is_empty());
}

#[tokio::test]
async fn a_port_the_os_refuses_suggests_the_next_free_port_too() {
    // covers: spec 0033 AC-4 (endpoint_unavailable)
    let rig = Rig::new().await;
    let port = Rig::free_port();
    rig.ports.answer(port, BindOutcome::Denied);
    rig.ports.answer(port + 1, BindOutcome::Free);
    let id = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_UNAVAILABLE.to_string()
    );
    let details = failure_details(&detail);
    assert_eq!(
        suggested(&details),
        Some((port + 1).to_string()),
        "{details:?}"
    );
}

#[tokio::test]
async fn a_port_another_instance_holds_suggests_a_port_no_instance_has() {
    // covers: spec 0033 AC-4 (endpoint_conflict, the other instances checked)
    let rig = Rig::new().await;
    let port = Rig::free_port();
    // The next port belongs to a third instance, so the one after it is
    // the first that no instance has.
    rig.ports.answer(port + 2, BindOutcome::Free);
    let holder = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });
    let _neighbour = rig.instance(&Spec {
        request: Some(port + 1),
        ..Spec::default()
    });
    let id = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_CONFLICT.to_string()
    );
    let details = failure_details(&detail);
    assert!(
        details.contains(&("otherInstance".to_owned(), holder.to_string())),
        "{details:?}"
    );
    assert_eq!(
        suggested(&details),
        Some((port + 2).to_string()),
        "{details:?}"
    );
}

#[tokio::test]
async fn no_free_candidate_leaves_the_suggested_port_out() {
    // covers: spec 0033 AC-4 (no candidate, no detail)
    let rig = Rig::new().await;
    let port = Rig::free_port();
    for candidate in port..=port.saturating_add(70) {
        rig.ports.answer(candidate, BindOutcome::InUse);
    }
    let id = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_IN_USE.to_string()
    );
    assert_eq!(suggested(&failure_details(&detail)), None);
}

#[tokio::test]
async fn an_early_exit_on_a_port_someone_else_took_suggests_a_free_port() {
    // covers: spec 0033 AC-4 (the early exit check reads the other instances once)
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    let port = port_of(&spawn);
    rig.ports.answer(port + 1, BindOutcome::Free);
    let _taker = TcpListener::bind((LOOPBACK, port)).unwrap();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(1)));
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_IN_USE.to_string()
    );
    let details = failure_details(&detail);
    assert_eq!(
        suggested(&details),
        Some((port + 1).to_string()),
        "{details:?}"
    );
    let facts = rig
        .sup
        .status(id)
        .last_failure
        .expect("the failure is known");
    assert_eq!(facts.code, FailureCode::ENDPOINT_IN_USE);
    assert_eq!(facts.exit_code, Some(1), "the exit that led to the check");
}

#[tokio::test]
async fn an_early_exit_whose_instance_list_fails_still_fails_without_a_suggestion() {
    // covers: spec 0033 AC-4 (a failed read of the other instances drops the detail)
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    let port = port_of(&spawn);
    let _taker = TcpListener::bind((LOOPBACK, port)).unwrap();
    rig.registry
        .fail_instance_lists(vec![RegistryError::Unavailable]);
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(1)));
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_IN_USE.to_string()
    );
    assert_eq!(suggested(&failure_details(&detail)), None);
    assert_eq!(
        rig.instance_row(id).last_failure().map(FailureCode::as_str),
        Some("endpoint_in_use")
    );
    assert!(
        rig.operations_of(id, OperationKind::StartInstance)
            .await
            .iter()
            .all(|view| view.operation.state() == OperationState::Failed),
        "nothing respawned"
    );
}
