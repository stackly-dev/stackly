use std::fmt;
use std::path::PathBuf;

use crate::capability::Capability;
use crate::id::{InstanceId, LeaseId, OperationId};
use crate::instance::InstanceLifecycle;
use crate::operation::OperationState;
use crate::provider::ProviderRevisionKey;
use crate::rule::Rule;
use crate::runtime::RemovalBlocker;
use crate::value::{EndpointName, MemberAlias, Port, ProviderName, RuntimeLine, SoftwareName};

/// The kind of value type a [`ValueError`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    /// A [`ShortKey`](crate::ShortKey).
    ShortKey,
    /// A [`LogicalRoot`](crate::LogicalRoot).
    LogicalRoot,
    /// An [`InstancePathKind`](crate::InstancePathKind).
    InstancePathKind,
    /// A [`RelativePath`](crate::RelativePath).
    RelativePath,
    /// An [`Os`](crate::Os).
    Os,
    /// An [`Arch`](crate::Arch).
    Arch,
    /// A [`Target`](crate::Target).
    Target,
    /// An [`InstanceName`](crate::InstanceName).
    InstanceName,
    /// A [`DisplayName`](crate::DisplayName).
    DisplayName,
    /// A [`SoftwareName`](crate::SoftwareName).
    SoftwareName,
    /// A [`ProviderName`](crate::ProviderName).
    ProviderName,
    /// A [`RevisionLabel`](crate::RevisionLabel).
    RevisionLabel,
    /// An [`ExactVersion`](crate::ExactVersion).
    ExactVersion,
    /// A [`RuntimeLine`](crate::RuntimeLine).
    RuntimeLine,
    /// A [`Sha256Digest`](crate::Sha256Digest).
    Sha256Digest,
    /// An [`InstallKind`](crate::InstallKind).
    InstallKind,
    /// An [`InstanceManifest`](crate::InstanceManifest) as a whole.
    InstanceManifest,
    /// A [`Port`](crate::Port).
    Port,
    /// A [`SocketName`](crate::SocketName).
    SocketName,
    /// An [`EndpointName`](crate::EndpointName).
    EndpointName,
    /// An [`IdempotencyKey`](crate::IdempotencyKey).
    IdempotencyKey,
    /// A [`SessionId`](crate::SessionId).
    SessionId,
    /// A [`Pid`](crate::Pid).
    Pid,
    /// A [`ProcessStart`](crate::ProcessStart).
    ProcessStart,
    /// A [`BootId`](crate::BootId).
    BootId,
    /// A [`LockState`](crate::LockState).
    LockState,
    /// An [`IdentityCheck`](crate::IdentityCheck).
    IdentityCheck,
    /// An [`IdentityCheckName`](crate::IdentityCheckName).
    IdentityCheckName,
    /// A [`ReasonCode`](crate::ReasonCode).
    ReasonCode,
    /// A [`FailureCode`](crate::FailureCode).
    FailureCode,
    /// A [`StageName`](crate::StageName).
    StageName,
    /// An [`ObservationDetail`](crate::ObservationDetail).
    ObservationDetail,
    /// An [`EnvironmentName`](crate::EnvironmentName).
    EnvironmentName,
    /// A [`MemberAlias`](crate::MemberAlias).
    MemberAlias,
    /// A [`ResourceRevision`](crate::ResourceRevision).
    ResourceRevision,
    /// A [`Capability`](crate::Capability).
    Capability,
    /// A [`Transport`](crate::Transport).
    Transport,
    /// A [`ReadinessState`](crate::ReadinessState).
    ReadinessState,
    /// A [`HealthState`](crate::HealthState).
    HealthState,
    /// A [`RuntimeState`](crate::RuntimeState).
    RuntimeState,
    /// An [`InstanceLifecycle`](crate::InstanceLifecycle).
    InstanceLifecycle,
    /// A [`DesiredState`](crate::DesiredState).
    DesiredState,
    /// A [`StartupPolicy`](crate::StartupPolicy).
    StartupPolicy,
    /// An [`OperationState`](crate::OperationState).
    OperationState,
    /// An [`OperationKind`](crate::OperationKind).
    OperationKind,
    /// An [`Ownership`](crate::Ownership).
    Ownership,
    /// A [`MethodName`](crate::MethodName).
    MethodName,
    /// An [`ErrorCodeName`](crate::ErrorCodeName).
    ErrorCodeName,
    /// A [`CompensatorName`](crate::CompensatorName).
    CompensatorName,
    /// A [`ClientCode`](crate::ClientCode).
    ClientCode,
    /// A [`DetailKey`](crate::DetailKey).
    DetailKey,
    /// A [`CustomItemKey`](crate::CustomItemKey).
    CustomItemKey,
    /// A [`JournalPayload`](crate::JournalPayload).
    JournalPayload,
    /// A [`StepSeq`](crate::StepSeq).
    StepSeq,
    /// A [`CreatedSeq`](crate::CreatedSeq).
    CreatedSeq,
    /// A [`StepState`](crate::StepState).
    StepState,
    /// A [`CreatedState`](crate::CreatedState).
    CreatedState,
    /// A [`CreatedItem`](crate::CreatedItem).
    CreatedItem,
    /// A [`TriggerOrigin`](crate::TriggerOrigin).
    TriggerOrigin,
    /// A [`RecoveryCause`](crate::RecoveryCause).
    RecoveryCause,
    /// A [`RecoveryHint`](crate::RecoveryHint).
    RecoveryHint,
    /// A [`FailureError`](crate::FailureError).
    FailureError,
    /// A [`LockKey`](crate::LockKey).
    LockKey,
    /// A [`SecretPurpose`](crate::SecretPurpose).
    SecretPurpose,
    /// A [`SecretOwnerKind`](crate::SecretOwnerKind).
    SecretOwnerKind,
    /// A [`SecretOwner`](crate::SecretOwner).
    SecretOwner,
    /// A [`SecretStatus`](crate::SecretStatus).
    SecretStatus,
    /// A [`SecretOrigin`](crate::SecretOrigin).
    SecretOrigin,
    /// A [`NeedsSecretReason`](crate::NeedsSecretReason).
    NeedsSecretReason,
    /// The text form of a
    /// [`ProviderRevisionKey`](crate::ProviderRevisionKey).
    ProviderRevisionKey,
    /// A [`SecretFieldName`](crate::SecretFieldName).
    SecretFieldName,
    /// An [`AuditAction`](crate::AuditAction).
    AuditAction,
    /// A [`PlatformStoreKey`](crate::PlatformStoreKey).
    PlatformStoreKey,
    /// An [`AuditActor`](crate::AuditActor).
    AuditActor,
    /// An [`AuditResult`](crate::AuditResult).
    AuditResult,
    /// An [`AuditResource`](crate::AuditResource).
    AuditResource,
    /// A [`SecretUser`](crate::SecretUser).
    SecretUser,
}

