//! Instances: the lifecycle state machine and the `Instance` aggregate,
//! which ties the lifecycle to readiness, health, and the data rules
//! (BR-001, BR-003, BR-008, BR-009, BR-010, BR-013, BR-017).

use jiff::Timestamp;

use crate::code::code_enum;
use crate::endpoint::{Endpoint, PortRequest};
use crate::error::{DomainError, InvariantDetail, InvariantError, Machine, TransitionError};
use crate::id::{InstanceId, OperationId, RuntimeId};
use crate::observation::{
    Health, HealthState, Ignored, ObservationOutcome, Readiness, ReadinessState,
};
use crate::operation::{Operation, OperationKind, OperationResource, OperationState};
use crate::provider::{AcceptedRevisionChange, LineCompatibility, ProviderRevisionKey};
use crate::secret::InstanceBlocker;
use crate::value::{
    DisplayName, EndpointName, ExactVersion, FailureCode, InstanceName, Port, ReasonCode,
    ResourceRevision, RuntimeLine, SoftwareName,
};

code_enum! {
    /// The lifecycle of an instance (PRD section 19.1). Readiness and health
    /// are separate values, never lifecycle states.
    pub enum InstanceLifecycle: InstanceLifecycle {
        /// Created; the data folder is not initialized yet.
        Uninitialized => "uninitialized",
        /// The data folder is being initialized.
        Initializing => "initializing",
        /// Not running.
        Stopped => "stopped",
        /// The process is starting.
        Starting => "starting",
        /// The process runs.
        Running => "running",
        /// The process is stopping. A stop past its timeout stays here.
        Stopping => "stopping",
        /// Initialization, a start, or the process failed.
        Failed => "failed",
        /// The process crashed and is being restarted.
        Recovering => "recovering",
        /// The identity of the process is lost; it is never signalled.
        Unknown => "unknown",
        /// Removed with its data kept; the final state.
        Retained => "retained",
    }
}

/// An event of the lifecycle state machine.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LifecycleEvent {
    /// Data initialization begins.
    InitBegin,
    /// Data initialization succeeded.
    InitSucceeded,
    /// Data initialization failed, or was interrupted.
    InitFailed {
        /// Why it failed, for example `interrupted`.
        reason: FailureCode,
    },
    /// A start begins.
    StartBegin,
    /// The process started.
    Spawned,
    /// The start failed.
    StartFailed {
        /// Why it failed.
        reason: FailureCode,
    },
    /// The process exited without a stop.
    Crashed {
        /// Whether the supervisor restarts it.
        will_retry: bool,
        /// Why it crashed.
        reason: FailureCode,
    },
    /// A stop begins.
    StopBegin,
    /// The process exited after a stop.
    Exited,
    /// The identity of the process can no longer be proven.
    IdentityLost,
    /// A process with a proven identity was found running.
    Adopted,
    /// No process is running after all.
    ConfirmedStopped,
    /// The instance is removed and its data kept.
    Retain {
        /// The operation that retains it.
        by: OperationId,
    },
}

impl LifecycleEvent {
    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(&self) -> &'static str {
        match self {
            Self::InitBegin => "init_begin",
            Self::InitSucceeded => "init_succeeded",
            Self::InitFailed { .. } => "init_failed",
            Self::StartBegin => "start_begin",
            Self::Spawned => "spawned",
            Self::StartFailed { .. } => "start_failed",
            Self::Crashed { .. } => "crashed",
            Self::StopBegin => "stop_begin",
            Self::Exited => "exited",
            Self::IdentityLost => "identity_lost",
            Self::Adopted => "adopted",
            Self::ConfirmedStopped => "confirmed_stopped",
            Self::Retain { .. } => "retain",
        }
    }
}

