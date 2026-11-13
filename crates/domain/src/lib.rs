//! The Stackly domain model: entities, invariants, and state machines.
//!
//! This crate is the center of the architecture. It imports no other Stackly
//! crate, no async runtime, and no OS API, so it builds for every target.
//! Values validate in their constructor, so a value that exists is always
//! valid, and value types with a text form travel through serde in it.
//! Time is always a parameter: the domain never reads a clock, except that
//! `generate()` on an ID reads the system time for its UUID version 7.
//!
//! The runtime state, the instance lifecycle, and readiness with health are
//! three separate types in separate fields, next to the operation state.
//! Each state machine is a table: `apply` returns the next state or a
//! [`TransitionError`], and the aggregates add their rules on top. Business
//! rules that the domain checks reject with an [`InvariantError`] whose
//! `rule()` names the BR.

mod artifact_format;
mod capability;
pub mod capacity;
mod catalog;
mod code;
pub mod doctor;
mod endpoint;
mod environment;
mod error;
#[macro_use]
mod id;
mod instance;
mod lease;
mod location;
mod manifest;
mod metrics;
mod observation;
mod operation;
mod os_version;
mod process;
mod provider;
mod registry;
mod restart;
mod rule;
mod runtime;
mod secret;
mod session;
mod supervisor;
mod target;
mod text;
mod value;

