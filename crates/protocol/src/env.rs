//! Environments (spec 0044): `stackly.yaml` checked (`env.validate`),
//! planned (`env.plan`), listed (`env.list`, `env.get`), registered
//! (`env.import`), and forgotten (`env.remove`).
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
    /// `provider`, `runtime`, `endpoint.<name>.port`, `ownership`, or
    /// `instanceRef`.
    pub field: String,
    /// What the manifest says.
    pub manifest: String,
    /// What the instance has.
    pub instance: String,
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