impl ValueKind {
    /// Returns the name of this kind as it appears in error messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShortKey => "short key",
            Self::LogicalRoot => "logical root",
            Self::InstancePathKind => "instance path kind",
            Self::RelativePath => "relative path",
            Self::Os => "os",
            Self::Arch => "arch",
            Self::Target => "target",
            Self::InstanceName => "instance name",
            Self::DisplayName => "display name",
            Self::SoftwareName => "software name",
            Self::ProviderName => "provider name",
            Self::RevisionLabel => "revision label",
            Self::ExactVersion => "exact version",
            Self::RuntimeLine => "runtime line",
            Self::Sha256Digest => "sha256 digest",
            Self::InstallKind => "install mode",
            Self::InstanceManifest => "instance manifest",
            Self::Port => "port",
            Self::SocketName => "socket name",
            Self::EndpointName => "endpoint name",
            Self::IdempotencyKey => "idempotency key",
            Self::SessionId => "session ID",
            Self::Pid => "process ID",
            Self::ProcessStart => "process start time",
            Self::BootId => "boot ID",
            Self::LockState => "lock state",
            Self::IdentityCheck => "identity check",
            Self::IdentityCheckName => "identity check name",
            Self::ReasonCode => "reason code",
            Self::FailureCode => "failure code",
            Self::StageName => "stage name",
            Self::ObservationDetail => "observation detail",
            Self::EnvironmentName => "environment name",
            Self::MemberAlias => "member alias",
            Self::ResourceRevision => "resource revision",
            Self::Capability => "capability",
            Self::Transport => "transport",
            Self::ReadinessState => "readiness state",
            Self::HealthState => "health state",
            Self::RuntimeState => "runtime state",
            Self::InstanceLifecycle => "instance lifecycle",
            Self::DesiredState => "desired state",
            Self::StartupPolicy => "startup policy",
            Self::OperationState => "operation state",
            Self::OperationKind => "operation kind",
            Self::Ownership => "ownership",
            Self::MethodName => "method name",
            Self::ErrorCodeName => "error code name",
            Self::CompensatorName => "compensator name",
            Self::ClientCode => "client code",
            Self::DetailKey => "detail key",
            Self::CustomItemKey => "custom item key",
            Self::JournalPayload => "journal payload",
            Self::StepSeq => "step sequence number",
            Self::CreatedSeq => "created item sequence number",
            Self::StepState => "step state",
            Self::CreatedState => "created item state",
            Self::CreatedItem => "created item",
            Self::TriggerOrigin => "trigger origin",
            Self::RecoveryCause => "recovery cause",
            Self::RecoveryHint => "recovery hint",
            Self::FailureError => "failure error",
            Self::LockKey => "lock key",
            Self::SecretPurpose => "secret purpose",
            Self::SecretOwnerKind => "secret owner kind",
            Self::SecretOwner => "secret owner",
            Self::SecretStatus => "secret status",
            Self::SecretOrigin => "secret origin",
            Self::NeedsSecretReason => "needs secret reason",
            Self::ProviderRevisionKey => "provider revision key",
            Self::SecretFieldName => "secret field name",
            Self::AuditAction => "audit action",
            Self::PlatformStoreKey => "platform store key",
            Self::AuditActor => "audit actor",
            Self::AuditResult => "audit result",
            Self::AuditResource => "audit resource",
            Self::SecretUser => "secret user",
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a value type rejects its input.
///
/// Value types validate in their constructor, so a value that exists is
/// always valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind}: {reason}")]
pub struct ValueError {
    kind: ValueKind,
    reason: &'static str,
}

impl ValueError {
    pub(crate) const fn new(kind: ValueKind, reason: &'static str) -> Self {
        Self { kind, reason }
    }

