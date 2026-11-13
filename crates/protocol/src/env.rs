//! Environments (spec 0044): `stackly.yaml` checked (`env.validate`),
//! planned (`env.plan`), listed (`env.list`, `env.get`), registered
//! (`env.import`), and forgotten (`env.remove`); its lock written
//! (`env.lock`) and its endpoints exported (`env.export`, spec 0045).
//!
//! Validate and plan are reads, not operations: they never write the
//! registry, a file, or the Keychain. Import and remove are operations that
//! write only the registry. The method prefix is `env.`; the resource kind
//! and lock key stay `environment`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::{ErrorBody, FieldError};
use crate::instance::{
    InstanceHealth, InstanceLifecycle, InstanceReadiness, PortRequest, PreflightCheck,
};
use crate::json::{FieldCode, Validate};
use crate::provider::{FindingSeverity, import_path_code, is_sha256};
use crate::stop_all::InstanceRef;
use crate::wire::{Count, Revision, Timestamp};

/// Most findings an [`EnvValidation`] carries; `omitted` counts the rest.
pub const ENV_FINDINGS_MAX: usize = 100;
/// Most services one manifest declares.
pub const ENV_SERVICES_MAX: usize = 32;
/// Most environments one registry holds; `env.list` answers all of them.
pub const ENV_LIST_MAX: usize = 64;

/// Whether an environment owns a service's instance.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnvOwnership {
    /// The environment owns the instance and its data.
    Dedicated,
    /// The environment borrows an existing instance.
    Shared,
    /// A value this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What a dependency waits for. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyConditionKind {
    /// The dependency is running and ready.
    Ready,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One `dependsOn` entry.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDependency {
    /// The alias depended on.
    pub service: String,
    /// What it waits for.
    pub condition: DependencyConditionKind,
    /// The wait limit the manifest wrote; none means the default of Up
    /// (spec 0046).
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
}

/// One service a manifest declares.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredServiceDto {
    /// The alias, the key in `services`.
    pub alias: String,
    /// The provider, `namespace/name`.
    pub provider: String,
    /// Dedicated or shared.
    pub ownership: EnvOwnership,
    /// The runtime selector the manifest wrote.
    #[serde(default)]
    pub selector: Option<String>,
    /// The instance a shared service borrows.
    #[serde(default)]
    pub instance_ref: Option<String>,
    /// The instance name the manifest wrote.
    #[serde(default)]
    pub instance_name: Option<String>,
    /// What it waits for, in manifest order.
    #[serde(default)]
    pub depends_on: Vec<ServiceDependency>,
}

/// One problem in a manifest. The message is a fixed template per code and
/// never holds a value from the file.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvFinding {
    /// How serious it is; every v1alpha1 code is `error`.
    pub severity: FindingSeverity,
    /// The stable snake_case code of spec 0044, *Aturan validasi*.
    pub code: String,
    /// `line:column` for syntax, a field path otherwise, or `.`.
    pub location: String,
    /// The alias of the service it is about.
    #[serde(default)]
    pub service: Option<String>,
    /// What is wrong, in English.
    pub message: String,
}

/// Params of `env.validate`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvValidateParams {
    /// The absolute path of the manifest file: at most 1024 bytes, no NUL,
    /// no `.` or `..` segment.
    pub path: String,
}

impl Validate for EnvValidateParams {
    fn validate(&self) -> Vec<FieldError> {
        import_path_code(&self.path)
            .map(|code| FieldError::new("path", code))
            .into_iter()
            .collect()
    }
}

