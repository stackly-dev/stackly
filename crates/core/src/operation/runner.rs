//! Running one queued operation (spec 0007, "Langkah", "Cancel", and
//! "Kegagalan dan error"): the start commit, the work on its own task, and
//! how it ends: finalize, failure with compensation, cancel, park, or
//! `needs_recovery`.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use jiff::Timestamp;
use stackly_domain::{
    CreatedState, ErrorCodeName, FailureCode, FailureError, OperationEvent, OperationFailure,
    OperationId, OperationState, Outcome, RecoveryCause, RegistryError, ResourceRef, StepState,
    WriteOp,
};
use tokio::time::Instant;

use super::compensation::{self, Mode};
use super::context::OperationContext;
use super::recovery::signal_shutdown;
use super::scheduler::{Job, QueueEntry, resource_revision};
use super::state::{OpHandle, OpRecord, pair};
use super::work::{FinalBatch, OperationOutcome, OperationWork, WorkError, WorkOutcome};
use super::{EngineError, Inner, Phase, codes};

/// How the work task ended.
enum Ended {
    Returned(Result<WorkOutcome, WorkError>),
    Panicked,
}

/// Why an operation fails.
pub(crate) enum Cause {
    /// A step returned its error.
    Step {
        code: ErrorCodeName,
        message: String,
        details: Vec<(stackly_domain::DetailKey, String)>,
        retryable: bool,
    },
    /// The engine failed, or a bug in the work.
    Engine(String),
    /// The work panicked.
    Panic,
}

impl Cause {
    /// The cause of an engine error a work ran into: a journal over its
    /// limits keeps its own catalog code and details (spec 0007, *Kode
    /// error*); anything else is a bug, stored as `internal.unexpected`.
    fn engine(error: &EngineError) -> Self {
        let limit = |code: ErrorCodeName, first: (&'static str, String), max: String| Self::Step {
            code,
            message: error.to_string(),
            details: vec![pair(first.0, first.1), pair("max", max)],
            retryable: false,
        };
        match error {
            EngineError::JournalFull { kind, max } => limit(
                codes::JOURNAL_FULL,
                ("kind", (*kind).to_owned()),
                max.to_string(),
            ),
            EngineError::PayloadTooLarge { field, max } => limit(
                codes::PAYLOAD_TOO_LARGE,
                ("field", (*field).to_owned()),
                max.to_string(),
            ),
            other => Self::Engine(other.to_string()),
        }
    }

    fn reason(&self) -> FailureCode {
        match self {
            Self::Panic => FailureCode::PANICKED,
            Self::Step { .. } | Self::Engine(_) => FailureCode::STEP_FAILED,
        }
    }

    fn recovery_cause(&self) -> RecoveryCause {
        match self {
            Self::Panic => RecoveryCause::Panicked,
            Self::Step { .. } | Self::Engine(_) => RecoveryCause::StepFailed,
        }
    }

    fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Step {
                retryable: true,
                ..
            }
        )
    }

    fn error(&self, inner: &Inner) -> FailureError {
        match self {
            Self::Step {
                code,
                message,
                details,
                ..
            } => inner.failure_error(code.clone(), message, details.clone()),
            Self::Engine(message) => {
                inner.failure_error(codes::INTERNAL_UNEXPECTED, message, Vec::new())
            }
            Self::Panic => inner.failure_error(
                codes::INTERNAL_UNEXPECTED,
                "the work of the operation panicked",
                Vec::new(),
            ),
        }
    }
}

/// How an operation closes after a compensation that worked.
pub(crate) enum Close {
    /// `cancelled`, from `cancel_requested`.
    Cancelled,
    /// `failed`, with why.
    Failed {
        reason: FailureCode,
        retryable: bool,
        error: FailureError,
    },
}

/// Runs a started queue entry, then gives its slot back. Every line the
/// runner logs, the work's own included, carries the `operation` span with
/// `operation_id` and `kind` (spec 0007 AC-19).
pub(crate) async fn run(inner: &Arc<Inner>, entry: QueueEntry) {
    let id = entry.id;
    match inner.handle(id) {
        Some(handle) => {
            let span = tracing::info_span!("operation", operation_id = %id, kind = %handle.kind);
            tracing::Instrument::instrument(run_entry(inner, &handle, entry), span).await;
        }
        None => inner.locks.release_operation(id),
    }
    inner.finish_slot();
}

