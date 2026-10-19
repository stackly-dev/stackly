//! The queue and the dispatcher (spec 0007, "Lock dan antrian"): `begin`
//! with its check order and idempotency, `cancel`, and the dispatcher that
//! starts queued operations oldest first once every lock key and a running
//! slot are free.

use std::sync::Arc;

use stackly_domain::{
    FailureCode, IdempotencyKey, LockKey, NewJournal, NewOperation, Operation, OperationCursor,
    OperationEvent, OperationFailure, OperationId, OperationJournal, OperationResource,
    OperationState, OperationTrigger, RegistryError, ResourceRevision, WriteBatch, WriteOp,
};
use tokio::time::Instant;

use super::limits::INPUT_MAX;
use super::state::{OpHandle, OpRecord, pair};
use super::work::OperationWork;
use super::{
    BeginError, BeginOutcome, BeginRequest, BusyPolicy, CancelDisposition, CancelOutcome,
    EngineError, Inner, LockHolder, Origin, Phase, codes, lock, runner,
};

/// What a queued entry runs once it starts.
pub(crate) enum Job {
    /// The work of a new, resumed, or rebuilt operation.
    Work(Box<dyn OperationWork>),
    /// Automatic compensation of an operation in `needs_recovery`.
    Compensate,
}

/// One queued operation.
pub(crate) struct QueueEntry {
    pub(crate) id: OperationId,
    pub(crate) cursor: OperationCursor,
    pub(crate) keys: Vec<LockKey>,
    /// Reserved by a `begin` whose first commit has not landed yet: it
    /// never starts, but its keys count as waited for.
    pub(crate) pending: bool,
    pub(crate) job: Job,
    /// Do not start before this time, after a failed start commit.
    pub(crate) not_before: Option<Instant>,
}

/// The queue and the running slots.
#[derive(Default)]
pub(crate) struct Scheduler {
    pub(crate) queue: Vec<QueueEntry>,
    pub(crate) running: u32,
    pub(crate) open: bool,
}