/// Reply of `env.validate`: also a success for a manifest that is not
/// valid.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvValidation {
    /// The file checked, as the request named it.
    pub file: String,
    /// Whether no finding is an error.
    pub valid: bool,
    /// The `apiVersion` of the file, when it is a supported one.
    #[serde(default)]
    pub api_version: Option<String>,
    /// Every `apiVersion` this daemon reads.
    pub supported_api_versions: Vec<String>,
    /// `metadata.name`, when it is a valid name.
    #[serde(default)]
    pub name: Option<String>,
    /// The SHA-256 of the bytes read.
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    /// The services that decoded far enough to describe.
    #[serde(default)]
    pub services: Vec<DeclaredServiceDto>,
    /// At most [`ENV_FINDINGS_MAX`] findings, in the order found.
    #[serde(default)]
    pub findings: Vec<EnvFinding>,
    /// Every finding, kept or not.
    pub finding_count: Count,
    /// The findings past the limit.
    pub omitted: Count,
}

/// Which manifest a plan reads. Tagged `type` and `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum EnvSource {
    /// The manifest at this absolute path.
    File(String),
    /// The manifest location of this registered environment.
    Registered(Uuid),
}

/// Params of `env.plan`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvPlanParams {
    /// The manifest.
    pub source: EnvSource,
    /// Whether nothing may be resolved from a manifest selector (spec
    /// 0045 AC-6). A daemon that does not know the field ignores it, so a
    /// client that sends true checks `EnvPlan.lock.locked`.
    #[serde(default)]
    pub locked: bool,
}

impl Validate for EnvPlanParams {
    fn validate(&self) -> Vec<FieldError> {
        match &self.source {
            EnvSource::File(path) => import_path_code(path)
                .map(|code| FieldError::new("source.content", code))
                .into_iter()
                .collect(),
            EnvSource::Registered(_) => Vec::new(),
        }
    }
}

/// How the manifest's name matches the registry. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanRegistration {
    /// No environment has the name.
    New,
    /// An environment has the name at this location.
    Registered,
    /// An environment has the name at another location.
    NameTaken,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// What Up does with a service. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    /// Create an instance, then start it.
    Create,
    /// Start an existing instance.
    Start,
    /// Leave an existing instance as it is.
    Keep,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One step of a plan item. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanStep {
    /// Install the runtime.
    InstallRuntime,
    /// Create the instance.
    CreateInstance,
    /// Start the instance.
    StartInstance,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Whether the provider of a `create` item may run. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanTrust {
    /// A catalog entry, or an approved local one.
    Trusted,
    /// A local entry not approved yet.
    ApprovalRequired,
    /// A revoked revision.
    Revoked,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// The state of one secret field of a `create` item. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanSecretStatus {
    /// Registered, with a value.
    Present,
    /// Registered without a value.
    Missing,
    /// Not in the registry.
    Unregistered,
    /// The store could not be asked.
    Unknown,
    /// No reference: create generates the value.
    Generated,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// The environment a plan or detail is about.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRef {
    /// The ID.
    pub id: Uuid,
    /// The name.
    pub name: String,
    /// The revision.
    pub revision: Revision,
}

/// The instance of a plan item.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanInstance {
    /// Its ID, when it exists.
    #[serde(default)]
    pub id: Option<Uuid>,
    /// Its name.
    pub name: String,
}

/// Where the runtime of a `create` item comes from. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSource {
    /// The artifact the lock names for this Mac.
    Lock,
    /// The manifest selector, resolved against the catalog; what a daemon
    /// without locks means.
    #[default]
    Catalog,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// The runtime of a `create` item.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanRuntime {
    /// The selector as the manifest wrote it, `stable` when it wrote none.
    pub selector: String,
    /// The exact version it resolved to.
    pub version: String,
    /// The artifact ID.
    pub artifact_id: String,
    /// The artifact digest.
    pub artifact_sha256: String,
    /// Whether Up installs it.
    pub install: bool,
    /// The installed runtime, when there is one.
    #[serde(default)]
    pub runtime_id: Option<Uuid>,
    /// The download size, when Up installs it.
    #[serde(default)]
    pub download_size: Option<Count>,
    /// Where it comes from.
    #[serde(default)]
    pub source: RuntimeSource,
}

