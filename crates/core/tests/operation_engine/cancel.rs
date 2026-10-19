//! Cancel and its dispositions, and the fake restore of AC-054 (spec 0007
//! AC-7).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use stackly_core::operation::testing::{EffectCounter, HoldHandle, ScriptedWork};
use stackly_core::operation::{
    BoxFuture, CancelDisposition, OperationContext, OperationOutcome, OperationWork, StepSpec,
    WorkOutcome, WorkResult,
};
use stackly_domain::{OperationKind, OperationState, Registry, WriteBatch, WriteOp};
use stackly_testkit::EnvironmentBuilder;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, fast, manager_request, request, wait_state};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_answers_with_the_disposition_of_the_state_the_operation_is_in() {
    let harness = Harness::new().await;
    let blocker = HoldHandle::new();
    let first = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("drain").hold(&blocker).boxed(),
        ))
        .await;
    blocker.started().await;
    let effects = EffectCounter::new();
    let queued = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").effects(&effects).boxed(),
        ))
        .await;

    let before_start = harness.engine.cancel(queued).await.unwrap();
    assert_eq!(
        before_start.disposition,
        CancelDisposition::CancelledBeforeStart
    );
    assert_eq!(
        before_start.view.operation.state(),
        OperationState::Cancelled
    );

    let requested = harness.engine.cancel(first).await.unwrap();
    assert_eq!(requested.disposition, CancelDisposition::Requested);
    wait_state(&harness.engine, first, OperationState::Cancelled).await;
    assert_eq!(effects.get(), 0, "the cancelled queued work never ran");

    let late = harness.engine.cancel(first).await.unwrap();
    assert_eq!(late.disposition, CancelDisposition::AlreadyFinal);

    let committing = HoldHandle::new();
    let past = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .step("swap")
                .irreversible()
                .hold(&committing)
                .boxed(),
        ))
        .await;
    committing.started().await;
    let continuing = harness.engine.cancel(past).await.unwrap();
    assert_eq!(
        continuing.disposition,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    let again = harness.engine.cancel(past).await.unwrap();
    assert_eq!(
        again.disposition,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    assert_eq!(
        again.view.operation.revision(),
        continuing.view.operation.revision(),
        "a second cancel commits nothing"
    );
    committing.release();
    let done = wait_state(&harness.engine, past, OperationState::Succeeded).await;
    assert!(done.view.journal.cancel_requested_at().is_some());
    assert!(done.view.journal.point_of_no_return_at().is_some());

    let broken = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("swap")
                .irreversible()
                .step("verify")
                .fail("manager.verify_failed", false)
                .boxed(),
        ))
        .await;
    wait_state(&harness.engine, broken, OperationState::NeedsRecovery).await;
    let recovery = harness.engine.cancel(broken).await.unwrap();
    assert_eq!(recovery.disposition, CancelDisposition::RecoveryRequired);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resumed_queued_operation_past_its_point_of_no_return_keeps_running_on_cancel() {
    let harness = Harness::with_limits(fast().with_slots(1, 64)).await;
    let env = EnvironmentBuilder::new().no_members();
    let id = env.id();
    harness
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            env.build(),
        )]))
        .unwrap();
    harness.engine.register_factory(
        OperationKind::EnvironmentDown,
        Arc::new(stackly_core::operation::testing::ScriptedFactory::always(
            ScriptedWork::new()
                .step("stop")
                .irreversible()
                .step("verify")
                .repeatable(),
        )),
    );
    let blocker = HoldHandle::new();
    let blocking = harness
        .begin(request(
            OperationKind::EnvironmentDown,
            stackly_domain::OperationResource::Environment(id),
            ScriptedWork::new().step("drain").hold(&blocker).boxed(),
        ))
        .await;
    blocker.started().await;
    let failing = harness
        .begin(request(
            OperationKind::EnvironmentDown,
            stackly_domain::OperationResource::Environment(id),
            ScriptedWork::new()
                .step("stop")
                .irreversible()
                .step("verify")
                .repeatable()
                .fail("environment.verify_failed", true)
                .boxed(),
        ))
        .await;
    blocker.release();
    wait_state(&harness.engine, blocking, OperationState::Succeeded).await;
    let stuck = wait_state(&harness.engine, failing, OperationState::NeedsRecovery).await;
    assert_eq!(
        stuck.view.operation.recovery_hint(),
        Some(stackly_domain::RecoveryHint::Resume)
    );

    // The only running slot stays taken, so the resumed operation is still
    // queued when the cancel lands.
    let slot = HoldHandle::new();
    harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("drain").hold(&slot).boxed(),
        ))
        .await;
    slot.started().await;

    let resumed = harness.engine.resume(failing, None).await.unwrap();
    let cancelled = harness.engine.cancel(failing).await.unwrap();

    assert_eq!(resumed.operation.state(), OperationState::Queued);
    assert_eq!(
        cancelled.disposition,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    assert_eq!(
        cancelled.view.operation.state(),
        OperationState::CancelRequested
    );
    slot.release();
    let done = wait_state(&harness.engine, failing, OperationState::Succeeded).await;
    assert!(done.view.journal.cancel_requested_at().is_some());
}

