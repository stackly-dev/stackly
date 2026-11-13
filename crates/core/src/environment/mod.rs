//! Environment manifests (spec 0044): `stackly.yaml` as it loads and
//! validates against the catalog.
//!
//! Nothing here touches macOS: the daemon reads the file through the
//! `Filesystem` port and hands the bytes in.

mod finding;
pub mod manifest;
mod validate;

pub use finding::{Finding, FindingCode, FindingLocation, Findings, MAX_FINDINGS, Severity};
#[cfg(feature = "json-schema")]
pub use manifest::json_schemas;
pub use manifest::{API_VERSION, KIND, MANIFEST_MAX_LEN, SUPPORTED_API_VERSIONS};
pub use validate::{
    EndpointRequest, EnvironmentManifest, MAX_DEPENDENCIES, MAX_SERVICES, MAX_TIMEOUT_SECONDS,
    ServiceSpec, Validation, manifest_digest, validate,
};
