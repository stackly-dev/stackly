//! Installed runtimes: their state machine, how they were installed, and
//! when they may be removed (BR-007, BR-019).

use std::collections::BTreeSet;
use std::path::PathBuf;

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{DomainError, InvariantDetail, InvariantError, Machine, TransitionError};
use crate::id::{BackupId, InstanceId, OperationId, RuntimeId};
use crate::manifest::InstallKind;
use crate::target::Target;
use crate::value::{ExactVersion, ResourceRevision, RuntimeLine, Sha256Digest, SoftwareName};

mod selector;

pub use selector::{RuntimeSelector, SELECTOR_MAX};

code_enum! {
    /// The state of an installed runtime (PRD section 19.1).
    pub enum RuntimeState: RuntimeState {
        /// Removed. The record stays until the registry drops it, and
        /// `resolve` installs the same identity again.
        Absent => "absent",
        /// The selector resolved to an exact identity; a new runtime starts
        /// here.
        Resolving => "resolving",
        /// The managed artifact is downloading.
        Downloading => "downloading",
        /// The artifact, or the external folder, is being verified.
        Verifying => "verifying",
        /// The files are being installed, or an external runtime registered.
        Installing => "installing",
        /// Ready to use.
        Installed => "installed",
        /// A step before `installed` failed.
        Failed => "failed",
        /// The installed files are gone.
        Missing => "missing",
        /// The installed files changed.
        Drifted => "drifted",
    }
}

/// An event of the runtime state machine. None carries data: `resolved`
/// reads the install mode from the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeEvent {
    /// Start installing a removed runtime again.
    Resolve,
    /// The exact artifact or external folder is known.
    Resolved,
    /// The managed artifact finished downloading.
    Downloaded,
    /// The artifact or folder passed verification.
    Verified,
    /// The install, or the registration, was committed.
    Committed,
    /// A step before `installed` failed.
    Failed,
    /// Try again after a failure, or after files went missing or drifted.
    Retry,
    /// Reconciliation found the files gone.
    ObservedMissing,
    /// Reconciliation found the files changed.
    ObservedDrift,
    /// Verification passed again.
    Reverified,
    /// The runtime was removed.
    Removed,
}

impl RuntimeEvent {
    /// Every event, in table order.
    pub const ALL: [Self; 11] = [
        Self::Resolve,
        Self::Resolved,
        Self::Downloaded,
        Self::Verified,
        Self::Committed,
        Self::Failed,
        Self::Retry,
        Self::ObservedMissing,
        Self::ObservedDrift,
        Self::Reverified,
        Self::Removed,
    ];

    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::Resolved => "resolved",
            Self::Downloaded => "downloaded",
            Self::Verified => "verified",
            Self::Committed => "committed",
            Self::Failed => "failed",
            Self::Retry => "retry",
            Self::ObservedMissing => "observed_missing",
            Self::ObservedDrift => "observed_drift",
            Self::Reverified => "reverified",
            Self::Removed => "removed",
        }
    }
}

impl RuntimeState {
    /// Applies `event` to a runtime installed the `install` way, following
    /// the table of spec 0004.
    ///
    /// # Errors
    ///
    /// Returns a [`TransitionError`] for a pair the table does not hold.
    pub const fn apply(
        self,
        event: RuntimeEvent,
        install: InstallKind,
    ) -> Result<Self, TransitionError> {
        use RuntimeEvent as E;
        use RuntimeState as S;
        match (self, event) {
            (S::Absent, E::Resolve) => Ok(S::Resolving),
            (S::Resolving, E::Resolved) => match install {
                InstallKind::Managed => Ok(S::Downloading),
                InstallKind::External => Ok(S::Verifying),
            },
            (S::Downloading, E::Downloaded) => Ok(S::Verifying),
            (S::Verifying, E::Verified) => Ok(S::Installing),
            (S::Installing, E::Committed) => Ok(S::Installed),
            (S::Resolving | S::Downloading | S::Verifying | S::Installing, E::Failed) => {
                Ok(S::Failed)
            }
            (S::Installed, E::ObservedMissing) => Ok(S::Missing),
            (S::Installed, E::ObservedDrift) => Ok(S::Drifted),
            (S::Missing | S::Drifted, E::Reverified) => Ok(S::Installed),
            (S::Failed | S::Missing | S::Drifted, E::Retry) => Ok(S::Resolving),
            (S::Failed | S::Installed | S::Missing | S::Drifted, E::Removed) => Ok(S::Absent),
            _ => Err(TransitionError::new(
                Machine::Runtime,
                self.as_code(),
                event.as_code(),
            )),
        }
    }
}

