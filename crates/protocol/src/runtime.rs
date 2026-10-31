//! The `runtime` area (spec 0018, *API surface*): the DTOs of
//! `runtime.install`, `runtime.resolve`, `runtime.list`, and `runtime.get`.
//!
//! The daemon resolves a selector to one exact artifact and works out the
//! support status; clients show them and never derive one from raw catalog
//! data. `runtime.install` starts an `install_runtime` operation (spec
//! 0007), so its request needs an `idempotencyKey`; an artifact that is
//! already installed answers at once without an operation. The only event
//! is the unit kind `runtime.state_changed`: clients fetch the runtime
//! again with `runtime.get`. `stacklyd` maps the domain and core types to
//! these DTOs and never sends a domain type.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::catalog::{
    DependencyKind, HostTarget, SupportReason, SupportStatus, TargetArch, TargetOs,
    provider_id_code,
};
use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::operation::OperationAccepted;
use crate::wire::{Count, Revision, Timestamp};

/// The longest `selector`, in bytes: `sha256:` and 64 hex characters, with
/// room for a long version.
pub const SELECTOR_MAX: usize = 80;
/// The smallest page size `runtime.list` accepts.
pub const RUNTIME_LIST_LIMIT_MIN: u64 = 1;
/// The largest page size `runtime.list` accepts.
pub const RUNTIME_LIST_LIMIT_MAX: u64 = 200;
/// The page size of `runtime.list` when `limit` is absent.
pub const RUNTIME_LIST_LIMIT_DEFAULT: u64 = 100;
/// The most JSON bytes the summaries of one `runtime.list` page add up to
/// (900 KiB), so the reply always fits in a control frame.
pub const RUNTIME_PAGE_BYTES_MAX: usize = 900 * 1024;

/// Params of `runtime.install` and `runtime.resolve`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstallRuntimeParams {
    /// The provider ID, such as `official/postgresql`.
    pub provider: String,
    /// What to install: `stable`, a line such as `18`, an exact version
    /// such as `18.6`, or `sha256:<64 hex>`. 1 to 80 bytes of letters,
    /// digits, and `.`, `-`, `_`, `+`, `:`.
    pub selector: String,
    /// The SHA-256 the person saw when they chose, 64 lowercase hex
    /// characters. A resolution to another artifact is refused with
    /// `runtime.selector_changed`.
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

impl Validate for InstallRuntimeParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if let Some(code) = provider_id_code(&self.provider) {
            fields.push(FieldError::new("provider", code));
        }
        if let Some(code) = selector_code(&self.selector) {
            fields.push(FieldError::new("selector", code));
        }
        if self
            .expected_sha256
            .as_deref()
            .is_some_and(|digest| !is_sha256(digest))
        {
            fields.push(FieldError::new("expectedSha256", FieldCode::InvalidFormat));
        }
        fields
    }
}

/// Why `selector` is not a selector, or `None` when it is one.
fn selector_code(selector: &str) -> Option<FieldCode> {
    if selector.is_empty() {
        Some(FieldCode::TooShort)
    } else if selector.len() > SELECTOR_MAX {
        Some(FieldCode::TooLong)
    } else if !selector
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+' | b':'))
    {
        Some(FieldCode::InvalidFormat)
    } else {
        None
    }
}

/// Whether `text` is 64 lowercase hex characters.
fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Params of `runtime.list`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListRuntimesParams {
    /// Most runtimes on the page, 1 to 200; absent means 100.
    #[serde(default)]
    pub limit: Option<Count>,
    /// The `next` cursor of the previous page, sent back unchanged.
    #[serde(default)]
    pub after: Option<Uuid>,
}

impl ListRuntimesParams {
    /// The page size: `limit`, or [`RUNTIME_LIST_LIMIT_DEFAULT`].
    #[must_use]
    pub fn limit_or_default(&self) -> u64 {
        self.limit.map_or(RUNTIME_LIST_LIMIT_DEFAULT, Count::get)
    }
}

impl Validate for ListRuntimesParams {
    fn validate(&self) -> Vec<FieldError> {
        if self.limit.is_some_and(|limit| {
            !(RUNTIME_LIST_LIMIT_MIN..=RUNTIME_LIST_LIMIT_MAX).contains(&limit.get())
        }) {
            vec![FieldError::new("limit", FieldCode::OutOfRange)]
        } else {
            Vec::new()
        }
    }
}

