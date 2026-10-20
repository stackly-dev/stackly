//! Operations: the seven state machine of PRD section 18.2, the kinds of
//! work, and progress (BR-006), with the journal and lock keys the engine
//! of feature 7 keeps for each operation (spec 0007).

mod journal;
mod lock;

use jiff::Timestamp;

pub use journal::{
    CreatedItem, CreatedRecord, CreatedSeq, CreatedState, FailureError, FailureItem,
    JournalPayload, JournalStep, JournalView, NewJournal, NewStep, OperationJournal,
    OperationTrigger, RecoveryCause, StepSeq, StepState, StoredCreatedRecord, StoredJournal,
    StoredStep, TriggerOrigin,
};
pub use lock::LockKey;

use crate::code::code_enum;
use crate::error::{InvariantDetail, InvariantError, Machine, ProgressError, TransitionError};
use crate::id::{EnvironmentId, InstanceId, OperationId, RuntimeId};
use crate::value::{FailureCode, IdempotencyKey, ResourceRevision, StageName};

code_enum! {
    /// The state of an operation (PRD section 18.2).
    pub enum OperationState: OperationState {
        /// Waiting to run, either new or resumed.
        Queued => "queued",
        /// Running.
        Running => "running",
        /// A cancel was asked for; the operation winds down.
        CancelRequested => "cancel_requested",
        /// Finished with success; final.
        Succeeded => "succeeded",
        /// Finished with a failure; final.
        Failed => "failed",
        /// Cancelled; final.
        Cancelled => "cancelled",
        /// Interrupted in a way it cannot resume from on its own.
        NeedsRecovery => "needs_recovery",
    }
}

impl OperationState {
    /// Returns whether the state is final: `succeeded`, `failed`, or
    /// `cancelled` accept no event.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    /// Applies `event` to an operation that has or has not `started` yet,
    /// following the table of spec 0004.
    ///
    /// # Errors
    ///
    /// Returns a [`TransitionError`] for a pair the table does not hold.
    pub const fn apply(
        self,
        event: &OperationEvent,
        started: bool,
    ) -> Result<Self, TransitionError> {
        use OperationEvent as E;
        use OperationState as S;
        match (self, event) {
            (S::Queued, E::Start) => Ok(S::Running),
            (S::Queued | S::Running | S::CancelRequested, E::Fail(_)) => Ok(S::Failed),
            (S::Queued, E::RequestCancel) => {
                if started {
                    Ok(S::CancelRequested)
                } else {
                    Ok(S::Cancelled)
                }
            }
            (S::Running | S::CancelRequested, E::Succeed) => Ok(S::Succeeded),
            (S::Running, E::RequestCancel) => Ok(S::CancelRequested),
            (S::Running, E::Interrupted { resumable }) => {
                if *resumable {
                    Ok(S::Queued)
                } else {
                    Ok(S::NeedsRecovery)
                }
            }
            (S::CancelRequested, E::Cancelled) => Ok(S::Cancelled),
            (S::CancelRequested, E::Interrupted { .. })
            | (S::Running | S::CancelRequested, E::CompensationFailed) => Ok(S::NeedsRecovery),
            (S::NeedsRecovery, E::Resume) => Ok(S::Queued),
            (S::NeedsRecovery, E::Resolve(outcome)) => Ok(outcome.state()),
            _ => Err(TransitionError::new(
                Machine::Operation,
                self.as_code(),
                event.as_code(),
            )),
        }
    }
}

/// How an operation in `needs_recovery` was settled, by hand or by the
/// engine's automatic compensation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// It is settled as succeeded.
    Succeeded,
    /// It is settled as failed, with why; `apply` records the failure.
    Failed(OperationFailure),
    /// It is settled as cancelled.
    Cancelled,
}

impl Outcome {
    /// Returns the final operation state this outcome names.
    #[must_use]
    pub const fn state(&self) -> OperationState {
        match self {
            Self::Succeeded => OperationState::Succeeded,
            Self::Failed(_) => OperationState::Failed,
            Self::Cancelled => OperationState::Cancelled,
        }
    }
}

/// Why an operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationFailure {
    code: FailureCode,
    retryable: bool,
}

impl OperationFailure {
    /// Creates a failure with its code and whether retrying can help.
    #[must_use]
    pub const fn new(code: FailureCode, retryable: bool) -> Self {
        Self { code, retryable }
    }