/// One endpoint of a plan item.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanEndpoint {
    /// The endpoint.
    pub name: String,
    /// Its host.
    pub host: String,
    /// Its port: what create asks for, or the effective port of an
    /// existing instance as `fixed`.
    pub port: PortRequest,
}

/// One secret field of a `create` item.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanSecret {
    /// The field.
    pub field: String,
    /// The reference the manifest names, never a value.
    #[serde(default)]
    pub reference: Option<String>,
    /// Its state.
    pub status: PlanSecretStatus,
}

/// How an existing member differs from its service. Never a blocker.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanDifference {
    /// `provider`, `runtime`, `endpoint.<name>.port`, `ownership`,
    /// `instanceRef`, or `lock`.
    pub field: String,
    /// What the manifest says.
    pub manifest: String,
    /// What the instance has.
    pub instance: String,
}

/// The catalog entry a `create` item uses.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanProviderRevision {
    /// The revision label.
    pub revision: String,
    /// The digest of its provider manifest.
    pub manifest_sha256: String,
}

/// The data location a local override chose for a `create` item.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanStorage {
    /// The registered data location.
    pub location_id: Uuid,
    /// Its path; clients show home as `~`.
    pub path: String,
}

/// One service of a plan.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    /// The alias.
    pub alias: String,
    /// Its ownership.
    pub ownership: EnvOwnership,
    /// What Up does.
    pub action: PlanAction,
    /// The steps, in order.
    #[serde(default)]
    pub steps: Vec<PlanStep>,
    /// The instance.
    pub instance: PlanInstance,
    /// The provider.
    pub provider: String,
    /// The trust of the provider, for a `create` item only.
    #[serde(default)]
    pub trust: Option<PlanTrust>,
    /// The runtime, for a `create` item whose selector resolved.
    #[serde(default)]
    pub runtime: Option<PlanRuntime>,
    /// For `create`: every IP endpoint of the package in package order;
    /// otherwise the IP endpoints of the instance.
    #[serde(default)]
    pub endpoints: Vec<PlanEndpoint>,
    /// The secret fields of a `create` item, in schema order.
    #[serde(default)]
    pub secrets: Vec<PlanSecret>,
    /// The aliases it waits for.
    #[serde(default)]
    pub waits_for: Vec<String>,
    /// How an existing member differs from the manifest.
    #[serde(default)]
    pub differences: Vec<PlanDifference>,
    /// The preflight rows of a `create` item.
    #[serde(default)]
    pub checks: Vec<PreflightCheck>,
    /// The catalog entry a `create` item uses.
    #[serde(default)]
    pub provider_revision: Option<PlanProviderRevision>,
    /// The data location a local override chose, for a `create` item.
    #[serde(default)]
    pub storage: Option<PlanStorage>,
}

/// One problem that makes a plan not applicable.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanBlocker {
    /// The alias it is about, if any.
    #[serde(default)]
    pub service: Option<String>,
    /// The error, with the code create, resolve, or the `env` area uses.
    pub error: ErrorBody,
}

/// A member the manifest no longer declares; never touched.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetachedMember {
    /// The alias.
    pub alias: String,
    /// The instance.
    pub instance: InstanceRef,
    /// The member's ownership.
    pub ownership: EnvOwnership,
}

