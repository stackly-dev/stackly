//! What a running work talks to (spec 0007, "API engine" and "Langkah,
//! titik tak bisa dibalik, dan resume").

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use stackly_domain::{
    Committed, CompensatorName, CreatedItem, CreatedRecord, CreatedSeq, CreatedState,
    CustomItemKey, EnvironmentId, InstanceId, JournalPayload, JournalStep, NewStep, OperationId,
    OperationKind, OperationResource, OperationState, OperationTrigger, Progress, ReasonCode,
    Registry, SecretRef, StageName, StepSeq, StepState, WriteBatch,
};
use stackly_platform::{Platform, PlatformError, SecretPresence};
use tokio::sync::watch;
use tokio::time::Instant;

use super::limits::{CHECKPOINT_MAX, CUSTOM_PAYLOAD_MAX, MAX_CREATED, MAX_STEPS};
use super::precheck::NO_OP_STAGE;
use super::state::{OpHandle, StopSignal};
use super::work::{CreatedHandle, FinalBatch, StepSpec, StepStart, WorkError};
use super::{EngineError, Inner, lock};
use crate::layout::PreparedLayout;
use crate::secret::{ConflictReason, SecretError};

/// The step a work runs now.
#[derive(Debug, Clone, Copy)]
struct OpenStep {
    seq: StepSeq,
    reversible: bool,
    repeatable: bool,
}

/// The work's view of its operation: steps, checkpoints, progress, created
/// items, scratch folder, and commits. Every call that writes commits
/// before it returns.
pub struct OperationContext {
    inner: Arc<Inner>,
    handle: Arc<OpHandle>,
    stop: watch::Receiver<StopSignal>,
    id: OperationId,
    kind: OperationKind,
    resource: OperationResource,
    trigger: OperationTrigger,
    input: JournalPayload,
    resumable: bool,
    close_interrupted: bool,
    open: Option<OpenStep>,
    poisoned: Option<EngineError>,
}