    /// Returns the failure code.
    #[must_use]
    pub const fn code(&self) -> &FailureCode {
        &self.code
    }

    /// Returns whether retrying can help.
    #[must_use]
    pub const fn retryable(&self) -> bool {
        self.retryable
    }
}

/// An event of the operation state machine.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OperationEvent {
    /// The engine starts, or restarts, the operation.
    Start,
    /// The work succeeded.
    Succeed,
    /// The work failed.
    Fail(OperationFailure),
    /// Someone asked to cancel.
    RequestCancel,
    /// The cancel finished.
    Cancelled,
    /// The daemon stopped while the operation ran.
    Interrupted {
        /// Whether the operation declares it can resume (feature 7).
        resumable: bool,
    },
    /// Undoing finished steps failed, or is impossible because an
    /// irreversible step already ran.
    CompensationFailed,
    /// The engine resumes an operation that can resume (feature 7).
    Resume,
    /// An operation in `needs_recovery` was settled by hand.
    Resolve(Outcome),
}

impl OperationEvent {
    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Succeed => "succeed",
            Self::Fail(_) => "fail",
            Self::RequestCancel => "request_cancel",
            Self::Cancelled => "cancelled",
            Self::Interrupted { .. } => "interrupted",
            Self::CompensationFailed => "compensation_failed",
            Self::Resume => "resume",
            Self::Resolve(_) => "resolve",
        }
    }
}

code_enum! {
    /// The kind of work an operation does. Data migration, backup, restore,
    /// and the provider revision update each have their own kind (BR-013).
    #[non_exhaustive]
    pub enum OperationKind: OperationKind {
        /// Install a managed runtime.
        InstallRuntime => "install_runtime",
        /// Register a runtime in a folder the user owns.
        RegisterExternalRuntime => "register_external_runtime",
        /// Remove a runtime.
        RemoveRuntime => "remove_runtime",
        /// Create an instance.
        CreateInstance => "create_instance",
        /// Start an instance.
        StartInstance => "start_instance",
        /// Stop an instance.
        StopInstance => "stop_instance",
        /// Restart an instance.
        RestartInstance => "restart_instance",
        /// Reload an instance's configuration.
        ReloadInstance => "reload_instance",
        /// Rename an instance.
        RenameInstance => "rename_instance",
        /// Remove an instance and keep its data.
        RemoveInstance => "remove_instance",
        /// Delete an instance and its data.
        PurgeInstance => "purge_instance",
        /// Move an instance to another runtime.
        RebindRuntime => "rebind_runtime",
        /// Move an instance to another provider revision.
        UpdateProviderRevision => "update_provider_revision",
        /// Back up an instance.
        BackupInstance => "backup_instance",
        /// Restore an instance.
        RestoreInstance => "restore_instance",
        /// Bring an environment up.
        EnvironmentUp => "environment_up",
        /// Take an environment down.
        EnvironmentDown => "environment_down",
        /// Update Stackly itself; its resource is the manager.
        UpdateManager => "update_manager",
        /// Migrate an instance's data to another line.
        MigrateData => "migrate_data",
    }
}

impl OperationKind {
    /// Returns whether this kind deletes instance data. Only
    /// `purge_instance` does; `remove_instance` keeps the data (BR-006).
    #[must_use]
    pub const fn deletes_data(self) -> bool {
        matches!(self, Self::PurgeInstance)
    }

    /// Returns whether an operation of this kind may resume after the
    /// daemon stopped while it ran (spec 0007, "Tabel resume per jenis
    /// operasi"): installs, environment up and down, and backups. Every
    /// other kind never resumes; what happens to it instead follows its
    /// [`interrupt_policy`](Self::interrupt_policy).
    #[must_use]
    pub const fn resumable(self) -> bool {
        matches!(
            self,
            Self::InstallRuntime
                | Self::EnvironmentUp
                | Self::EnvironmentDown
                | Self::BackupInstance
        )
    }

    /// Returns what the engine does with an operation of this kind that
    /// the daemon interrupted (spec 0007, "Tabel resume per jenis
    /// operasi"): start, stop, restart, and reload of an instance close as
    /// interrupted, because their process outlives the daemon and the
    /// supervisor adopts it; every other kind is compensated before its
    /// point of no return.
    #[must_use]
    pub const fn interrupt_policy(self) -> InterruptPolicy {
        match self {
            Self::StartInstance
            | Self::StopInstance
            | Self::RestartInstance
            | Self::ReloadInstance => InterruptPolicy::CloseInterrupted,
            _ => InterruptPolicy::Compensate,
        }
    }
}