/// Params of `runtime.get`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetRuntimeParams {
    /// The runtime.
    pub runtime_id: Uuid,
}

impl Validate for GetRuntimeParams {}

/// The archive format of an artifact. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    /// A gzip compressed tar archive.
    TarGz,
    /// A zip archive.
    Zip,
    /// A single executable.
    Binary,
    /// A format this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How a runtime was installed. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstallModeKind {
    /// Stackly downloaded and installed it.
    Managed,
    /// Found on the system and registered.
    External,
    /// A mode this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The state of a runtime. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    /// Removed.
    Absent,
    /// The selector resolved to an exact identity.
    Resolving,
    /// The artifact is downloading.
    Downloading,
    /// The artifact is being verified.
    Verifying,
    /// The files are being installed.
    Installing,
    /// Ready to use.
    Installed,
    /// A step before `installed` failed.
    Failed,
    /// The installed files are gone.
    Missing,
    /// The installed files changed.
    Drifted,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What a compatibility record tested. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityScope {
    /// The artifact alone ran under a LaunchAgent.
    Runtime,
    /// A provider package ran against the artifact.
    Provider,
    /// A scope this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The result of a compatibility record. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityResult {
    /// Every check passed.
    Pass,
    /// A check failed.
    Fail,
    /// A result this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How an artifact is signed at install. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SigningPolicy {
    /// Keep a valid signature, or sign ad hoc when a file has none.
    KeepOrAdhoc,
    /// Always sign every Mach-O file ad hoc.
    AdhocPipeline,
    /// A policy this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The kind of signature an executable carries. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignatureKindDto {
    /// Signed with a Developer ID.
    DeveloperId,
    /// Signed ad hoc.
    Adhoc,
    /// Signed by the linker.
    LinkerSigned,
    /// Another signature, such as Apple's.
    Other,
    /// Not signed.
    Unsigned,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The one artifact a selector resolved to, as `runtime.resolve` and
/// `runtime.install` answer it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedArtifact {
    /// The provider ID.
    pub provider: String,
    /// The software, such as `postgresql`.
    pub software: String,
    /// The artifact ID from `versions.json`.
    pub artifact_id: String,
    /// The exact version.
    pub version: String,
    /// The line, such as `18`.
    pub line: String,
    /// The OS and architecture it is built for.
    pub target: HostTarget,
    /// The SHA-256 of the archive, 64 lowercase hex characters.
    pub sha256: String,
    /// The archive size in bytes.
    pub size: Count,
    /// The unpacked size in bytes, once `versions.json` records it.
    #[serde(default)]
    pub unpacked_size: Option<Count>,
    /// The archive format.
    pub format: ArtifactFormat,
    /// The oldest macOS it runs on, such as `13.0`.
    pub min_os: String,
    /// Whether `@stable` selects it.
    pub stable: bool,
    /// How supported it is on this Mac.
    pub support: SupportStatus,
    /// Why.
    pub support_reason: SupportReason,
}

/// An instance that uses a runtime.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUser {
    /// The instance.
    pub instance_id: Uuid,
    /// Its name.
    pub name: String,
    /// True for a removed instance whose data is kept.
    pub retained: bool,
}

/// One runtime as a list row or a card shows it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSummary {
    /// The runtime.
    pub id: Uuid,
    /// Its revision, raised by every change.
    pub revision: Revision,
    /// The software, such as `postgresql`.
    pub software: String,
    /// The catalog's display name for the software; the software name when
    /// the catalog has none.
    pub display_name: String,
    /// The exact version.
    pub version: String,
    /// The line.
    pub line: String,
    /// The OS and architecture.
    pub target: HostTarget,
    /// How it was installed.
    pub install_mode: InstallModeKind,
    /// Its state.
    pub state: RuntimeState,
    /// The SHA-256 of the artifact it came from; null for an external
    /// runtime.
    #[serde(default)]
    pub artifact_sha256: Option<String>,
    /// When it was installed.
    #[serde(default)]
    pub installed_at: Option<Timestamp>,
    /// When it was last verified.
    #[serde(default)]
    pub last_verified_at: Option<Timestamp>,
    /// How supported it is on this Mac.
    pub support: SupportStatus,
    /// Why.
    pub support_reason: SupportReason,
    /// The instances that use it, retained ones included.
    #[serde(default)]
    pub used_by: Vec<RuntimeUser>,
    /// The runtimes that depend on it.
    #[serde(default)]
    pub dependent_runtimes: Vec<Uuid>,
}

