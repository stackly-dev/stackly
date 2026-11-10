//! The audit rows of operations (spec 0035 AC-1, AC-3, AC-4): every path of
//! *Baris audit operasi* writes exactly its rows in the batch of the commit
//! that changes the state, a commit that fails leaves none, and nothing of a
//! secret value reaches a row.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use stackly_core::operation::testing::{HoldHandle, ScriptedFactory, ScriptedWork};
use stackly_core::operation::{
    AuditNote, BoxFuture, CancelDisposition, FinalBatch, OperationContext, OperationOutcome,
    OperationWork, Origin, StepSpec, WorkError, WorkOutcome, WorkResult,
};
use stackly_domain::{
    AuditAction, AuditActor, AuditEventRecord, AuditQuery, AuditResource, AuditResult,
    ConfigRevisionNumber, OperationId, OperationKind, OperationResource, OperationState,
    OperationTrigger, Registry, RegistryError, RuntimeId,
};
use stackly_platform::SecretValue;

use crate::common::{
    Harness, TestError, fast, instance, manager_request, record_no_op, request, request_origin,
    wait_state, wait_until,
};

/// The value the redactor learned; no row may hold it.
const LEAKED: &str = "fixture-credential-audit-5b7d";

/// Every audit row of `id`, oldest `(occurred_at, id)` first.
fn rows(harness: &Harness, id: OperationId) -> Vec<AuditEventRecord> {
    let query = AuditQuery {
        operation: Some(id),
        ..AuditQuery::page(500, None)
    };
    let mut items = harness
        .registry
        .audit_events(query)
        .expect("the audit rows read")
        .value
        .items;
    items.reverse();
    items
}

/// The action and result of every row, in order.
fn actions(rows: &[AuditEventRecord]) -> Vec<(String, AuditResult)> {
    rows.iter()
        .map(|row| (row.action.to_string(), row.result))
        .collect()
}

fn finished(result: AuditResult) -> (String, AuditResult) {
    (AuditAction::OPERATION_FINISHED.to_string(), result)
}

fn gui() -> OperationTrigger {
    OperationTrigger::user(Some("gui".parse().expect("a client code")), true)
}

/// Waits until `id` has `count` audit rows.
async fn wait_rows(harness: &Harness, id: OperationId, count: usize) -> Vec<AuditEventRecord> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let found = rows(harness, id);
        if found.len() >= count {
            return found;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "operation {id} has {} audit rows, not {count}: {found:?}",
            found.len()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

// covers: AC-1 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_success_a_step_failure_and_a_panic_each_write_one_finished_row() {
    let harness = Harness::new().await;
    let succeeded = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;
    wait_state(&harness.engine, succeeded, OperationState::Succeeded).await;
    let [row] = &rows(&harness, succeeded)[..] else {
        panic!("one row: {:?}", rows(&harness, succeeded));
    };
    assert_eq!(row.action, AuditAction::OPERATION_FINISHED);
    assert_eq!(row.result, AuditResult::Succeeded);
    assert_eq!(row.actor, AuditActor::Daemon, "a background operation");
    assert_eq!(row.client, None);
    assert_eq!(row.operation, Some(succeeded));
    assert_eq!(row.operation_kind, Some(OperationKind::UpdateManager));
    assert_eq!(row.resource, None, "the manager is not written");
    assert_eq!(row.safe_details.get("state"), Some("succeeded"));

    let target = instance(&harness);
    let mut failing = request(
        OperationKind::StartInstance,
        OperationResource::Instance(target),
        ScriptedWork::new()
            .step("render")
            .fail("supervisor.endpoint_in_use", true)
            .boxed(),
    );
    failing.origin = Origin::Request(request_origin("instance.start", "start-1", 1));
    harness.tick();
    let failed = match harness.engine.begin(failing).await {
        Ok(outcome) => outcome.view().expect("a created operation").operation.id(),
        Err(error) => panic!("the begin failed: {error:?}"),
    };
    let view = wait_state(&harness.engine, failed, OperationState::Failed).await;
    let [row] = &rows(&harness, failed)[..] else {
        panic!("one row: {:?}", rows(&harness, failed));
    };
    assert_eq!(row.result, AuditResult::Failed);
    assert_eq!(row.actor, AuditActor::User);
    assert_eq!(row.client.as_ref().map(|c| c.as_str()), Some("cli"));
    assert_eq!(row.instance, Some(target));
    assert_eq!(row.resource, Some(AuditResource::Instance(target)));
    assert_eq!(row.safe_details.get("reason"), Some("step_failed"));
    assert_eq!(
        row.safe_details.get("errorCode"),
        view.view
            .journal
            .failure_error()
            .map(|error| error.code().as_str())
    );
    assert_eq!(
        row.safe_details.get("stage").map(str::to_owned),
        view.view.operation.stage().map(ToString::to_string),
        "the last stage, when the work set one"
    );

    let panicked = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").panics().boxed(),
        ))
        .await;
    wait_state(&harness.engine, panicked, OperationState::Failed).await;
    let found = rows(&harness, panicked);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].safe_details.get("reason"), Some("panicked"));
}

