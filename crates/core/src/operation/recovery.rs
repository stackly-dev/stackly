//! Recovery at start, resume, abandon, and shutdown (spec 0007, "Start
//! daemon dan pemulihan" and "Shutdown").

use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{
    FailureCode, JournalView, OperationEvent, OperationFailure, OperationId, OperationState,
    Outcome, RecoveryCause, RecoveryHint, ResourceRevision, StepState, WriteOp,
};
use tokio::time::Instant;

use super::compensation::{self, Mode};
use super::runner::{abandoned_outcome, write_final};
use super::scheduler::{Job, QueueEntry, close_for_recovery, recovery_holder};
use super::state::{OpHandle, OpRecord, pair};
use super::work::OperationWork;
use super::{
    AbandonOutcome, EngineError, Inner, LockHolder, NotAbandonableReason, NotResumableReason,
    OperationView, Phase, RecoveryReport, ShutdownReport, codes, lock,
};
use crate::ownership::remove_owned_tree;

fn journal_view(record: &OpRecord) -> JournalView {
    JournalView::new(
        record.journal.clone(),
        record.steps.clone(),
        record.created.clone(),
        record.locks.clone(),
    )
}

fn refused() -> EngineError {
    EngineError::Internal {
        reason: "the operation state machine refused a recovery event",
    }
}

/// Rebuilds the work of an operation through its kind's factory.
fn rebuild(inner: &Inner, record: &OpRecord) -> Option<Box<dyn OperationWork>> {
    let factory = inner.factory(record.operation.kind())?;
    match factory.rebuild(&record.operation, &journal_view(record)) {
        Ok(work) => Some(work),
        Err(error) => {
            tracing::warn!(
                operation_id = %record.operation.id(),
                %error,
                "the work of an operation could not be rebuilt"
            );
            None
        }
    }
}

fn enqueue(inner: &Inner, handle: &OpHandle, keys: Vec<stackly_domain::LockKey>, job: Job) {
    inner.sched().insert(QueueEntry {
        id: handle.id,
        cursor: handle.cursor,
        keys,
        pending: false,
        job,
        not_before: None,
    });
}

/// Takes the keys of an operation in `needs_recovery`.
fn hold_keys(inner: &Inner, record: &OpRecord) {
    let holder = LockHolder::Operation {
        id: record.operation.id(),
        state: OperationState::NeedsRecovery,
    };
    if let Err(busy) = inner.locks.try_take(&record.locks, holder) {
        tracing::warn!(
            operation_id = %record.operation.id(),
            key = %busy.key,
            "a key of an operation in needs_recovery is held twice"
        );
    }
}

fn interrupted_error(inner: &Inner) -> stackly_domain::FailureError {
    inner.failure_error(
        codes::INTERRUPTED,
        "the daemon stopped while the operation ran",
        Vec::new(),
    )
}

/// Marks every running step interrupted.
fn interrupt_steps(record: &mut OpRecord, now: jiff::Timestamp) {
    for step in &mut record.steps {
        if step.state() == StepState::Running && step.interrupt(now).is_err() {
            tracing::warn!("a running step could not be marked interrupted");
        }
    }
}