/// What the engine does with an operation the daemon interrupted, by kind
/// (spec 0007, "Tabel resume per jenis operasi").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterruptPolicy {
    /// Resume it when it can; otherwise compensate it before its point of
    /// no return and leave it in `needs_recovery` after it.
    Compensate,
    /// Close it `failed` with reason `interrupted` (or `cancelled` when a
    /// cancel was asked for), with no compensation and no resume. Past its
    /// point of no return a failure of its work still ends `failed`, never
    /// `needs_recovery`, and a cancel reaches it in any step.
    CloseInterrupted,
}

code_enum! {
    /// What the user can do about an operation in `needs_recovery`.
    pub enum RecoveryHint: RecoveryHint {
        /// The engine compensates it on its own once the daemon is ready.
        AutoCompensate => "auto_compensate",
        /// It can resume from its last step.
        Resume => "resume",
        /// Only abandoning it, after undoing what can be undone, is left.
        Abandon => "abandon",
        /// It needs a look by hand.
        Manual => "manual",
    }
}

/// What an operation works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationResource {
    /// A runtime.
    Runtime(RuntimeId),
    /// An instance.
    Instance(InstanceId),
    /// An environment.
    Environment(EnvironmentId),
    /// Stackly itself.
    Manager,
}

/// How far an operation got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Progress {
    /// No count is known.
    Indeterminate,
    /// `done` of `total` steps, bytes, or items, with `done <= total`.
    Counted {
        /// What is done.
        done: u64,
        /// The whole.
        total: u64,
    },
}

/// The values a new [`Operation`] starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOperation {
    /// The new operation's ID, created by the daemon.
    pub id: OperationId,
    /// The kind of work.
    pub kind: OperationKind,
    /// What it works on.
    pub resource: OperationResource,
    /// The client's idempotency key, if any.
    pub idempotency_key: Option<IdempotencyKey>,
    /// The time it was created, from the platform clock.
    pub created_at: Timestamp,
}

/// Every field of an [`Operation`] as the registry stores it, for
/// [`Operation::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOperation {
    /// The ID.
    pub id: OperationId,
    /// The kind of work.
    pub kind: OperationKind,
    /// What it works on.
    pub resource: OperationResource,
    /// The client's idempotency key, if any.
    pub idempotency_key: Option<IdempotencyKey>,
    /// The state.
    pub state: OperationState,
    /// When it first started.
    pub started_at: Option<Timestamp>,
    /// The current stage.
    pub stage: Option<StageName>,
    /// The progress.
    pub progress: Progress,
    /// The failure of a `fail` event.
    pub failure: Option<OperationFailure>,
    /// What can be done about it in `needs_recovery`.
    pub recovery_hint: Option<RecoveryHint>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it last changed.
    pub updated_at: Timestamp,
    /// The revision.
    pub revision: ResourceRevision,
}

/// One operation and where it stands.
///
/// Its changes (`apply`, `set_stage`, `set_progress`, `set_recovery_hint`)
/// move `updated_at` but not the revision: [`Operation::touch`] raises the
/// revision by exactly 1, once per registry batch that touches the
/// operation or its journal rows (spec 0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    id: OperationId,
    kind: OperationKind,
    resource: OperationResource,
    idempotency_key: Option<IdempotencyKey>,
    state: OperationState,
    started_at: Option<Timestamp>,
    stage: Option<StageName>,
    progress: Progress,
    failure: Option<OperationFailure>,
    recovery_hint: Option<RecoveryHint>,
    created_at: Timestamp,
    updated_at: Timestamp,
    revision: ResourceRevision,
}

impl Operation {
    /// Creates a `queued` operation that never started, with indeterminate
    /// progress, at revision 1.
    #[must_use]
    pub fn new(new: NewOperation) -> Self {
        Self {
            id: new.id,
            kind: new.kind,
            resource: new.resource,
            idempotency_key: new.idempotency_key,
            state: OperationState::Queued,
            started_at: None,
            stage: None,
            progress: Progress::Indeterminate,
            failure: None,
            recovery_hint: None,
            created_at: new.created_at,
            updated_at: new.created_at,
            revision: ResourceRevision::INITIAL,
        }
    }