    /// Returns the kind of value that was rejected.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        self.kind
    }

    /// Returns the rule the input broke, for example
    /// `must be 8 lowercase hex characters`.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }

    /// Returns the business rule this error enforces. Value rules come from
    /// the PRD's validation IDs, not its business rules, so this is always
    /// `None`.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        None
    }
}

/// One of the domain's state machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Machine {
    /// The state of an installed runtime.
    Runtime,
    /// The lifecycle of an instance.
    InstanceLifecycle,
    /// The state of an operation.
    Operation,
    /// The state of one step in an operation journal.
    JournalStep,
    /// The state of one item an operation recorded as created.
    CreatedRecord,
}

impl Machine {
    /// Returns the stable snake_case code, for example `instance_lifecycle`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::InstanceLifecycle => "instance_lifecycle",
            Self::Operation => "operation",
            Self::JournalStep => "journal_step",
            Self::CreatedRecord => "created_record",
        }
    }

    /// Returns the name used in error messages, for example
    /// `instance lifecycle`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::InstanceLifecycle => "instance lifecycle",
            Self::Operation => "operation",
            Self::JournalStep => "journal step",
            Self::CreatedRecord => "created record",
        }
    }
}

impl fmt::Display for Machine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a state machine has no transition for an event in
/// its current state. The state does not change.
///
/// ```
/// use stackly_domain::{Machine, TransitionError};
///
/// let err = TransitionError::new(Machine::InstanceLifecycle, "running", "start_begin");
/// assert_eq!(
///     err.to_string(),
///     "instance lifecycle cannot apply start_begin in state running",
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{machine} cannot apply {event} in state {from}")]
pub struct TransitionError {
    machine: Machine,
    from: &'static str,
    event: &'static str,
}

