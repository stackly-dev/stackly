//! What the engine keeps per live operation, and the single writer of each
//! operation's rows: every change is applied to a copy, committed as one
//! batch that raises the operation's revision by exactly 1, and only then
//! swapped in and reported.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use stackly_domain::{
    CreatedRecord, CreatedSeq, DetailKey, ErrorCodeName, FailureError, FailureItem,
    InterruptPolicy, JournalStep, JournalView, LockKey, Operation, OperationCursor, OperationId,
    OperationJournal, OperationKind, OperationQuery, OperationState, Progress, RecoveryHint,
    Registry, RegistryError, Rule, StepSeq, StepState, WriteBatch, WriteOp,
};
use tokio::sync::{Mutex as AsyncMutex, watch};
use tokio::time::Instant;

use super::limits::FAILURE_ITEMS_MAX_BYTES;
use super::{
    EngineError, EngineStats, Inner, OperationDetailView, OperationListPage, OperationView,
    RecoveryAction, failure, lock,
};

/// The rows of one operation as the engine last committed them, plus the
/// steps and created records changed in memory after their own commit
/// failed, which the next batch writes.
#[derive(Debug, Clone)]
pub(crate) struct OpRecord {
    pub(crate) operation: Operation,
    pub(crate) journal: OperationJournal,
    pub(crate) steps: Vec<JournalStep>,
    pub(crate) created: Vec<CreatedRecord>,
    pub(crate) locks: Vec<LockKey>,
    unwritten_steps: Vec<StepSeq>,
    unwritten_created: Vec<CreatedSeq>,
}

impl OpRecord {
    pub(crate) fn from_view(operation: Operation, view: JournalView) -> Self {
        Self::new(
            operation,
            view.journal,
            view.steps,
            view.created,
            view.locks,
        )
    }

    pub(crate) const fn new(
        operation: Operation,
        journal: OperationJournal,
        steps: Vec<JournalStep>,
        created: Vec<CreatedRecord>,
        locks: Vec<LockKey>,
    ) -> Self {
        Self {
            operation,
            journal,
            steps,
            created,
            locks,
            unwritten_steps: Vec::new(),
            unwritten_created: Vec::new(),
        }
    }

    /// Remembers that the step `seq` changed in memory only, because its
    /// commit failed, so the next batch writes it (spec 0007, "kalau
    /// tulisan itu gagal, hasilnya disimpan di memori dan ditulis di batch
    /// akhir").
    pub(crate) fn mark_unwritten_step(&mut self, seq: StepSeq) {
        if !self.unwritten_steps.contains(&seq) {
            self.unwritten_steps.push(seq);
        }
    }

    /// Remembers that the created record `seq` changed in memory only.
    pub(crate) fn mark_unwritten_created(&mut self, seq: CreatedSeq) {
        if !self.unwritten_created.contains(&seq) {
            self.unwritten_created.push(seq);
        }
    }

    /// Forgets what was unwritten, once a batch wrote it.
    fn written(&mut self) {
        self.unwritten_steps.clear();
        self.unwritten_created.clear();
    }

    /// The step that runs now, if any.
    pub(crate) fn open_step(&self) -> Option<&JournalStep> {
        self.steps
            .iter()
            .find(|step| step.state() == StepState::Running)
    }

    /// The upsert of every step in `next` that differs from `self` or
    /// that `self` changed without writing it.
    pub(crate) fn changed_steps(&self, next: &Self) -> Vec<WriteOp> {
        let id = next.operation.id();
        next.steps
            .iter()
            .filter(|step| !self.steps.contains(step) || self.unwritten_steps.contains(&step.seq()))
            .map(|step| WriteOp::UpsertStep {
                operation: id,
                step: step.clone(),
            })
            .collect()
    }

    /// The upsert of every created record in `next` that differs from
    /// `self` or that `self` changed without writing it.
    pub(crate) fn changed_created(&self, next: &Self) -> Vec<WriteOp> {
        let id = next.operation.id();
        next.created
            .iter()
            .filter(|record| {
                !self.created.contains(record) || self.unwritten_created.contains(&record.seq())
            })
            .map(|record| WriteOp::UpsertCreated {
                operation: id,
                record: record.clone(),
            })
            .collect()
    }
}