/// Reply of `env.plan`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvPlan {
    /// What `env.validate` says of the same bytes.
    pub validation: EnvValidation,
    /// How the name matches; absent for a manifest that is not valid.
    #[serde(default)]
    pub registration: Option<PlanRegistration>,
    /// The environment of the same name, if any.
    #[serde(default)]
    pub environment: Option<EnvironmentRef>,
    /// Where that environment is registered.
    #[serde(default)]
    pub registered_location: Option<String>,
    /// Whether the manifest is valid and nothing blocks.
    pub applicable: bool,
    /// The digest Up compares, only when applicable.
    #[serde(default)]
    pub plan_digest: Option<String>,
    /// One item per service, in manifest order.
    #[serde(default)]
    pub items: Vec<PlanItem>,
    /// The start order.
    #[serde(default)]
    pub order: Vec<String>,
    /// What blocks the plan.
    #[serde(default)]
    pub blockers: Vec<PlanBlocker>,
    /// Members the manifest no longer declares.
    #[serde(default)]
    pub detached: Vec<DetachedMember>,
    /// How many secret fields need a value from the user.
    pub needs_secrets: Count,
    /// The lock next to the manifest; always sent by a daemon that knows
    /// locks.
    #[serde(default)]
    pub lock: PlanLock,
    /// The local override file, when there is one.
    #[serde(default)]
    pub local: Option<PlanLocal>,
}

/// The state of a lock against its manifest. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    /// There is no lock file.
    #[default]
    Absent,
    /// Every dedicated service matches its entry.
    InSync,
    /// An alias does not match.
    Stale,
    /// The lock is not a valid lock.
    Invalid,
    /// The file can't be read.
    Unreadable,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Why an alias does not match its lock entry. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LockStaleReason {
    /// A dedicated service without an entry.
    NotLocked,
    /// The entry names another provider.
    ProviderChanged,
    /// The entry names another selector.
    SelectorChanged,
    /// The locked version does not satisfy the selector.
    SelectorUnsatisfied,
    /// An entry whose alias is not a dedicated service of the manifest.
    NotInManifest,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One alias that does not match the lock.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LockStaleItem {
    /// The alias.
    pub service: String,
    /// Why.
    pub reason: LockStaleReason,
}

/// The lock of a plan.
#[typeshare]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanLock {
    /// The lock file name, such as `stackly.lock`.
    pub file: String,
    /// Its state.
    pub state: LockState,
    /// The digest of a valid lock (`in_sync` or `stale`).
    #[serde(default)]
    pub sha256: Option<String>,
    /// Whether the daemon planned with `locked`; false from a daemon that
    /// does not know the field.
    #[serde(default)]
    pub locked: bool,
    /// Every target of every entry, by text.
    #[serde(default)]
    pub targets: Vec<String>,
    /// The aliases that do not match.
    #[serde(default)]
    pub stale: Vec<LockStaleItem>,
}

/// The state of a local override file that is there. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocalState {
    /// It has no error finding.
    Present,
    /// It has an error finding.
    Invalid,
    /// It can't be read.
    Unreadable,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// What became of one local override. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OverrideState {
    /// A `create` draft uses it.
    Applied,
    /// A port of a service with a member: only compared.
    Compared,
    /// A data location of a service with a member: never applied.
    NotAppliedExisting,
    /// The manifest declares no such service.
    Ignored,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One local override.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanOverride {
    /// The alias as the file wrote it.
    pub service: String,
    /// `endpoints.<name>.port` or `storage.dataLocation`.
    pub field: String,
    /// The port, `auto`, or the path with home as `~`.
    pub value: String,
    /// What became of it.
    pub state: OverrideState,
}

/// The local override file of a plan.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanLocal {
    /// The file name, such as `.stackly.local.yaml`.
    pub file: String,
    /// Its state.
    pub state: LocalState,
    /// The digest of the bytes read.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Every override, the ignored ones included.
    #[serde(default)]
    pub overrides: Vec<PlanOverride>,
    /// Every finding; an `error` one is the `env.local_invalid` blocker.
    #[serde(default)]
    pub findings: Vec<EnvFinding>,
}