impl TransitionError {
    /// Creates the error for `event` in state `from`, both given as their
    /// stable snake_case codes. The state machines in this crate are the
    /// callers; other crates read it.
    #[must_use]
    pub const fn new(machine: Machine, from: &'static str, event: &'static str) -> Self {
        Self {
            machine,
            from,
            event,
        }
    }

    /// Returns the machine that refused the event.
    #[must_use]
    pub const fn machine(&self) -> Machine {
        self.machine
    }

    /// Returns the code of the state the machine was in.
    #[must_use]
    pub const fn from(&self) -> &'static str {
        self.from
    }

    /// Returns the code of the refused event.
    #[must_use]
    pub const fn event(&self) -> &'static str {
        self.event
    }

    /// Returns the business rule this error enforces. A missing transition
    /// is not a business rule, so this is always `None`.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        None
    }
}

/// What an [`InvariantError`] rejected.
///
/// Each detail maps to at most one business rule through
/// [`InvariantDetail::rule`]. Details that are not a BR violation map to
/// `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvariantDetail {
    /// Two data roots are the same folder, or one is inside the other
    /// (BR-002).
    DataRootOverlap {
        /// The data root that was checked.
        candidate: PathBuf,
        /// The data root it overlaps.
        existing: PathBuf,
    },
    /// A fixed port conflicts with another endpoint, and no other port is
    /// ever substituted (BR-005).
    FixedPortConflict {
        /// The fixed port.
        port: Port,
    },
    /// An `Auto` endpoint has no candidate port, or the candidate conflicts
    /// (BR-005).
    AutoPortUnavailable {
        /// The candidate the caller found, if any.
        candidate: Option<Port>,
    },
    /// A fixed port endpoint was to be bound to another port, and no other
    /// port is ever substituted (BR-005).
    FixedPortSubstituted {
        /// The fixed port.
        fixed: Port,
        /// The port it was to be bound to.
        requested: Port,
    },
    /// The instance has no IP endpoint of that name. Not a BR.
    NoIpEndpoint {
        /// The name that was asked for.
        endpoint: EndpointName,
    },
    /// References still need the runtime (BR-007).
    RemovalBlocked {
        /// Every reference that blocks the removal.
        blockers: Vec<RemovalBlocker>,
    },
    /// A start was asked for before data initialization succeeded
    /// (BR-008).
    DataNotInitialized {
        /// The instance.
        instance: InstanceId,
    },
    /// The runtime's line may not run the data's line (BR-009).
    IncompatibleRuntimeLine {
        /// The line that initialized the data.
        data_line: RuntimeLine,
        /// The line of the requested runtime.
        runtime_line: RuntimeLine,
    },
    /// A readiness of `ready` arrived while the lifecycle is not `running`
    /// (BR-010).
    ReadyWithoutProcess {
        /// The lifecycle at the time.
        lifecycle: InstanceLifecycle,
    },
    /// A health other than `unknown` arrived while the lifecycle is not
    /// `running`. Not a BR.
    HealthWithoutProcess {
        /// The lifecycle at the time.
        lifecycle: InstanceLifecycle,
    },
    /// A rebind was asked for while the lifecycle is not `stopped`. Not a
    /// BR.
    RebindRequiresStopped {
        /// The lifecycle at the time.
        lifecycle: InstanceLifecycle,
    },
    /// A rebind to a runtime of other software. Not a BR.
    RebindOtherSoftware {
        /// The instance's software.
        current: SoftwareName,
        /// The software of the requested runtime.
        requested: SoftwareName,
    },
    /// The data line or version would change without a running
    /// `migrate_data` operation on the instance (BR-013).
    DataLineChangeWithoutMigration {
        /// The operation that was offered.
        operation: OperationId,
    },
    /// The provider does not support a capability (BR-016).
    Unsupported {
        /// The missing capability.
        capability: Capability,
    },
    /// An accepted revision change does not start from the instance's
    /// current revision (BR-017).
    RevisionChangeNotFromCurrent {
        /// The instance's current revision.
        current: Box<ProviderRevisionKey>,
        /// The revision the change starts from.
        from: Box<ProviderRevisionKey>,
    },
    /// An accepted revision change moves to another provider (BR-017).
    RevisionChangeOtherProvider {
        /// The instance's provider.
        current: ProviderName,
        /// The provider the change moves to.
        requested: ProviderName,
    },
    /// Two members of an environment share an alias. Not a BR.
    DuplicateMemberAlias {
        /// The alias.
        alias: MemberAlias,
    },
    /// An instance joins an environment more than once. Not a BR.
    DuplicateMemberInstance {
        /// The instance.
        instance: InstanceId,
    },
    /// An environment lease carries a session. Not a BR.
    EnvironmentLeaseWithSession {
        /// The lease.
        lease: LeaseId,
    },
    /// A startup lease has no session. Not a BR.
    StartupLeaseWithoutSession {
        /// The lease.
        lease: LeaseId,
    },
    /// A released lease was released again. Not a BR.
    LeaseAlreadyReleased {
        /// The lease.
        lease: LeaseId,
    },
    /// Only an `active` startup lease can be suppressed. Not a BR.
    SuppressNeedsActiveStartupLease {
        /// The lease.
        lease: LeaseId,
    },
    /// A stored record handed to a `restore` constructor breaks an
    /// invariant between its fields. Not a BR: the registry reports it as
    /// corrupt.
    InconsistentRecord {
        /// What was restored, for example `instance`.
        resource: &'static str,
        /// The invariant it breaks.
        reason: &'static str,
    },
}