/// What a running work is asked to stop for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StopSignal {
    /// A user cancel.
    pub(crate) cancel: bool,
    /// Shutdown parks a resumable operation.
    pub(crate) park: bool,
    /// Shutdown stops an operation that cannot resume, before its point of
    /// no return.
    pub(crate) halt: bool,
    /// Shutdown leaves an operation that closes as interrupted to the next
    /// start: its work stops with nothing more written.
    pub(crate) abandon: bool,
}

/// The progress a work reported and when progress was last committed.
#[derive(Debug, Default)]
pub(crate) struct ProgressCell {
    pub(crate) latest: Option<Progress>,
    pub(crate) last_commit: Option<Instant>,
    pub(crate) trailing: bool,
}

/// One live operation: not final, known to this engine.
pub(crate) struct OpHandle {
    pub(crate) id: OperationId,
    pub(crate) kind: OperationKind,
    pub(crate) cursor: OperationCursor,
    pub(crate) resumable: bool,
    /// Its kind closes as interrupted ([`InterruptPolicy::CloseInterrupted`]).
    pub(crate) close_interrupted: bool,
    /// A runner works on it now.
    pub(crate) in_runner: AtomicBool,
    pub(crate) record: AsyncMutex<OpRecord>,
    pub(crate) stop: watch::Sender<StopSignal>,
    pub(crate) compensating: AtomicBool,
    pub(crate) progress: Mutex<ProgressCell>,
    state: Mutex<OperationState>,
}

impl OpHandle {
    pub(crate) fn new(record: OpRecord) -> Arc<Self> {
        let (stop, _) = watch::channel(StopSignal::default());
        Arc::new(Self {
            id: record.operation.id(),
            kind: record.operation.kind(),
            cursor: OperationCursor::of(&record.operation),
            resumable: record.journal.resumable(),
            close_interrupted: record.operation.kind().interrupt_policy()
                == InterruptPolicy::CloseInterrupted,
            in_runner: AtomicBool::new(false),
            state: Mutex::new(record.operation.state()),
            record: AsyncMutex::new(record),
            stop,
            compensating: AtomicBool::new(false),
            progress: Mutex::new(ProgressCell::default()),
        })
    }

    /// The state as last committed.
    pub(crate) fn state(&self) -> OperationState {
        *lock(&self.state)
    }

    /// Changes the stop signal and wakes every waiting context.
    pub(crate) fn signal(&self, change: impl FnOnce(&mut StopSignal)) {
        self.stop.send_modify(change);
    }

    /// Claims the right to compensate; false when someone else holds it.
    pub(crate) fn try_claim(&self) -> bool {
        self.compensating
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub(crate) fn unclaim(&self) {
        self.compensating.store(false, Ordering::SeqCst);
    }

    pub(crate) fn is_compensating(&self) -> bool {
        self.compensating.load(Ordering::SeqCst)
    }
}

/// Which event a commit owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visible {
    State,
    Progress,
    Nothing,
}

fn visible(before: &OpRecord, after: &OpRecord) -> Visible {
    let (old, new) = (&before.operation, &after.operation);
    if old.state() != new.state() {
        return Visible::State;
    }
    let (old_journal, new_journal) = (&before.journal, &after.journal);
    if old.stage() != new.stage()
        || old.progress() != new.progress()
        || old.recovery_hint() != new.recovery_hint()
        || old_journal.waiting_on() != new_journal.waiting_on()
        || old_journal.point_of_no_return_at() != new_journal.point_of_no_return_at()
        || old_journal.cancel_requested_at() != new_journal.cancel_requested_at()
    {
        Visible::Progress
    } else {
        Visible::Nothing
    }
}

impl Inner {
    /// The time from the platform clock.
    pub(crate) fn now(&self) -> Timestamp {
        self.deps.platform.clock().now()
    }