/// How many services of an environment are ready. Open: spec 0046 may add
/// values; an older client shows them as "Unknown".
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStatus {
    /// Every service is ready.
    Ready,
    /// Some services are ready.
    PartiallyReady,
    /// No service is ready.
    Down,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One environment in a list.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSummary {
    /// The ID.
    pub id: Uuid,
    /// The unique name.
    pub name: String,
    /// The revision of the registry row.
    pub revision: Revision,
    /// The absolute path of the manifest; clients show home as `~`.
    #[serde(default)]
    pub manifest_location: Option<String>,
    /// The digest of the manifest last accepted.
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    /// The digest of the lock the last Up used, as the registry holds it.
    #[serde(default)]
    pub lock_sha256: Option<String>,
    /// The status.
    pub status: EnvironmentStatus,
    /// The services whose instance runs and is ready.
    pub ready_count: Count,
    /// The services of the snapshot.
    pub service_count: Count,
    /// When the row last changed.
    pub updated_at: Timestamp,
}

/// Reply of `env.list`: at most [`ENV_LIST_MAX`] environments, by name.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentList {
    /// Every environment.
    #[serde(default)]
    pub items: Vec<EnvironmentSummary>,
}

/// Params of `env.get`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvGetParams {
    /// The environment.
    pub id: Uuid,
    /// Whether the daemon reads the manifest file for `manifestState`.
    #[serde(default)]
    pub check_manifest: bool,
}

impl Validate for EnvGetParams {
    fn validate(&self) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Why a service is not ready. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotReadyReason {
    /// No instance exists yet.
    NotCreated,
    /// The instance is stopped.
    Stopped,
    /// The instance is starting.
    Starting,
    /// The instance is stopping.
    Stopping,
    /// The instance failed.
    Failed,
    /// It runs but is not ready.
    NotReady,
    /// Its state is unknown.
    Unavailable,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Why a service is not ready, with a redacted detail.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceNotReady {
    /// The reason.
    pub reason: NotReadyReason,
    /// The readiness detail or the last failure.
    #[serde(default)]
    pub detail: Option<String>,
}

/// The instance of an environment service.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstance {
    /// The ID.
    pub id: Uuid,
    /// The name.
    pub name: String,
    /// The exact runtime version.
    pub runtime_version: String,
    /// The lifecycle.
    pub lifecycle: InstanceLifecycle,
    /// The readiness.
    pub readiness: InstanceReadiness,
    /// The health.
    pub health: InstanceHealth,
}

/// One service of a registered environment.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentService {
    /// The alias.
    pub alias: String,
    /// The provider.
    pub provider: String,
    /// Its ownership.
    pub ownership: EnvOwnership,
    /// The runtime selector the manifest wrote.
    #[serde(default)]
    pub selector: Option<String>,
    /// The instance a shared service borrows.
    #[serde(default)]
    pub instance_ref: Option<String>,
    /// Its instance, absent when it does not exist yet.
    #[serde(default)]
    pub instance: Option<ServiceInstance>,
    /// Why it is not ready; absent when ready.
    #[serde(default)]
    pub not_ready: Option<ServiceNotReady>,
}

/// How the manifest file compares to the snapshot. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ManifestState {
    /// The file matches.
    InSync,
    /// The file changed since import.
    Changed,
    /// No location, or the file is gone.
    Missing,
    /// The file can't be read.
    Unreadable,
    /// The file has error findings.
    Invalid,
    /// The file declares another name.
    NameChanged,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Reply of `env.get`, `env.import`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDetail {
    /// The summary.
    pub summary: EnvironmentSummary,
    /// One per service of the snapshot.
    #[serde(default)]
    pub services: Vec<EnvironmentService>,
    /// Members the snapshot no longer declares.
    #[serde(default)]
    pub detached: Vec<DetachedMember>,
    /// The manifest state; only with `checkManifest`.
    #[serde(default)]
    pub manifest_state: Option<ManifestState>,
    /// The error findings of an `invalid` manifest; only with
    /// `checkManifest`.
    #[serde(default)]
    pub problems: Option<Count>,
    /// The lock; only with `checkManifest` and a manifest `in_sync` or
    /// `changed`.
    #[serde(default)]
    pub lock: Option<EnvLockInfo>,
    /// The local override file; only when `lock` is sent.
    #[serde(default)]
    pub local: Option<EnvLocalInfo>,
}