pub use artifact_format::ArtifactFormat;
pub use capability::{Capability, CapabilitySet};
pub use capacity::CapacityLevel;
pub use catalog::SupportStatus;
pub use endpoint::{
    Endpoint, IpEndpoint, PortRequest, Transport, UnixEndpoint, endpoints_conflict, resolve_port,
};
pub use environment::{
    DeclaredService, DependencyCondition, DependencyEdge, Environment, EnvironmentMember,
    ManifestLocation, Ownership, StoredEnvironment,
};
pub use error::{
    DomainError, InvariantDetail, InvariantError, JournalError, Machine, ProgressError,
    TransitionError, ValueError, ValueKind,
};
pub use id::{
    BackupId, EnvironmentId, InstanceId, InvalidId, LeaseId, OperationId, ProviderSourceId,
    RestartPlanId, RuntimeId, ShortKey, StorageLocationId,
};
pub use instance::{
    ApplyGate, DesiredState, ExitKind, Instance, InstanceLifecycle, LifecycleEvent,
    LifecycleRefusal, NewInstance, StartupPolicy, StoredInstance, apply_allowed, rename_allowed,
};
pub use lease::{
    Lease, LeaseHolder, LeaseState, StopDecision, StopRequest, StoredLease, evaluate_stop,
    should_autostart,
};
pub use location::{
    AuditRetentionDays, BUILT_IN_LOCATION_IDS, DaemonSettings, LOCATION_PATH_MAX, LocationKind,
    LocationRule, SettingsPatch, StorageLocation, TELEMETRY_COPY_VERSION, UpdateChannel,
};
pub use manifest::{
    INSTANCE_MANIFEST_SCHEMA, InstallKind, InstanceManifest, ManifestData, ManifestRuntime,
    NewInstanceManifest, RetainedMarker,
};
pub use metrics::{
    CounterReading, HistoryId, MemberRead, Metric, MetricsHistory, MetricsSample, ProcessCounters,
    ProcessMeasure, ProcessRead, ProcessUsage, Rates, ResourceUsage, SamplesPage, TreeNode,
    UnavailableReason, UsageScope, measure, process_name, process_tree, rates,
};
pub use observation::{
    Health, HealthState, Ignored, Observation, ObservationOutcome, ObservedState, Readiness,
    ReadinessState,
};
pub use operation::{
    CancelAvailability, CreatedItem, CreatedRecord, CreatedSeq, CreatedState, FailureError,
    FailureItem, InterruptPolicy, JournalPayload, JournalStep, JournalView, LockKey, NewJournal,
    NewOperation, NewStep, Operation, OperationEvent, OperationFailure, OperationJournal,
    OperationKind, OperationResource, OperationState, OperationTrigger, Outcome, Progress,
    RecoveryCause, RecoveryHint, StepSeq, StepState, StoredCreatedRecord, StoredJournal,
    StoredOperation, StoredStep, TriggerOrigin, cancel_availability, force_intent,
};
pub use os_version::OsVersion;
pub use process::{
    BootId, IdentityCheck, IdentityCheckName, IdentityEvidence, LockState, Pid, ProcessIdentity,
    ProcessObservation, ProcessStart,
};
pub use provider::{
    AcceptedRevisionChange, LineCompatibility, ProviderRevision, ProviderRevisionKey,
};
pub use registry::{
    AUDIT_MAX_ROWS, ApprovalState, ArtifactRecord, AuditAction, AuditActor, AuditCursor,
    AuditEventId, AuditEventRecord, AuditPage, AuditQuery, AuditResource, AuditResult, BatchError,
    CODES_MAX, CommitGuard, Committed, CompatibilityRecordRow, CompatibilityResult,
    CompatibilityScope, ConfigRevisionId, ConfigRevisionNumber, ConfigRevisionOrigin,
    ConfigRevisionRecord, ConfigRevisionStatus, ConfigValuesJson, DependencyManifestJson,
    EndpointRequests, Generation, GuardDecision, GuardView, Guarded, ImportDecision,
    InstallDetails, InstanceFilter, InstancePathEntry, InstancePathKind, LeaseFilter, LogicalRoot,
    NotDeletableReason, OperationCursor, OperationPage, OperationQuery, PlatformStoreKey,
    ProviderImportRecord, ProviderRevisionRecord, ProviderSourceKind, ProviderSourceRecord,
    ProviderSourceStatus, PruneReport, QUICK_CHECK_MESSAGES_MAX, QuickCheck, Registry,
    RegistryError, RegistryId, RelativePath, ResourceRef, RetentionPolicy, SafeDetails,
    SafeDetailsError, SecretFieldName, SecretReferenceFilter, SecretReferenceId,
    SecretReferenceRecord, SecretReferenceView, SecretUse, SecretUser, SignatureInfoJson,
    TrustDecision, UNRECOGNIZED_ROWS_PER_COLUMN, UNRECOGNIZED_VALUE_MAX_LEN, UnrecognizedValue,
    Versioned, WriteBatch, WriteOp, approval_state,
};
pub use restart::{
    NewRestartPlan, PlanCloseReason, PlanDecision, PlanInstance, PlanOutcome, PlanReason,
    PlanState, RestartPlan, RestartPlanError, StopIntent, StoredRestartPlan, plan_decision,
    plan_digest_text,
};
pub use rule::Rule;
pub use runtime::{
    InstallMode, NewRuntime, RemovalBlocker, RemovalPlan, Runtime, RuntimeEvent, RuntimeReferences,
    RuntimeSelector, RuntimeState, SELECTOR_MAX, StoredRuntime, plan_runtime_removal,
};
pub use secret::{
    InstanceBlocker, NeedsSecretItem, NeedsSecretReason, SecretOrigin, SecretOwner,
    SecretOwnerKind, SecretPurpose, SecretRef, SecretRefError, SecretStatus,
};
pub use session::{DaemonSession, SESSIONS_KEPT, SessionEndReason, StoredSession};
pub use target::{Arch, Os, Target, native_support};
pub use value::{
    ClientCode, CompensatorName, CustomItemKey, DetailKey, DisplayName, EndpointName,
    EnvironmentName, ErrorCodeName, ExactVersion, FailureCode, IdempotencyKey, InstanceName,
    MemberAlias, MethodName, ObservationDetail, Port, ProviderName, ReasonCode, ResourceRevision,
    RevisionLabel, RuntimeLine, SessionId, Sha256Digest, SocketName, SoftwareName, StageName,
    TeamId,
};