async fn run_entry(inner: &Arc<Inner>, handle: &Arc<OpHandle>, entry: QueueEntry) {
    handle.in_runner.store(true, Ordering::SeqCst);
    // Shutdown may be counting the slots it waits for right now.
    inner.idle.notify_waiters();
    match entry.job {
        Job::Work(work) => {
            let parts = (entry.cursor, entry.keys);
            run_work(inner, handle, parts, work).await;
        }
        Job::Compensate => auto_compensate(inner, handle).await,
    }
    // Cleared before the slot goes back, so shutdown never leaves out a
    // slot that is already free.
    handle.in_runner.store(false, Ordering::SeqCst);
}

/// Puts a work back in the queue, to start after the retry interval.
fn requeue(
    inner: &Inner,
    id: OperationId,
    parts: (
        stackly_domain::OperationCursor,
        Vec<stackly_domain::LockKey>,
    ),
    work: Box<dyn OperationWork>,
) {
    inner.locks.release_operation(id);
    inner.sched().insert(QueueEntry {
        id,
        cursor: parts.0,
        keys: parts.1,
        pending: false,
        job: Job::Work(work),
        not_before: Some(Instant::now() + inner.limits.journal_retry),
    });
    inner.wake.notify_one();
}

async fn run_work(
    inner: &Arc<Inner>,
    handle: &Arc<OpHandle>,
    parts: (
        stackly_domain::OperationCursor,
        Vec<stackly_domain::LockKey>,
    ),
    work: Box<dyn OperationWork>,
) {
    let id = handle.id;
    let mut record = handle.record.lock().await;
    match record.operation.state() {
        OperationState::Queued => {
            if record.operation.started_at().is_none()
                && let Some(expected) = record.journal.expected_revision()
            {
                match resource_revision(inner, record.operation.resource()).await {
                    Ok(Some(Some(actual))) if actual != expected => {
                        let error = inner.failure_error(
                            codes::REVISION_MISMATCH,
                            &format!("the resource is at revision {actual}, not {expected}"),
                            vec![pair("expected", expected), pair("actual", actual)],
                        );
                        precondition_failed(inner, handle, &mut record, error).await;
                        return;
                    }
                    Ok(None) => {
                        let error = inner.failure_error(
                            codes::NOT_FOUND,
                            "the resource no longer exists",
                            Vec::new(),
                        );
                        precondition_failed(inner, handle, &mut record, error).await;
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(operation_id = %id, %error, "the revision check failed");
                        drop(record);
                        requeue(inner, id, parts, work);
                        return;
                    }
                    Ok(_) => {}
                }
            }
            let started = inner
                .write(handle, &mut record, |next, now| {
                    next.operation
                        .apply(OperationEvent::Start, now)
                        .map_err(|_| EngineError::Internal {
                            reason: "a queued operation refused to start",
                        })?;
                    next.journal.set_waiting_on(None);
                    Ok(Vec::new())
                })
                .await;
            if let Err(error) = started {
                tracing::warn!(operation_id = %id, %error, "the start commit failed");
                drop(record);
                requeue(inner, id, parts, work);
                return;
            }
        }
        OperationState::CancelRequested => {
            if record.journal.point_of_no_return_at().is_none() {
                compensate_then_close(inner, handle, &mut record, Close::Cancelled).await;
                return;
            }
        }
        _ => {
            drop(record);
            inner.locks.release_operation(id);
            return;
        }
    }
    if inner.phase() == Phase::ShuttingDown {
        // Shutdown began while this runner took its slot: the stop it
        // signalled missed this operation.
        signal_shutdown(handle);
    }
    let ctx = OperationContext::new(
        Arc::clone(inner),
        Arc::clone(handle),
        record.journal.trigger().clone(),
        record.journal.input().clone(),
        record.operation.resource(),
        record.journal.resumable(),
    );
    drop(record);
    let span = tracing::Span::current();
    let ended = match tokio::spawn(tracing::Instrument::instrument(work.run(ctx), span)).await {
        Ok(result) => Ended::Returned(result),
        Err(_) => Ended::Panicked,
    };
    settle(inner, handle, ended).await;
}