/// How a runtime got onto the machine.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InstallMode {
    /// Stackly installed the artifact with this digest; its path follows
    /// from the layout.
    Managed {
        /// The digest of the installed artifact.
        artifact_sha256: Sha256Digest,
    },
    /// The runtime lives in a folder the user owns (BR-019).
    External {
        /// The folder the user registered.
        root: PathBuf,
        /// The digest Stackly observed for it, if any.
        observed_sha256: Option<Sha256Digest>,
    },
}

impl InstallMode {
    /// Returns whether the runtime is managed or external.
    #[must_use]
    pub const fn kind(&self) -> InstallKind {
        match self {
            Self::Managed { .. } => InstallKind::Managed,
            Self::External { .. } => InstallKind::External,
        }
    }
}

/// The values a new [`Runtime`] starts from, once its selector resolved to
/// an exact identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRuntime {
    /// The runtime's ID, created by the daemon.
    pub id: RuntimeId,
    /// The software it runs.
    pub software: SoftwareName,
    /// The exact version.
    pub version: ExactVersion,
    /// The line of that version.
    pub line: RuntimeLine,
    /// The OS and architecture it is built for.
    pub target: Target,
    /// How it is installed.
    pub install: InstallMode,
    /// The runtimes it needs.
    pub depends_on: BTreeSet<RuntimeId>,
}

/// Every field of a [`Runtime`] as the registry stores it, for
/// [`Runtime::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRuntime {
    /// The runtime's ID.
    pub id: RuntimeId,
    /// The software it runs.
    pub software: SoftwareName,
    /// The exact version.
    pub version: ExactVersion,
    /// The line of that version.
    pub line: RuntimeLine,
    /// The OS and architecture it is built for.
    pub target: Target,
    /// How it is installed.
    pub install: InstallMode,
    /// The current state.
    pub state: RuntimeState,
    /// When the install or registration was last committed.
    pub installed_at: Option<Timestamp>,
    /// When verification last passed.
    pub last_verified_at: Option<Timestamp>,
    /// The runtimes it needs.
    pub depends_on: BTreeSet<RuntimeId>,
    /// The revision.
    pub revision: ResourceRevision,
}

/// An installed runtime: one exact version of one software for one target.
///
/// Every successful mutation raises [`Runtime::revision`] by exactly 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    id: RuntimeId,
    software: SoftwareName,
    version: ExactVersion,
    line: RuntimeLine,
    target: Target,
    install: InstallMode,
    state: RuntimeState,
    installed_at: Option<Timestamp>,
    last_verified_at: Option<Timestamp>,
    depends_on: BTreeSet<RuntimeId>,
    revision: ResourceRevision,
}

impl Runtime {
    /// Creates a runtime in `resolving`, the state of a selector that
    /// resolved to an exact identity, at revision 1.
    #[must_use]
    pub fn new(new: NewRuntime) -> Self {
        Self {
            id: new.id,
            software: new.software,
            version: new.version,
            line: new.line,
            target: new.target,
            install: new.install,
            state: RuntimeState::Resolving,
            installed_at: None,
            last_verified_at: None,
            depends_on: new.depends_on,
            revision: ResourceRevision::INITIAL,
        }
    }

