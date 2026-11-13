//! Environment manifests (spec 0044): `stackly.yaml` as it loads and
//! validates against the catalog, the read only plan of what Up would do,
//! and the status of a registered environment.
//!
//! Nothing here touches macOS: the daemon reads the file through the
//! `Filesystem` port and hands the bytes in.

mod finding;
pub mod manifest;
pub mod plan;
pub mod status;
mod validate;

pub use finding::{Finding, FindingCode, FindingLocation, Findings, MAX_FINDINGS, Severity};
#[cfg(feature = "json-schema")]
pub use manifest::json_schemas;
pub use manifest::{API_VERSION, KIND, MANIFEST_MAX_LEN, SUPPORTED_API_VERSIONS};
pub use plan::{
    BlockerKind, DetachedMember, EnvironmentRef, InstanceView, LockInputs, NOT_INITIALIZED,
    OverrideInputs, Plan, PlanAction, PlanBlocker, PlanDeps, PlanDifference, PlanEndpoint,
    PlanError, PlanInputs, PlanItem, PlanRuntime, PlanSecret, PlanSecretStatus, PlanStep,
    PlanTrust, Registration, draft_for, plan, planned_runtime_placeholder_id, start_order,
};
pub use status::{
    EnvironmentStatus, EnvironmentView, ManifestRead, ManifestState, NotReadyReason, ServiceStatus,
    environment_view, manifest_state, not_ready,
};
pub use validate::{
    EndpointRequest, EnvironmentManifest, MAX_DEPENDENCIES, MAX_SERVICES, MAX_TIMEOUT_SECONDS,
    ServiceSpec, Validation, manifest_digest, validate,
};