/// The lock of a registered environment.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvLockInfo {
    /// The file name.
    pub file: String,
    /// Its state.
    pub state: LockState,
    /// Every target of every entry, by text.
    #[serde(default)]
    pub targets: Vec<String>,
    /// How many services it locks.
    pub services: Count,
}

/// The local override file of a registered environment.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvLocalInfo {
    /// The file name.
    pub file: String,
    /// Its state; absent when there is no file.
    #[serde(default)]
    pub state: Option<LocalState>,
    /// Every override, by whether its service has a member.
    #[serde(default)]
    pub overrides: Vec<PlanOverride>,
    /// The error findings.
    pub problems: Count,
}

/// Params of `env.import`, an `import_environment` operation; the request
/// carries the `idempotencyKey`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportEnvironmentParams {
    /// The absolute path of the manifest, as for [`EnvValidateParams`].
    pub path: String,
    /// The `manifestSha256` of the plan the client showed.
    pub expected_manifest_sha256: String,
    /// The environment whose manifest location moves to this file.
    #[serde(default)]
    pub relink_environment_id: Option<Uuid>,
}

impl Validate for ImportEnvironmentParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields: Vec<FieldError> = import_path_code(&self.path)
            .map(|code| FieldError::new("path", code))
            .into_iter()
            .collect();
        if !is_sha256(&self.expected_manifest_sha256) {
            fields.push(FieldError::new(
                "expectedManifestSha256",
                FieldCode::InvalidFormat,
            ));
        }
        fields
    }
}

/// Params of `env.remove`, a `remove_environment` operation.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveEnvironmentParams {
    /// The environment.
    pub id: Uuid,
    /// The revision the client showed.
    pub expected_revision: Revision,
}

impl Validate for RemoveEnvironmentParams {
    fn validate(&self) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Reply of `env.remove`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemovedEnvironment {
    /// The ID it had.
    pub id: Uuid,
    /// The name it had.
    pub name: String,
}

/// One environment an instance belongs to.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceEnvironment {
    /// The environment.
    pub environment_id: Uuid,
    /// Its name.
    pub name: String,
    /// The alias of the instance in it.
    pub alias: String,
    /// Whether it owns the instance.
    pub ownership: EnvOwnership,
}

/// What `env.lock` picks again. Tagged `type` and `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum LockRefresh {
    /// Keep every entry that is still good.
    None,
    /// Pick every entry again.
    All,
    /// Pick these aliases again: 1 to 32.
    Services(Vec<String>),
}

/// Params of `env.lock`: writes the lock next to the manifest. Not an
/// operation: it writes that one file and never the registry.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvLockParams {
    /// The manifest.
    pub source: EnvSource,
    /// What is picked again.
    pub refresh: LockRefresh,
}

impl Validate for EnvLockParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields: Vec<FieldError> = match &self.source {
            EnvSource::File(path) => import_path_code(path)
                .map(|code| FieldError::new("source.content", code))
                .into_iter()
                .collect(),
            EnvSource::Registered(_) => Vec::new(),
        };
        if let LockRefresh::Services(aliases) = &self.refresh {
            if aliases.is_empty() || aliases.len() > ENV_SERVICES_MAX {
                fields.push(FieldError::new("refresh.content", FieldCode::OutOfRange));
            }
            if aliases
                .iter()
                .any(|alias| alias.is_empty() || alias.len() > 63)
            {
                fields.push(FieldError::new("refresh.content", FieldCode::InvalidFormat));
            }
        }
        fields
    }
}