impl InstanceLifecycle {
    /// Applies `event`, following the table of spec 0004. This is the pure
    /// machine; [`Instance::apply_lifecycle`] adds the aggregate rules.
    ///
    /// # Errors
    ///
    /// Returns a [`TransitionError`] for a pair the table does not hold.
    pub const fn apply(self, event: &LifecycleEvent) -> Result<Self, TransitionError> {
        use InstanceLifecycle as S;
        use LifecycleEvent as E;
        match (self, event) {
            (S::Uninitialized, E::InitBegin) => Ok(S::Initializing),
            (S::Initializing, E::InitSucceeded) => Ok(S::Stopped),
            (S::Initializing, E::InitFailed { .. }) | (S::Starting, E::StartFailed { .. }) => {
                Ok(S::Failed)
            }
            (S::Stopped | S::Failed, E::StartBegin) => Ok(S::Starting),
            (S::Starting | S::Recovering, E::Spawned) | (S::Unknown, E::Adopted) => Ok(S::Running),
            (S::Running | S::Recovering, E::Crashed { will_retry, .. }) => {
                if *will_retry {
                    Ok(S::Recovering)
                } else {
                    Ok(S::Failed)
                }
            }
            (S::Starting | S::Running | S::Recovering, E::StopBegin) => Ok(S::Stopping),
            (S::Stopping, E::Exited) | (S::Unknown, E::ConfirmedStopped) => Ok(S::Stopped),
            (S::Starting | S::Running | S::Stopping | S::Recovering, E::IdentityLost) => {
                Ok(S::Unknown)
            }
            (S::Uninitialized | S::Stopped | S::Failed, E::Retain { .. }) => Ok(S::Retained),
            _ => Err(TransitionError::new(
                Machine::InstanceLifecycle,
                self.as_code(),
                event.as_code(),
            )),
        }
    }
}

code_enum! {
    /// Whether the user wants an instance running. It is stored apart from
    /// the lifecycle, and lifecycle events never change it.
    pub enum DesiredState: DesiredState {
        /// The user wants it running.
        Running => "running",
        /// The user wants it stopped.
        Stopped => "stopped",
    }
}

code_enum! {
    /// When an instance starts on its own.
    pub enum StartupPolicy: StartupPolicy {
        /// Only when the user starts it.
        Manual => "manual",
        /// At login, while the session holds a startup lease (feature 31).
        AtLogin => "at_login",
    }
}

/// The values a new [`Instance`] starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInstance {
    /// The new instance's ID, created by the daemon.
    pub id: InstanceId,
    /// The instance name.
    pub name: InstanceName,
    /// The optional display name.
    pub display_name: Option<DisplayName>,
    /// The exact provider revision, never a selector (BR-001).
    pub provider_revision: ProviderRevisionKey,
    /// The software of that provider revision.
    pub software: SoftwareName,
    /// The exact runtime, never a selector (BR-001).
    pub runtime_id: RuntimeId,
    /// The runtime line that initializes the data.
    pub data_line: RuntimeLine,
    /// The exact runtime version that initializes the data.
    pub data_version: ExactVersion,
    /// The endpoints.
    pub endpoints: Vec<Endpoint>,
    /// When the instance starts on its own.
    pub startup: StartupPolicy,
    /// The time of the create, from the platform clock.
    pub created_at: Timestamp,
    /// The create operation.
    pub created_by: OperationId,
}

/// An instance: one data folder run by one exact runtime under one exact
/// provider revision.
///
/// Lifecycle, desired state, readiness, and health are separate fields.
/// Every mutation raises [`Instance::revision`] by exactly 1, except
/// readiness, health, and blocker observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    id: InstanceId,
    name: InstanceName,
    display_name: Option<DisplayName>,
    provider_revision: ProviderRevisionKey,
    software: SoftwareName,
    runtime_id: RuntimeId,
    data_line: RuntimeLine,
    data_version: ExactVersion,
    data_initialized_at: Option<Timestamp>,
    endpoints: Vec<Endpoint>,
    desired: DesiredState,
    startup: StartupPolicy,
    lifecycle: InstanceLifecycle,
    last_failure: Option<FailureCode>,
    readiness: Readiness,
    health: Health,
    revision: ResourceRevision,
    created_at: Timestamp,
    created_by: OperationId,
    retained_at: Option<Timestamp>,
    retained_by: Option<OperationId>,
    blocker: Option<InstanceBlocker>,
}

