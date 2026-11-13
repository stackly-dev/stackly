//! Environment manifests (spec 0044): `stackly.yaml` as it loads and
//! validates against the catalog, the read only plan of what Up would do,
//! and the status of a registered environment.
//!
//! Nothing here touches macOS: the daemon reads the file through the
//! `Filesystem` port and hands the bytes in.

pub mod children;
pub mod down;
pub mod export;
mod finding;
pub mod local;
pub mod lock;
pub mod manifest;
pub mod plan;
pub mod register;
pub mod status;
pub mod up;
mod validate;

pub use finding::{Finding, FindingCode, FindingLocation, Findings, MAX_FINDINGS, Severity};

/// Every editor JSON Schema of the environment files, with the file stem
/// `cargo xtask codegen` writes each under in `schemas/environment/`: the
/// manifest (spec 0044 AC-24), the lock, and the local overrides (spec
/// 0045 AC-20). They describe structure and names; the loaders stay the
/// authority.
#[cfg(feature = "json-schema")]
#[must_use]
pub fn json_schemas() -> Vec<(&'static str, schemars::Schema)> {
    let mut schemas = manifest::json_schemas();
    schemas.extend(lock::json_schemas());
    schemas.extend(local::json_schemas());
    schemas
}
pub use manifest::{API_VERSION, KIND, MANIFEST_MAX_LEN, SUPPORTED_API_VERSIONS, declared_name};
pub use plan::{
    BlockerKind, DetachedMember, EnvironmentRef, InstanceView, NOT_INITIALIZED, Plan, PlanAction,
    PlanBlocker, PlanDeps, PlanDifference, PlanEndpoint, PlanError, PlanInputs, PlanItem,
    PlanLocal, PlanLock, PlanOverride, PlanRuntime, PlanSecret, PlanSecretStatus, PlanStep,
    PlanStorage, PlanTrust, Registration, RuntimeSource, create_draft, draft_for,
    manifest_file_name, plan, planned_runtime_placeholder_id, start_order,
};
pub use register::{
    ENVIRONMENT_LIMIT, EnvOpError, ImportRequest, LEASE_ITEMS_MAX, RegisterDecision, RegisterInput,
    UnreadableReason, import_work, manifest_location, read_manifest, register, remove_work,
    unregister,
};
pub use status::{
    EnvironmentStatus, EnvironmentView, ManifestRead, ManifestState, NotReadyReason, ServiceStatus,
    environment_view, manifest_state, not_ready,
};
pub use validate::{
    EndpointRequest, EnvironmentManifest, MAX_DEPENDENCIES, MAX_SERVICES, MAX_TIMEOUT_SECONDS,
    ServiceSpec, Validation, manifest_digest, validate,
};