// covers: AC-1, AC-3 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancel_before_start_writes_the_senders_row_then_the_finished_row_in_one_batch() {
    let harness = Harness::new().await;
    let blocker = HoldHandle::new();
    let first = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("drain").hold(&blocker).boxed(),
        ))
        .await;
    blocker.started().await;
    let queued = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;

    // The dispatcher's own commit of `waiting_on` lands first, so the
    // armed failure hits the cancel. A commit that fails leaves no row; the
    // next cancel writes them once.
    wait_until(&harness.engine, queued, |detail| {
        detail.view.journal.waiting_on() == Some(first)
    })
    .await;
    harness.registry.fail_next_commit(RegistryError::NoSpace);
    assert!(harness.engine.cancel(queued, gui()).await.is_err());
    assert!(
        rows(&harness, queued).is_empty(),
        "a failed commit wrote a row"
    );

    let outcome = harness.engine.cancel(queued, gui()).await.unwrap();
    assert_eq!(outcome.disposition, CancelDisposition::CancelledBeforeStart);
    let found = rows(&harness, queued);
    assert_eq!(
        actions(&found),
        [
            (
                AuditAction::OPERATION_CANCEL.to_string(),
                AuditResult::Succeeded
            ),
            finished(AuditResult::Cancelled),
        ]
    );
    let (cancel, closed) = (&found[0], &found[1]);
    assert_eq!(
        cancel.safe_details.get("disposition"),
        Some("cancelled_before_start")
    );
    assert_eq!(cancel.actor, AuditActor::User, "the sender's row");
    assert_eq!(cancel.client.as_ref().map(|c| c.as_str()), Some("gui"));
    assert_eq!(closed.actor, AuditActor::Daemon, "the trigger's row");
    assert_eq!(closed.client, None);
    assert_eq!(
        closed
            .occurred_at
            .duration_since(cancel.occurred_at)
            .as_nanos(),
        1,
        "rows of one batch are a nanosecond apart, sender first"
    );

    let late = harness.engine.cancel(queued, gui()).await.unwrap();
    assert_eq!(late.disposition, CancelDisposition::AlreadyFinal);
    assert_eq!(
        rows(&harness, queued).len(),
        2,
        "an answer that changes nothing"
    );
    blocker.release();
    wait_state(&harness.engine, first, OperationState::Succeeded).await;
}

// covers: AC-1, AC-3 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_requested_cancel_writes_one_row_and_the_compensation_closes_it_cancelled() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("stage").hold(&hold).boxed(),
        ))
        .await;
    hold.started().await;

    let first = harness.engine.cancel(id, gui()).await.unwrap();
    assert_eq!(first.disposition, CancelDisposition::Requested);
    // The hold gives way to the cancel, so the second one may already find
    // the operation final; either answer changes nothing.
    let second = harness.engine.cancel(id, gui()).await.unwrap();
    assert!(
        matches!(
            second.disposition,
            CancelDisposition::Requested | CancelDisposition::AlreadyFinal
        ),
        "{:?}",
        second.disposition
    );
    hold.release();
    wait_state(&harness.engine, id, OperationState::Cancelled).await;
    let found = wait_rows(&harness, id, 2).await;
    assert_eq!(
        actions(&found),
        [
            (
                AuditAction::OPERATION_CANCEL.to_string(),
                AuditResult::Succeeded
            ),
            finished(AuditResult::Cancelled),
        ]
    );
    assert_eq!(found[0].safe_details.get("disposition"), Some("requested"));
    assert_eq!(found[1].safe_details.get("state"), Some("cancelled"));
}

// covers: AC-1, AC-3 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancel_past_the_point_of_no_return_continues_and_says_so_in_the_finished_row() {
    let harness = Harness::new().await;
    let committing = HoldHandle::new();
    let id = harness
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
    let outcome = harness.engine.cancel(id, gui()).await.unwrap();
    assert_eq!(
        outcome.disposition,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    harness.engine.cancel(id, gui()).await.unwrap();
    committing.release();
    wait_state(&harness.engine, id, OperationState::Succeeded).await;
    let found = wait_rows(&harness, id, 2).await;
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(
        found[0].safe_details.get("disposition"),
        Some("continuing_past_point_of_no_return")
    );
    assert_eq!(found[1].result, AuditResult::Succeeded);
    assert_eq!(found[1].safe_details.get("cancelRequested"), Some("true"));
}

