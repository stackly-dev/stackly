//! Panics, works that end with items still intended, journal writes that
//! fail, and payloads over their limit (spec 0007 AC-16).

use std::time::{Duration, Instant};

use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_core::operation::{
    BeginError, BoxFuture, EngineLimits, OperationContext, OperationOutcome, OperationWork,
    StepSpec, WorkOutcome, WorkResult, codes,
};
use stackly_domain::{
    CreatedState, JournalPayload, LockKey, OperationKind, OperationResource, OperationState,
    RegistryError, RuntimeId, StepState,
};

use crate::common::{Harness, manager_request, request, wait_state};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panicking_work_is_compensated_and_failed_and_the_engine_keeps_running() {
    let harness = Harness::new().await;

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .creates_instance_tree()
                .step("swap")
                .panics()
                .boxed(),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    assert_eq!(
        failed.view.operation.failure().map(|f| f.code().as_str()),
        Some("panicked")
    );
    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("internal.unexpected")
    );
    assert_eq!(failed.created[0].state(), CreatedState::Compensated);
    assert_eq!(failed.steps[1].state(), StepState::Failed);
    let next = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;
    wait_state(&harness.engine, next, OperationState::Succeeded).await;
}

/// Records an instance tree and never confirms or declines it.
struct Forgetful;

impl OperationWork for Forgetful {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "create".parse().expect("create step name is valid"),
            ))
            .await?;
            ctx.new_instance_id().await?;
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_work_that_succeeds_with_an_intended_item_is_compensated_and_failed() {
    let harness = Harness::new().await;

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(Forgetful),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    assert_eq!(
        failed.view.operation.failure().map(|f| f.code().as_str()),
        Some("step_failed")
    );
    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("internal.unexpected")
    );
    assert_eq!(failed.created[0].state(), CreatedState::Compensated);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_journal_write_that_fails_stops_the_work_and_the_final_state_still_lands() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .creates_instance_tree()
                .hold(&hold)
                .step("swap")
                .boxed(),
        ))
        .await;
    hold.started().await;

    harness.registry.fail_next_commit(RegistryError::NoSpace);
    hold.release();
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("internal.unexpected")
    );
    assert_eq!(failed.created[0].state(), CreatedState::Compensated);
    assert!(
        std::fs::read_dir(harness.layout.layout().instances_dir())
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_input_over_its_limit_is_refused_before_anything_is_written() {
    let harness = Harness::new().await;
    let text = format!(r#"{{"blob":"{}"}}"#, "x".repeat(20_000));
    let mut begin = manager_request(
        OperationKind::UpdateManager,
        ScriptedWork::new().step("swap").boxed(),
    );
    begin.input = JournalPayload::new(text, usize::MAX).unwrap();

    let refused = harness.engine.begin(begin).await;

    assert_eq!(
        refused,
        Err(BeginError::PayloadTooLarge {
            field: "input",
            max: 16_384
        })
    );
    assert_eq!(harness.engine.stats().queued, 0);
}

#[test]
fn every_engine_code_is_a_distinct_area_reason_code() {
    let mut seen = std::collections::BTreeSet::new();
    for code in codes::ALL {
        assert!(seen.insert(code.as_str()), "{code} is listed twice");
        assert!(code.as_str().contains('.'));
    }
    assert_eq!(seen.len(), codes::ALL.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_start_commit_frees_the_key_and_slot_and_retries_after_the_interval() {
    let retry = Duration::from_millis(300);
    let harness = Harness::idle(
        EngineLimits::default()
            .with_slots(1, 64)
            .with_intervals(Duration::from_millis(100), retry),
    );
    harness.engine.recover().await.unwrap();
    let unlucky = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;
    let mut install = request(
        OperationKind::InstallRuntime,
        OperationResource::Runtime(RuntimeId::generate()),
        ScriptedWork::new().step("download").boxed(),
    );
    install.creates_resource = true;
    let other = harness.begin(install).await;

    harness.registry.fail_next_commit(RegistryError::NoSpace);
    let started = Instant::now();
    harness.engine.start();
    wait_state(&harness.engine, other, OperationState::Succeeded).await;

    let waiting = harness.engine.get(unlucky).await.unwrap().unwrap();
    assert_eq!(waiting.view.operation.state(), OperationState::Queued);
    assert_eq!(waiting.view.operation.started_at(), None);
    assert!(
        started.elapsed() < retry,
        "the other operation took the freed slot before the retry"
    );
    assert_eq!(harness.engine.locks().holder(&LockKey::Manager), None);
    drop(
        harness
            .engine
            .locks()
            .try_lock_internal(&[LockKey::Manager], "probe")
            .expect("the failed start released its key"),
    );
    let done = wait_state(&harness.engine, unlucky, OperationState::Succeeded).await;
    assert!(
        started.elapsed() >= retry,
        "the start ran again only after the retry interval"
    );
    assert_eq!(done.steps.len(), 1);
}
