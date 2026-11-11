//! `provider.validate` (spec 0036, *Kontrak wire*): the daemon checks a
//! provider package folder with the validator every Stackly tool shares and
//! answers what it found, without running, approving, or storing anything.
//!
//! A broken package is not a method error: the reply still succeeds with
//! `loadable: false` and its findings. The only error is
//! `request.invalid_params` for a path that is not absolute.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::wire::Count;

/// The longest `path` of [`ProviderValidateParams`], in bytes.
pub const PROVIDER_PATH_MAX: usize = 4096;
/// The most findings one [`ProviderValidation`] carries; the rest are
/// counted in `omitted`.
pub const VALIDATION_FINDINGS_MAX: usize = 100;

/// Params of `provider.validate`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderValidateParams {
    /// The absolute path of the package folder: 1 to
    /// [`PROVIDER_PATH_MAX`] bytes, starting with `/`, without NUL.
    pub path: String,
}

impl Validate for ProviderValidateParams {
    /// `path` is 1 to [`PROVIDER_PATH_MAX`] bytes, starts with `/`, and
    /// holds no NUL; anything else is `invalid_format`.
    fn validate(&self) -> Vec<FieldError> {
        let path = &self.path;
        if path.is_empty()
            || path.len() > PROVIDER_PATH_MAX
            || !path.starts_with('/')
            || path.contains('\0')
        {
            vec![FieldError::new("path", FieldCode::InvalidFormat)]
        } else {
            Vec::new()
        }
    }
}

/// Reply of `provider.validate`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderValidation {
    /// The last component of the path.
    pub folder: String,
    /// Whether the package loads: no finding has origin `package`.
    pub loadable: bool,
    /// Whether the package may be published: no finding is an error.
    pub publishable: bool,
    /// The raw `apiVersion` string of `provider.yaml`, when the package
    /// could be read and the field is a string.
    #[serde(default)]
    pub api_version: Option<String>,
    /// Every `apiVersion` this daemon loads.
    pub supported_api_versions: Vec<SupportedApiVersion>,
    /// Who the package is, when it validated.
    #[serde(default)]
    pub identity: Option<PackageIdentity>,
    /// The first [`VALIDATION_FINDINGS_MAX`] findings: `package`, then
    /// `authoring`, then `scenarios`.
    pub findings: Vec<ValidationFinding>,
    /// Every finding, those left out included.
    pub finding_count: Count,
    /// `findingCount` minus the length of `findings`.
    pub omitted: Count,
}

/// One `apiVersion` the daemon loads.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SupportedApiVersion {
    /// The value, such as `stackly/v1alpha1`.
    pub name: String,
    /// Whether new packages should use it.
    pub status: ApiVersionStatus,
}

/// The status of a supported `apiVersion`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApiVersionStatus {
    /// The version for new packages.
    Current,
    /// Still loads and runs, with one warning.
    Deprecated,
    /// A status this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Who a validated package is.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PackageIdentity {
    /// `metadata.id`, such as `official/nats`.
    pub provider: String,
    /// `metadata.revision`.
    pub revision: String,
    /// The digest of the package files, lowercase hex.
    pub manifest_sha256: String,
    /// The software it provides.
    pub software: String,
    /// `metadata.displayName`.
    pub display_name: String,
}

/// One problem the validator found.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationFinding {
    /// How serious it is.
    pub severity: FindingSeverity,
    /// Which rule set found it.
    pub origin: FindingOrigin,
    /// The package file, such as `provider.yaml` or
    /// `tests/scenarios.yaml`, or empty for the package as a whole.
    pub file: String,
    /// Where in the file: a field path, `line:column`, or `.`.
    pub location: String,
    /// The stable snake_case issue code of spec 0009.
    pub code: String,
    /// What is wrong, in English; never a secret or a settings value.
    pub message: String,
    /// A stable snake_case reason, for the codes that declare one.
    #[serde(default)]
    pub reason: Option<String>,
}

/// How serious a [`ValidationFinding`] is.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    /// The package is not publishable.
    Error,
    /// The package is publishable, but something should change.
    Warning,
    /// A severity this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Which rule set a [`ValidationFinding`] comes from.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingOrigin {
    /// The package cannot load anywhere.
    Package,
    /// A tightening for package authors; the package still loads.
    Authoring,
    /// `tests/scenarios.yaml`.
    Scenarios,
    /// An origin this build does not know.
    #[serde(other)]
    Unrecognized,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(path: &str) -> ProviderValidateParams {
        ProviderValidateParams {
            path: path.to_owned(),
        }
    }

    #[test]
    fn a_path_must_be_absolute_short_and_without_nul() {
        assert!(params("/Users/me/providers/mytool").validate().is_empty());
        let invalid = [FieldError::new("path", FieldCode::InvalidFormat)];
        assert_eq!(params("").validate(), invalid);
        assert_eq!(params("providers/mytool").validate(), invalid);
        assert_eq!(params("/a\0b").validate(), invalid);
        assert_eq!(
            params(&format!("/{}", "a".repeat(PROVIDER_PATH_MAX))).validate(),
            invalid
        );
        assert!(
            params(&format!("/{}", "a".repeat(PROVIDER_PATH_MAX - 1)))
                .validate()
                .is_empty()
        );
    }

    #[test]
    fn values_this_build_does_not_know_read_unrecognized() {
        let status: ApiVersionStatus = serde_json::from_str("\"retired\"").unwrap();
        let severity: FindingSeverity = serde_json::from_str("\"fatal\"").unwrap();
        let origin: FindingOrigin = serde_json::from_str("\"lint\"").unwrap();
        assert_eq!(status, ApiVersionStatus::Unrecognized);
        assert_eq!(severity, FindingSeverity::Unrecognized);
        assert_eq!(origin, FindingOrigin::Unrecognized);
    }
}