/// Every stored field of an [`Instance`], for [`Instance::restore`].
/// Readiness and health are never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredInstance {
    /// The ID.
    pub id: InstanceId,
    /// The name.
    pub name: InstanceName,
    /// The display name, if any.
    pub display_name: Option<DisplayName>,
    /// The exact provider revision.
    pub provider_revision: ProviderRevisionKey,
    /// The software.
    pub software: SoftwareName,
    /// The exact runtime.
    pub runtime_id: RuntimeId,
    /// The runtime line of the data.
    pub data_line: RuntimeLine,
    /// The exact runtime version of the data.
    pub data_version: ExactVersion,
    /// When data initialization succeeded.
    pub data_initialized_at: Option<Timestamp>,
    /// The endpoints, in order.
    pub endpoints: Vec<Endpoint>,
    /// The desired state.
    pub desired: DesiredState,
    /// The startup policy.
    pub startup: StartupPolicy,
    /// The lifecycle state.
    pub lifecycle: InstanceLifecycle,
    /// Why the last init, start, or process failed.
    pub last_failure: Option<FailureCode>,
    /// The revision.
    pub revision: ResourceRevision,
    /// When the instance was created.
    pub created_at: Timestamp,
    /// The operation that created it.
    pub created_by: OperationId,
    /// When it was retained.
    pub retained_at: Option<Timestamp>,
    /// The operation that retained it.
    pub retained_by: Option<OperationId>,
}

impl Instance {
    /// Creates an instance: `uninitialized`, desired `stopped`, readiness
    /// and health `unknown` with reason `never_observed`, revision 1.
    #[must_use]
    pub fn new(new: NewInstance) -> Self {
        Self {
            id: new.id,
            name: new.name,
            display_name: new.display_name,
            provider_revision: new.provider_revision,
            software: new.software,
            runtime_id: new.runtime_id,
            data_line: new.data_line,
            data_version: new.data_version,
            data_initialized_at: None,
            endpoints: new.endpoints,
            desired: DesiredState::Stopped,
            startup: new.startup,
            lifecycle: InstanceLifecycle::Uninitialized,
            last_failure: None,
            readiness: Readiness::unknown(new.created_at, ReasonCode::NEVER_OBSERVED),
            health: Health::unknown(new.created_at, ReasonCode::NEVER_OBSERVED),
            revision: ResourceRevision::INITIAL,
            created_at: new.created_at,
            created_by: new.created_by,
            retained_at: None,
            retained_by: None,
            blocker: None,
        }
    }

    /// Rebuilds an instance the registry stored, with readiness and health
    /// `unknown`, reason `daemon_restarted`, observed at `loaded_at` (the
    /// time the registry was opened), so the first probe after a restart is
    /// never taken for a stale one. Only the registry and test fakes call
    /// it.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] when `retained` and
    /// `retained_at` disagree, `retained_at` and `retained_by` disagree, or
    /// a lifecycle that runs or ran the software has no
    /// `data_initialized_at` (BR-008).
    pub fn restore(stored: StoredInstance, loaded_at: Timestamp) -> Result<Self, InvariantError> {
        let inconsistent = |reason| InvariantDetail::InconsistentRecord {
            resource: "instance",
            reason,
        };
        let retained = stored.lifecycle == InstanceLifecycle::Retained;
        if retained != stored.retained_at.is_some() {
            return Err(inconsistent("retained_at is set exactly when it is retained").into());
        }
        if stored.retained_at.is_some() != stored.retained_by.is_some() {
            return Err(inconsistent("retained_at and retained_by are set together").into());
        }
        let needs_data = matches!(
            stored.lifecycle,
            InstanceLifecycle::Stopped
                | InstanceLifecycle::Starting
                | InstanceLifecycle::Running
                | InstanceLifecycle::Stopping
                | InstanceLifecycle::Recovering
                | InstanceLifecycle::Unknown
        );
        if needs_data && stored.data_initialized_at.is_none() {
            return Err(inconsistent("its lifecycle needs initialized data (BR-008)").into());
        }
        Ok(Self {
            id: stored.id,
            name: stored.name,
            display_name: stored.display_name,
            provider_revision: stored.provider_revision,
            software: stored.software,
            runtime_id: stored.runtime_id,
            data_line: stored.data_line,
            data_version: stored.data_version,
            data_initialized_at: stored.data_initialized_at,
            endpoints: stored.endpoints,
            desired: stored.desired,
            startup: stored.startup,
            lifecycle: stored.lifecycle,
            last_failure: stored.last_failure,
            readiness: Readiness::unknown(loaded_at, ReasonCode::DAEMON_RESTARTED),
            health: Health::unknown(loaded_at, ReasonCode::DAEMON_RESTARTED),
            revision: stored.revision,
            created_at: stored.created_at,
            created_by: stored.created_by,
            retained_at: stored.retained_at,
            retained_by: stored.retained_by,
            blocker: None,
        })
    }

