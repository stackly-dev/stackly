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
mod catalog;
mod code;
mod endpoint;
mod environment;
mod error;
#[macro_use]
mod id;
mod instance;
mod lease;
mod manifest;
mod observation;
mod operation;
mod os_version;
mod process;
mod provider;
mod registry;
mod rule;
mod runtime;
mod secret;
mod supervisor;
mod target;
mod text;
mod value;

pub use artifact_format::ArtifactFormat;
pub use capability::{Capability, CapabilitySet};
pub use catalog::SupportStatus;
pub use endpoint::{
    Endpoint, IpEndpoint, PortRequest, Transport, UnixEndpoint, endpoints_conflict, resolve_port,
};
pub use environment::{Environment, EnvironmentMember, Ownership, StoredEnvironment};
pub use error::{
    DomainError, InvariantDetail, InvariantError, JournalError, Machine, ProgressError,
    TransitionError, ValueError, ValueKind,
};
pub use id::{
    BackupId, EnvironmentId, InstanceId, InvalidId, LeaseId, OperationId, RuntimeId, ShortKey,
};
pub use instance::{
    DesiredState, Instance, InstanceLifecycle, LifecycleEvent, NewInstance, StartupPolicy,
    StoredInstance,
};
pub use lease::{
    Lease, LeaseHolder, LeaseState, StopDecision, StopRequest, StoredLease, evaluate_stop,
    should_autostart,
};
pub use manifest::{
    INSTANCE_MANIFEST_SCHEMA, InstallKind, InstanceManifest, ManifestData, ManifestRuntime,
    NewInstanceManifest, RetainedMarker,
};
pub use observation::{
    Health, HealthState, Ignored, Observation, ObservationOutcome, ObservedState, Readiness,
    ReadinessState,
};
pub use operation::{
    CreatedItem, CreatedRecord, CreatedSeq, CreatedState, FailureError, FailureItem,
    InterruptPolicy, JournalPayload, JournalStep, JournalView, LockKey, NewJournal, NewOperation,
    NewStep, Operation, OperationEvent, OperationFailure, OperationJournal, OperationKind,
    OperationResource, OperationState, OperationTrigger, Outcome, Progress, RecoveryCause,
    RecoveryHint, StepSeq, StepState, StoredCreatedRecord, StoredJournal, StoredOperation,
    StoredStep, TriggerOrigin,
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
    ArtifactRecord, AuditAction, AuditActor, AuditCursor, AuditEventId, AuditEventRecord,
    AuditPage, AuditQuery, AuditResource, AuditResult, BatchError, CODES_MAX, Committed,
    CompatibilityRecordRow, CompatibilityResult, CompatibilityScope, ConfigRevisionId,
    ConfigRevisionRecord, ConfigRevisionStatus, ConfigValuesJson, DependencyManifestJson,
    Generation, InstallDetails, InstanceFilter, InstancePathEntry, InstancePathKind, LeaseFilter,
    LogicalRoot, OperationCursor, OperationPage, OperationQuery, PlatformStoreKey,
    ProviderRevisionRecord, PruneReport, Registry, RegistryError, RegistryId, RelativePath,
    ResourceRef, RetentionPolicy, SafeDetails, SafeDetailsError, SecretFieldName,
    SecretReferenceFilter, SecretReferenceId, SecretReferenceRecord, SecretReferenceView,
    SecretUse, SecretUser, SignatureInfoJson, Versioned, WriteBatch, WriteOp,
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
pub use target::{Arch, Os, Target, native_support};
pub use value::{
    ClientCode, CompensatorName, CustomItemKey, DetailKey, DisplayName, EndpointName,
    EnvironmentName, ErrorCodeName, ExactVersion, FailureCode, IdempotencyKey, InstanceName,
    MemberAlias, MethodName, ObservationDetail, Port, ProviderName, ReasonCode, ResourceRevision,
    RevisionLabel, RuntimeLine, SessionId, Sha256Digest, SocketName, SoftwareName, StageName,
    TeamId,
};