async fn precondition_failed(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    error: FailureError,
) {
    let id = handle.id;
    let closed = write_final(inner, handle, record, |next, now| {
        next.operation
            .apply(
                OperationEvent::Fail(OperationFailure::new(
                    FailureCode::PRECONDITION_FAILED,
                    false,
                )),
                now,
            )
            .map_err(|_| refused("fail"))?;
        next.journal.set_failure_error(Some(error.clone()));
        next.journal.set_waiting_on(None);
        Ok(vec![WriteOp::DeleteLocks { operation: id }])
    })
    .await;
    if closed {
        inner.forget(id);
    } else {
        inner.locks.release_operation(id);
    }
}

const fn refused(_event: &'static str) -> EngineError {
    EngineError::Internal {
        reason: "the operation state machine refused an engine event",
    }
}

/// Ends the open step of `next` as `state`, with `code` when it failed.
fn end_open_step(
    next: &mut OpRecord,
    state: StepState,
    code: Option<&ErrorCodeName>,
    now: Timestamp,
) {
    if let Some(step) = next
        .steps
        .iter_mut()
        .find(|step| step.state() == StepState::Running)
    {
        let ended = match (state, code) {
            (StepState::Failed, Some(code)) => step.fail(code.clone(), now),
            (StepState::Done, _) => step.finish(None, now),
            _ => step.interrupt(now),
        };
        if ended.is_err() {
            tracing::warn!("the open step could not be ended");
        }
    }
}

async fn settle(inner: &Arc<Inner>, handle: &Arc<OpHandle>, ended: Ended) {
    let mut record = handle.record.lock().await;
    match ended {
        Ended::Returned(Ok(outcome)) => succeed(inner, handle, &mut record, outcome).await,
        Ended::Returned(Err(WorkError::Abandoned)) => {
            // Shutdown left it as it is: `running` in the registry until
            // the next start's `recover` closes it as interrupted.
            tracing::info!(
                operation_id = %handle.id,
                kind = %handle.kind,
                "operation left to the next start"
            );
        }
        Ended::Returned(Err(WorkError::Cancelled))
            if handle.close_interrupted
                && record.journal.point_of_no_return_at().is_some()
                && record.journal.cancel_requested_at().is_some() =>
        {
            // Past the point of no return a kind that closes as
            // interrupted answered the cancel itself (spec 0010: stop the
            // process); nothing is compensated.
            mark_open_step(inner, handle, &mut record, StepState::Interrupted, None).await;
            close_without_compensation(inner, handle, &mut record, &Close::Cancelled).await;
        }
        Ended::Returned(Err(WorkError::Cancelled)) => {
            let close = if record.journal.cancel_requested_at().is_some() {
                Close::Cancelled
            } else {
                Close::Failed {
                    reason: FailureCode::DAEMON_STOPPING,
                    retryable: true,
                    error: inner.failure_error(
                        codes::DAEMON_STOPPING,
                        "the daemon stopped before the operation finished",
                        Vec::new(),
                    ),
                }
            };
            mark_open_step(inner, handle, &mut record, StepState::Interrupted, None).await;
            compensate_then_close(inner, handle, &mut record, close).await;
        }
        Ended::Returned(Err(WorkError::Parked)) => park(inner, handle, &mut record).await,
        Ended::Returned(Err(WorkError::Failed(error))) => {
            let cause = Cause::Step {
                code: error.code(),
                message: error.to_string(),
                details: error.details(),
                retryable: error.retryable(),
            };
            fail(inner, handle, &mut record, cause).await;
        }
        Ended::Returned(Err(WorkError::Engine(error))) => {
            fail(inner, handle, &mut record, Cause::engine(&error)).await;
        }
        Ended::Panicked => fail(inner, handle, &mut record, Cause::Panic).await,
    }
}