pub(crate) async fn recover(inner: &Arc<Inner>) -> Result<RecoveryReport, EngineError> {
    {
        let mut phase = lock(&inner.phase);
        if *phase != Phase::Created {
            return Err(EngineError::InvalidUse {
                reason: "recover runs once, before start",
            });
        }
        *phase = Phase::Recovering;
    }
    let unfinished = match inner
        .read(|registry| registry.unfinished_operations().map(|read| read.value))
        .await
    {
        Ok(unfinished) => unfinished,
        Err(error) => {
            inner.set_phase(Phase::Created);
            return Err(error);
        }
    };
    let mut report = RecoveryReport::default();
    let mut interrupted = 0;
    for (operation, view) in unfinished {
        let record = OpRecord::from_view(operation, view);
        let handle = OpHandle::new(record);
        inner.ops().insert(handle.id, Arc::clone(&handle));
        let mut record = handle.record.lock().await;
        match record.operation.state() {
            OperationState::Running => {
                interrupted += 1;
                recover_running(inner, &handle, &mut record, &mut report).await;
            }
            OperationState::CancelRequested => {
                interrupted += 1;
                recover_cancel_requested(inner, &handle, &mut record, &mut report).await;
            }
            OperationState::NeedsRecovery => {
                hold_keys(inner, &record);
                if record.operation.recovery_hint() == Some(RecoveryHint::AutoCompensate) {
                    enqueue(inner, &handle, record.locks.clone(), Job::Compensate);
                    report.auto_compensating += 1;
                } else {
                    report.needs_recovery += 1;
                }
            }
            OperationState::Queued => {
                recover_queued(inner, &handle, &mut record, &mut report).await;
            }
            OperationState::Succeeded | OperationState::Failed | OperationState::Cancelled => {
                drop(record);
                inner.ops().remove(&handle.id);
            }
        }
    }
    let waiting: Vec<(OperationId, Vec<stackly_domain::LockKey>)> = inner
        .sched()
        .queue
        .iter()
        .filter(|entry| matches!(entry.job, Job::Work(_)))
        .map(|entry| (entry.id, entry.keys.clone()))
        .collect();
    for (id, keys) in waiting {
        if let Some((holder, key)) = recovery_holder(inner, &keys)
            && close_for_recovery(inner, id, holder, &key).await
        {
            inner.sched().remove(id);
            report.closed_for_recovery += 1;
        }
    }
    clean_scratch(inner, &mut report).await;
    {
        let mut counters = inner.counters();
        counters.interrupted_at_start = interrupted;
        counters.resumed_at_start = report.resumed;
        counters.auto_compensated_at_start = report.auto_compensating;
    }
    tracing::info!(
        resumed = report.resumed,
        auto_compensating = report.auto_compensating,
        needs_recovery = report.needs_recovery,
        queued = report.queued,
        rebuild_failed = report.rebuild_failed,
        closed_for_recovery = report.closed_for_recovery,
        scratch_removed = report.scratch_removed,
        scratch_leftover = report.scratch_leftover,
        "operation recovery finished"
    );
    inner.set_phase(Phase::Ready);
    Ok(report)
}

async fn recover_running(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    report: &mut RecoveryReport,
) {
    let stopped_repeatable = record.open_step().is_none_or(|step| step.repeatable());
    let failed_step = record
        .steps
        .iter()
        .any(|step| step.state() == StepState::Failed);
    let can_resume = record.journal.resumable()
        && record.journal.compensation_started_at().is_none()
        && !failed_step
        && stopped_repeatable;
    let work = if can_resume {
        rebuild(inner, record)
    } else {
        None
    };
    if let Some(work) = work {
        let parked = write_final(inner, handle, record, |next, now| {
            interrupt_steps(next, now);
            next.operation
                .apply(OperationEvent::Interrupted { resumable: true }, now)
                .map_err(|_| refused())?;
            Ok(Vec::new())
        })
        .await;
        if parked {
            enqueue(inner, handle, record.locks.clone(), Job::Work(work));
            report.resumed += 1;
        }
        return;
    }
    let error = interrupted_error(inner);
    let entered = write_final(inner, handle, record, |next, now| {
        interrupt_steps(next, now);
        next.operation
            .apply(OperationEvent::Interrupted { resumable: false }, now)
            .map_err(|_| refused())?;
        let mut hint = inner.hint(next, false);
        if hint == RecoveryHint::Resume && !can_resume {
            hint = RecoveryHint::Manual;
        }
        next.operation
            .set_recovery_hint(hint, now)
            .map_err(|_| refused())?;
        next.journal
            .set_recovery_cause(Some(RecoveryCause::Interrupted));
        next.journal.set_failure_error(Some(error.clone()));
        Ok(Vec::new())
    })
    .await;
    if entered {
        settle_needs_recovery(inner, handle, record, report);
    }
}