// covers: AC-1, AC-3 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn needs_recovery_writes_its_row_and_abandon_writes_the_senders_row_before_finished() {
    let harness = Harness::new().await;
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .creates_instance_tree()
                .step("swap")
                .irreversible()
                .creates_instance_tree()
                .step("verify")
                .fail("manager.verify_failed", false)
                .boxed(),
        ))
        .await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;
    let found = rows(&harness, id);
    assert_eq!(
        actions(&found),
        [(
            AuditAction::OPERATION_NEEDS_RECOVERY.to_string(),
            AuditResult::NeedsRecovery
        )]
    );
    assert_eq!(found[0].safe_details.get("cause"), Some("step_failed"));
    assert_eq!(found[0].safe_details.get("hint"), Some("manual"));
    assert_eq!(
        found[0].safe_details.get("errorCode"),
        Some("manager.verify_failed")
    );

    assert!(
        harness
            .engine
            .abandon(id, stuck.view.operation.revision().next(), gui())
            .await
            .is_err()
    );
    assert_eq!(
        rows(&harness, id).len(),
        1,
        "a refused abandon writes nothing"
    );
    let abandoned = harness
        .engine
        .abandon(id, stuck.view.operation.revision(), gui())
        .await
        .unwrap();
    let found = rows(&harness, id);
    assert_eq!(
        actions(&found),
        [
            (
                AuditAction::OPERATION_NEEDS_RECOVERY.to_string(),
                AuditResult::NeedsRecovery
            ),
            (
                AuditAction::OPERATION_ABANDON.to_string(),
                AuditResult::Succeeded
            ),
            finished(AuditResult::Failed),
        ]
    );
    assert_eq!(
        found[1].safe_details.get("leftovers"),
        Some(abandoned.leftovers.len().to_string().as_str())
    );
    assert_eq!(found[1].client.as_ref().map(|c| c.as_str()), Some("gui"));
    assert_eq!(found[2].safe_details.get("reason"), Some("abandoned"));
    assert_eq!(found[2].actor, AuditActor::Daemon);
}

// covers: AC-3 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resume_writes_one_row_and_a_refused_resume_writes_none() {
    let harness = Harness::new().await;
    harness.engine.register_factory(
        OperationKind::InstallRuntime,
        Arc::new(ScriptedFactory::always(
            ScriptedWork::new()
                .step("fetch")
                .irreversible()
                .step("verify")
                .repeatable(),
        )),
    );
    let mut begin = request(
        OperationKind::InstallRuntime,
        OperationResource::Runtime(RuntimeId::generate()),
        ScriptedWork::new()
            .step("fetch")
            .irreversible()
            .step("verify")
            .repeatable()
            .fail("runtime.checksum_mismatch", true)
            .boxed(),
    );
    begin.creates_resource = true;
    let id = harness.begin(begin).await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;
    assert_eq!(rows(&harness, id).len(), 1);
    assert_eq!(
        rows(&harness, id)[0].safe_details.get("hint"),
        Some("resume")
    );
    assert!(matches!(
        rows(&harness, id)[0].resource,
        Some(AuditResource::Runtime(_))
    ));

    harness
        .engine
        .resume(id, Some(stuck.view.operation.revision()), gui())
        .await
        .unwrap();
    wait_state(&harness.engine, id, OperationState::Succeeded).await;
    let found = wait_rows(&harness, id, 3).await;
    assert_eq!(
        actions(&found),
        [
            (
                AuditAction::OPERATION_NEEDS_RECOVERY.to_string(),
                AuditResult::NeedsRecovery
            ),
            (
                AuditAction::OPERATION_RESUME.to_string(),
                AuditResult::Succeeded
            ),
            finished(AuditResult::Succeeded),
        ]
    );
    assert_eq!(found[1].client.as_ref().map(|c| c.as_str()), Some("gui"));
    assert!(harness.engine.resume(id, None, gui()).await.is_err());
    assert_eq!(
        rows(&harness, id).len(),
        3,
        "a refused resume writes nothing"
    );
}

// covers: AC-1 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_no_op_answer_that_writes_records_one_succeeded_row() {
    let harness = Harness::new().await;
    let target = instance(&harness);
    let view = record_no_op(
        &harness,
        target,
        request_origin("instance.stop", "noop-1", 7),
    )
    .await;
    let id = view.operation.id();
    let found = rows(&harness, id);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].result, AuditResult::Succeeded);
    assert_eq!(found[0].instance, Some(target));
    assert_eq!(found[0].client.as_ref().map(|c| c.as_str()), Some("cli"));
}