impl InvariantDetail {
    /// Returns the business rule this detail breaks, or `None` when it is
    /// not a BR violation.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        match self {
            Self::DataRootOverlap { .. } => Some(Rule::Br002),
            Self::FixedPortConflict { .. }
            | Self::AutoPortUnavailable { .. }
            | Self::FixedPortSubstituted { .. } => Some(Rule::Br005),
            Self::RemovalBlocked { .. } => Some(Rule::Br007),
            Self::DataNotInitialized { .. } => Some(Rule::Br008),
            Self::IncompatibleRuntimeLine { .. } => Some(Rule::Br009),
            Self::ReadyWithoutProcess { .. } => Some(Rule::Br010),
            Self::DataLineChangeWithoutMigration { .. } => Some(Rule::Br013),
            Self::Unsupported { .. } => Some(Rule::Br016),
            Self::RevisionChangeNotFromCurrent { .. }
            | Self::RevisionChangeOtherProvider { .. } => Some(Rule::Br017),
            Self::HealthWithoutProcess { .. }
            | Self::NoIpEndpoint { .. }
            | Self::RebindRequiresStopped { .. }
            | Self::RebindOtherSoftware { .. }
            | Self::DuplicateMemberAlias { .. }
            | Self::DuplicateMemberInstance { .. }
            | Self::EnvironmentLeaseWithSession { .. }
            | Self::StartupLeaseWithoutSession { .. }
            | Self::LeaseAlreadyReleased { .. }
            | Self::SuppressNeedsActiveStartupLease { .. }
            | Self::InconsistentRecord { .. } => None,
        }
    }
}