/// Records how the open step ended, best effort.
async fn mark_open_step(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    state: StepState,
    code: Option<&ErrorCodeName>,
) {
    if record.open_step().is_none() {
        return;
    }
    let written = inner
        .write(handle, record, |next, now| {
            end_open_step(next, state, code, now);
            Ok(Vec::new())
        })
        .await;
    if written.is_err() {
        let now = inner.now();
        let open = record.open_step().map(stackly_domain::JournalStep::seq);
        end_open_step(record, state, code, now);
        // Kept in memory; the next batch, at the latest the final one,
        // writes it.
        if let Some(seq) = open {
            record.mark_unwritten_step(seq);
        }
    }
}

/// Parks a resumable operation at shutdown: `queued` again, its keys free.
async fn park(inner: &Inner, handle: &OpHandle, record: &mut OpRecord) {
    let id = handle.id;
    if record.operation.state() == OperationState::CancelRequested {
        let past = record.journal.point_of_no_return_at().is_some();
        if !past {
            compensate_then_close(inner, handle, record, Close::Cancelled).await;
            return;
        }
    }
    let parked = write_final(inner, handle, record, |next, now| {
        end_open_step(next, StepState::Interrupted, None, now);
        next.operation
            .apply(OperationEvent::Interrupted { resumable: true }, now)
            .map_err(|_| refused("interrupted"))?;
        Ok(Vec::new())
    })
    .await;
    if !parked {
        tracing::warn!(operation_id = %id, "the parked state was not committed");
    }
    inner.locks.release_operation(id);
}

/// A failed operation: compensated before its point of no return, else
/// `needs_recovery`, or `failed` with no compensation for a kind that
/// closes as interrupted.
pub(crate) async fn fail(inner: &Inner, handle: &OpHandle, record: &mut OpRecord, cause: Cause) {
    let error = cause.error(inner);
    mark_open_step(inner, handle, record, StepState::Failed, Some(error.code())).await;
    tracing::info!(
        operation_id = %handle.id,
        kind = %handle.kind,
        code = %error.code(),
        "operation work failed"
    );
    if record.journal.point_of_no_return_at().is_none() {
        let close = if record.journal.cancel_requested_at().is_some() {
            Close::Cancelled
        } else {
            Close::Failed {
                reason: cause.reason(),
                retryable: cause.retryable(),
                error,
            }
        };
        compensate_then_close(inner, handle, record, close).await;
    } else if handle.close_interrupted {
        let close = Close::Failed {
            reason: cause.reason(),
            retryable: cause.retryable(),
            error,
        };
        close_without_compensation(inner, handle, record, &close).await;
    } else {
        enter_needs_recovery(inner, handle, record, cause.recovery_cause(), error, false).await;
    }
}

/// Compensates, then closes as `close`, or enters `needs_recovery` with
/// hint `abandon` when a record failed.
pub(crate) async fn compensate_then_close(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    close: Close,
) {
    let claimed = handle.try_claim();
    let summary = compensation::compensate(inner, handle, record, Mode::All).await;
    if claimed {
        handle.unclaim();
    }
    if let Some((item, detail)) = summary.failed {
        let error = inner.failure_error(
            codes::COMPENSATION_FAILED,
            &format!("undoing {item} failed"),
            vec![pair("item", &item), pair("detail", &detail)],
        );
        enter_needs_recovery(
            inner,
            handle,
            record,
            RecoveryCause::CompensationFailed,
            error,
            true,
        )
        .await;
        return;
    }
    close_without_compensation(inner, handle, record, &close).await;
}

/// Commits the final state `close` names and releases the operation's
/// keys, with no compensation.
async fn close_without_compensation(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    close: &Close,
) {
    let id = handle.id;
    let closed = write_final(inner, handle, record, |next, now| {
        match close {
            Close::Cancelled => next
                .operation
                .apply(OperationEvent::Cancelled, now)
                .map_err(|_| refused("cancelled"))?,
            Close::Failed {
                reason,
                retryable,
                error,
            } => {
                next.operation
                    .apply(
                        OperationEvent::Fail(OperationFailure::new(reason.clone(), *retryable)),
                        now,
                    )
                    .map_err(|_| refused("fail"))?;
                next.journal.set_failure_error(Some(error.clone()));
                OperationState::Failed
            }
        };
        Ok(vec![WriteOp::DeleteLocks { operation: id }])
    })
    .await;
    if closed {
        inner.forget(id);
    }
}