/// Holds the keys of an operation that just entered `needs_recovery`, and
/// schedules its automatic compensation when its hint says so.
fn settle_needs_recovery(
    inner: &Inner,
    handle: &OpHandle,
    record: &OpRecord,
    report: &mut RecoveryReport,
) {
    hold_keys(inner, record);
    if record.operation.recovery_hint() == Some(RecoveryHint::AutoCompensate) {
        enqueue(inner, handle, record.locks.clone(), Job::Compensate);
        report.auto_compensating += 1;
    } else {
        report.needs_recovery += 1;
    }
}

async fn recover_cancel_requested(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    report: &mut RecoveryReport,
) {
    let error = interrupted_error(inner);
    let entered = write_final(inner, handle, record, |next, now| {
        interrupt_steps(next, now);
        next.operation
            .apply(OperationEvent::Interrupted { resumable: false }, now)
            .map_err(|_| refused())?;
        let hint = inner.hint(next, false);
        next.operation
            .set_recovery_hint(hint, now)
            .map_err(|_| refused())?;
        next.journal
            .set_recovery_cause(Some(RecoveryCause::Interrupted));
        next.journal.set_failure_error(Some(error.clone()));
        Ok(Vec::new())
    })
    .await;
    if entered {
        settle_needs_recovery(inner, handle, record, report);
    }
}

async fn recover_queued(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    report: &mut RecoveryReport,
) {
    if let Some(work) = rebuild(inner, record) {
        enqueue(inner, handle, record.locks.clone(), Job::Work(work));
        report.queued += 1;
        return;
    }
    report.rebuild_failed += 1;
    let id = handle.id;
    let error = interrupted_error(inner);
    if record.operation.started_at().is_none() {
        let closed = write_final(inner, handle, record, |next, now| {
            next.operation
                .apply(
                    OperationEvent::Fail(OperationFailure::new(FailureCode::INTERRUPTED, true)),
                    now,
                )
                .map_err(|_| refused())?;
            next.journal.set_failure_error(Some(error.clone()));
            Ok(vec![WriteOp::DeleteLocks { operation: id }])
        })
        .await;
        if closed {
            inner.ops().remove(&id);
        }
        return;
    }
    let entered = write_final(inner, handle, record, |next, now| {
        next.operation
            .apply(OperationEvent::Start, now)
            .map_err(|_| refused())?;
        next.operation
            .apply(OperationEvent::Interrupted { resumable: false }, now)
            .map_err(|_| refused())?;
        next.operation
            .set_recovery_hint(RecoveryHint::Manual, now)
            .map_err(|_| refused())?;
        next.journal
            .set_recovery_cause(Some(RecoveryCause::Interrupted));
        next.journal.set_failure_error(Some(error.clone()));
        Ok(Vec::new())
    })
    .await;
    if entered {
        hold_keys(inner, record);
        report.needs_recovery += 1;
    }
}