impl fmt::Debug for OperationContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationContext")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl OperationContext {
    pub(super) fn new(
        inner: Arc<Inner>,
        handle: Arc<OpHandle>,
        trigger: OperationTrigger,
        input: JournalPayload,
        resource: OperationResource,
        resumable: bool,
    ) -> Self {
        let stop = handle.stop.subscribe();
        Self {
            id: handle.id,
            kind: handle.kind,
            close_interrupted: handle.close_interrupted,
            inner,
            handle,
            stop,
            resource,
            trigger,
            input,
            resumable,
            open: None,
            poisoned: None,
        }
    }

    /// Returns the operation's ID, the value of `createdByOperation` in
    /// what it creates.
    #[must_use]
    pub const fn id(&self) -> OperationId {
        self.id
    }

    /// Returns the kind of work.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        self.kind
    }

    /// Returns what the operation works on.
    #[must_use]
    pub const fn resource(&self) -> OperationResource {
        self.resource
    }

    /// Returns who started it.
    #[must_use]
    pub const fn trigger(&self) -> &OperationTrigger {
        &self.trigger
    }

    /// Returns its stored input.
    #[must_use]
    pub const fn input(&self) -> Option<&JournalPayload> {
        Some(&self.input)
    }

    /// Returns the registry, to read. Writes go through
    /// [`commit`](Self::commit) or the work's finalize.
    #[must_use]
    pub fn registry(&self) -> &Arc<dyn Registry> {
        &self.inner.deps.registry
    }

    /// Returns the platform ports.
    #[must_use]
    pub fn platform(&self) -> &Arc<Platform> {
        &self.inner.deps.platform
    }

    /// Returns the checked layout.
    #[must_use]
    pub fn layout(&self) -> &Arc<PreparedLayout> {
        &self.inner.deps.layout
    }

    fn healthy(&self) -> Result<(), WorkError> {
        match &self.poisoned {
            Some(error) => Err(WorkError::Engine(error.clone())),
            None => Ok(()),
        }
    }

    /// Remembers a failed journal write: every later call returns it.
    fn poison(&mut self, error: EngineError) -> WorkError {
        self.poisoned = Some(error.clone());
        WorkError::Engine(error)
    }

    /// Whether shutdown or a cancel stops the operation at a step boundary.
    /// A kind that closes as interrupted takes a cancel in any step, since
    /// its point of no return only withholds compensation.
    fn stop_at_boundary(&self, past: bool) -> Option<WorkError> {
        let signal = *self.stop.borrow();
        if signal.abandon {
            return Some(WorkError::Abandoned);
        }
        if signal.cancel && (!past || self.close_interrupted) {
            return Some(WorkError::Cancelled);
        }
        if signal.park && self.resumable {
            return Some(WorkError::Parked);
        }
        if signal.halt && !self.resumable && !past {
            return Some(WorkError::Cancelled);
        }
        None
    }

    /// Whether a stop applies inside the open step. A kind that closes as
    /// interrupted takes a cancel or a shutdown abandon in any step.
    fn stop_inside_step(&self) -> Option<WorkError> {
        let signal = *self.stop.borrow();
        if self.close_interrupted {
            return if signal.abandon {
                Some(WorkError::Abandoned)
            } else if signal.cancel {
                Some(WorkError::Cancelled)
            } else {
                None
            };
        }
        let open = self.open?;
        if (signal.cancel || (signal.halt && !self.resumable)) && open.reversible {
            return Some(WorkError::Cancelled);
        }
        if signal.park && self.resumable && open.repeatable {
            return Some(WorkError::Parked);
        }
        None
    }

    /// Begins a step. It first honours a cancel (before the point of no
    /// return, or in any step of a kind that closes as interrupted), a
    /// shutdown park (for an operation that can resume), a shutdown stop
    /// (for one that cannot, before the point of no return), and a shutdown
    /// abandon (for a kind that closes as interrupted, with nothing
    /// written).
    /// A step that already finished before a restart returns
    /// [`StepStart::AlreadyDone`] with its checkpoint, a repeatable step
    /// that stopped runs again, and a step with `reversible: false`
    /// commits the point of no return before it returns. A step still open
    /// finishes first, with no checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Cancelled`], [`WorkError::Parked`], or
    /// [`WorkError::Abandoned`] when the operation stops here, and
    /// [`WorkError::Engine`] when the step cannot be recorded.
    pub async fn begin_step(&mut self, spec: StepSpec) -> Result<StepStart, WorkError> {
        self.healthy()?;
        if self.stop.borrow().abandon {
            return Err(WorkError::Abandoned);
        }
        if self.open.is_some() {
            self.finish_step(None).await?;
        }
        let handle = Arc::clone(&self.handle);
        let mut record = handle.record.lock().await;
        let past = record.journal.point_of_no_return_at().is_some();
        if let Some(stop) = self.stop_at_boundary(past) {
            return Err(stop);
        }
        let existing = record
            .steps
            .iter()
            .position(|step| step.name() == &spec.name);
        let start = match existing.map(|index| (index, record.steps[index].state())) {
            Some((_, StepState::Done)) => {
                let checkpoint =
                    existing.and_then(|index| record.steps[index].checkpoint().cloned());
                return Ok(StepStart::AlreadyDone { checkpoint });
            }
            Some((index, _)) => {
                let step = &record.steps[index];
                if !step.repeatable() {
                    return Err(self.poison(EngineError::InvalidUse {
                        reason: "a step that is not repeatable never runs twice",
                    }));
                }
                Retry(index)
            }
            None => {
                let next = u16::try_from(record.steps.len() + 1).unwrap_or(u16::MAX);
                let seq = StepSeq::new(next).map_err(|_| {
                    self.poison(EngineError::JournalFull {
                        kind: "steps",
                        max: MAX_STEPS,
                    })
                })?;
                New(seq)
            }
        };
        let reversible = spec.reversible;
        let written = self
            .inner
            .write(&handle, &mut record, |next, now| {
                match start {
                    Retry(index) => {
                        next.steps[index]
                            .retry(now)
                            .map_err(|_| EngineError::InvalidUse {
                                reason: "a step that is not repeatable never runs twice",
                            })?
                    }
                    New(seq) => next.steps.push(JournalStep::begin(
                        NewStep {
                            seq,
                            name: spec.name.clone(),
                            repeatable: spec.repeatable,
                            reversible: spec.reversible,
                        },
                        now,
                    )),
                }
                if !reversible {
                    next.journal.mark_point_of_no_return(now);
                }
                Ok(Vec::new())
            })
            .await;
        if let Err(error) = written {
            return Err(self.poison(error));
        }
        let step = match start {
            Retry(index) => &record.steps[index],
            New(_) => record.steps.last().expect("a step was just recorded"),
        };
        self.open = Some(OpenStep {
            seq: step.seq(),
            reversible: step.reversible(),
            repeatable: step.repeatable(),
        });
        Ok(StepStart::Run {
            attempt: step.attempts(),
        })
    }

    /// Finishes the open step with what it hands on.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] when no step is open, the checkpoint is
    /// over its limit, or the commit fails.
    pub async fn finish_step(
        &mut self,
        checkpoint: Option<JournalPayload>,
    ) -> Result<(), WorkError> {
        self.healthy()?;
        let Some(open) = self.open else {
            return Err(WorkError::Engine(EngineError::InvalidUse {
                reason: "finish_step needs an open step",
            }));
        };
        if checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.byte_len() > CHECKPOINT_MAX)
        {
            return Err(WorkError::Engine(EngineError::PayloadTooLarge {
                field: "checkpoint",
                max: CHECKPOINT_MAX,
            }));
        }
        let handle = Arc::clone(&self.handle);
        let mut record = handle.record.lock().await;
        let written = self
            .inner
            .write(&handle, &mut record, |next, now| {
                let step = next
                    .steps
                    .iter_mut()
                    .find(|step| step.seq() == open.seq)
                    .ok_or(EngineError::InvalidUse {
                        reason: "the open step is not in the journal",
                    })?;
                step.finish(checkpoint, now)
                    .map_err(|_| EngineError::InvalidUse {
                        reason: "the open step is not running",
                    })?;
                Ok(Vec::new())
            })
            .await;
        drop(record);
        match written {
            Ok(()) => {
                self.open = None;
                Ok(())
            }
            Err(error) => Err(self.poison(error)),
        }
    }

    /// Returns an error when the operation must stop at this safe point:
    /// a cancel or shutdown stop inside a reversible step, a shutdown park
    /// inside a repeatable step of an operation that can resume, or, for a
    /// kind that closes as interrupted, a cancel or shutdown abandon
    /// anywhere.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Cancelled`], [`WorkError::Parked`],
    /// [`WorkError::Abandoned`], or the [`WorkError::Engine`] of an earlier
    /// failed write.
    pub fn check_cancel(&self) -> Result<(), WorkError> {
        self.healthy()?;
        match self.stop_inside_step() {
            Some(stop) => Err(stop),
            None => Ok(()),
        }
    }

    /// Completes once [`check_cancel`](Self::check_cancel) would fail; use
    /// it in `tokio::select!` around long waits.
    pub async fn stopped(&self) {
        let mut stop = self.stop.clone();
        loop {
            if self.stop_inside_step().is_some() {
                return;
            }
            if stop.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
            stop.borrow_and_update();
        }
    }

    /// Records the stage the work entered, at once.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] for the reserved [`NO_OP_STAGE`] or
    /// when the commit fails.
    pub async fn set_stage(&mut self, stage: StageName) -> Result<(), WorkError> {
        self.healthy()?;
        if stage == NO_OP_STAGE {
            return Err(WorkError::Engine(EngineError::InvalidUse {
                reason: "the no_op stage is reserved for no operation answers",
            }));
        }
        let handle = Arc::clone(&self.handle);
        let mut record = handle.record.lock().await;
        let written = self
            .inner
            .write(&handle, &mut record, |next, now| {
                next.operation
                    .set_stage(stage, now)
                    .map_err(|_| EngineError::InvalidUse {
                        reason: "the stage changes only while the operation runs",
                    })?;
                Ok(Vec::new())
            })
            .await;
        drop(record);
        written.map_err(|error| self.poison(error))
    }

    /// Reports progress. The latest value wins: it is committed at once
    /// when the last progress commit is a full interval old, and otherwise
    /// once at the end of the interval.
    pub fn set_progress(&mut self, progress: Progress) {
        let interval = self.inner.limits.progress_interval;
        let delay = {
            let mut cell = lock(&self.handle.progress);
            cell.latest = Some(progress);
            if cell.trailing {
                return;
            }
            let now = Instant::now();
            match cell.last_commit.map(|at| at + interval) {
                Some(at) if at > now => {
                    cell.trailing = true;
                    Some(at)
                }
                _ => {
                    // The leading commit: later reports in this window wait
                    // for the trailing one.
                    cell.last_commit = Some(now);
                    None
                }
            }
        };
        let inner = Arc::clone(&self.inner);
        let handle = Arc::clone(&self.handle);
        tokio::spawn(async move {
            if let Some(at) = delay {
                tokio::time::sleep_until(at).await;
                lock(&handle.progress).trailing = false;
            }
            commit_progress(&inner, &handle).await;
        });
    }

    fn open_step(&self, name: &'static str) -> Result<OpenStep, WorkError> {
        self.open
            .ok_or(WorkError::Engine(EngineError::InvalidUse { reason: name }))
    }

    /// Commits an `intended` record of `item` for the open step.
    async fn intend(
        &mut self,
        item: CreatedItem,
        payload: Option<JournalPayload>,
    ) -> Result<CreatedHandle, WorkError> {
        self.healthy()?;
        let open = self.open_step("an item is recorded only inside a step")?;
        let handle = Arc::clone(&self.handle);
        let mut record = handle.record.lock().await;
        if let Some(existing) = record
            .created
            .iter()
            .find(|created| created.item() == &item)
        {
            return Ok(CreatedHandle {
                seq: existing.seq(),
            });
        }
        let next = u16::try_from(record.created.len() + 1).unwrap_or(u16::MAX);
        let seq = CreatedSeq::new(next).map_err(|_| {
            self.poison(EngineError::JournalFull {
                kind: "created",
                max: MAX_CREATED,
            })
        })?;
        let written =
            self.inner
                .write(&handle, &mut record, |next, now| {
                    let created = CreatedRecord::intend(seq, open.seq, item, payload, now)
                        .map_err(|_| EngineError::PayloadTooLarge {
                            field: "payload",
                            max: CUSTOM_PAYLOAD_MAX,
                        })?;
                    next.created.push(created);
                    Ok(Vec::new())
                })
                .await;
        drop(record);
        written.map_err(|error| self.poison(error))?;
        Ok(CreatedHandle { seq })
    }

    /// Records the intent to create the folders of the new resource the
    /// request named (`creates_resource`), before the first write to disk.
    /// A resource the registry already holds is never claimed, so its
    /// folders are never compensated (BR-023).
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] outside a step, for a resource that is
    /// not an instance or environment or that the registry already holds,
    /// or when the registry read or the commit fails.
    pub async fn claim_resource(&mut self) -> Result<CreatedHandle, WorkError> {
        self.healthy()?;
        let resource = self.resource;
        let item = match resource {
            OperationResource::Instance(id) => CreatedItem::InstanceTree(id),
            OperationResource::Environment(id) => CreatedItem::EnvironmentTree(id),
            _ => {
                return Err(WorkError::Engine(EngineError::InvalidUse {
                    reason: "only a new instance or environment has folders to claim",
                }));
            }
        };
        let exists = self
            .inner
            .read(move |registry| {
                Ok(match resource {
                    OperationResource::Instance(id) => registry.instance(id)?.value.is_some(),
                    OperationResource::Environment(id) => registry.environment(id)?.value.is_some(),
                    _ => false,
                })
            })
            .await
            .map_err(WorkError::Engine)?;
        if exists {
            return Err(WorkError::Engine(EngineError::InvalidUse {
                reason: "a resource the registry already holds is not the operation's to claim",
            }));
        }
        self.intend(item, None).await
    }

    /// Makes a new instance ID whose short key no known instance uses, and
    /// records the intent to create its folders.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] outside a step or when the registry
    /// read or the commit fails.
    pub async fn new_instance_id(&mut self) -> Result<(InstanceId, CreatedHandle), WorkError> {
        self.healthy()?;
        let known = self
            .inner
            .read(|registry| registry.known_instance_ids().map(|known| known.value))
            .await
            .map_err(WorkError::Engine)?;
        let id = loop {
            let id = InstanceId::generate();
            if known
                .iter()
                .all(|other| other.short_key() != id.short_key())
            {
                break id;
            }
        };
        let handle = self.intend(CreatedItem::InstanceTree(id), None).await?;
        Ok((id, handle))
    }

    /// Makes a new environment ID and records the intent to create its
    /// folder.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] outside a step or when the commit
    /// fails.
    pub async fn new_environment_id(
        &mut self,
    ) -> Result<(EnvironmentId, CreatedHandle), WorkError> {
        let id = EnvironmentId::generate();
        let handle = self.intend(CreatedItem::EnvironmentTree(id), None).await?;
        Ok((id, handle))
    }

    /// Records the intent to create a secret reference, before `put`
    /// stores its value (spec 0007, spec 0011). The reference must be
    /// `local:instance/<id>/<purpose>` of the operation's instance or of
    /// one it made with [`new_instance_id`](Self::new_instance_id). Before
    /// the record, a probe of the secret store must find no item; a record
    /// the operation holds already is returned as it is. Compensation
    /// deletes the reference through the `secret_ref` compensator.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] outside a step, for a reference of
    /// another instance, or when the commit fails, and [`WorkError::Failed`]
    /// with `secret.conflict` when the store holds an item already, or with
    /// the store's error when the probe fails. Neither failure records
    /// anything.
    pub async fn record_secret_intent(
        &mut self,
        reference: SecretRef,
    ) -> Result<CreatedHandle, WorkError> {
        self.healthy()?;
        self.open_step("an item is recorded only inside a step")?;
        let item = CreatedItem::SecretRef(reference.clone());
        let owner = reference
            .is_instance_reference()
            .then(|| reference.segments().nth(1))
            .flatten()
            .and_then(|id| id.parse::<InstanceId>().ok());
        let (recorded, claimed) = {
            let record = self.handle.record.lock().await;
            let recorded = record
                .created
                .iter()
                .find(|created| created.item() == &item)
                .map(CreatedRecord::seq);
            let claimed = owner.is_some_and(|id| {
                self.resource == OperationResource::Instance(id)
                    || record
                        .created
                        .iter()
                        .any(|created| created.item() == &CreatedItem::InstanceTree(id))
            });
            (recorded, claimed)
        };
        if let Some(seq) = recorded {
            return Ok(CreatedHandle { seq });
        }
        if !claimed {
            return Err(WorkError::Engine(EngineError::InvalidUse {
                reason: "a secret intent names a reference of an instance the operation made",
            }));
        }
        let platform = Arc::clone(&self.inner.deps.platform);
        let probed = {
            let reference = reference.clone();
            tokio::task::spawn_blocking(move || platform.secrets().probe(&reference))
                .await
                .map_err(|_| {
                    WorkError::Engine(EngineError::Internal {
                        reason: "the secret store probe task stopped",
                    })
                })?
        };
        match probed {
            Ok(SecretPresence::Missing) => self.intend(item, None).await,
            Ok(SecretPresence::Present) => {
                Err(WorkError::Failed(Box::new(SecretError::Conflict {
                    reference,
                    reason: ConflictReason::AlreadyExists,
                })))
            }
            Err(error) => Err(WorkError::Failed(Box::new(SecretError::Store(error)))),
        }
    }

    /// Records the intent to create a custom item that `compensator`
    /// undoes, with what it needs to prove ownership.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] outside a step, for a payload over
    /// [`CUSTOM_PAYLOAD_MAX`], or when the commit fails.
    pub async fn record_custom_intent(
        &mut self,
        compensator: CompensatorName,
        key: CustomItemKey,
        payload: Option<JournalPayload>,
    ) -> Result<CreatedHandle, WorkError> {
        self.intend(CreatedItem::Custom { compensator, key }, payload)
            .await
    }

    async fn settle_created(
        &mut self,
        item: CreatedHandle,
        change: impl FnOnce(&mut CreatedRecord, jiff::Timestamp) -> bool,
    ) -> Result<(), WorkError> {
        self.healthy()?;
        let handle = Arc::clone(&self.handle);
        let mut record = handle.record.lock().await;
        let written = self
            .inner
            .write(&handle, &mut record, |next, now| {
                let created = next
                    .created
                    .iter_mut()
                    .find(|created| created.seq() == item.seq)
                    .ok_or(EngineError::InvalidUse {
                        reason: "the handle names no created item of this operation",
                    })?;
                if change(created, now) {
                    Ok(Vec::new())
                } else {
                    Err(EngineError::InvalidUse {
                        reason: "the created item is not intended",
                    })
                }
            })
            .await;
        drop(record);
        written.map_err(|error| self.poison(error))
    }

    /// Marks the item created.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] when the item is not `intended` or the
    /// commit fails.
    pub async fn confirm_created(&mut self, item: CreatedHandle) -> Result<(), WorkError> {
        self.settle_created(item, |created, _| created.confirm().is_ok())
            .await
    }

    /// Marks the item not created because it already existed
    /// (`already_exists`, `short_key_taken`): it is `not_owned` and never
    /// compensated.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] when the item is not `intended` or the
    /// commit fails.
    pub async fn not_created(
        &mut self,
        item: CreatedHandle,
        reason: ReasonCode,
    ) -> Result<(), WorkError> {
        self.settle_created(item, move |created, now| {
            created.state() == CreatedState::Intended
                && created
                    .settle(CreatedState::NotOwned, Some(reason), now)
                    .is_ok()
        })
        .await
    }

    /// Returns the operation's scratch folder, `operations/<id>/`, creating
    /// it the first time.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] when the folder cannot be created.
    pub async fn scratch_dir(&mut self) -> Result<PathBuf, WorkError> {
        self.healthy()?;
        let platform = Arc::clone(&self.inner.deps.platform);
        let path = self
            .inner
            .deps
            .layout
            .layout()
            .operations_dir()
            .join(self.id.to_string());
        let target = path.clone();
        let created =
            tokio::task::spawn_blocking(move || match platform.fs().create_dir_private(&target) {
                Ok(()) | Err(PlatformError::AlreadyExists { .. }) => Ok(()),
                Err(error) => Err(error),
            })
            .await
            .map_err(|_| {
                WorkError::Engine(EngineError::Internal {
                    reason: "the scratch folder task stopped",
                })
            })?;
        created.map_err(|error| WorkError::Engine(EngineError::Platform(error)))?;
        Ok(path)
    }

    /// Commits a batch in the middle of the work, for state that must be
    /// seen before the operation ends. Everything it inserts needs a custom
    /// compensator.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] when the commit fails.
    pub async fn commit(&mut self, batch: FinalBatch) -> Result<Committed, WorkError> {
        self.healthy()?;
        let mut write = WriteBatch::new(batch.ops);
        write.expected_generation = batch.expected_generation;
        self.inner.commit(write).await.map_err(WorkError::Engine)
    }
}

use StartKind::{New, Retry};

#[derive(Debug, Clone, Copy)]
enum StartKind {
    Retry(usize),
    New(StepSeq),
}

/// Commits the latest reported progress, if it still differs and the
/// operation still takes progress; a commit for one that already moved on
/// would raise its revision with nothing to report.
pub(crate) async fn commit_progress(inner: &Inner, handle: &OpHandle) {
    let mut record = handle.record.lock().await;
    let reporting = matches!(
        record.operation.state(),
        OperationState::Running | OperationState::CancelRequested
    );
    let latest = lock(&handle.progress).latest;
    if !reporting || latest.is_none_or(|progress| progress == record.operation.progress()) {
        return;
    }
    if let Err(error) = inner
        .write(handle, &mut record, |_, _| Ok(Vec::new()))
        .await
    {
        tracing::warn!(operation_id = %handle.id, %error, "a progress commit failed");
    }
}