    /// Rebuilds an operation the registry stored. Only the registry and
    /// test fakes call it.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] when a `failed` operation has
    /// no failure, a recovery hint is set outside `needs_recovery`, or
    /// counted progress is past its total.
    pub fn restore(stored: StoredOperation) -> Result<Self, InvariantError> {
        let inconsistent = |reason| InvariantDetail::InconsistentRecord {
            resource: "operation",
            reason,
        };
        if stored.state == OperationState::Failed && stored.failure.is_none() {
            return Err(inconsistent("a failed operation has no failure").into());
        }
        if stored.recovery_hint.is_some() && stored.state != OperationState::NeedsRecovery {
            return Err(inconsistent("a recovery hint is set outside needs_recovery").into());
        }
        if let Progress::Counted { done, total } = stored.progress
            && done > total
        {
            return Err(inconsistent("progress is past its total").into());
        }
        Ok(Self {
            id: stored.id,
            kind: stored.kind,
            resource: stored.resource,
            idempotency_key: stored.idempotency_key,
            state: stored.state,
            started_at: stored.started_at,
            stage: stored.stage,
            progress: stored.progress,
            failure: stored.failure,
            recovery_hint: stored.recovery_hint,
            created_at: stored.created_at,
            updated_at: stored.updated_at,
            revision: stored.revision,
        })
    }

    /// Returns the ID.
    #[must_use]
    pub const fn id(&self) -> OperationId {
        self.id
    }