/// One library an artifact links.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInfo {
    /// The library, such as `libssl.3.dylib`.
    pub name: String,
    /// Where it comes from.
    pub kind: DependencyKind,
    /// The install name the binaries load it by.
    pub install_name: String,
    /// Its version, if known.
    #[serde(default)]
    pub version: Option<String>,
    /// Its license, if known.
    #[serde(default)]
    pub license: Option<String>,
    /// Where its source is.
    #[serde(default)]
    pub source_url: Option<String>,
}

/// The signature of one executable.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableSignature {
    /// The path inside the runtime folder.
    pub path: String,
    /// The kind of signature.
    pub kind: SignatureKindDto,
    /// The Apple team ID of a Developer ID signature.
    #[serde(default)]
    pub team_id: Option<String>,
}

/// What the install found and did about signatures.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    /// The policy the artifact asked for.
    pub policy: SigningPolicy,
    /// True when the install signed files ad hoc.
    pub adhoc_signed_at_install: bool,
    /// How many Mach-O files the runtime holds.
    pub mach_o_files: Count,
    /// The signature of each executable.
    #[serde(default)]
    pub executables: Vec<ExecutableSignature>,
}

/// What the registry knows about the artifact a runtime came from.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInfo {
    /// The artifact ID from `versions.json`.
    #[serde(default)]
    pub catalog_id: Option<String>,
    /// The SHA-256 of the archive.
    pub sha256: String,
    /// The archive size in bytes.
    #[serde(default)]
    pub size: Option<Count>,
    /// The unpacked size in bytes.
    #[serde(default)]
    pub unpacked_size: Option<Count>,
    /// The archive format.
    #[serde(default)]
    pub format: Option<ArtifactFormat>,
    /// The oldest macOS it runs on.
    #[serde(default)]
    pub minimum_os: Option<String>,
    /// The build number.
    #[serde(default)]
    pub build: Option<Count>,
    /// Where it was downloaded from.
    #[serde(default)]
    pub source: Option<String>,
    /// Its signatures.
    #[serde(default)]
    pub signature: Option<SignatureInfo>,
    /// Its libraries.
    #[serde(default)]
    pub dependencies: Vec<DependencyInfo>,
    /// What validation noted without failing.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// The provider package a `provider` record was tested with.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRevisionRef {
    /// The package ID.
    pub id: String,
    /// The package revision.
    pub revision: String,
    /// The SHA-256 of the manifest.
    pub manifest_sha256: String,
}

/// One compatibility record with the artifact it belongs to (PRD section
/// 32.2).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityRecordView {
    /// What was tested.
    pub scope: CompatibilityScope,
    /// How it went.
    pub result: CompatibilityResult,
    /// The package a `provider` record was tested with.
    #[serde(default)]
    pub provider_revision: Option<ProviderRevisionRef>,
    /// The OS of the host.
    pub os: TargetOs,
    /// The OS version of the host, such as `27.0`.
    pub os_version: String,
    /// The OS build of the host, such as `26A428`.
    #[serde(default)]
    pub os_build: Option<String>,
    /// The architecture of the host.
    pub arch: TargetArch,
    /// The checks that ran and passed.
    #[serde(default)]
    pub checks: Vec<String>,
    /// What the record does not cover.
    #[serde(default)]
    pub known_limitations: Vec<String>,
    /// One line about the run.
    #[serde(default)]
    pub summary: Option<String>,
    /// When it was tested.
    pub tested_at: Timestamp,
    /// Where the evidence is, relative to `evidence/`.
    pub evidence: String,
    /// Where the artifact was downloaded from.
    #[serde(default)]
    pub source_url: Option<String>,
    /// The SHA-256 of the artifact.
    pub sha256: String,
}

/// One runtime with everything `stackly runtime inspect` and the
/// inspector show.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDetail {
    /// The row.
    pub summary: RuntimeSummary,
    /// The runtime folder, absolute.
    pub path: String,
    /// The artifact it came from.
    #[serde(default)]
    pub artifact: Option<ArtifactInfo>,
    /// The artifact's executables, relative to the runtime folder.
    #[serde(default)]
    pub executables: Vec<String>,
    /// Its compatibility records: the record that decided the support first,
    /// then the rest newest first by `testedAt`.
    #[serde(default)]
    pub compatibility: Vec<CompatibilityRecordView>,
}