    /// Runs a read on a blocking thread.
    pub(crate) async fn read<T, F>(&self, read: F) -> Result<T, EngineError>
    where
        T: Send + 'static,
        F: FnOnce(&dyn Registry) -> Result<T, RegistryError> + Send + 'static,
    {
        let registry = Arc::clone(&self.deps.registry);
        tokio::task::spawn_blocking(move || read(registry.as_ref()))
            .await
            .map_err(|_| EngineError::Internal {
                reason: "a registry read task stopped",
            })?
            .map_err(EngineError::Registry)
    }

    /// Commits a batch on a blocking thread.
    pub(crate) async fn commit(
        &self,
        batch: WriteBatch,
    ) -> Result<stackly_domain::Committed, EngineError> {
        self.commit_releasing(batch, None).await
    }

    /// [`Inner::commit`] that, with `release`, also releases that
    /// operation's keys in the same critical section of the lock table
    /// ([`LockTable::release_with`]): a reader that sees the commit in the
    /// registry or in an event never finds those keys still held.
    ///
    /// [`LockTable::release_with`]: super::LockTable::release_with
    async fn commit_releasing(
        &self,
        batch: WriteBatch,
        release: Option<OperationId>,
    ) -> Result<stackly_domain::Committed, EngineError> {
        let registry = Arc::clone(&self.deps.registry);
        let locks = self.locks.clone();
        tokio::task::spawn_blocking(move || match release {
            Some(id) => locks.release_with(id, || registry.commit(batch)),
            None => registry.commit(batch),
        })
        .await
        .map_err(|_| EngineError::Internal {
            reason: "a registry commit task stopped",
        })?
        .map_err(|err| EngineError::Registry(err.error))
    }

    /// Commits a change of one operation: `change` edits a copy of its rows
    /// and returns more writes, the copy picks up the latest reported
    /// progress, the operation is touched once, and the batch holds the
    /// operation, its journal, its changed steps and created records, and
    /// the extra writes. A change to a final state releases the operation's
    /// keys together with the commit. On success the copy replaces `record`
    /// and the visible change is reported; on failure nothing changes.
    pub(crate) async fn write<F>(
        &self,
        handle: &OpHandle,
        record: &mut OpRecord,
        change: F,
    ) -> Result<(), EngineError>
    where
        F: FnOnce(&mut OpRecord, Timestamp) -> Result<Vec<WriteOp>, EngineError>,
    {
        self.write_at(handle, record, None, change).await
    }

    /// [`Inner::write`] with a batch that applies only at `generation`.
    pub(crate) async fn write_at<F>(
        &self,
        handle: &OpHandle,
        record: &mut OpRecord,
        generation: Option<stackly_domain::Generation>,
        change: F,
    ) -> Result<(), EngineError>
    where
        F: FnOnce(&mut OpRecord, Timestamp) -> Result<Vec<WriteOp>, EngineError>,
    {
        let now = self.now();
        let mut next = record.clone();
        // Folded before the change, while the state still takes progress,
        // so a commit that ends the run carries the last value (AC-13).
        let progressed = self.fold_progress(handle, &mut next, now);
        let extra = change(&mut next, now)?;
        next.operation.touch(now);
        let mut ops = vec![
            WriteOp::UpdateOperation {
                operation: next.operation.clone(),
                expected: record.operation.revision(),
            },
            WriteOp::UpdateJournal(next.journal.clone()),
        ];
        ops.extend(record.changed_steps(&next));
        ops.extend(record.changed_created(&next));
        ops.extend(extra);
        let mut batch = WriteBatch::new(ops);
        batch.expected_generation = generation;
        // A final operation holds no key (spec 0007, "Lock dan antrian"),
        // and nobody may see it final while one is still held.
        let release = next.operation.state().is_final().then_some(handle.id);
        self.commit_releasing(batch, release).await?;
        if progressed {
            lock(&handle.progress).last_commit = Some(Instant::now());
        }
        let shown = visible(record, &next);
        *record = next;
        record.written();
        self.after_commit(handle, record, shown);
        Ok(())
    }