    /// Returns the kind of work.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        self.kind
    }

    /// Returns what it works on.
    #[must_use]
    pub const fn resource(&self) -> OperationResource {
        self.resource
    }

    /// Returns the client's idempotency key, if any.
    #[must_use]
    pub const fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }

    /// Returns the state.
    #[must_use]
    pub const fn state(&self) -> OperationState {
        self.state
    }

    /// Returns when it first started, or `None` if it never did.
    #[must_use]
    pub const fn started_at(&self) -> Option<Timestamp> {
        self.started_at
    }

    /// Returns the current stage, if any.
    #[must_use]
    pub const fn stage(&self) -> Option<&StageName> {
        self.stage.as_ref()
    }

    /// Returns the progress.
    #[must_use]
    pub const fn progress(&self) -> Progress {
        self.progress
    }

    /// Returns the failure of a `fail` event, if any.
    #[must_use]
    pub const fn failure(&self) -> Option<&OperationFailure> {
        self.failure.as_ref()
    }

    /// Returns what can be done about it, while it is in `needs_recovery`.
    #[must_use]
    pub const fn recovery_hint(&self) -> Option<RecoveryHint> {
        self.recovery_hint
    }

    /// Returns when it was created.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// Returns when it last changed.
    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }

    /// Returns the revision: 1 when created, raised by exactly 1 by each
    /// [`Operation::touch`]. The registry compares it before every update.
    #[must_use]
    pub const fn revision(&self) -> ResourceRevision {
        self.revision
    }

    /// Raises the revision by 1 and records `at` as the last change. The
    /// engine calls it once before it builds each registry batch that
    /// touches the operation or its journal rows.
    pub const fn touch(&mut self, at: Timestamp) {
        self.updated_at = at;
        self.revision = self.revision.next();
    }

    /// Applies `event` at `at`. The first `start` records `started_at`,
    /// `fail` and `resolve(failed)` record the failure, and leaving
    /// `needs_recovery` clears the recovery hint. It moves `updated_at`,
    /// not the revision.
    ///
    /// # Errors
    ///
    /// Returns a [`TransitionError`] for a pair the table does not hold; the
    /// operation does not change.
    pub fn apply(
        &mut self,
        event: OperationEvent,
        at: Timestamp,
    ) -> Result<OperationState, TransitionError> {
        let next = self.state.apply(&event, self.started_at.is_some())?;
        match event {
            OperationEvent::Start => {
                self.started_at.get_or_insert(at);
            }
            OperationEvent::Fail(failure) | OperationEvent::Resolve(Outcome::Failed(failure)) => {
                self.failure = Some(failure)
            }
            _ => {}
        }
        if next != OperationState::NeedsRecovery {
            self.recovery_hint = None;
        }
        self.state = next;
        self.updated_at = at;
        Ok(next)
    }

    /// Records at `at` what can be done about the operation while it is in
    /// `needs_recovery`. It moves `updated_at`, not the revision.
    ///
    /// # Errors
    ///
    /// Returns a [`TransitionError`] outside `needs_recovery`; the
    /// operation does not change.
    pub fn set_recovery_hint(
        &mut self,
        hint: RecoveryHint,
        at: Timestamp,
    ) -> Result<(), TransitionError> {
        if self.state != OperationState::NeedsRecovery {
            return Err(TransitionError::new(
                Machine::Operation,
                self.state.as_code(),
                "set_recovery_hint",
            ));
        }
        self.recovery_hint = Some(hint);
        self.updated_at = at;
        Ok(())
    }

    /// Reports progress at `at`. It moves `updated_at`, not the revision.
    ///
    /// # Errors
    ///
    /// Returns [`ProgressError::NotAllowedInState`] outside `running` and
    /// `cancel_requested`, and [`ProgressError::OutOfRange`] when `done` is
    /// more than `total`. The operation does not change.
    pub fn set_progress(&mut self, progress: Progress, at: Timestamp) -> Result<(), ProgressError> {
        self.check_reporting()?;
        if let Progress::Counted { done, total } = progress
            && done > total
        {
            return Err(ProgressError::OutOfRange { done, total });
        }
        self.progress = progress;
        self.updated_at = at;
        Ok(())
    }

    /// Reports the stage the operation entered at `at`. It moves
    /// `updated_at`, not the revision.
    ///
    /// # Errors
    ///
    /// Returns [`ProgressError::NotAllowedInState`] outside `running` and
    /// `cancel_requested`; the operation does not change.
    pub fn set_stage(&mut self, stage: StageName, at: Timestamp) -> Result<(), ProgressError> {
        self.check_reporting()?;
        self.stage = Some(stage);
        self.updated_at = at;
        Ok(())
    }

    /// Progress and stage change only while the work runs or winds down.
    const fn check_reporting(&self) -> Result<(), ProgressError> {
        match self.state {
            OperationState::Running | OperationState::CancelRequested => Ok(()),
            state => Err(ProgressError::NotAllowedInState { state }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn operation() -> Operation {
        Operation::new(NewOperation {
            id: OperationId::generate(),
            kind: OperationKind::InstallRuntime,
            resource: OperationResource::Manager,
            idempotency_key: Some("req-1".parse().unwrap()),
            created_at: at("2026-01-01T00:00:00Z"),
        })
    }

    #[test]
    fn the_first_start_records_started_at_and_a_restart_keeps_it() {
        let mut op = operation();
        op.apply(OperationEvent::Start, at("2026-01-01T00:00:01Z"))
            .unwrap();
        op.apply(
            OperationEvent::Interrupted { resumable: true },
            at("2026-01-01T00:00:02Z"),
        )
        .unwrap();
        op.apply(OperationEvent::Start, at("2026-01-01T00:00:03Z"))
            .unwrap();

        assert_eq!(op.started_at(), Some(at("2026-01-01T00:00:01Z")));
        assert_eq!(op.updated_at(), at("2026-01-01T00:00:03Z"));
    }

    #[test]
    fn cancel_before_any_start_cancels_and_after_a_start_asks() {
        let mut fresh = operation();
        assert_eq!(
            fresh.apply(OperationEvent::RequestCancel, at("2026-01-01T00:00:01Z")),
            Ok(OperationState::Cancelled)
        );

        let mut resumed = operation();
        resumed
            .apply(OperationEvent::Start, at("2026-01-01T00:00:01Z"))
            .unwrap();
        resumed
            .apply(
                OperationEvent::Interrupted { resumable: true },
                at("2026-01-01T00:00:02Z"),
            )
            .unwrap();
        assert_eq!(
            resumed.apply(OperationEvent::RequestCancel, at("2026-01-01T00:00:03Z")),
            Ok(OperationState::CancelRequested)
        );
    }

    #[test]
    fn fail_records_the_failure() {
        let mut op = operation();
        let failure = OperationFailure::new("download_failed".parse().unwrap(), true);

        op.apply(
            OperationEvent::Fail(failure.clone()),
            at("2026-01-01T00:00:01Z"),
        )
        .unwrap();

        assert_eq!(op.failure(), Some(&failure));
        assert!(op.state().is_final());
    }

    #[test]
    fn a_recovery_hint_lives_only_in_needs_recovery() {
        let mut op = operation();
        assert!(
            op.set_recovery_hint(RecoveryHint::Manual, at("2026-01-01T00:00:01Z"))
                .is_err()
        );
        op.apply(OperationEvent::Start, at("2026-01-01T00:00:01Z"))
            .unwrap();
        op.apply(
            OperationEvent::CompensationFailed,
            at("2026-01-01T00:00:02Z"),
        )
        .unwrap();
        op.set_recovery_hint(RecoveryHint::Abandon, at("2026-01-01T00:00:03Z"))
            .unwrap();
        assert_eq!(op.recovery_hint(), Some(RecoveryHint::Abandon));

        op.apply(OperationEvent::Resume, at("2026-01-01T00:00:04Z"))
            .unwrap();

        assert_eq!(op.recovery_hint(), None);
        assert_eq!(op.revision(), ResourceRevision::INITIAL);
    }

    #[test]
    fn resolving_as_failed_records_the_failure() {
        let mut op = operation();
        op.apply(OperationEvent::Start, at("2026-01-01T00:00:01Z"))
            .unwrap();
        op.apply(
            OperationEvent::Interrupted { resumable: false },
            at("2026-01-01T00:00:02Z"),
        )
        .unwrap();
        let failure = OperationFailure::new(FailureCode::INTERRUPTED, true);

        let state = op
            .apply(
                OperationEvent::Resolve(Outcome::Failed(failure.clone())),
                at("2026-01-01T00:00:03Z"),
            )
            .unwrap();

        assert_eq!(state, OperationState::Failed);
        assert_eq!(op.failure(), Some(&failure));
    }

    #[test]
    fn only_the_kinds_in_the_resume_table_are_resumable() {
        let resumable: Vec<&str> = OperationKind::ALL
            .iter()
            .filter(|kind| kind.resumable())
            .map(|kind| kind.as_code())
            .collect();

        assert_eq!(
            resumable,
            [
                "install_runtime",
                "backup_instance",
                "environment_up",
                "environment_down",
            ]
        );
    }

    #[test]
    fn only_the_four_instance_process_kinds_close_as_interrupted() {
        let closing: Vec<&str> = OperationKind::ALL
            .iter()
            .filter(|kind| kind.interrupt_policy() == InterruptPolicy::CloseInterrupted)
            .map(|kind| kind.as_code())
            .collect();

        assert_eq!(
            closing,
            [
                "start_instance",
                "stop_instance",
                "restart_instance",
                "reload_instance",
            ]
        );
        assert!(
            OperationKind::ALL
                .iter()
                .filter(|kind| kind.resumable())
                .all(|kind| kind.interrupt_policy() == InterruptPolicy::Compensate),
            "a kind that closes as interrupted never resumes"
        );
    }

    #[test]
    fn progress_must_fit_its_total_and_change_only_while_running() {
        let mut op = operation();
        let half = Progress::Counted { done: 5, total: 10 };

        assert_eq!(
            op.set_progress(half, at("2026-01-01T00:00:01Z")),
            Err(ProgressError::NotAllowedInState {
                state: OperationState::Queued
            })
        );
        op.apply(OperationEvent::Start, at("2026-01-01T00:00:01Z"))
            .unwrap();
        assert_eq!(op.set_progress(half, at("2026-01-01T00:00:02Z")), Ok(()));
        assert_eq!(
            op.set_progress(
                Progress::Counted {
                    done: 11,
                    total: 10
                },
                at("2026-01-01T00:00:03Z")
            ),
            Err(ProgressError::OutOfRange {
                done: 11,
                total: 10
            })
        );
        assert_eq!(op.progress(), half);
        op.apply(OperationEvent::RequestCancel, at("2026-01-01T00:00:04Z"))
            .unwrap();
        assert_eq!(
            op.set_stage("rollback".parse().unwrap(), at("2026-01-01T00:00:05Z")),
            Ok(())
        );
        op.apply(OperationEvent::Cancelled, at("2026-01-01T00:00:06Z"))
            .unwrap();
        assert!(op.set_progress(half, at("2026-01-01T00:00:07Z")).is_err());
        assert_eq!(op.stage().map(StageName::as_str), Some("rollback"));
    }
}