/// Moves an operation to `needs_recovery` with its hint, cause, and error.
/// Its keys stay held; queued operations that need them are closed by the
/// dispatcher.
pub(crate) async fn enter_needs_recovery(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    cause: RecoveryCause,
    error: FailureError,
    compensation_failed: bool,
) {
    let entered = write_final(inner, handle, record, |next, now| {
        if next.operation.state() != OperationState::NeedsRecovery {
            next.operation
                .apply(OperationEvent::CompensationFailed, now)
                .map_err(|_| refused("compensation_failed"))?;
        }
        let hint = inner.hint(next, compensation_failed);
        next.operation
            .set_recovery_hint(hint, now)
            .map_err(|_| refused("set_recovery_hint"))?;
        next.journal.set_recovery_cause(Some(cause));
        next.journal.set_failure_error(Some(error.clone()));
        Ok(Vec::new())
    })
    .await;
    if !entered {
        tracing::error!(operation_id = %handle.id, "the needs_recovery state was not committed");
    }
    inner.wake.notify_one();
}

fn transient(error: &EngineError) -> bool {
    matches!(
        error,
        EngineError::Registry(
            RegistryError::NoSpace | RegistryError::Busy | RegistryError::Io { .. }
        )
    )
}

/// Commits a change that ends a phase of the operation, retrying a journal
/// write that failed every retry interval until it lands or shutdown
/// begins. Returns whether it landed.
pub(crate) async fn write_final<F>(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    change: F,
) -> bool
where
    F: Fn(&mut OpRecord, Timestamp) -> Result<Vec<WriteOp>, EngineError>,
{
    loop {
        match inner.write(handle, record, &change).await {
            Ok(()) => return true,
            Err(error) if transient(&error) && inner.phase() != Phase::ShuttingDown => {
                tracing::warn!(operation_id = %handle.id, %error, "a journal write failed; retrying");
                tokio::time::sleep(inner.limits.journal_retry).await;
            }
            Err(error) => {
                tracing::error!(operation_id = %handle.id, %error, "a journal write failed");
                return false;
            }
        }
    }
}

/// A work that returned success: cancel before the point of no return
/// still wins, intended records are a bug, and otherwise the final batch
/// commits with `succeeded`.
async fn succeed(inner: &Inner, handle: &OpHandle, record: &mut OpRecord, outcome: WorkOutcome) {
    let id = handle.id;
    if record.journal.cancel_requested_at().is_some()
        && record.journal.point_of_no_return_at().is_none()
    {
        mark_open_step(inner, handle, record, StepState::Done, None).await;
        compensate_then_close(inner, handle, record, Close::Cancelled).await;
        return;
    }
    if record
        .created
        .iter()
        .any(|created| created.state() == CreatedState::Intended)
    {
        let cause = Cause::Engine("the work succeeded with items still intended".to_owned());
        fail(inner, handle, record, cause).await;
        return;
    }
    let payload = match outcome.result.to_payload() {
        Ok(payload) => payload,
        Err(error) => {
            fail(inner, handle, record, Cause::engine(&error)).await;
            return;
        }
    };
    let mut finalize = outcome.finalize;
    let mut conflicts = 0;
    loop {
        let batch = match finalize.take() {
            None => Ok(FinalBatch::default()),
            Some(mut build) => {
                let registry = Arc::clone(&inner.deps.registry);
                let built = tokio::task::spawn_blocking(move || {
                    let batch = build(registry.as_ref());
                    (build, batch)
                })
                .await;
                match built {
                    Ok((build, batch)) => {
                        finalize = Some(build);
                        batch
                    }
                    Err(_) => {
                        fail(inner, handle, record, Cause::Panic).await;
                        return;
                    }
                }
            }
        };
        let batch = match batch {
            Ok(batch) => batch,
            Err(WorkError::Failed(error)) => {
                let cause = Cause::Step {
                    code: error.code(),
                    message: error.to_string(),
                    details: error.details(),
                    retryable: error.retryable(),
                };
                fail(inner, handle, record, cause).await;
                return;
            }
            Err(other) => {
                fail(inner, handle, record, Cause::Engine(format!("{other:?}"))).await;
                return;
            }
        };
        let generation = batch.expected_generation;
        let ops = batch.ops;
        let written = inner
            .write_at(handle, record, generation, |next, now| {
                end_open_step(next, StepState::Done, None, now);
                next.operation
                    .apply(OperationEvent::Succeed, now)
                    .map_err(|_| refused("succeed"))?;
                next.journal
                    .set_result(Some(payload.clone()))
                    .map_err(|_| EngineError::PayloadTooLarge {
                        field: "result",
                        max: super::limits::RESULT_MAX,
                    })?;
                for created in &mut next.created {
                    if created.state() == CreatedState::Created {
                        created
                            .settle(CreatedState::Kept, None, now)
                            .map_err(|_| refused("kept"))?;
                    }
                }
                let mut ops = ops.clone();
                ops.push(WriteOp::DeleteLocks { operation: id });
                Ok(ops)
            })
            .await;
        match written {
            Ok(()) => {
                inner.forget(id);
                compensation::remove_scratch(inner, id).await;
                return;
            }
            Err(EngineError::Registry(RegistryError::GenerationConflict { .. }))
                if conflicts < inner.limits.finalize_retries =>
            {
                conflicts += 1;
            }
            Err(EngineError::Registry(RegistryError::RevisionConflict {
                resource,
                expected,
                actual,
            })) if resource != ResourceRef::Operation(id) => {
                let cause = Cause::Step {
                    code: codes::REVISION_MISMATCH,
                    message: format!("{resource} is at revision {actual}, not {expected}"),
                    details: vec![pair("expected", expected), pair("actual", actual)],
                    retryable: true,
                };
                fail(inner, handle, record, cause).await;
                return;
            }
            Err(error) if transient(&error) && inner.phase() != Phase::ShuttingDown => {
                tokio::time::sleep(inner.limits.journal_retry).await;
            }
            Err(error) => {
                fail(inner, handle, record, Cause::Engine(error.to_string())).await;
                return;
            }
        }
    }
}