    /// Copies the latest reported progress into `next` when it changed and
    /// the state takes progress; true when it did.
    fn fold_progress(&self, handle: &OpHandle, next: &mut OpRecord, now: Timestamp) -> bool {
        let latest = lock(&handle.progress).latest;
        match latest {
            Some(progress) if progress != next.operation.progress() => {
                next.operation.set_progress(progress, now).is_ok()
            }
            _ => false,
        }
    }

    /// Records the committed state and reports what changed.
    fn after_commit(&self, handle: &OpHandle, record: &OpRecord, shown: Visible) {
        let state = record.operation.state();
        *lock(&handle.state) = state;
        self.locks.set_state(handle.id, state);
        let view = || self.view(record, Some(handle));
        match shown {
            Visible::State => {
                tracing::info!(
                    operation_id = %handle.id,
                    kind = %handle.kind,
                    state = %state,
                    revision = %record.operation.revision(),
                    "operation state changed"
                );
                self.deps.events.state_changed(&view());
            }
            Visible::Progress => self.deps.events.progress(&view()),
            Visible::Nothing => {}
        }
    }

    /// Reports a new operation, right after its first commit.
    pub(crate) fn report_created(&self, handle: &OpHandle, record: &OpRecord) {
        self.after_commit(handle, record, Visible::State);
    }

    /// Records and reports a change committed outside [`Inner::write`],
    /// for example in a batch of several operations.
    pub(crate) fn report_change(&self, handle: &OpHandle, before: &OpRecord, after: &OpRecord) {
        self.after_commit(handle, after, visible(before, after));
    }

    /// The view of an operation's rows.
    pub(crate) fn view(&self, record: &OpRecord, handle: Option<&OpHandle>) -> OperationView {
        self.view_of(record.operation.clone(), record.journal.clone(), handle)
    }

    /// The view of an operation and its journal.
    pub(crate) fn view_of(
        &self,
        operation: Operation,
        journal: OperationJournal,
        handle: Option<&OpHandle>,
    ) -> OperationView {
        let auto_compensating = handle.is_some_and(OpHandle::is_compensating);
        let recovery_actions = if operation.state() == OperationState::NeedsRecovery {
            let mut actions = Vec::new();
            if !auto_compensating {
                if operation.recovery_hint() == Some(RecoveryHint::Resume) {
                    actions.push(RecoveryAction::Resume);
                }
                actions.push(RecoveryAction::Abandon);
            }
            actions
        } else {
            Vec::new()
        };
        OperationView {
            operation,
            journal,
            recovery_actions,
            auto_compensating,
        }
    }

    /// The hint of an operation entering `needs_recovery` (spec 0007,
    /// "Aturan hint").
    pub(crate) fn hint(&self, record: &OpRecord, compensation_failed: bool) -> RecoveryHint {
        let past = record.journal.point_of_no_return_at().is_some();
        if !past {
            return if compensation_failed {
                RecoveryHint::Abandon
            } else {
                RecoveryHint::AutoCompensate
            };
        }
        let stopped_step_repeatable = record
            .steps
            .iter()
            .rev()
            .find(|step| {
                matches!(
                    step.state(),
                    StepState::Failed | StepState::Interrupted | StepState::Running
                )
            })
            .is_none_or(JournalStep::repeatable);
        if record.journal.resumable()
            && self.factory(record.operation.kind()).is_some()
            && record.journal.compensation_started_at().is_none()
            && stopped_step_repeatable
        {
            RecoveryHint::Resume
        } else {
            RecoveryHint::Manual
        }
    }

    /// A failure error with its message and detail values redacted and
    /// trimmed to their limits, and no items or rule.
    pub(crate) fn failure_error(
        &self,
        code: ErrorCodeName,
        message: &str,
        details: Vec<(DetailKey, String)>,
    ) -> FailureError {
        self.failure_error_with(code, message, details, Vec::new(), None)
    }

