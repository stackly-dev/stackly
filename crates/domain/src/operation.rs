//! Operations: the seven state machine of PRD section 18.2, the kinds of
//! work, and progress (BR-006). The engine with its journal, locks, and
//! idempotency comes with feature 7.

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{Machine, ProgressError, TransitionError};
use crate::id::{EnvironmentId, InstanceId, OperationId, RuntimeId};
use crate::value::{FailureCode, IdempotencyKey, StageName};

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

/// How an operation in `needs_recovery` was settled by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// It is settled as succeeded.
    Succeeded,
    /// It is settled as failed.
    Failed,
    /// It is settled as cancelled.
    Cancelled,
}

impl Outcome {
    /// Returns the final operation state this outcome names.
    #[must_use]
    pub const fn state(self) -> OperationState {
        match self {
            Self::Succeeded => OperationState::Succeeded,
            Self::Failed => OperationState::Failed,
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
    /// Undoing finished steps failed.
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

/// One operation and where it stands.
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
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl Operation {
    /// Creates a `queued` operation that never started, with indeterminate
    /// progress.
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
            created_at: new.created_at,
            updated_at: new.created_at,
        }
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

    /// Applies `event` at `at`. The first `start` records `started_at`, and
    /// `fail` records the failure.
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
            OperationEvent::Fail(failure) => self.failure = Some(failure),
            _ => {}
        }
        self.state = next;
        self.updated_at = at;
        Ok(next)
    }

    /// Reports progress at `at`.
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

    /// Reports the stage the operation entered at `at`.
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
