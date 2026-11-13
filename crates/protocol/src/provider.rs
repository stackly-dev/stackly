//! The provider area: `provider.validate` (spec 0036, *Kontrak wire*) and
//! the import, approval, revocation, and removal of local provider packages
//! (spec 0037, *API surface*).
//!
//! `provider.validate` checks a package folder with the validator every
//! Stackly tool shares and answers what it found, without running,
//! approving, or storing anything. A broken package is not a method error:
//! the reply still succeeds with `loadable: false` and its findings. The
//! only error is `request.invalid_params` for a path that is not absolute.
//!
//! An imported package is data Stackly does not trust: it can be inspected
//! but nothing from it is downloaded or run until the user approves exactly
//! its bytes (`packageSha256`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::catalog::{
    CatalogIssue, PROVIDER_ID_MAX, ProviderCapability, ProviderCategory, TargetArch, TargetOs,
};
use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::wire::{Count, Revision, Timestamp};

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

/// The longest import path of [`PreviewImportParams`] and
/// [`ImportProviderParams`], in bytes (spec 0037 AC-2).
pub const IMPORT_PATH_MAX: usize = 1024;
/// The most entries of `ListProvidersReply.revocations`.
pub const REVOCATIONS_MAX: usize = 64;
/// The most versions an [`ImportPreview`] lists.
pub const PREVIEW_VERSIONS_MAX: usize = 16;
/// The most findings an [`ImportPreview`] carries; the rest are counted in
/// `omittedFindings`.
pub const PREVIEW_FINDINGS_MAX: usize = 100;
/// The most import issues an [`ImportPreview`] carries.
pub const PREVIEW_IMPORT_ISSUES_MAX: usize = 100;

/// `path` of an import request: 1 to [`IMPORT_PATH_MAX`] bytes, absolute,
/// without NUL and without a `.` or `..` component (refused, never
/// normalized).
pub(crate) fn import_path_code(path: &str) -> Option<FieldCode> {
    if path.is_empty() {
        Some(FieldCode::TooShort)
    } else if path.len() > IMPORT_PATH_MAX {
        Some(FieldCode::TooLong)
    } else if !path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        Some(FieldCode::InvalidFormat)
    } else {
        None
    }
}

/// `provider` of a provider operation: a provider ID with its namespace.
fn namespaced_provider_code(provider: &str) -> Option<FieldCode> {
    crate::catalog::provider_id_code(provider).or_else(|| {
        let valid = provider.len() <= PROVIDER_ID_MAX
            && provider
                .split_once('/')
                .is_some_and(|(namespace, name)| !namespace.is_empty() && !name.is_empty());
        (!valid).then_some(FieldCode::InvalidFormat)
    })
}

/// A SHA-256 digest: 64 lowercase hex characters.
pub(crate) fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Params of `provider.preview_import`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewImportParams {
    /// The absolute path of the package folder: 1 to [`IMPORT_PATH_MAX`]
    /// bytes, without a `.` or `..` component.
    pub path: String,
}

impl Validate for PreviewImportParams {
    fn validate(&self) -> Vec<FieldError> {
        import_path_code(&self.path)
            .map(|code| FieldError::new("path", code))
            .into_iter()
            .collect()
    }
}

/// Params of `provider.import`, an `import_provider` operation; the
/// request carries the `idempotencyKey`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportProviderParams {
    /// The absolute path of the package folder, as for
    /// [`PreviewImportParams`].
    pub path: String,
    /// The `packageSha256` of the preview the client showed; the import is
    /// refused with `provider.import_changed` when the folder now holds
    /// other bytes.
    pub expected_package_sha256: String,
}

impl Validate for ImportProviderParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields: Vec<FieldError> = import_path_code(&self.path)
            .map(|code| FieldError::new("path", code))
            .into_iter()
            .collect();
        if !is_sha256(&self.expected_package_sha256) {
            fields.push(FieldError::new(
                "expectedPackageSha256",
                FieldCode::InvalidFormat,
            ));
        }
        fields
    }
}

/// Params of `provider.approve`, an `approve_provider` operation.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideProviderParams {
    /// The imported provider, with its namespace.
    pub provider: String,
    /// The `local.packageSha256` of the detail the client showed.
    pub expected_package_sha256: String,
}