    /// Returns the ID. Every path of the instance follows from it.
    #[must_use]
    pub const fn id(&self) -> InstanceId {
        self.id
    }

    /// Returns the name.
    #[must_use]
    pub const fn name(&self) -> &InstanceName {
        &self.name
    }

    /// Returns the display name, if any.
    #[must_use]
    pub const fn display_name(&self) -> Option<&DisplayName> {
        self.display_name.as_ref()
    }

    /// Returns the exact provider revision.
    #[must_use]
    pub const fn provider_revision(&self) -> &ProviderRevisionKey {
        &self.provider_revision
    }

    /// Returns the software.
    #[must_use]
    pub const fn software(&self) -> &SoftwareName {
        &self.software
    }

    /// Returns the exact runtime.
    #[must_use]
    pub const fn runtime_id(&self) -> RuntimeId {
        self.runtime_id
    }

    /// Returns the runtime line of the data.
    #[must_use]
    pub const fn data_line(&self) -> &RuntimeLine {
        &self.data_line
    }

    /// Returns the exact runtime version of the data.
    #[must_use]
    pub const fn data_version(&self) -> &ExactVersion {
        &self.data_version
    }

    /// Returns when data initialization succeeded, or `None` while it has
    /// not.
    #[must_use]
    pub const fn data_initialized_at(&self) -> Option<Timestamp> {
        self.data_initialized_at
    }