/// A restore in two steps: write the new data to staging in the scratch
/// folder (reversible), then swap it over the target (irreversible).
struct FakeRestore {
    target: PathBuf,
    staging_hold: HoldHandle,
    commit_hold: HoldHandle,
}

impl OperationWork for FakeRestore {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "staging".parse().expect("staging step name is valid"),
            ))
            .await?;
            let staging = ctx.scratch_dir().await?.join("restored.db");
            std::fs::write(&staging, b"restored").expect("staged database can be written");
            self.staging_hold.mark_started();
            tokio::select! {
                () = self.staging_hold.released() => {}
                () = ctx.stopped() => {}
            }
            ctx.check_cancel()?;
            ctx.finish_step(None).await?;
            ctx.begin_step(
                StepSpec::new("commit".parse().expect("commit step name is valid")).irreversible(),
            )
            .await?;
            self.commit_hold.mark_started();
            self.commit_hold.released().await;
            std::fs::rename(&staging, &self.target).expect("staged database can replace target");
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancelled_fake_restore_keeps_the_target_or_reports_its_point_of_no_return() {
    let ev = evidence!(
        covers = ["REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "a cancel during the staging step of a fake restore leaves the sentinel target identical and removes the staging folder, and a cancel after the irreversible commit step began shows the point of no return and ends succeeded, never cancelled",
    );
    let harness = Harness::new().await;
    let target = harness.root.path().join("orders.db");
    std::fs::write(&target, b"original").unwrap();

    let staging_hold = HoldHandle::new();
    let staged = harness
        .begin(manager_request(
            OperationKind::RestoreInstance,
            Box::new(FakeRestore {
                target: target.clone(),
                staging_hold: staging_hold.clone(),
                commit_hold: HoldHandle::new(),
            }),
        ))
        .await;
    staging_hold.started().await;
    let scratch = harness
        .layout
        .layout()
        .operations_dir()
        .join(staged.to_string());
    assert!(scratch.join("restored.db").is_file());
    assert_eq!(
        harness.engine.cancel(staged).await.unwrap().disposition,
        CancelDisposition::Requested
    );
    wait_state(&harness.engine, staged, OperationState::Cancelled).await;
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    assert!(!scratch.exists(), "the staging folder is gone");

    let commit_hold = HoldHandle::new();
    let committed = harness
        .begin(manager_request(
            OperationKind::RestoreInstance,
            Box::new(FakeRestore {
                target: target.clone(),
                staging_hold: {
                    let open = HoldHandle::new();
                    open.release();
                    open
                },
                commit_hold: commit_hold.clone(),
            }),
        ))
        .await;
    commit_hold.started().await;
    let cancel = harness.engine.cancel(committed).await.unwrap();
    assert_eq!(
        cancel.disposition,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    assert!(cancel.view.journal.point_of_no_return_at().is_some());
    commit_hold.release();
    let done = wait_state(&harness.engine, committed, OperationState::Succeeded).await;
    assert!(done.view.journal.cancel_requested_at().is_some());
    assert_eq!(std::fs::read(&target).unwrap(), b"restored");
    tokio::time::sleep(Duration::from_millis(20)).await;
    ev.pass();
}