impl Validate for DecideProviderParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields: Vec<FieldError> = namespaced_provider_code(&self.provider)
            .map(|code| FieldError::new("provider", code))
            .into_iter()
            .collect();
        if !is_sha256(&self.expected_package_sha256) {
            fields.push(FieldError::new(
                "expectedPackageSha256",
                FieldCode::InvalidFormat,
            ));
        }
        fields
    }
}

/// Params of `provider.revoke` and `provider.remove`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTargetParams {
    /// The imported provider, with its namespace.
    pub provider: String,
}

impl Validate for ProviderTargetParams {
    fn validate(&self) -> Vec<FieldError> {
        namespaced_provider_code(&self.provider)
            .map(|code| FieldError::new("provider", code))
            .into_iter()
            .collect()
    }
}

/// Reply of `provider.preview_import`: what importing the folder would
/// bring in. It reads the folder and writes nothing.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// `metadata.id`, when it reads.
    #[serde(default)]
    pub provider: Option<String>,
    /// `metadata.displayName`, when it reads.
    #[serde(default)]
    pub display_name: Option<String>,
    /// `metadata.software`, when it reads.
    #[serde(default)]
    pub software: Option<String>,
    /// `metadata.category`, when it reads.
    #[serde(default)]
    pub category: Option<ProviderCategory>,
    /// `metadata.description`, when it reads.
    #[serde(default)]
    pub description: Option<String>,
    /// `metadata.revision`, when it reads.
    #[serde(default)]
    pub provider_revision: Option<String>,
    /// The namespace of `metadata.id`, never verified.
    #[serde(default)]
    pub publisher: Option<String>,
    /// The manifest digest, when the package validated.
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    /// The `versions.json` digest, when the file was read.
    #[serde(default)]
    pub versions_sha256: Option<String>,
    /// The digest of every package file, the bytes an approval binds, when
    /// the files were read.
    #[serde(default)]
    pub package_sha256: Option<String>,
    /// The bytes of every package file together.
    pub package_bytes: Count,
    /// How many files the package has.
    pub file_count: Count,
    /// The folder, with the home folder written `~`.
    pub origin_path: String,
    /// Whether `provider.yaml` carries a quarantine attribute; null when
    /// the attribute could not be read.
    #[serde(default)]
    pub quarantined: Option<bool>,
    /// The agent that set the attribute, when it was read.
    #[serde(default)]
    pub quarantine_agent: Option<String>,
    /// The exact versions, first [`PREVIEW_VERSIONS_MAX`].
    #[serde(default)]
    pub versions: Vec<String>,
    /// How many exact versions there are.
    pub version_count: Count,
    /// What the package may run, read, and download; null when it does not
    /// validate.
    #[serde(default)]
    pub permissions: Option<ProviderPermissions>,
    /// The first [`PREVIEW_FINDINGS_MAX`] validator findings, without the
    /// `scenarios` ones.
    #[serde(default)]
    pub findings: Vec<ValidationFinding>,
    /// Findings left out of `findings`.
    pub omitted_findings: Count,
    /// What the import rules refuse.
    #[serde(default)]
    pub import_issues: Vec<CatalogIssue>,
    /// Whether it can be imported.
    pub verdict: ImportVerdict,
    /// The import of the same provider ID that is stored now, if any.
    #[serde(default)]
    pub existing: Option<ExistingImport>,
    /// What differs from the last decision on that provider; empty without
    /// a decision.
    #[serde(default)]
    pub changes: Vec<PermissionChange>,
    /// The revocation of the installed catalog for this package's key.
    #[serde(default)]
    pub revocation: Option<RevocationInfo>,
}

/// Whether a previewed package can be imported.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportVerdict {
    /// True when no `package` or `authoring` finding is an error, no
    /// import issue holds, and the import limits allow it.
    pub importable: bool,
    /// Why not, when not importable.
    #[serde(default)]
    pub reason: Option<ImportBlockReason>,
}

/// The stored import of the same provider ID.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExistingImport {
    /// Its `metadata.revision`.
    pub provider_revision: String,
    /// Its package digest.
    pub package_sha256: String,
    /// Where its stored bytes stand.
    pub approval: ApprovalState,
    /// Whether the previewed folder holds exactly these bytes.
    pub same_package: bool,
    /// Whether its store copy loads and matches.
    pub stored_copy_valid: bool,
}