    /// Creates a runtime that an install just committed: it walks
    /// `resolved`, `downloaded`, `verified`, and `committed` from
    /// `resolving`, and returns the runtime `installed` at revision 1,
    /// because the registry insert needs revision 1 and one commit is one
    /// mutation (spec 0018).
    ///
    /// `verified_at` is when validation passed and `committed_at` when the
    /// install was committed.
    ///
    /// # Errors
    ///
    /// Returns a [`TransitionError`] when the state table rejects one of
    /// the four events for this install mode, which a managed runtime does
    /// not.
    pub fn committed(
        new: NewRuntime,
        verified_at: Timestamp,
        committed_at: Timestamp,
    ) -> Result<Self, TransitionError> {
        let mut runtime = Self::new(new);
        for event in [
            RuntimeEvent::Resolved,
            RuntimeEvent::Downloaded,
            RuntimeEvent::Verified,
            RuntimeEvent::Committed,
        ] {
            let at = if event == RuntimeEvent::Committed {
                committed_at
            } else {
                verified_at
            };
            runtime.apply(event, at)?;
        }
        runtime.revision = ResourceRevision::INITIAL;
        Ok(runtime)
    }

    /// Rebuilds a runtime the registry stored. Only the registry and test
    /// fakes call it.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] when the runtime depends on
    /// itself, or is `installed`, `missing`, or `drifted` without
    /// `installed_at`, which only a committed install sets.
    pub fn restore(stored: StoredRuntime) -> Result<Self, InvariantError> {
        let inconsistent = |reason| InvariantDetail::InconsistentRecord {
            resource: "runtime",
            reason,
        };
        if stored.depends_on.contains(&stored.id) {
            return Err(inconsistent("it depends on itself").into());
        }
        let was_installed = matches!(
            stored.state,
            RuntimeState::Installed | RuntimeState::Missing | RuntimeState::Drifted
        );
        if was_installed && stored.installed_at.is_none() {
            return Err(inconsistent("an installed runtime has no installed_at").into());
        }
        Ok(Self {
            id: stored.id,
            software: stored.software,
            version: stored.version,
            line: stored.line,
            target: stored.target,
            install: stored.install,
            state: stored.state,
            installed_at: stored.installed_at,
            last_verified_at: stored.last_verified_at,
            depends_on: stored.depends_on,
            revision: stored.revision,
        })
    }

    /// Returns the runtime's ID.
    #[must_use]
    pub const fn id(&self) -> RuntimeId {
        self.id
    }

    /// Returns the software.
    #[must_use]
    pub const fn software(&self) -> &SoftwareName {
        &self.software
    }

    /// Returns the exact version.
    #[must_use]
    pub const fn version(&self) -> &ExactVersion {
        &self.version
    }

    /// Returns the line.
    #[must_use]
    pub const fn line(&self) -> &RuntimeLine {
        &self.line
    }

    /// Returns the OS and architecture it is built for.
    #[must_use]
    pub const fn target(&self) -> Target {
        self.target
    }

    /// Returns how it is installed.
    #[must_use]
    pub const fn install(&self) -> &InstallMode {
        &self.install
    }

    /// Returns the current state.
    #[must_use]
    pub const fn state(&self) -> RuntimeState {
        self.state
    }

    /// Returns when the install or registration was last committed.
    #[must_use]
    pub const fn installed_at(&self) -> Option<Timestamp> {
        self.installed_at
    }

    /// Returns when verification last passed.
    #[must_use]
    pub const fn last_verified_at(&self) -> Option<Timestamp> {
        self.last_verified_at
    }

    /// Returns the runtimes this one needs.
    #[must_use]
    pub const fn depends_on(&self) -> &BTreeSet<RuntimeId> {
        &self.depends_on
    }

    /// Returns the revision: 1 when created, raised by exactly 1 per
    /// successful mutation. The registry compares it before every update.
    #[must_use]
    pub const fn revision(&self) -> ResourceRevision {
        self.revision
    }

    /// Applies `event` at `at` and raises the revision. `committed` records
    /// `installed_at`, and `verified` and `reverified` record
    /// `last_verified_at`. Removal goes through [`Runtime::remove`] instead,
    /// which checks references first.
    ///
    /// # Errors
    ///
    /// Returns a [`TransitionError`] for a pair the table does not hold, or
    /// for `removed`; the runtime does not change.
    pub fn apply(
        &mut self,
        event: RuntimeEvent,
        at: Timestamp,
    ) -> Result<RuntimeState, TransitionError> {
        if event == RuntimeEvent::Removed {
            return Err(TransitionError::new(
                Machine::Runtime,
                self.state.as_code(),
                event.as_code(),
            ));
        }
        self.state = self.state.apply(event, self.install.kind())?;
        match event {
            RuntimeEvent::Committed => self.installed_at = Some(at),
            RuntimeEvent::Verified | RuntimeEvent::Reverified => self.last_verified_at = Some(at),
            _ => {}
        }
        self.revision = self.revision.next();
        Ok(self.state)
    }