    /// Like [`Inner::failure_error`], with the items and rule an
    /// [`OperationError`](super::OperationError) carried. The field values
    /// of the items are redacted and trimmed; items above the count and
    /// byte limits are dropped from the back and added to the `omitted`
    /// detail.
    pub(crate) fn failure_error_with(
        &self,
        code: ErrorCodeName,
        message: &str,
        details: Vec<(DetailKey, String)>,
        items: Vec<FailureItem>,
        rule: Option<Rule>,
    ) -> FailureError {
        let redactor = &self.deps.redactor;
        let mut text = truncate_chars(&redactor.redact(message), FailureError::MESSAGE_MAX);
        if text.is_empty() {
            "the operation failed".clone_into(&mut text);
        }
        let mut details: Vec<(DetailKey, String)> = details
            .into_iter()
            .map(|(key, value)| {
                let value = redactor.redact(&value);
                (key, truncate_bytes(&value, FailureError::DETAIL_VALUE_MAX))
            })
            .collect();
        let fitted = failure::fit(items, redactor.as_ref(), FAILURE_ITEMS_MAX_BYTES);
        failure::add_omitted(&mut details, fitted.omitted);
        let error = FailureError::new(code.clone(), text, details)
            .and_then(|error| error.with_items(fitted.items))
            .unwrap_or_else(|_| {
                FailureError::new(code, "the operation failed", Vec::new())
                    .expect("a fixed short message with no details is a valid failure error")
            });
        match rule {
            Some(rule) => error.with_rule(rule),
            None => error,
        }
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

pub(super) fn truncate_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// One detail of a failure error, with a key fixed in the source.
pub(crate) fn pair(key: &'static str, value: impl ToString) -> (DetailKey, String) {
    (
        key.parse()
            .expect("detail keys in the engine are fixed camelCase words"),
        value.to_string(),
    )
}

pub(crate) async fn detail(
    inner: &Inner,
    id: OperationId,
) -> Result<Option<OperationDetailView>, EngineError> {
    let found = inner
        .read(move |registry| {
            let operation = registry.operation(id)?.value;
            let journal = registry.operation_journal(id)?.value;
            Ok(operation.zip(journal))
        })
        .await?;
    Ok(found.map(|(operation, view)| {
        let handle = inner.handle(id);
        OperationDetailView {
            view: inner.view_of(operation, view.journal, handle.as_deref()),
            steps: view.steps,
            created: view.created,
        }
    }))
}

pub(crate) async fn list(
    inner: &Inner,
    query: OperationQuery,
) -> Result<OperationListPage, EngineError> {
    let (page, journals) = inner
        .read(move |registry| {
            let page = registry.operations(query)?.value;
            let ids: Vec<OperationId> = page.items.iter().map(Operation::id).collect();
            let journals = registry.operation_journals(&ids)?.value;
            Ok((page, journals))
        })
        .await?;
    let items = page
        .items
        .into_iter()
        .filter_map(|operation| {
            let journal = journals
                .iter()
                .find(|journal| journal.operation_id() == operation.id())?
                .clone();
            let handle = inner.handle(operation.id());
            Some(inner.view_of(operation, journal, handle.as_deref()))
        })
        .collect();
    Ok(OperationListPage {
        items,
        next: page.next,
    })
}

pub(crate) fn stats(inner: &Inner) -> EngineStats {
    let (running, queued) = {
        let sched = inner.sched();
        (sched.running, sched.queued_work())
    };
    let needs_recovery = inner
        .ops()
        .values()
        .filter(|handle| handle.state() == OperationState::NeedsRecovery)
        .count();
    let counters = inner.counters();
    EngineStats {
        running,
        queued,
        needs_recovery: u32::try_from(needs_recovery).unwrap_or(u32::MAX),
        max_running: inner.limits.max_running,
        max_queued: inner.limits.max_queued,
        interrupted_at_start: counters.interrupted_at_start,
        resumed_at_start: counters.resumed_at_start,
        auto_compensated_at_start: counters.auto_compensated_at_start,
        rejected_queue_full: counters.rejected_queue_full,
    }
}