/// What a package may do (spec 0037, *Ringkasan izin*), for display and
/// comparison only.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPermissions {
    /// The supported capabilities, in the capability order of spec 0017.
    #[serde(default)]
    pub capabilities: Vec<ProviderCapability>,
    /// The folders it writes: `data`, `config`, `logs`, `temp`, `run`.
    #[serde(default)]
    pub managed_filesystem: Vec<String>,
    /// The programs it runs.
    #[serde(default)]
    pub executables: Vec<PermissionExecutable>,
    /// The secret fields it hands to a process.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// The endpoints it declares.
    #[serde(default)]
    pub endpoints: Vec<PermissionEndpoint>,
    /// The artifacts it may download.
    #[serde(default)]
    pub downloads: Vec<PermissionDownload>,
    /// The `openWith` entries.
    #[serde(default)]
    pub open_with: Vec<PermissionOpenWith>,
}

/// One program a package runs.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionExecutable {
    /// `runtime` or the alias.
    pub name: String,
    /// The path under the runtime root.
    pub path: String,
    /// Where the manifest runs it.
    #[serde(default)]
    pub uses: Vec<ExecutableUse>,
}

/// One endpoint a package declares.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionEndpoint {
    /// The endpoint name.
    pub name: String,
    /// `tcp`, `udp`, or `unix`.
    pub transport: String,
    /// `client`, `monitoring`, or `admin`.
    pub purpose: String,
    /// The suggested port of an IP endpoint.
    #[serde(default)]
    pub default_port: Option<u16>,
}

/// One artifact a package may download.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDownload {
    /// The artifact ID.
    pub artifact: String,
    /// The exact version.
    pub version: String,
    /// The OS it is built for.
    pub os: TargetOs,
    /// The architecture it is built for.
    pub arch: TargetArch,
    /// The SHA-256 the package author wrote for it.
    pub sha256: String,
    /// The download size in bytes.
    pub size: Count,
    /// The unique hosts of its URLs, at most 8.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// The signing policy code, such as `keep_or_adhoc`.
    pub signing_policy: String,
}

/// One `openWith` entry of a package.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOpenWith {
    /// The entry name.
    pub name: String,
    /// `app`, `terminal`, `browser`, or `reveal`.
    pub kind: String,
    /// The first bundle ID of an `app` entry or the executable of a
    /// `terminal` entry.
    #[serde(default)]
    pub target: Option<String>,
}

/// The import facts and last decision of a local entry, in
/// `ProviderDetail.local`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalProviderInfo {
    /// When the last import committed.
    pub imported_at: Timestamp,
    /// The folder it was imported from, with the home folder written `~`.
    pub origin_path: String,
    /// Whether the origin `provider.yaml` carried a quarantine attribute.
    #[serde(default)]
    pub quarantined: Option<bool>,
    /// The digest of its `versions.json`.
    pub versions_sha256: String,
    /// The digest of the stored bytes, which `provider.approve` sends back.
    pub package_sha256: String,
    /// The bytes of the stored package.
    pub package_bytes: Count,
    /// The last decision, on these or other bytes.
    #[serde(default)]
    pub decision: Option<TrustDecisionInfo>,
    /// What the stored package may do; null for an invalid entry.
    #[serde(default)]
    pub permissions: Option<ProviderPermissions>,
    /// What differs from the last decision, only while `pending` with a
    /// decision.
    #[serde(default)]
    pub changes: Vec<PermissionChange>,
}

/// One trust decision.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrustDecisionInfo {
    /// `approved` or `revoked`.
    pub decision: ApprovalState,
    /// When it was decided.
    pub decided_at: Timestamp,
    /// The revision label of the bytes it covers.
    pub provider_revision: String,
    /// The digest of the bytes it covers.
    pub package_sha256: String,
}

/// When and by whom a revision was revoked.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevocationInfo {
    /// Who revoked it.
    pub by: RevocationSource,
    /// Since when.
    pub since: Timestamp,
    /// The catalog's reason.
    #[serde(default)]
    pub reason: Option<String>,
    /// The revision of the catalog that revokes it.
    #[serde(default)]
    pub catalog_revision: Option<Revision>,
}

/// One entry of the installed catalog's `revoked` list.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRevocation {
    /// The provider ID.
    pub provider: String,
    /// The revoked revision label.
    pub provider_revision: String,
    /// The revoked manifest digest; null for every digest of the revision.
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    /// Why.
    pub reason: String,
    /// Since when, `YYYY-MM-DD`.
    pub since: String,
}

/// Content of the `catalog.changed` event: an operation changed the local
/// entries, so a client reads `catalog.list_providers` again.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogChanged {
    /// The provider that changed.
    #[serde(default)]
    pub provider: Option<String>,
    /// What happened to it.
    pub change: CatalogChangeKind,
}