impl fmt::Display for InvariantDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataRootOverlap {
                candidate,
                existing,
            } => write!(
                f,
                "data root {} overlaps the data root {}",
                candidate.display(),
                existing.display()
            ),
            Self::FixedPortConflict { port } => write!(
                f,
                "fixed port {port} conflicts with another endpoint, and no other port is used"
            ),
            Self::AutoPortUnavailable { candidate: None } => {
                f.write_str("no free port was found for an auto port endpoint")
            }
            Self::AutoPortUnavailable {
                candidate: Some(port),
            } => write!(f, "candidate port {port} conflicts with another endpoint"),
            Self::FixedPortSubstituted { fixed, requested } => write!(
                f,
                "fixed port {fixed} cannot be bound as port {requested}, and no other port is used"
            ),
            Self::NoIpEndpoint { endpoint } => {
                write!(f, "the instance has no ip endpoint named {endpoint}")
            }
            Self::RemovalBlocked { blockers } => {
                write!(
                    f,
                    "the runtime is still in use by {} references",
                    blockers.len()
                )
            }
            Self::DataNotInitialized { instance } => write!(
                f,
                "instance {instance} cannot start before its data is initialized"
            ),
            Self::IncompatibleRuntimeLine {
                data_line,
                runtime_line,
            } => write!(
                f,
                "a runtime of line {runtime_line} cannot run data of line {data_line}"
            ),
            Self::ReadyWithoutProcess { lifecycle } => {
                write!(f, "an instance in state {lifecycle} cannot be ready")
            }
            Self::HealthWithoutProcess { lifecycle } => write!(
                f,
                "an instance in state {lifecycle} has no health other than unknown"
            ),
            Self::RebindRequiresStopped { lifecycle } => write!(
                f,
                "an instance in state {lifecycle} cannot be rebound; it must be stopped"
            ),
            Self::RebindOtherSoftware { current, requested } => write!(
                f,
                "an instance of {current} cannot be rebound to a runtime of {requested}"
            ),
            Self::DataLineChangeWithoutMigration { operation } => write!(
                f,
                "operation {operation} is not a running migrate_data operation on this instance"
            ),
            Self::Unsupported { capability } => {
                write!(f, "the provider does not support {capability}")
            }
            Self::RevisionChangeNotFromCurrent { current, from } => write!(
                f,
                "the change starts from revision {} of {}, not the current revision {} of {}",
                from.revision(),
                from.provider(),
                current.revision(),
                current.provider()
            ),
            Self::RevisionChangeOtherProvider { current, requested } => write!(
                f,
                "a revision change cannot move from provider {current} to {requested}"
            ),
            Self::DuplicateMemberAlias { alias } => {
                write!(f, "alias {alias} is used by more than one member")
            }
            Self::DuplicateMemberInstance { instance } => {
                write!(f, "instance {instance} is a member more than once")
            }
            Self::EnvironmentLeaseWithSession { lease } => {
                write!(f, "environment lease {lease} cannot have a session")
            }
            Self::StartupLeaseWithoutSession { lease } => {
                write!(f, "startup lease {lease} needs a session")
            }
            Self::LeaseAlreadyReleased { lease } => {
                write!(f, "lease {lease} is already released")
            }
            Self::SuppressNeedsActiveStartupLease { lease } => write!(
                f,
                "lease {lease} cannot be suppressed; only an active startup lease can"
            ),
            Self::InconsistentRecord { resource, reason } => {
                write!(f, "a stored {resource} is inconsistent: {reason}")
            }
        }
    }
}

/// Error returned when an action would break a domain invariant.
///
/// The message ends with the rule ID when there is one.
///
/// ```
/// use stackly_domain::{InvariantDetail, InvariantError, Rule};
///
/// let err = InvariantError::new(InvariantDetail::DataRootOverlap {
///     candidate: "/data/a/b".into(),
///     existing: "/data/a".into(),
/// });
/// assert_eq!(err.rule(), Some(Rule::Br002));
/// assert_eq!(
///     err.to_string(),
///     "data root /data/a/b overlaps the data root /data/a (BR-002)",
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvariantError {
    detail: InvariantDetail,
}