/// Deletes the scratch folder of every operation that is final and either
/// succeeded or never passed its point of no return; leaves the rest.
async fn clean_scratch(inner: &Inner, report: &mut RecoveryReport) {
    let platform = Arc::clone(&inner.deps.platform);
    let dir = inner.deps.layout.layout().operations_dir();
    let listed = {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || platform.fs().read_dir(&dir)).await
    };
    let entries = match listed {
        Ok(Ok(entries)) => entries,
        Ok(Err(error)) => {
            tracing::warn!(%error, "the operations folder could not be listed");
            return;
        }
        Err(_) => return,
    };
    for entry in entries {
        let name = entry.name.to_string_lossy().into_owned();
        let Ok(id) = name.parse::<OperationId>() else {
            tracing::warn!(entry = %name, "scratch_leftover");
            report.scratch_leftover += 1;
            continue;
        };
        if inner.handle(id).is_some() {
            continue;
        }
        let found = inner
            .read(move |registry| {
                let operation = registry.operation(id)?.value;
                let journals = registry.operation_journals(&[id])?.value;
                Ok(operation.zip(journals.into_iter().next()))
            })
            .await;
        let removable = matches!(
            &found,
            Ok(Some((operation, journal)))
                if operation.state().is_final()
                    && (operation.state() == OperationState::Succeeded
                        || journal.point_of_no_return_at().is_none())
        );
        if !removable {
            tracing::warn!(operation_id = %id, "scratch_leftover");
            report.scratch_leftover += 1;
            continue;
        }
        let platform = Arc::clone(&inner.deps.platform);
        let layout = Arc::clone(&inner.deps.layout);
        let path = dir.join(&name);
        let removed =
            tokio::task::spawn_blocking(move || remove_owned_tree(&platform, &layout, &path)).await;
        if matches!(removed, Ok(Ok(_))) {
            report.scratch_removed += 1;
        } else {
            report.scratch_leftover += 1;
        }
    }
}

fn not_ready(inner: &Inner) -> Option<EngineError> {
    match inner.phase() {
        Phase::Created | Phase::Recovering => Some(EngineError::NotReady),
        Phase::ShuttingDown => Some(EngineError::ShuttingDown),
        Phase::Ready | Phase::Started => None,
    }
}

/// The state of an operation the engine does not hold in memory.
async fn stored_state(inner: &Inner, id: OperationId) -> Result<OperationState, EngineError> {
    inner
        .read(move |registry| registry.operation(id).map(|read| read.value))
        .await?
        .map(|operation| operation.state())
        .ok_or(EngineError::NotFound(id))
}

pub(crate) async fn resume(
    inner: &Arc<Inner>,
    id: OperationId,
    expected: Option<ResourceRevision>,
) -> Result<OperationView, EngineError> {
    if let Some(error) = not_ready(inner) {
        return Err(error);
    }
    let not_resumable = |reason| EngineError::NotResumable {
        operation: id,
        reason,
    };
    let Some(handle) = inner.handle(id) else {
        stored_state(inner, id).await?;
        return Err(not_resumable(NotResumableReason::WrongState));
    };
    let mut record = handle.record.lock().await;
    if record.operation.state() != OperationState::NeedsRecovery {
        return Err(not_resumable(NotResumableReason::WrongState));
    }
    if handle.is_compensating()
        || record.operation.recovery_hint() == Some(RecoveryHint::AutoCompensate)
    {
        return Err(not_resumable(NotResumableReason::AutoCompensating));
    }
    if let Some(expected) = expected
        && expected != record.operation.revision()
    {
        return Err(EngineError::RevisionMismatch {
            expected,
            actual: record.operation.revision(),
        });
    }
    if record.operation.recovery_hint() != Some(RecoveryHint::Resume) {
        let stopped_repeatable = record
            .steps
            .iter()
            .rev()
            .find(|step| matches!(step.state(), StepState::Failed | StepState::Interrupted))
            .is_none_or(|step| step.repeatable());
        let reason = if !record.journal.resumable() {
            NotResumableReason::KindNotResumable
        } else if record.journal.compensation_started_at().is_some() {
            NotResumableReason::CompensationFailed
        } else if inner.factory(record.operation.kind()).is_none() {
            NotResumableReason::NoFactory
        } else if !stopped_repeatable {
            NotResumableReason::StepNotRepeatable
        } else {
            NotResumableReason::WrongState
        };
        return Err(not_resumable(reason));
    }
    let work = rebuild(inner, &record).ok_or(not_resumable(NotResumableReason::NoFactory))?;
    inner
        .write(&handle, &mut record, |next, now| {
            next.operation
                .apply(OperationEvent::Resume, now)
                .map_err(|_| refused())?;
            next.journal.set_recovery_cause(None);
            next.journal.set_failure_error(None);
            Ok(Vec::new())
        })
        .await?;
    inner.locks.release_operation(id);
    enqueue(inner, &handle, record.locks.clone(), Job::Work(work));
    inner.wake.notify_one();
    Ok(inner.view(&record, Some(&handle)))
}