/// Automatic compensation of an operation the last start found
/// interrupted before its point of no return.
async fn auto_compensate(inner: &Inner, handle: &OpHandle) {
    let id = handle.id;
    if !handle.try_claim() {
        return;
    }
    let mut record = handle.record.lock().await;
    let due = record.operation.state() == OperationState::NeedsRecovery
        && record.operation.recovery_hint() == Some(stackly_domain::RecoveryHint::AutoCompensate);
    if !due {
        handle.unclaim();
        return;
    }
    let summary = compensation::compensate(inner, handle, &mut record, Mode::All).await;
    handle.unclaim();
    if let Some((item, detail)) = summary.failed {
        let error = inner.failure_error(
            codes::COMPENSATION_FAILED,
            &format!("undoing {item} failed"),
            vec![pair("item", &item), pair("detail", &detail)],
        );
        enter_needs_recovery(
            inner,
            handle,
            &mut record,
            RecoveryCause::CompensationFailed,
            error,
            true,
        )
        .await;
        return;
    }
    let interrupted = inner.failure_error(
        codes::INTERRUPTED,
        "the daemon stopped while the operation ran",
        Vec::new(),
    );
    let closed = write_final(inner, handle, &mut record, |next, now| {
        let outcome = if next.journal.cancel_requested_at().is_some() {
            Outcome::Cancelled
        } else {
            next.journal.set_failure_error(Some(interrupted.clone()));
            Outcome::Failed(OperationFailure::new(FailureCode::INTERRUPTED, true))
        };
        next.operation
            .apply(OperationEvent::Resolve(outcome), now)
            .map_err(|_| refused("resolve"))?;
        next.journal.set_recovery_cause(None);
        Ok(vec![WriteOp::DeleteLocks { operation: id }])
    })
    .await;
    if closed {
        inner.forget(id);
    }
}

/// The outcome of an abandoned operation, as its result.
pub(crate) fn abandoned_outcome(record: &OpRecord) -> OperationOutcome {
    OperationOutcome {
        resources: Vec::new(),
        partial: record
            .created
            .iter()
            .map(super::work::PartialItem::of_record)
            .collect(),
    }
}