/// An install that has not ended.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstallingRuntime {
    /// The `install_runtime` operation; its stage and progress come from
    /// `operation.get`.
    pub operation_id: Uuid,
    /// The runtime it will create.
    pub runtime_id: Uuid,
    /// The provider ID.
    pub provider: String,
    /// The software.
    pub software: String,
    /// The catalog's display name for the software.
    pub display_name: String,
    /// The exact version.
    pub version: String,
    /// The OS and architecture.
    pub target: HostTarget,
    /// The SHA-256 of the artifact.
    pub sha256: String,
}

/// Reply of `runtime.list`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeListPage {
    /// The runtimes on this page.
    #[serde(default)]
    pub items: Vec<RuntimeSummary>,
    /// The cursor of the next page, to send back as `after`; null on the
    /// last page.
    #[serde(default)]
    pub next: Option<Uuid>,
    /// The installs that have not ended; only on the first page.
    #[serde(default)]
    pub installing: Vec<InstallingRuntime>,
}

/// What `runtime.install` did.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum InstallOutcome {
    /// An `install_runtime` operation started, or an earlier request with
    /// the same idempotency key started it.
    Started(Box<OperationAccepted>),
    /// The artifact is installed already; nothing ran.
    AlreadyInstalled(Box<RuntimeSummary>),
}

/// Reply of `runtime.install`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstallRuntimeReply {
    /// The artifact the selector resolved to.
    pub resolved: ResolvedArtifact,
    /// What happened.
    pub outcome: InstallOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd";

    fn install(provider: &str, selector: &str, digest: Option<&str>) -> InstallRuntimeParams {
        InstallRuntimeParams {
            provider: provider.to_owned(),
            selector: selector.to_owned(),
            expected_sha256: digest.map(str::to_owned),
        }
    }

    fn paths(fields: Vec<FieldError>) -> Vec<String> {
        fields.iter().map(|field| field.path().to_owned()).collect()
    }

    #[test]
    fn install_params_accept_every_selector_shape() {
        for selector in [
            "stable",
            "18",
            "18.6",
            "26.8.30.8",
            "9.1.2-r2",
            &format!("sha256:{DIGEST}"),
        ] {
            let params = install("official/postgresql", selector, Some(DIGEST));
            assert_eq!(params.validate(), Vec::new(), "{selector}");
        }
    }

    #[test]
    fn install_params_name_each_bad_field() {
        assert_eq!(
            paths(install("Official/PG", "stable", None).validate()),
            ["provider"]
        );
        assert_eq!(
            paths(install("official/nats", "", None).validate()),
            ["selector"]
        );
        for bad in ["a b", "18/6", "18;6", &"1".repeat(SELECTOR_MAX + 1)] {
            assert_eq!(
                paths(install("official/nats", bad, None).validate()),
                ["selector"],
                "{bad}"
            );
        }
        for bad in ["", "ABC", &DIGEST.to_uppercase(), &DIGEST[1..]] {
            assert_eq!(
                paths(install("official/nats", "18", Some(bad)).validate()),
                ["expectedSha256"],
                "{bad}"
            );
        }
        assert_eq!(
            paths(install("", "", Some("x")).validate()),
            ["provider", "selector", "expectedSha256"]
        );
    }

    #[test]
    fn list_params_accept_limits_from_1_to_200_and_a_uuid_cursor() {
        for limit in [None, Some(1), Some(100), Some(RUNTIME_LIST_LIMIT_MAX)] {
            let params = ListRuntimesParams {
                limit: limit.and_then(Count::new),
                after: None,
            };
            assert_eq!(params.validate(), Vec::new(), "{limit:?}");
        }
        for limit in [0, 201] {
            let params = ListRuntimesParams {
                limit: Count::new(limit),
                after: None,
            };
            assert_eq!(paths(params.validate()), ["limit"], "{limit}");
        }
        assert_eq!(ListRuntimesParams::default().limit_or_default(), 100);
        assert!(serde_json::from_str::<ListRuntimesParams>(r#"{"after":"nope"}"#).is_err());
    }

    #[test]
    fn the_outcome_is_tagged_by_its_variant() {
        let json = serde_json::json!({
            "type": "alreadyInstalled",
            "content": serde_json::Value::Null
        });
        // A null content is not a runtime summary.
        assert!(serde_json::from_value::<InstallOutcome>(json).is_err());
        assert!(
            serde_json::from_value::<InstallOutcome>(serde_json::json!({ "type": "other" }))
                .is_err()
        );
    }
}