/// Reply of `provider.remove`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemovedProvider {
    /// The provider that was removed.
    pub provider: String,
}

/// Where an imported provider's stored bytes stand. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    /// No decision covers the stored bytes.
    Pending,
    /// The stored bytes are approved.
    Approved,
    /// The approval of the stored bytes was revoked.
    Revoked,
    /// A state this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Who revoked a revision. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevocationSource {
    /// The user.
    User,
    /// The installed catalog.
    Catalog,
    /// A source this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Why a package cannot be imported. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportBlockReason {
    /// A validator error or an import rule.
    PackageInvalid,
    /// An import limit.
    ImportLimit,
    /// A reason this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// What differs from the last decision (spec 0037, *Kode perubahan*). Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionChange {
    /// The revision label.
    Revision,
    /// The programs it runs.
    Executables,
    /// The files it downloads.
    Downloads,
    /// The download hosts.
    DownloadHosts,
    /// The capabilities.
    Capabilities,
    /// The folders it writes.
    Filesystem,
    /// The secrets it receives.
    Secrets,
    /// The endpoints.
    Endpoints,
    /// The apps it opens.
    OpenWith,
    /// Other bytes only.
    Content,
    /// A change this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// Where a package runs a program. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableUse {
    /// It starts the server.
    Start,
    /// It runs during setup.
    Initialize,
    /// It runs on stop.
    Stop,
    /// It runs on reload.
    Reload,
    /// It runs health checks.
    Probe,
    /// It runs once at install.
    VersionCheck,
    /// It opens in Terminal.
    OpenWith,
    /// A use this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// What happened in a `catalog.changed` event. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogChangeKind {
    /// A package was imported.
    Imported,
    /// An import was approved.
    Approved,
    /// An approval was revoked.
    Revoked,
    /// An import was removed.
    Removed,
    /// A change this build does not know.
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
        let foreign = |text: &str| format!("\"{text}\"");
        assert_eq!(
            serde_json::from_str::<ApprovalState>(&foreign("frozen")).unwrap(),
            ApprovalState::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<RevocationSource>(&foreign("vendor")).unwrap(),
            RevocationSource::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<ImportBlockReason>(&foreign("too_big")).unwrap(),
            ImportBlockReason::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<PermissionChange>(&foreign("network")).unwrap(),
            PermissionChange::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<ExecutableUse>(&foreign("backup")).unwrap(),
            ExecutableUse::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<CatalogChangeKind>(&foreign("renamed")).unwrap(),
            CatalogChangeKind::Unrecognized
        );
    }

    #[test]
    fn an_import_path_is_absolute_short_and_never_normalized() {
        let check = |path: &str| {
            PreviewImportParams {
                path: path.to_owned(),
            }
            .validate()
        };
        assert!(check("/Users/me/acme-demo").is_empty());
        assert_eq!(check(""), [FieldError::new("path", FieldCode::TooShort)]);
        for path in ["acme", "/a/./b", "/a/../b", "/a\0b"] {
            assert_eq!(
                check(path),
                [FieldError::new("path", FieldCode::InvalidFormat)],
                "{path}"
            );
        }
        assert_eq!(
            check(&format!("/{}", "a".repeat(IMPORT_PATH_MAX))),
            [FieldError::new("path", FieldCode::TooLong)]
        );
    }

    #[test]
    fn provider_operations_need_a_namespaced_id_and_a_digest() {
        let digest = "a".repeat(64);
        let decide = |provider: &str, sha: &str| {
            DecideProviderParams {
                provider: provider.to_owned(),
                expected_package_sha256: sha.to_owned(),
            }
            .validate()
        };
        assert!(decide("acme/demo", &digest).is_empty());
        assert_eq!(
            decide("demo", &digest),
            [FieldError::new("provider", FieldCode::InvalidFormat)]
        );
        assert_eq!(
            decide("acme/demo", "ABC"),
            [FieldError::new(
                "expectedPackageSha256",
                FieldCode::InvalidFormat
            )]
        );
        assert_eq!(
            ProviderTargetParams {
                provider: "acme/".to_owned()
            }
            .validate(),
            [FieldError::new("provider", FieldCode::InvalidFormat)]
        );
        let import = ImportProviderParams {
            path: "relative".to_owned(),
            expected_package_sha256: digest,
        };
        assert_eq!(
            import.validate(),
            [FieldError::new("path", FieldCode::InvalidFormat)]
        );
    }
}