impl InvariantError {
    /// Creates the error for `detail`. Its rule follows from the detail.
    #[must_use]
    pub const fn new(detail: InvariantDetail) -> Self {
        Self { detail }
    }

    /// Returns the business rule that was broken, or `None` when the
    /// invariant is not a BR.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        self.detail.rule()
    }

    /// Returns what was rejected.
    #[must_use]
    pub const fn detail(&self) -> &InvariantDetail {
        &self.detail
    }
}

impl From<InvariantDetail> for InvariantError {
    fn from(detail: InvariantDetail) -> Self {
        Self::new(detail)
    }
}

impl fmt::Display for InvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.rule() {
            Some(rule) => write!(f, "{} ({rule})", self.detail),
            None => write!(f, "{}", self.detail),
        }
    }
}

impl std::error::Error for InvariantError {}

/// Error returned when a progress or stage report is refused. The
/// operation does not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProgressError {
    /// `done` is more than `total`.
    #[error("progress {done} is more than its total {total}")]
    OutOfRange {
        /// What is done.
        done: u64,
        /// The whole.
        total: u64,
    },
    /// Progress changes only while the operation is `running` or
    /// `cancel_requested`.
    #[error("progress cannot change in state {state}")]
    NotAllowedInState {
        /// The operation's state.
        state: OperationState,
    },
}

/// Error returned when an operation journal value would break a rule of
/// spec 0007: a payload over its limit, a payload on an item that takes
/// none, a step run twice that is not repeatable, or a step or created
/// record event its state does not accept. The value does not change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum JournalError {
    /// A payload is longer than its field allows.
    #[error("{field} is {len} bytes, more than its {max} byte limit")]
    PayloadTooLarge {
        /// The field, for example `input` or `checkpoint`.
        field: &'static str,
        /// The payload's length in bytes.
        len: usize,
        /// The longest payload the field takes.
        max: usize,
    },
    /// Only a custom item carries a payload.
    #[error("only a custom item carries a payload")]
    PayloadNotAllowed,
    /// The step is not repeatable, so it runs at most once.
    #[error("step {seq} is not repeatable")]
    NotRepeatable {
        /// The step's sequence number.
        seq: u16,
    },
    /// The step or record does not accept the event in its state.
    #[error(transparent)]
    Transition(#[from] TransitionError),
}

impl ProgressError {
    /// Returns the business rule this error enforces. Progress rules are
    /// not business rules, so this is always `None`.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        None
    }
}

/// Error of an action that can fail both ways: a missing transition, or a
/// broken invariant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DomainError {
    /// The state machine has no transition for the event.
    #[error(transparent)]
    Transition(#[from] TransitionError),
    /// The action would break an invariant.
    #[error(transparent)]
    Invariant(#[from] InvariantError),
}

impl DomainError {
    /// Returns the business rule this error enforces, if any.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        match self {
            Self::Transition(err) => err.rule(),
            Self::Invariant(err) => err.rule(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_error_names_machine_state_and_event_and_has_no_rule() {
        let err = TransitionError::new(Machine::Operation, "succeeded", "start");

        assert_eq!(
            err.to_string(),
            "operation cannot apply start in state succeeded"
        );
        assert_eq!(err.machine().as_code(), "operation");
        assert_eq!((err.from(), err.event()), ("succeeded", "start"));
        assert_eq!(err.rule(), None);
    }

    #[test]
    fn invariant_error_takes_its_rule_from_the_detail() {
        let detail = InvariantDetail::DataRootOverlap {
            candidate: PathBuf::from("a"),
            existing: PathBuf::from("a"),
        };
        let err = InvariantError::from(detail.clone());

        assert_eq!(err.rule(), Some(Rule::Br002));
        assert_eq!(err.detail(), &detail);
    }

    #[test]
    fn value_errors_carry_no_business_rule() {
        let err = ValueError::new(ValueKind::InstanceName, "must not be empty");

        assert_eq!(err.to_string(), "invalid instance name: must not be empty");
        assert_eq!(err.rule(), None);
    }
}