impl Scheduler {
    /// Queued operations that wait to run work, reserved ones included.
    pub(crate) fn queued_work(&self) -> u32 {
        let count = self
            .queue
            .iter()
            .filter(|entry| matches!(entry.job, Job::Work(_)))
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    /// Inserts an entry in `(created_at, id)` order.
    pub(crate) fn insert(&mut self, entry: QueueEntry) {
        let at = self
            .queue
            .iter()
            .position(|other| other.cursor > entry.cursor)
            .unwrap_or(self.queue.len());
        self.queue.insert(at, entry);
    }

    pub(crate) fn remove(&mut self, id: OperationId) -> Option<QueueEntry> {
        let at = self.queue.iter().position(|entry| entry.id == id)?;
        Some(self.queue.remove(at))
    }

    /// The oldest queued operation that waits for one of `keys`.
    fn waiter(&self, keys: &[LockKey]) -> Option<OperationId> {
        self.queue
            .iter()
            .find(|entry| entry.keys.iter().any(|key| keys.contains(key)))
            .map(|entry| entry.id)
    }
}

/// The holder in `needs_recovery` of one of `keys`, if any.
pub(crate) fn recovery_holder(inner: &Inner, keys: &[LockKey]) -> Option<(OperationId, LockKey)> {
    keys.iter().find_map(|key| match inner.locks.holder(key) {
        Some(LockHolder::Operation {
            id,
            state: OperationState::NeedsRecovery,
        }) => Some((id, key.clone())),
        _ => None,
    })
}

/// The lock keys of a request: the resource's own key first, then the
/// extra keys, without repeats.
fn keys_of(resource: OperationResource, extra: &[LockKey]) -> Vec<LockKey> {
    let mut keys = vec![LockKey::for_resource(resource)];
    for key in extra {
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    keys
}

/// The current revision of a resource, `None` when it does not exist, and
/// `Ok(None)` for the manager, which has none.
pub(crate) async fn resource_revision(
    inner: &Inner,
    resource: OperationResource,
) -> Result<Option<Option<ResourceRevision>>, EngineError> {
    inner
        .read(move |registry| {
            Ok(match resource {
                OperationResource::Runtime(id) => registry
                    .runtime(id)?
                    .value
                    .map(|runtime| Some(runtime.revision())),
                OperationResource::Instance(id) => registry
                    .instance(id)?
                    .value
                    .map(|instance| Some(instance.revision())),
                OperationResource::Environment(id) => registry
                    .environment(id)?
                    .value
                    .map(|environment| Some(environment.revision())),
                OperationResource::Manager => Some(None),
            })
        })
        .await
}

fn begin_registry_error(error: EngineError) -> BeginError {
    match error {
        EngineError::Registry(error) => BeginError::Registry(error),
        other => BeginError::Registry(RegistryError::Io {
            detail: other.to_string(),
        }),
    }
}

/// Looks the idempotency key up: the same request replays its operation,
/// another request with the key is refused.
async fn lookup(
    inner: &Inner,
    origin: &super::RequestOrigin,
) -> Result<Option<BeginOutcome>, BeginError> {
    let key: IdempotencyKey = origin.key.clone();
    let found = inner
        .read(move |registry| registry.operation_by_idempotency_key(&key))
        .await
        .map_err(begin_registry_error)?
        .value;
    let Some((operation, journal)) = found else {
        return Ok(None);
    };
    if journal.method() != Some(&origin.method)
        || journal.fingerprint() != Some(&origin.fingerprint)
    {
        return Err(BeginError::KeyReused {
            existing: operation.id(),
        });
    }
    let handle = inner.handle(operation.id());
    Ok(Some(BeginOutcome::Replayed(inner.view_of(
        operation,
        journal,
        handle.as_deref(),
    ))))
}

pub(crate) async fn begin(
    inner: &Arc<Inner>,
    request: BeginRequest,
) -> Result<BeginOutcome, BeginError> {
    match inner.phase() {
        Phase::Created | Phase::Recovering => return Err(BeginError::NotReady),
        Phase::ShuttingDown => return Err(BeginError::ShuttingDown),
        Phase::Ready | Phase::Started => {}
    }
    if request.creates_resource && request.expected_revision.is_some() {
        return Err(BeginError::InvalidRequest {
            reason: "a new resource has no revision to expect",
        });
    }
    if request.resource == OperationResource::Manager
        && (request.creates_resource || request.expected_revision.is_some())
    {
        return Err(BeginError::InvalidRequest {
            reason: "the manager is never created and has no revision",
        });
    }
    if request.input.byte_len() > INPUT_MAX {
        return Err(BeginError::PayloadTooLarge {
            field: "input",
            max: INPUT_MAX,
        });
    }
    let _gate = inner.begin_gate.lock().await;
    if let Origin::Request(origin) = &request.origin
        && let Some(outcome) = lookup(inner, origin).await?
    {
        return Ok(outcome);
    }
    if !request.creates_resource {
        let current = resource_revision(inner, request.resource)
            .await
            .map_err(begin_registry_error)?;
        match (current, request.expected_revision) {
            (None, _) => return Err(BeginError::ResourceNotFound),
            (Some(Some(actual)), Some(expected)) if actual != expected => {
                return Err(BeginError::RevisionMismatch { expected, actual });
            }
            _ => {}
        }
    }

    let keys = keys_of(request.resource, &request.extra_locks);
    let now = inner.now();
    let id = OperationId::generate();
    let (trigger, idempotency_key, request_id) = match &request.origin {
        Origin::Request(origin) => (
            OperationTrigger::user(Some(origin.client.clone()), origin.interactive),
            Some(origin.key.clone()),
            Some((origin.method.clone(), origin.fingerprint)),
        ),
        Origin::Background => (OperationTrigger::background(), None, None),
    };
    let operation = Operation::new(NewOperation {
        id,
        kind: request.kind,
        resource: request.resource,
        idempotency_key,
        created_at: now,
    });
    let journal = OperationJournal::new(NewJournal {
        operation_id: id,
        request: request_id,
        trigger,
        resumable: request.kind.resumable(),
        input: request.input,
        expected_revision: request.expected_revision,
    })
    .map_err(|_| BeginError::PayloadTooLarge {
        field: "input",
        max: INPUT_MAX,
    })?;
    {
        let mut sched = inner.sched();
        if let Some((operation, key)) = recovery_holder(inner, &keys) {
            return Err(BeginError::RecoveryRequired { operation, key });
        }
        if request.busy == BusyPolicy::Reject {
            if let Some(holder) = keys.iter().find_map(|key| inner.locks.holder(key)) {
                return Err(BeginError::Busy { holder });
            }
            if let Some(waiter) = sched.waiter(&keys) {
                return Err(BeginError::Busy {
                    holder: LockHolder::Operation {
                        id: waiter,
                        state: OperationState::Queued,
                    },
                });
            }
        }
        if sched.queued_work() >= inner.limits.max_queued {
            inner.counters().rejected_queue_full += 1;
            return Err(BeginError::QueueFull {
                max: inner.limits.max_queued,
            });
        }
        sched.insert(QueueEntry {
            id,
            cursor: OperationCursor::of(&operation),
            keys: keys.clone(),
            pending: true,
            job: Job::Work(request.work),
            not_before: None,
        });
    }

    let batch = WriteBatch::new(vec![
        WriteOp::InsertOperation(operation.clone()),
        WriteOp::InsertJournal(journal.clone()),
        WriteOp::InsertLocks {
            operation: id,
            keys: keys.clone(),
        },
    ]);
    if let Err(error) = inner.commit(batch).await {
        inner.sched().remove(id);
        return match (&error, &request.origin) {
            (
                EngineError::Registry(RegistryError::IdempotencyKeyTaken { .. }),
                Origin::Request(origin),
            ) => lookup(inner, origin)
                .await?
                .ok_or_else(|| begin_registry_error(error)),
            _ => Err(begin_registry_error(error)),
        };
    }
    let record = OpRecord {
        operation,
        journal,
        steps: Vec::new(),
        created: Vec::new(),
        locks: keys,
    };
    let handle = OpHandle::new(record.clone());
    inner.ops().insert(id, Arc::clone(&handle));
    if let Some(entry) = inner.sched().queue.iter_mut().find(|entry| entry.id == id) {
        entry.pending = false;
    }
    inner.report_created(&handle, &record);
    inner.wake.notify_one();
    Ok(BeginOutcome::Created(inner.view(&record, Some(&handle))))
}

/// The disposition of a cancel on an operation that already had one.
fn requested(record: &OpRecord) -> CancelDisposition {
    if record.journal.point_of_no_return_at().is_some() {
        CancelDisposition::ContinuingPastPointOfNoReturn
    } else {
        CancelDisposition::Requested
    }
}

pub(crate) async fn cancel(
    inner: &Arc<Inner>,
    id: OperationId,
) -> Result<CancelOutcome, EngineError> {
    if matches!(inner.phase(), Phase::Created | Phase::Recovering) {
        return Err(EngineError::NotReady);
    }
    let Some(handle) = inner.handle(id) else {
        let found = inner
            .read(move |registry| {
                let operation = registry.operation(id)?.value;
                let journal = registry.operation_journal(id)?.value;
                Ok(operation.zip(journal))
            })
            .await?;
        let (operation, view) = found.ok_or(EngineError::NotFound(id))?;
        let disposition = match operation.state() {
            OperationState::NeedsRecovery => CancelDisposition::RecoveryRequired,
            _ => CancelDisposition::AlreadyFinal,
        };
        return Ok(CancelOutcome {
            view: inner.view_of(operation, view.journal, None),
            disposition,
        });
    };
    let mut record = handle.record.lock().await;
    let state = record.operation.state();
    let disposition = match state {
        OperationState::Succeeded | OperationState::Failed | OperationState::Cancelled => {
            CancelDisposition::AlreadyFinal
        }
        OperationState::NeedsRecovery => CancelDisposition::RecoveryRequired,
        OperationState::CancelRequested => requested(&record),
        OperationState::Queued if record.operation.started_at().is_none() => {
            inner
                .write(&handle, &mut record, |next, now| {
                    next.operation
                        .apply(OperationEvent::RequestCancel, now)
                        .map_err(|_| EngineError::Internal {
                            reason: "a queued operation refused a cancel",
                        })?;
                    next.journal.mark_cancel_requested(now);
                    next.journal.set_waiting_on(None);
                    Ok(vec![WriteOp::DeleteLocks { operation: id }])
                })
                .await?;
            inner.sched().remove(id);
            inner.forget(id);
            inner.wake.notify_one();
            CancelDisposition::CancelledBeforeStart
        }
        OperationState::Queued | OperationState::Running => {
            inner
                .write(&handle, &mut record, |next, now| {
                    next.operation
                        .apply(OperationEvent::RequestCancel, now)
                        .map_err(|_| EngineError::Internal {
                            reason: "an operation refused a cancel",
                        })?;
                    next.journal.mark_cancel_requested(now);
                    Ok(Vec::new())
                })
                .await?;
            handle.signal(|stop| stop.cancel = true);
            requested(&record)
        }
    };
    Ok(CancelOutcome {
        view: inner.view(&record, Some(&handle)),
        disposition,
    })
}

impl Inner {
    /// Drops a final operation from memory.
    pub(crate) fn forget(&self, id: OperationId) {
        self.ops().remove(&id);
        self.locks.release_operation(id);
        self.idle.notify_waiters();
    }

    /// Gives a running slot back and wakes the dispatcher.
    pub(crate) fn finish_slot(&self) {
        {
            let mut sched = self.sched();
            sched.running = sched.running.saturating_sub(1);
        }
        self.wake.notify_one();
        self.idle.notify_waiters();
    }
}

/// Opens the dispatcher once.
pub(crate) fn start(inner: &Arc<Inner>) {
    {
        let mut phase = lock(&inner.phase);
        if *phase != Phase::Ready {
            return;
        }
        *phase = Phase::Started;
    }
    inner.sched().open = true;
    let inner = Arc::clone(inner);
    tokio::spawn(async move { dispatch(inner).await });
}

/// What one dispatcher pass decided.
#[derive(Default)]
struct Pass {
    starts: Vec<QueueEntry>,
    closures: Vec<(QueueEntry, OperationId, LockKey)>,
    waiting: Vec<(OperationId, Option<OperationId>)>,
    retry_at: Option<Instant>,
}

async fn dispatch(inner: Arc<Inner>) {
    loop {
        let notified = inner.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if inner.phase() == Phase::ShuttingDown {
            break;
        }
        let pass = plan(&inner);
        for entry in pass.starts {
            let inner = Arc::clone(&inner);
            tokio::spawn(async move { runner::run(&inner, entry).await });
        }
        for (entry, holder, key) in pass.closures {
            if !close_for_recovery(&inner, entry.id, holder, &key).await
                && inner.handle(entry.id).is_some()
            {
                let mut entry = entry;
                entry.not_before = Some(Instant::now() + inner.limits.journal_retry);
                inner.sched().insert(entry);
            }
        }
        write_waiting(&inner, pass.waiting).await;
        match pass.retry_at {
            Some(at) => {
                tokio::select! {
                    () = &mut notified => {}
                    () = tokio::time::sleep_until(at) => {}
                }
            }
            None => notified.await,
        }
    }
}

/// One pass over the queue, oldest first.
fn plan(inner: &Inner) -> Pass {
    let mut pass = Pass::default();
    let mut sched = inner.sched();
    if !sched.open {
        return pass;
    }
    let now = Instant::now();
    let mut older: Vec<(OperationCursor, OperationId, Vec<LockKey>)> = Vec::new();
    let mut index = 0;
    while index < sched.queue.len() {
        let entry = &sched.queue[index];
        let older_blocker = older
            .iter()
            .filter(|(_, _, keys)| keys.iter().any(|key| entry.keys.contains(key)))
            .map(|(cursor, id, _)| (*cursor, *id))
            .min();
        let compensating = matches!(entry.job, Job::Compensate);
        let delayed = entry.not_before.filter(|at| *at > now);
        if entry.pending || delayed.is_some() {
            if let Some(at) = delayed {
                pass.retry_at = Some(pass.retry_at.map_or(at, |other| other.min(at)));
            }
            older.push((entry.cursor, entry.id, entry.keys.clone()));
            index += 1;
            continue;
        }
        if !compensating && let Some((holder, key)) = recovery_holder(inner, &entry.keys) {
            let entry = sched.queue.remove(index);
            pass.closures.push((entry, holder, key));
            continue;
        }
        let mut held_blocker: Option<(OperationCursor, OperationId)> = None;
        let mut internal = false;
        if !compensating {
            for key in &entry.keys {
                match inner.locks.holder(key) {
                    Some(LockHolder::Operation { id, .. }) if id != entry.id => {
                        let cursor = inner
                            .handle(id)
                            .map_or(entry.cursor, |handle| handle.cursor);
                        let candidate = (cursor, id);
                        held_blocker = Some(held_blocker.map_or(candidate, |b| b.min(candidate)));
                    }
                    Some(LockHolder::Internal(_)) => internal = true,
                    _ => {}
                }
            }
        }
        let blocker = match (held_blocker, older_blocker) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let slot = sched.running < inner.limits.max_running;
        if blocker.is_none() && !internal && slot {
            let taken = compensating
                || inner
                    .locks
                    .try_take(
                        &entry.keys,
                        LockHolder::Operation {
                            id: entry.id,
                            state: OperationState::Queued,
                        },
                    )
                    .is_ok();
            if taken {
                let entry = sched.queue.remove(index);
                sched.running += 1;
                pass.starts.push(entry);
                continue;
            }
        }
        if !compensating {
            pass.waiting.push((entry.id, blocker.map(|(_, id)| id)));
        }
        older.push((entry.cursor, entry.id, entry.keys.clone()));
        index += 1;
    }
    pass
}

/// Commits every changed `waiting_on` of one pass in one batch.
async fn write_waiting(inner: &Inner, waiting: Vec<(OperationId, Option<OperationId>)>) {
    let mut wanted: Vec<(Arc<OpHandle>, Option<OperationId>)> = waiting
        .into_iter()
        .filter_map(|(id, want)| inner.handle(id).map(|handle| (handle, want)))
        .collect();
    wanted.sort_by_key(|(handle, _)| handle.id);
    let mut guards = Vec::new();
    for (handle, want) in &wanted {
        let guard = handle.record.lock().await;
        let queued = matches!(
            guard.operation.state(),
            OperationState::Queued | OperationState::CancelRequested
        );
        if queued && guard.journal.waiting_on() != *want {
            guards.push((handle, guard, *want));
        }
    }
    if guards.is_empty() {
        return;
    }
    let now = inner.now();
    let mut ops = Vec::new();
    let mut nexts = Vec::new();
    for (_, guard, want) in &guards {
        let mut next = (**guard).clone();
        next.journal.set_waiting_on(*want);
        next.operation.touch(now);
        ops.push(WriteOp::UpdateOperation {
            operation: next.operation.clone(),
            expected: guard.operation.revision(),
        });
        ops.push(WriteOp::UpdateJournal(next.journal.clone()));
        nexts.push(next);
    }
    match inner.commit(WriteBatch::new(ops)).await {
        Ok(_) => {
            for ((handle, guard, _), next) in guards.iter_mut().zip(nexts) {
                let before = std::mem::replace(&mut **guard, next);
                inner.report_change(handle, &before, guard);
            }
        }
        Err(error) => tracing::warn!(%error, "the waiting_on batch was not committed"),
    }
}

/// Closes a queued operation that needs a key held by an operation in
/// `needs_recovery`: `failed` with reason `precondition_failed`.
pub(crate) async fn close_for_recovery(
    inner: &Inner,
    id: OperationId,
    holder: OperationId,
    key: &LockKey,
) -> bool {
    let Some(handle) = inner.handle(id) else {
        return false;
    };
    let mut record = handle.record.lock().await;
    if record.operation.state() != OperationState::Queued {
        return false;
    }
    let error = inner.failure_error(
        codes::RECOVERY_REQUIRED,
        &format!("operation {holder} needs recovery and holds {key}"),
        vec![pair("operationId", holder), pair("lockKey", key)],
    );
    let written = inner
        .write(&handle, &mut record, |next, now| {
            next.operation
                .apply(
                    OperationEvent::Fail(OperationFailure::new(
                        FailureCode::PRECONDITION_FAILED,
                        false,
                    )),
                    now,
                )
                .map_err(|_| EngineError::Internal {
                    reason: "a queued operation refused to fail",
                })?;
            next.journal.set_failure_error(Some(error));
            next.journal.set_waiting_on(None);
            Ok(vec![WriteOp::DeleteLocks { operation: id }])
        })
        .await;
    drop(record);
    match written {
        Ok(()) => {
            inner.forget(id);
            true
        }
        Err(error) => {
            tracing::warn!(operation_id = %id, %error, "a queued operation could not be closed");
            false
        }
    }
}