/// How one service of `env.lock` changed. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LockChange {
    /// A new alias.
    Added,
    /// The same version.
    Kept,
    /// Another version.
    Updated,
    /// The entry was dropped.
    Removed,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One service of the reply of `env.lock`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LockedService {
    /// The alias.
    pub alias: String,
    /// The provider.
    pub provider: String,
    /// The selector.
    pub selector: String,
    /// The version locked; absent for a removed or blocked entry.
    #[serde(default)]
    pub version: Option<String>,
    /// The artifact for this Mac, when the entry has one.
    #[serde(default)]
    pub artifact_id: Option<String>,
    /// How it changed.
    pub change: LockChange,
    /// The version before, when it changed.
    #[serde(default)]
    pub previous_version: Option<String>,
}

/// Reply of `env.lock`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvLockResult {
    /// What `env.validate` says of the manifest.
    pub validation: EnvValidation,
    /// The lock file, as an absolute path; clients show home as `~`.
    pub lock_file: String,
    /// Whether the file was written.
    pub written: bool,
    /// Whether the new bytes differ from the file that was there.
    pub changed: bool,
    /// The digest of the lock, when it was written or is up to date.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Every target of every entry, by text.
    #[serde(default)]
    pub targets: Vec<String>,
    /// One per service, in manifest order, then removed entries.
    #[serde(default)]
    pub services: Vec<LockedService>,
    /// What stops the write.
    #[serde(default)]
    pub blockers: Vec<PlanBlocker>,
}

/// How `env.export` writes its text. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// `NAME=value` per line.
    #[default]
    Dotenv,
    /// `export NAME='value'` per line.
    Shell,
    /// A value this build does not know; the daemon refuses it.
    #[serde(other)]
    Unrecognized,
}

/// Params of `env.export`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvExportParams {
    /// The environment: a registered one, or the file it was imported
    /// from.
    pub source: EnvSource,
    /// How the text is written.
    #[serde(default)]
    pub format: ExportFormat,
}

impl Validate for EnvExportParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields: Vec<FieldError> = match &self.source {
            EnvSource::File(path) => import_path_code(path)
                .map(|code| FieldError::new("source.content", code))
                .into_iter()
                .collect(),
            EnvSource::Registered(_) => Vec::new(),
        };
        if self.format == ExportFormat::Unrecognized {
            fields.push(FieldError::new("format", FieldCode::InvalidFormat));
        }
        fields
    }
}

/// What an export variable holds. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportValueKind {
    /// The host.
    Host,
    /// The port.
    Port,
    /// `host:port`, IPv6 in brackets.
    Address,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One variable of an export.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportVariable {
    /// The name, such as `STACKLY_POSTGRES_CLIENT_PORT`.
    pub name: String,
    /// The value: a host, a port, or an address.
    pub value: String,
    /// The service.
    pub service: String,
    /// The endpoint.
    pub endpoint: String,
    /// What it holds.
    pub kind: ExportValueKind,
    /// Whether the port may change at the next start.
    pub may_change: bool,
}

/// Why a service or endpoint is not exported. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportSkipReason {
    /// The service has no instance yet.
    NotCreated,
    /// An `auto` endpoint that never had a port.
    PortNotAssigned,
    /// A Unix socket endpoint.
    UnixEndpoint,
    /// The pinned package can't be read.
    PackageUnavailable,
    /// The instance is retained.
    Retained,
    /// An earlier variable has the name.
    NameCollision,
    /// The redactor changed the value.
    Redacted,
    /// A value this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One service or endpoint left out of an export.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportSkipped {
    /// The service.
    pub service: String,
    /// The endpoint, when only one is left out.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Why.
    pub reason: ExportSkipReason,
}

/// Reply of `env.export`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvExport {
    /// The environment.
    pub environment: EnvironmentRef,
    /// How `text` is written.
    pub format: ExportFormat,
    /// The variables as text, one line each.
    pub text: String,
    /// The variables.
    #[serde(default)]
    pub variables: Vec<ExportVariable>,
    /// What is left out.
    #[serde(default)]
    pub skipped: Vec<ExportSkipped>,
}