pub(crate) async fn abandon(
    inner: &Arc<Inner>,
    id: OperationId,
    expected: ResourceRevision,
) -> Result<AbandonOutcome, EngineError> {
    if let Some(error) = not_ready(inner) {
        return Err(error);
    }
    let Some(handle) = inner.handle(id) else {
        let state = stored_state(inner, id).await?;
        return Err(EngineError::NotAbandonable {
            operation: id,
            state,
            reason: NotAbandonableReason::WrongState,
        });
    };
    let mut record = handle.record.lock().await;
    let state = record.operation.state();
    if state != OperationState::NeedsRecovery {
        return Err(EngineError::NotAbandonable {
            operation: id,
            state,
            reason: NotAbandonableReason::WrongState,
        });
    }
    if expected != record.operation.revision() {
        return Err(EngineError::RevisionMismatch {
            expected,
            actual: record.operation.revision(),
        });
    }
    if !handle.try_claim() {
        return Err(EngineError::NotAbandonable {
            operation: id,
            state,
            reason: NotAbandonableReason::AutoCompensating,
        });
    }
    compensation::compensate(inner, &handle, &mut record, Mode::Reversible).await;
    handle.unclaim();
    let leftovers: Vec<_> = record
        .created
        .iter()
        .filter(|created| created.state() != stackly_domain::CreatedState::Compensated)
        .cloned()
        .collect();
    let payload = abandoned_outcome(&record).to_payload()?;
    let error = inner.failure_error(
        codes::ABANDONED,
        "the operation was abandoned",
        vec![pair("leftovers", leftovers.len())],
    );
    inner
        .write(&handle, &mut record, |next, now| {
            next.operation
                .apply(
                    OperationEvent::Resolve(Outcome::Failed(OperationFailure::new(
                        FailureCode::ABANDONED,
                        false,
                    ))),
                    now,
                )
                .map_err(|_| refused())?;
            next.journal
                .set_result(Some(payload))
                .map_err(|_| EngineError::PayloadTooLarge {
                    field: "result",
                    max: super::limits::RESULT_MAX,
                })?;
            next.journal.set_failure_error(Some(error));
            next.journal.set_recovery_cause(None);
            Ok(vec![WriteOp::DeleteLocks { operation: id }])
        })
        .await?;
    let view = inner.view(&record, Some(&handle));
    drop(record);
    inner.forget(id);
    inner.wake.notify_one();
    Ok(AbandonOutcome { view, leftovers })
}

pub(crate) async fn shutdown(inner: &Arc<Inner>, grace: Duration) -> ShutdownReport {
    inner.set_phase(Phase::ShuttingDown);
    inner.sched().open = false;
    inner.wake.notify_one();
    let mut report = ShutdownReport::default();
    let running: Vec<Arc<OpHandle>> = inner
        .ops()
        .values()
        .filter(|handle| handle.state() == OperationState::Running)
        .cloned()
        .collect();
    for handle in running {
        if handle.resumable {
            handle.signal(|stop| stop.park = true);
            report.parked += 1;
        } else {
            handle.signal(|stop| stop.halt = true);
            report.stopped += 1;
        }
    }
    let deadline = Instant::now() + grace;
    loop {
        let idle = inner.idle.notified();
        tokio::pin!(idle);
        idle.as_mut().enable();
        if inner.sched().running == 0 {
            break;
        }
        tokio::select! {
            () = &mut idle => {}
            () = tokio::time::sleep_until(deadline) => break,
        }
    }
    report.left_running = inner.sched().running;
    tracing::info!(
        parked = report.parked,
        stopped = report.stopped,
        left_running = report.left_running,
        "operation engine stopped"
    );
    report
}