/// A finalize that makes the first final commit fail once, as a full disk
/// would, so the engine commits the final batch again.
fn failing_once(registry: Arc<stackly_testkit::FakeRegistry>) -> stackly_core::operation::Finalize {
    let armed = Arc::new(AtomicBool::new(false));
    Box::new(move |_: &dyn Registry| {
        if !armed.swap(true, Ordering::SeqCst) {
            registry.fail_next_commit(RegistryError::NoSpace);
        }
        Ok(FinalBatch::default())
    })
}

/// A work on an instance that sets its audit note, then succeeds with a
/// finalize that fails its first commit, or fails with a message that holds
/// the learned value.
struct Noted {
    fail: bool,
    registry: Arc<stackly_testkit::FakeRegistry>,
}

impl OperationWork for Noted {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.set_audit_note(
                AuditNote::new(Some(ConfigRevisionNumber::FIRST)).with("ports", "main 5432"),
            );
            ctx.begin_step(StepSpec::new("render".parse().expect("a stage name")))
                .await?;
            // A later note replaces the first one.
            ctx.set_audit_note(
                AuditNote::new(Some(
                    ConfigRevisionNumber::new(4).expect("a revision number"),
                ))
                .with("ports", "main 5433")
                .with("settings", format!("password={LEAKED}")),
            );
            if self.fail {
                return Err(WorkError::Failed(TestError::boxed(
                    "provider.init_failed",
                    false,
                )));
            }
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: Some(failing_once(self.registry)),
            })
        })
    }
}

// covers: AC-1, AC-4 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_work_note_reaches_the_row_redacted_and_a_failed_final_commit_writes_it_once() {
    let harness = Harness::new().await;
    harness
        .redactor
        .learn(&SecretValue::new(LEAKED.to_owned()).expect("a valid fixture value"));
    let target = instance(&harness);
    let id = harness
        .begin(request(
            OperationKind::ApplyConfig,
            OperationResource::Instance(target),
            Box::new(Noted {
                fail: false,
                registry: Arc::clone(&harness.registry),
            }),
        ))
        .await;
    wait_state(&harness.engine, id, OperationState::Succeeded).await;
    let found = rows(&harness, id);
    assert_eq!(
        found.len(),
        1,
        "the failed final commit left no row: {found:?}"
    );
    let row = &found[0];
    assert_eq!(row.config_revision.map(ConfigRevisionNumber::get), Some(4));
    assert_eq!(row.safe_details.get("ports"), Some("main 5433"));
    assert!(
        !format!("{row:?}").contains(&LEAKED[..10]),
        "the row holds the learned value: {row:?}"
    );

    let failed = harness
        .begin(request(
            OperationKind::ApplyConfig,
            OperationResource::Instance(target),
            Box::new(Noted {
                fail: true,
                registry: Arc::clone(&harness.registry),
            }),
        ))
        .await;
    wait_state(&harness.engine, failed, OperationState::Failed).await;
    let found = rows(&harness, failed);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].safe_details.get("ports"), Some("main 5433"));
    assert_eq!(
        found[0].config_revision.map(ConfigRevisionNumber::get),
        Some(4),
        "a failure's row carries the note too"
    );
    assert!(!format!("{:?}", found[0]).contains(&LEAKED[..10]));
}

// covers: AC-1 (spec 0035)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_and_a_restart_close_operations_with_their_rows() {
    let harness = Harness::new().await;
    let target = instance(&harness);
    let spawned = HoldHandle::new();
    let start = harness
        .begin(request(
            OperationKind::StartInstance,
            OperationResource::Instance(target),
            ScriptedWork::new()
                .step("spawn")
                .irreversible()
                .hold(&spawned)
                .boxed(),
        ))
        .await;
    spawned.started().await;
    let halted = HoldHandle::new();
    let update = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .hold(&halted)
                .check_cancel_every(1)
                .boxed(),
        ))
        .await;
    halted.started().await;

    let shutdown = harness.engine.shutdown(Duration::from_millis(500));
    let release = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        halted.release();
    };
    let (report, ()) = tokio::join!(shutdown, release);
    assert_eq!(report.stopped, 1, "{report:?}");
    wait_until(&harness.engine, update, |detail| {
        detail.view.operation.state() == OperationState::Failed
    })
    .await;
    let found = rows(&harness, update);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].safe_details.get("reason"), Some("daemon_stopping"));

    // The start is left running; the next daemon closes it as interrupted.
    let (engine, _events) = harness.restarted(fast());
    engine.recover().await.unwrap();
    let found = rows(&harness, start);
    assert_eq!(actions(&found), [finished(AuditResult::Failed)]);
    assert_eq!(found[0].safe_details.get("reason"), Some("interrupted"));
    assert_eq!(found[0].instance, Some(target));
    assert_eq!(
        found[0].config_revision, None,
        "a note never survives a restart"
    );
}