    /// Removes the runtime: it checks the `removed` transition and
    /// [`plan_runtime_removal`], then moves to `absent`, raises the revision,
    /// and returns the plan.
    ///
    /// # Errors
    ///
    /// Returns a [`DomainError`]: a transition error when the state does
    /// not allow removal, or the BR-007 invariant error of
    /// [`plan_runtime_removal`]. The runtime does not change.
    pub fn remove(&mut self, refs: &RuntimeReferences) -> Result<RemovalPlan, DomainError> {
        let next = self
            .state
            .apply(RuntimeEvent::Removed, self.install.kind())?;
        let plan = plan_runtime_removal(self, refs)?;
        self.state = next;
        self.revision = self.revision.next();
        Ok(plan)
    }
}

/// Everything that still refers to a runtime, filled by the caller from the
/// registry (features 5 and 55).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeReferences {
    /// Instances on this runtime, in every lifecycle state, `stopped` and
    /// `retained` included.
    pub instances: Vec<InstanceId>,
    /// Runtimes that depend on this one.
    pub dependent_runtimes: Vec<RuntimeId>,
    /// Operations that are not finished and touch this runtime.
    pub active_operations: Vec<OperationId>,
    /// Recovery snapshots that need this runtime to restore.
    pub recovery_snapshots: Vec<BackupId>,
}

/// One reference that keeps a runtime from being removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemovalBlocker {
    /// An instance runs on it.
    Instance(InstanceId),
    /// Another runtime depends on it.
    DependentRuntime(RuntimeId),
    /// An unfinished operation touches it.
    ActiveOperation(OperationId),
    /// A recovery snapshot needs it.
    RecoverySnapshot(BackupId),
}

/// What removing a runtime does with its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemovalPlan {
    /// Delete the managed files under the runtimes folder.
    DeleteManagedFiles,
    /// Only forget the registration; the user's folder stays (BR-019).
    UnregisterOnly,
}