    /// Returns the endpoints.
    #[must_use]
    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }

    /// Returns the desired state.
    #[must_use]
    pub const fn desired(&self) -> DesiredState {
        self.desired
    }

    /// Returns the startup policy.
    #[must_use]
    pub const fn startup(&self) -> StartupPolicy {
        self.startup
    }

    /// Returns the lifecycle state.
    #[must_use]
    pub const fn lifecycle(&self) -> InstanceLifecycle {
        self.lifecycle
    }

    /// Returns why the last init, start, or process failed, until the next
    /// `spawned` or `adopted`.
    #[must_use]
    pub const fn last_failure(&self) -> Option<&FailureCode> {
        self.last_failure.as_ref()
    }

    /// Returns the stored readiness. Read it through
    /// [`Readiness::effective`] to age it.
    #[must_use]
    pub const fn readiness(&self) -> &Readiness {
        &self.readiness
    }

    /// Returns the stored health. Read it through [`Health::effective`] to
    /// age it.
    #[must_use]
    pub const fn health(&self) -> &Health {
        &self.health
    }

    /// Returns the revision.
    #[must_use]
    pub const fn revision(&self) -> ResourceRevision {
        self.revision
    }

    /// Returns when the instance was created.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// Returns the operation that created the instance.
    #[must_use]
    pub const fn created_by(&self) -> OperationId {
        self.created_by
    }

    /// Returns when the instance was retained, if it was.
    #[must_use]
    pub const fn retained_at(&self) -> Option<Timestamp> {
        self.retained_at
    }

    /// Returns the operation that retained the instance, if it was.
    #[must_use]
    pub const fn retained_by(&self) -> Option<OperationId> {
        self.retained_by
    }

    /// Returns why the instance is blocked right now, if it is. The
    /// blocker lives only in the daemon's memory: a new or restored
    /// instance has none.
    #[must_use]
    pub const fn blocker(&self) -> Option<&InstanceBlocker> {
        self.blocker.as_ref()
    }

    /// Applies a lifecycle event at `at`, with the aggregate rules on top
    /// of the pure machine:
    ///
    /// - `start_begin` is refused while the data is not initialized
    ///   (BR-008); `init_succeeded` records `data_initialized_at`.
    /// - `init_failed`, `start_failed`, and `crashed` record
    ///   `last_failure`; `spawned` and `adopted` clear it. `retain` records
    ///   `retained_at` and `retained_by`.
    /// - Entering `running` sets readiness `not_ready` and health
    ///   `unknown`, both with reason `awaiting_probe`; leaving `running`
    ///   sets both `unknown` with reason `not_running`.
    ///
    /// The desired state never changes here.
    ///
    /// # Errors
    ///
    /// Returns a [`DomainError`]: a transition error for a pair the table
    /// does not hold, or the BR-008 invariant error
    /// [`InvariantDetail::DataNotInitialized`]. The instance does not
    /// change.
    pub fn apply_lifecycle(
        &mut self,
        event: LifecycleEvent,
        at: Timestamp,
    ) -> Result<InstanceLifecycle, DomainError> {
        let next = self.lifecycle.apply(&event)?;
        if event == LifecycleEvent::StartBegin && self.data_initialized_at.is_none() {
            return Err(InvariantError::from(InvariantDetail::DataNotInitialized {
                instance: self.id,
            })
            .into());
        }
        let was_running = self.lifecycle == InstanceLifecycle::Running;
        self.lifecycle = next;
        match event {
            LifecycleEvent::InitSucceeded => self.data_initialized_at = Some(at),
            LifecycleEvent::InitFailed { reason }
            | LifecycleEvent::StartFailed { reason }
            | LifecycleEvent::Crashed { reason, .. } => self.last_failure = Some(reason),
            LifecycleEvent::Spawned | LifecycleEvent::Adopted => self.last_failure = None,
            LifecycleEvent::Retain { by } => {
                self.retained_at = Some(at);
                self.retained_by = Some(by);
            }
            _ => {}
        }
        if next == InstanceLifecycle::Running {
            self.readiness = Readiness::new(
                ReadinessState::NotReady,
                at,
                ReasonCode::AWAITING_PROBE,
                None,
            );
            self.health = Health::unknown(at, ReasonCode::AWAITING_PROBE);
        } else if was_running {
            self.readiness = Readiness::unknown(at, ReasonCode::NOT_RUNNING);
            self.health = Health::unknown(at, ReasonCode::NOT_RUNNING);
        }
        self.bump_revision();
        Ok(next)
    }

    /// Stores a readiness observation. One made before the stored one is
    /// ignored. It never changes the lifecycle, the desired state, or the
    /// revision.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] for BR-010 with
    /// [`InvariantDetail::ReadyWithoutProcess`] when it says `ready` while
    /// the lifecycle is not `running`.
    pub fn observe_readiness(
        &mut self,
        readiness: Readiness,
    ) -> Result<ObservationOutcome, InvariantError> {
        if self.readiness.is_newer_than(&readiness) {
            return Ok(ObservationOutcome::Ignored(Ignored::Stale));
        }
        if readiness.state() == ReadinessState::Ready
            && self.lifecycle != InstanceLifecycle::Running
        {
            return Err(InvariantDetail::ReadyWithoutProcess {
                lifecycle: self.lifecycle,
            }
            .into());
        }
        self.readiness = readiness;
        Ok(ObservationOutcome::Applied)
    }

    /// Stores a health observation. One made before the stored one is
    /// ignored. It never changes the lifecycle, the desired state, or the
    /// revision.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::HealthWithoutProcess`] (not a BR) when it says
    /// anything but `unknown` while the lifecycle is not `running`.
    pub fn observe_health(&mut self, health: Health) -> Result<ObservationOutcome, InvariantError> {
        if self.health.is_newer_than(&health) {
            return Ok(ObservationOutcome::Ignored(Ignored::Stale));
        }
        if health.state() != HealthState::Unknown && self.lifecycle != InstanceLifecycle::Running {
            return Err(InvariantDetail::HealthWithoutProcess {
                lifecycle: self.lifecycle,
            }
            .into());
        }
        self.health = health;
        Ok(ObservationOutcome::Applied)
    }

    /// Stores a blocker observation, for example a needs secret blocker
    /// after a failed resolve (spec 0011, AC-11). One observed before the
    /// stored blocker is ignored. It never changes the lifecycle, the
    /// desired state, or the revision.
    pub fn observe_blocker(&mut self, blocker: InstanceBlocker) -> ObservationOutcome {
        if self
            .blocker
            .as_ref()
            .is_some_and(|stored| stored.observed_at() > blocker.observed_at())
        {
            return ObservationOutcome::Ignored(Ignored::Stale);
        }
        self.blocker = Some(blocker);
        ObservationOutcome::Applied
    }

    /// Removes the blocker and returns it, for example once a resolve
    /// succeeds or every needed secret is present again. It never changes
    /// the lifecycle, the desired state, or the revision.
    pub fn clear_blocker(&mut self) -> Option<InstanceBlocker> {
        self.blocker.take()
    }

    /// Renames the instance. Only the name changes: the ID, and so every
    /// path, stays (BR-003).
    pub fn rename(&mut self, name: InstanceName) {
        self.name = name;
        self.bump_revision();
    }

    /// Sets the desired state.
    pub fn set_desired(&mut self, desired: DesiredState) {
        self.desired = desired;
        self.bump_revision();
    }

    /// Records `port` as the bound port of the IP endpoint named `endpoint`:
    /// the port given to the process spawned for it, committed in the
    /// `spawned` batch and shown as final once readiness succeeds (spec
    /// 0010, REQ-010). A fixed endpoint can only be bound to its own port.
    /// Like [`set_desired`](Self::set_desired), it raises the revision by
    /// exactly 1, also when the port is the same.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] and leaves the instance unchanged when
    /// no IP endpoint has that name ([`InvariantDetail::NoIpEndpoint`]),
    /// or when `port` is not the port of a fixed endpoint (BR-005,
    /// [`InvariantDetail::FixedPortSubstituted`]).
    pub fn set_bound_port(
        &mut self,
        endpoint: &EndpointName,
        port: Port,
    ) -> Result<(), InvariantError> {
        let ip = self
            .endpoints
            .iter_mut()
            .find_map(|candidate| match candidate {
                Endpoint::Ip(ip) if ip.name() == endpoint => Some(ip),
                Endpoint::Ip(_) | Endpoint::Unix(_) => None,
            })
            .ok_or_else(|| InvariantDetail::NoIpEndpoint {
                endpoint: endpoint.clone(),
            })?;
        if let PortRequest::Fixed(fixed) = ip.request()
            && fixed != port
        {
            return Err(InvariantDetail::FixedPortSubstituted {
                fixed,
                requested: port,
            }
            .into());
        }
        ip.set_bound(port);
        self.bump_revision();
        Ok(())
    }

    /// Moves the instance to another runtime of the same software, the only
    /// way its runtime changes (BR-001).
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] and leaves the instance unchanged when
    /// the lifecycle is not `stopped` ([`InvariantDetail::RebindRequiresStopped`]),
    /// the runtime runs other software ([`InvariantDetail::RebindOtherSoftware`]),
    /// or `compat` does not allow the runtime's line for the data's line
    /// (BR-009, [`InvariantDetail::IncompatibleRuntimeLine`]).
    pub fn rebind(
        &mut self,
        runtime_id: RuntimeId,
        software: &SoftwareName,
        line: &RuntimeLine,
        compat: &LineCompatibility,
    ) -> Result<(), InvariantError> {
        if self.lifecycle != InstanceLifecycle::Stopped {
            return Err(InvariantDetail::RebindRequiresStopped {
                lifecycle: self.lifecycle,
            }
            .into());
        }
        if software != &self.software {
            return Err(InvariantDetail::RebindOtherSoftware {
                current: self.software.clone(),
                requested: software.clone(),
            }
            .into());
        }
        if !compat.allows(&self.data_line, line) {
            return Err(InvariantDetail::IncompatibleRuntimeLine {
                data_line: self.data_line.clone(),
                runtime_line: line.clone(),
            }
            .into());
        }
        self.runtime_id = runtime_id;
        self.bump_revision();
        Ok(())
    }

    /// Records that a data migration moved the data to `line` and
    /// `version`, the only way they change (BR-013).
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] for BR-013 with
    /// [`InvariantDetail::DataLineChangeWithoutMigration`] unless
    /// `operation` is a `running` `migrate_data` operation on this instance.
    pub fn record_data_migration(
        &mut self,
        operation: &Operation,
        line: RuntimeLine,
        version: ExactVersion,
    ) -> Result<(), InvariantError> {
        let is_migration = operation.kind() == OperationKind::MigrateData
            && operation.state() == OperationState::Running
            && operation.resource() == OperationResource::Instance(self.id);
        if !is_migration {
            return Err(InvariantDetail::DataLineChangeWithoutMigration {
                operation: operation.id(),
            }
            .into());
        }
        self.data_line = line;
        self.data_version = version;
        self.bump_revision();
        Ok(())
    }

    /// Moves the instance to the revision an accepted change names, the
    /// only way its provider revision changes (BR-017).
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] for BR-017 and leaves the instance
    /// unchanged when the change does not start from the current revision
    /// ([`InvariantDetail::RevisionChangeNotFromCurrent`]) or moves to
    /// another provider ([`InvariantDetail::RevisionChangeOtherProvider`]).
    pub fn apply_provider_revision(
        &mut self,
        change: &AcceptedRevisionChange,
    ) -> Result<(), InvariantError> {
        if change.from() != &self.provider_revision {
            return Err(InvariantDetail::RevisionChangeNotFromCurrent {
                current: Box::new(self.provider_revision.clone()),
                from: Box::new(change.from().clone()),
            }
            .into());
        }
        if change.to().provider() != self.provider_revision.provider() {
            return Err(InvariantDetail::RevisionChangeOtherProvider {
                current: self.provider_revision.provider().clone(),
                requested: change.to().provider().clone(),
            }
            .into());
        }
        self.provider_revision = change.to().clone();
        self.bump_revision();
        Ok(())
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::OperationId;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn instance() -> Instance {
        Instance::new(NewInstance {
            id: InstanceId::generate(),
            name: "orders-db".parse().unwrap(),
            display_name: None,
            provider_revision: ProviderRevisionKey::new(
                "official/postgresql".parse().unwrap(),
                "1".parse().unwrap(),
                "ab".repeat(32).parse().unwrap(),
            ),
            software: "postgresql".parse().unwrap(),
            runtime_id: RuntimeId::generate(),
            data_line: "17".parse().unwrap(),
            data_version: "17.6".parse().unwrap(),
            endpoints: Vec::new(),
            startup: StartupPolicy::Manual,
            created_at: at("2026-01-01T00:00:00Z"),
            created_by: OperationId::generate(),
        })
    }

    fn failure(code: &str) -> FailureCode {
        code.parse().unwrap()
    }

    #[test]
    fn a_new_instance_is_uninitialized_desired_stopped_and_never_observed() {
        let instance = instance();

        assert_eq!(instance.lifecycle(), InstanceLifecycle::Uninitialized);
        assert_eq!(instance.desired(), DesiredState::Stopped);
        assert_eq!(instance.readiness().state(), ReadinessState::Unknown);
        assert_eq!(instance.readiness().reason(), &ReasonCode::NEVER_OBSERVED);
        assert_eq!(instance.health().reason(), &ReasonCode::NEVER_OBSERVED);
        assert_eq!(instance.revision(), ResourceRevision::INITIAL);
        assert_eq!(instance.data_initialized_at(), None);
    }

    #[test]
    fn failures_are_recorded_and_cleared_by_a_running_process() {
        let mut instance = instance();
        instance
            .apply_lifecycle(LifecycleEvent::InitBegin, at("2026-01-01T00:00:01Z"))
            .unwrap();
        instance
            .apply_lifecycle(LifecycleEvent::InitSucceeded, at("2026-01-01T00:00:02Z"))
            .unwrap();
        instance
            .apply_lifecycle(LifecycleEvent::StartBegin, at("2026-01-01T00:00:03Z"))
            .unwrap();
        instance
            .apply_lifecycle(
                LifecycleEvent::StartFailed {
                    reason: failure("port_in_use"),
                },
                at("2026-01-01T00:00:04Z"),
            )
            .unwrap();

        assert_eq!(instance.last_failure(), Some(&failure("port_in_use")));
        assert_eq!(
            instance.data_initialized_at(),
            Some(at("2026-01-01T00:00:02Z"))
        );

        instance
            .apply_lifecycle(LifecycleEvent::StartBegin, at("2026-01-01T00:00:05Z"))
            .unwrap();
        instance
            .apply_lifecycle(LifecycleEvent::Spawned, at("2026-01-01T00:00:06Z"))
            .unwrap();
        assert_eq!(instance.last_failure(), None);
    }

    #[test]
    fn a_refused_event_leaves_the_instance_unchanged() {
        let mut instance = instance();
        let before = instance.clone();

        let err = instance
            .apply_lifecycle(LifecycleEvent::StopBegin, at("2026-01-01T00:00:01Z"))
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "instance lifecycle cannot apply stop_begin in state uninitialized"
        );
        assert_eq!(instance, before);
    }

    #[test]
    fn retain_records_when_and_by_which_operation_and_is_final() {
        let mut instance = instance();
        let by = OperationId::generate();
        instance
            .apply_lifecycle(LifecycleEvent::Retain { by }, at("2026-01-01T00:00:09Z"))
            .unwrap();

        assert_eq!(instance.retained_at(), Some(at("2026-01-01T00:00:09Z")));
        assert_eq!(instance.retained_by(), Some(by));
        assert!(
            instance
                .apply_lifecycle(LifecycleEvent::InitBegin, at("2026-01-01T00:00:10Z"))
                .is_err()
        );
    }

    #[test]
    fn rebind_to_other_software_or_while_not_stopped_is_refused() {
        let mut instance = instance();
        let compat = LineCompatibility::default();
        let line: RuntimeLine = "17".parse().unwrap();

        let err = instance
            .rebind(
                RuntimeId::generate(),
                &"postgresql".parse().unwrap(),
                &line,
                &compat,
            )
            .unwrap_err();
        assert_eq!(
            err.detail(),
            &InvariantDetail::RebindRequiresStopped {
                lifecycle: InstanceLifecycle::Uninitialized
            }
        );

        instance
            .apply_lifecycle(LifecycleEvent::InitBegin, at("2026-01-01T00:00:01Z"))
            .unwrap();
        instance
            .apply_lifecycle(LifecycleEvent::InitSucceeded, at("2026-01-01T00:00:02Z"))
            .unwrap();
        let before = instance.clone();
        let err = instance
            .rebind(
                RuntimeId::generate(),
                &"mysql".parse().unwrap(),
                &line,
                &compat,
            )
            .unwrap_err();
        assert!(matches!(
            err.detail(),
            InvariantDetail::RebindOtherSoftware { .. }
        ));
        assert_eq!(err.rule(), None);
        assert_eq!(instance, before);
    }

    #[test]
    fn set_bound_port_records_the_spawned_port_and_raises_the_revision() {
        use crate::endpoint::{IpEndpoint, Transport, UnixEndpoint};
        use crate::rule::Rule;

        let port = |number| Port::new(number).unwrap();
        let ip = |name: &str, request| {
            Endpoint::Ip(IpEndpoint::new(
                name.parse().unwrap(),
                Transport::Tcp,
                "127.0.0.1".parse().unwrap(),
                request,
                None,
            ))
        };
        let mut instance = instance();
        instance.endpoints = vec![
            ip("main", PortRequest::Auto),
            ip("admin", PortRequest::Fixed(port(5432))),
            Endpoint::Unix(UnixEndpoint::new(
                "socket".parse().unwrap(),
                ".s.PGSQL.5432".parse().unwrap(),
            )),
        ];
        let name = |text: &str| text.parse::<EndpointName>().unwrap();

        instance.set_bound_port(&name("main"), port(6001)).unwrap();
        instance.set_bound_port(&name("admin"), port(5432)).unwrap();
        instance.set_bound_port(&name("main"), port(6001)).unwrap();

        assert_eq!(instance.revision().get(), 4);
        let bound: Vec<_> = instance
            .endpoints()
            .iter()
            .map(|endpoint| match endpoint {
                Endpoint::Ip(ip) => ip.bound(),
                Endpoint::Unix(_) => None,
            })
            .collect();
        assert_eq!(bound, [Some(port(6001)), Some(port(5432)), None]);

        let before = instance.clone();
        let err = instance
            .set_bound_port(&name("admin"), port(5433))
            .unwrap_err();
        assert_eq!(
            err.detail(),
            &InvariantDetail::FixedPortSubstituted {
                fixed: port(5432),
                requested: port(5433)
            }
        );
        assert_eq!(err.rule(), Some(Rule::Br005));
        for missing in ["socket", "replica"] {
            let err = instance
                .set_bound_port(&name(missing), port(7000))
                .unwrap_err();
            assert_eq!(
                err.detail(),
                &InvariantDetail::NoIpEndpoint {
                    endpoint: name(missing)
                }
            );
            assert_eq!(err.rule(), None);
        }
        assert_eq!(instance, before);
    }

    #[test]
    fn set_desired_and_rename_each_raise_the_revision_by_one() {
        let mut instance = instance();

        instance.set_desired(DesiredState::Running);
        assert_eq!(instance.revision().get(), 2);
        assert_eq!(instance.lifecycle(), InstanceLifecycle::Uninitialized);
        instance.rename("sales-db".parse().unwrap());
        assert_eq!(instance.revision().get(), 3);
    }
}