/// Decides whether a runtime may be removed and what removal does.
///
/// Every reference in `refs` blocks the removal (BR-007). A managed runtime
/// that nothing refers to has its files deleted; an external runtime is
/// only unregistered, and its files are never deleted (BR-019).
///
/// # Errors
///
/// Returns an [`InvariantError`] for BR-007 with
/// [`InvariantDetail::RemovalBlocked`], listing every blocker.
pub fn plan_runtime_removal(
    runtime: &Runtime,
    refs: &RuntimeReferences,
) -> Result<RemovalPlan, InvariantError> {
    let blockers: Vec<RemovalBlocker> = refs
        .instances
        .iter()
        .copied()
        .map(RemovalBlocker::Instance)
        .chain(
            refs.dependent_runtimes
                .iter()
                .copied()
                .map(RemovalBlocker::DependentRuntime),
        )
        .chain(
            refs.active_operations
                .iter()
                .copied()
                .map(RemovalBlocker::ActiveOperation),
        )
        .chain(
            refs.recovery_snapshots
                .iter()
                .copied()
                .map(RemovalBlocker::RecoverySnapshot),
        )
        .collect();
    if !blockers.is_empty() {
        return Err(InvariantDetail::RemovalBlocked { blockers }.into());
    }
    Ok(match runtime.install.kind() {
        InstallKind::Managed => RemovalPlan::DeleteManagedFiles,
        InstallKind::External => RemovalPlan::UnregisterOnly,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::{Arch, Os};

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn runtime(install: InstallMode) -> Runtime {
        Runtime::new(NewRuntime {
            id: RuntimeId::generate(),
            software: "postgresql".parse().unwrap(),
            version: "17.6".parse().unwrap(),
            line: "17".parse().unwrap(),
            target: Target::new(Os::Macos, Arch::Aarch64),
            install,
            depends_on: BTreeSet::new(),
        })
    }

    fn managed() -> InstallMode {
        InstallMode::Managed {
            artifact_sha256: "ab".repeat(32).parse().unwrap(),
        }
    }

    #[test]
    fn a_managed_runtime_downloads_and_records_its_install_and_verify_times() {
        let mut runtime = runtime(managed());

        assert_eq!(runtime.state(), RuntimeState::Resolving);
        for (event, minute) in [
            (RuntimeEvent::Resolved, 1),
            (RuntimeEvent::Downloaded, 2),
            (RuntimeEvent::Verified, 3),
            (RuntimeEvent::Committed, 4),
        ] {
            runtime
                .apply(event, at(&format!("2026-01-01T00:0{minute}:00Z")))
                .unwrap();
        }

        assert_eq!(runtime.state(), RuntimeState::Installed);
        assert_eq!(runtime.last_verified_at(), Some(at("2026-01-01T00:03:00Z")));
        assert_eq!(runtime.installed_at(), Some(at("2026-01-01T00:04:00Z")));
    }

    #[test]
    fn a_committed_runtime_is_installed_at_revision_one_with_both_times() {
        let new = |install| NewRuntime {
            id: RuntimeId::generate(),
            software: "nats".parse().unwrap(),
            version: "2.15.1".parse().unwrap(),
            line: "2.15".parse().unwrap(),
            target: Target::new(Os::Macos, Arch::Aarch64),
            install,
            depends_on: BTreeSet::new(),
        };

        let runtime = Runtime::committed(
            new(managed()),
            at("2026-01-01T00:03:00Z"),
            at("2026-01-01T00:04:00Z"),
        )
        .unwrap();

        assert_eq!(runtime.state(), RuntimeState::Installed);
        assert_eq!(runtime.revision(), ResourceRevision::INITIAL);
        assert_eq!(runtime.last_verified_at(), Some(at("2026-01-01T00:03:00Z")));
        assert_eq!(runtime.installed_at(), Some(at("2026-01-01T00:04:00Z")));

        let external = InstallMode::External {
            root: PathBuf::from("pg"),
            observed_sha256: None,
        };
        assert!(
            Runtime::committed(
                new(external),
                at("2026-01-01T00:03:00Z"),
                at("2026-01-01T00:04:00Z")
            )
            .is_err()
        );
    }

    #[test]
    fn an_external_runtime_skips_the_download() {
        let mut runtime = runtime(InstallMode::External {
            root: PathBuf::from("pg"),
            observed_sha256: None,
        });

        let state = runtime
            .apply(RuntimeEvent::Resolved, at("2026-01-01T00:00:00Z"))
            .unwrap();

        assert_eq!(state, RuntimeState::Verifying);
    }

    #[test]
    fn a_refused_event_leaves_the_runtime_unchanged() {
        let mut runtime = runtime(managed());
        let before = runtime.clone();

        let err = runtime
            .apply(RuntimeEvent::Committed, at("2026-01-01T00:00:00Z"))
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "runtime cannot apply committed in state resolving"
        );
        assert_eq!(runtime, before);
    }

    #[test]
    fn removed_goes_through_remove_not_apply() {
        let mut runtime = runtime(managed());
        runtime
            .apply(RuntimeEvent::Failed, at("2026-01-01T00:00:00Z"))
            .unwrap();

        assert!(
            runtime
                .apply(RuntimeEvent::Removed, at("2026-01-01T00:00:00Z"))
                .is_err()
        );
        assert_eq!(
            runtime.remove(&RuntimeReferences::default()).unwrap(),
            RemovalPlan::DeleteManagedFiles
        );
        assert_eq!(runtime.state(), RuntimeState::Absent);
    }

    #[test]
    fn remove_in_a_state_without_removed_is_a_transition_error() {
        let mut runtime = runtime(managed());

        let err = runtime.remove(&RuntimeReferences::default()).unwrap_err();

        assert!(matches!(err, DomainError::Transition(_)), "{err}");
        assert_eq!(runtime.state(), RuntimeState::Resolving);
    }
}
