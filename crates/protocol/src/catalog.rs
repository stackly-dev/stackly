//! The `catalog` area (spec 0017, *API surface*): the DTOs of
//! `catalog.list_providers` and `catalog.get_provider`.
//!
//! The daemon works out every status, reason, and install verdict here;
//! clients show them and never derive one from raw catalog data. The
//! catalog never changes while the daemon runs, so the area has no event.
//! Both methods only read and need a handshake, and the catalog folder
//! path never reaches the wire.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::wire::{Count, Revision, Timestamp};

/// The longest `GetProviderParams.provider`, in bytes.
pub const PROVIDER_ID_MAX: usize = 127;

/// Params of `catalog.get_provider`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderParams {
    /// The provider ID, such as `official/nats`: 1 to 127 bytes of
    /// `[a-z0-9/-]` with at most one `/`. A name without its namespace
    /// passes, so the daemon can suggest the full ID, but never matches a
    /// provider.
    pub provider: String,
}

impl Validate for GetProviderParams {
    fn validate(&self) -> Vec<FieldError> {
        provider_id_code(&self.provider)
            .map(|code| FieldError::new("provider", code))
            .into_iter()
            .collect()
    }
}

/// Why `provider` is not a provider ID, or `None` when it is one: 1 to
/// [`PROVIDER_ID_MAX`] bytes of `[a-z0-9/-]` with at most one `/`.
pub(crate) fn provider_id_code(provider: &str) -> Option<FieldCode> {
    let provider = provider.as_bytes();
    if provider.is_empty() {
        Some(FieldCode::TooShort)
    } else if provider.len() > PROVIDER_ID_MAX {
        Some(FieldCode::TooLong)
    } else if !provider
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'/' | b'-'))
        || provider.iter().filter(|&&b| b == b'/').count() > 1
    {
        Some(FieldCode::InvalidFormat)
    } else {
        None
    }
}

/// Reply of `catalog.list_providers`: every entry in display order, in one
/// page.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListProvidersReply {
    /// The facts about the catalog.
    pub catalog: CatalogInfo,
    /// Provider and invalid entries first, planned entries last; then
    /// category, display name, and ID.
    #[serde(default)]
    pub items: Vec<ProviderSummary>,
}

/// The facts about the catalog the daemon loaded.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInfo {
    /// The catalog revision.
    pub revision: Revision,
    /// When the catalog was published, from `catalog.json`.
    pub published_at: Timestamp,
    /// When the daemon loaded it, for diagnostics.
    pub loaded_at: Timestamp,
    /// The content digest from `catalog.json`, 64 lowercase hex
    /// characters.
    pub content_sha256: String,
    /// Where the catalog came from.
    pub source: CatalogSourceKind,
    /// How far the catalog is trusted.
    pub trust: TrustLevel,
    /// The target of this Mac, which every install verdict is for.
    pub host: HostTarget,
    /// Whether the catalog was published more than 180 days before the
    /// request.
    pub outdated: bool,
    /// How many entries failed validation.
    pub invalid_count: Count,
}

/// An OS and architecture pair.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostTarget {
    /// The OS.
    pub os: TargetOs,
    /// The architecture.
    pub arch: TargetArch,
}

/// One catalog entry as a card or a list row shows it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    /// The provider ID, such as `official/nats`.
    pub provider: String,
    /// The catalog revision, the same for every entry.
    pub revision: Revision,
    /// The name shown; an invalid entry shows its ID.
    pub display_name: String,
    /// The software the provider runs, such as `nats`; the key that joins
    /// installed runtimes to the entry. Null for planned and invalid
    /// entries.
    #[serde(default)]
    pub software: Option<String>,
    /// The category; null only for an invalid entry whose manifest
    /// metadata does not decode.
    #[serde(default)]
    pub category: Option<ProviderCategory>,
    /// What the provider is; null for invalid entries.
    #[serde(default)]
    pub description: Option<String>,
    /// What this Mac can do with the entry.
    pub state: CatalogEntryState,
    /// Why the entry has its state; null for `available`.
    #[serde(default)]
    pub state_reason: Option<SupportReason>,
    /// A note for the state, such as the reason an artifact is blocked.
    #[serde(default)]
    pub state_note: Option<String>,
    /// The provider revision of the package, such as `1.0.0`; null for
    /// planned and invalid entries.
    #[serde(default)]
    pub provider_revision: Option<String>,
    /// Who publishes the package: `Stackly` for the `official` namespace.
    #[serde(default)]
    pub publisher: Option<String>,
    /// How far the entry is trusted, the same as the catalog's.
    pub trust: TrustLevel,
    /// The first 16 exact versions with an `available` artifact for this
    /// Mac, in `versions.json` order.
    #[serde(default)]
    pub versions: Vec<String>,
    /// How many exact versions have an `available` artifact for this Mac.
    pub version_count: Count,
    /// The distinct licenses of those artifacts, in `versions.json` order.
    #[serde(default)]
    pub licenses: Vec<String>,
    /// One or two characters for the tile.
    pub mark: String,
    /// The tile color.
    pub tile: TileColor,
}

/// Reply of `catalog.get_provider`: one entry with its versions, its
/// artifacts, and what is wrong with it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetail {
    /// The entry as the list shows it.
    pub summary: ProviderSummary,
    /// The software the provider runs; null for planned and invalid
    /// entries.
    #[serde(default)]
    pub software: Option<String>,
    /// The digest of the provider package, 64 lowercase hex characters;
    /// null for planned and invalid entries.
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    /// The eight capabilities in [`ProviderCapability::ALL`] order; empty
    /// for planned and invalid entries.
    #[serde(default)]
    pub capabilities: Vec<CapabilityStatus>,
    /// One row per exact version, in `versions.json` order.
    #[serde(default)]
    pub versions: Vec<CatalogVersion>,
    /// What the catalog says about an OS without artifacts.
    #[serde(default)]
    pub platform_notes: Vec<PlatformNoteDto>,
    /// The note of a planned entry.
    #[serde(default)]
    pub note: Option<String>,
    /// What failed validation; only for invalid entries.
    #[serde(default)]
    pub issues: Vec<CatalogIssue>,
}

/// Whether a provider has one capability.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityStatus {
    /// The capability.
    pub capability: ProviderCapability,
    /// Whether the provider revision declares it.
    pub supported: bool,
}

/// One exact version of a provider.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVersion {
    /// The exact version, such as `2.15.1`.
    pub version: String,
    /// The runtime line, such as `2.15`.
    pub line: String,
    /// One cell per OS, always `macos`, `linux`, then `windows`.
    #[serde(default)]
    pub support: Vec<TargetSupport>,
    /// Whether this version installs on this Mac.
    pub install: InstallVerdict,
    /// Its artifacts, in `versions.json` order.
    #[serde(default)]
    pub artifacts: Vec<CatalogArtifact>,
}

/// The install gate's verdict for one version on this Mac.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstallVerdict {
    /// Whether the version can be installed.
    pub installable: bool,
    /// Why not; null when it can.
    #[serde(default)]
    pub reason: Option<SupportReason>,
}

/// The support of one version on one OS: one cell of the version matrix.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetSupport {
    /// The OS.
    pub os: TargetOs,
    /// The architectures of the version's artifacts for the OS, by code;
    /// empty when there are none.
    #[serde(default)]
    pub arches: Vec<TargetArch>,
    /// The support status.
    pub status: SupportStatus,
    /// Why the cell has its status.
    #[serde(default)]
    pub reason: Option<SupportReason>,
    /// A note, such as a platform note or the reason an artifact is
    /// blocked.
    #[serde(default)]
    pub note: Option<String>,
}

/// One artifact of a version.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArtifact {
    /// The artifact ID from `versions.json`.
    pub id: String,
    /// The OS it is built for.
    pub os: TargetOs,
    /// The architecture it is built for.
    pub arch: TargetArch,
    /// Its status in `versions.json`.
    pub status: ArtifactStatus,
    /// The SHA-256 of the archive, 64 lowercase hex characters; only for
    /// `available` artifacts. Clients match installed runtimes and running
    /// installs by it.
    #[serde(default)]
    pub sha256: Option<String>,
    /// The oldest OS version it runs on, such as `13.0`; only for
    /// `available` artifacts.
    #[serde(default)]
    pub min_os: Option<String>,
    /// The archive size in bytes; only for `available` artifacts.
    #[serde(default)]
    pub download_size: Option<Count>,
    /// The unpacked size in bytes, once `versions.json` records it.
    #[serde(default)]
    pub installed_size: Option<Count>,
    /// The license, such as `Apache-2.0`.
    #[serde(default)]
    pub license: Option<String>,
    /// Where the license text is.
    #[serde(default)]
    pub license_url: Option<String>,
    /// The host of the first download URL and its first path segment,
    /// such as `github.com/nats-io`.
    #[serde(default)]
    pub source: Option<String>,
    /// Why the artifact is blocked; only for `blocked` artifacts.
    #[serde(default)]
    pub blocked_reason: Option<String>,
    /// Its dependencies; null while `versions.json` does not record them.
    #[serde(default)]
    pub dependencies: Option<Vec<ArtifactDependency>>,
}

/// One dependency of an artifact.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDependency {
    /// The dependency, such as `openssl`.
    pub name: String,
    /// Where it comes from.
    pub kind: DependencyKind,
    /// Its version, if known.
    #[serde(default)]
    pub version: Option<String>,
    /// Its license, if known.
    #[serde(default)]
    pub license: Option<String>,
}

/// What the catalog says about one OS a provider has no artifact for.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformNoteDto {
    /// `linux` or `windows`.
    pub os: TargetOs,
    /// `planned` or `unsupported`.
    pub status: SupportStatus,
    /// The note.
    pub note: String,
}

/// One thing wrong with an invalid entry.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogIssue {
    /// The file, such as `provider.yaml` or `catalog.json`.
    pub file: String,
    /// Where in the file, such as `metadata.id`.
    pub location: String,
    /// The issue code, such as `catalog_digest_mismatch`.
    pub code: String,
    /// What is wrong, in lowercase English.
    pub message: String,
    /// Which limit or rule, only for codes with a reason list, such as
    /// `total_bytes` for `catalog_limit`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Where a catalog came from. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSourceKind {
    /// The catalog inside the app bundle.
    Bundled,
    /// A catalog folder from `STACKLY_CATALOG_DIR`.
    Development,
    /// A source this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How far a catalog is trusted. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// The catalog shipped with Stackly.
    Official,
    /// A development catalog, never shown as official.
    Development,
    /// A trust level this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The category of a provider. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCategory {
    /// A database server.
    Database,
    /// A message broker.
    Messaging,
    /// An analytics store.
    Analytics,
    /// A cache.
    Cache,
    /// A proxy.
    Proxy,
    /// A test provider.
    Testing,
    /// A category this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What this Mac can do with a catalog entry. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogEntryState {
    /// At least one version installs on this Mac.
    Available,
    /// The artifact for this Mac is blocked.
    Blocked,
    /// No version installs on this Mac.
    Unsupported,
    /// On the roadmap, without a package.
    Planned,
    /// The package failed validation.
    Invalid,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The support status of a version on one OS (PRD section 5.1). Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportStatus {
    /// Planned for a later release.
    Planned,
    /// Builds exist but were not tested here.
    Unverified,
    /// Tested, with known gaps.
    Experimental,
    /// Tested with a compatibility record.
    Supported,
    /// Not supported.
    Unsupported,
    /// Blocked in the catalog.
    Blocked,
    /// A status this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Why an entry, a matrix cell, or an install verdict has its status.
/// Open: feature 18 adds reasons.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportReason {
    /// A build exists but was not tested on this Mac.
    Untested,
    /// No build for the OS.
    NoBuild,
    /// Builds for the OS exist, but not for its architecture.
    NoBuildForArch,
    /// No build for this Mac.
    NoBuildForHost,
    /// The catalog's platform note decides.
    PlatformNote,
    /// The artifact is blocked.
    ArtifactBlocked,
    /// The artifact is a test fixture.
    FixtureOnly,
    /// The provider is planned.
    ProviderPlanned,
    /// The provider package failed validation.
    PackageInvalid,
    /// The artifact needs a newer macOS than this Mac runs (spec 0018).
    RequiresNewerOs,
    /// A passing provider record for this provider revision, macOS major,
    /// and architecture decides (spec 0018).
    ProviderTested,
    /// A passing runtime record for this macOS major and architecture
    /// decides (spec 0018).
    RuntimeTested,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// An OS of a target. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetOs {
    /// macOS.
    Macos,
    /// Linux.
    Linux,
    /// Windows.
    Windows,
    /// An OS this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

impl TargetOs {
    /// The OS of each version matrix cell, in order.
    pub const MATRIX: [Self; 3] = [Self::Macos, Self::Linux, Self::Windows];
}

/// An architecture of a target. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetArch {
    /// 64 bit ARM, such as Apple Silicon.
    Aarch64,
    /// 64 bit x86.
    #[serde(rename = "x86_64")]
    X86_64,
    /// An architecture this build does not know. The daemon never sends
    /// it.
    #[serde(other)]
    Unrecognized,
}

/// The status of an artifact in `versions.json`. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    /// A pinned download.
    Available,
    /// Blocked, with a reason.
    Blocked,
    /// A test fixture, never installable.
    Fixture,
    /// A status this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// A provider capability. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapability {
    /// Run more than one instance side by side.
    MultiInstance,
    /// Keep data across restarts.
    Persistence,
    /// Reload the configuration without a restart.
    Reload,
    /// Expose metrics.
    Metrics,
    /// Serve TLS.
    Tls,
    /// Back up while the instance is stopped.
    ColdBackup,
    /// Back up while the instance runs.
    OnlineBackup,
    /// Back up as a logical dump.
    LogicalBackup,
    /// A capability this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

impl ProviderCapability {
    /// Every capability, in the order a provider detail lists them
    /// (spec 0017, *Urutan capability*).
    pub const ALL: [Self; 8] = [
        Self::MultiInstance,
        Self::Persistence,
        Self::Reload,
        Self::Metrics,
        Self::Tls,
        Self::ColdBackup,
        Self::OnlineBackup,
        Self::LogicalBackup,
    ];
}

/// Where an artifact dependency comes from. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// Inside the artifact.
    Bundled,
    /// Another artifact Stackly installs.
    Own,
    /// Part of macOS.
    System,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The system color of a provider tile. Open: clients show an
/// unrecognized color as gray.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TileColor {
    /// System blue.
    Blue,
    /// System cyan.
    Cyan,
    /// System teal.
    Teal,
    /// System green.
    Green,
    /// System yellow.
    Yellow,
    /// System orange.
    Orange,
    /// System red.
    Red,
    /// System brown.
    Brown,
    /// System indigo.
    Indigo,
    /// System gray.
    Gray,
    /// A color this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(provider: &str) -> Vec<(String, String)> {
        GetProviderParams {
            provider: provider.to_owned(),
        }
        .validate()
        .iter()
        .map(|field| (field.path().to_owned(), field.code().to_owned()))
        .collect()
    }

    #[test]
    fn a_provider_id_or_a_bare_name_passes_validation() {
        for provider in ["official/nats", "nats", "testing/fake", "a-1/b-2"] {
            assert_eq!(fields(provider), Vec::new(), "{provider}");
        }
        assert_eq!(fields(&"a".repeat(PROVIDER_ID_MAX)), Vec::new());
    }

    #[test]
    fn a_bad_provider_id_names_the_field_and_its_code() {
        let one = |code: &str| vec![("provider".to_owned(), code.to_owned())];
        assert_eq!(fields(""), one("too_short"));
        assert_eq!(fields(&"a".repeat(PROVIDER_ID_MAX + 1)), one("too_long"));
        for provider in [
            "Official/NATS",
            "official/nats/2",
            "official nats",
            "ofi_cial/x",
        ] {
            assert_eq!(fields(provider), one("invalid_format"), "{provider}");
        }
    }

    #[test]
    fn open_enums_read_a_foreign_value_as_unrecognized() {
        fn foreign<T: serde::de::DeserializeOwned>() -> T {
            serde_json::from_str("\"from_the_future\"").unwrap()
        }
        assert_eq!(
            foreign::<CatalogSourceKind>(),
            CatalogSourceKind::Unrecognized
        );
        assert_eq!(foreign::<TrustLevel>(), TrustLevel::Unrecognized);
        assert_eq!(
            foreign::<ProviderCategory>(),
            ProviderCategory::Unrecognized
        );
        assert_eq!(
            foreign::<CatalogEntryState>(),
            CatalogEntryState::Unrecognized
        );
        assert_eq!(foreign::<SupportStatus>(), SupportStatus::Unrecognized);
        assert_eq!(foreign::<SupportReason>(), SupportReason::Unrecognized);
        assert_eq!(foreign::<TargetOs>(), TargetOs::Unrecognized);
        assert_eq!(foreign::<TargetArch>(), TargetArch::Unrecognized);
        assert_eq!(foreign::<ArtifactStatus>(), ArtifactStatus::Unrecognized);
        assert_eq!(
            foreign::<ProviderCapability>(),
            ProviderCapability::Unrecognized
        );
        assert_eq!(foreign::<DependencyKind>(), DependencyKind::Unrecognized);
        assert_eq!(foreign::<TileColor>(), TileColor::Unrecognized);
    }

    #[test]
    fn wire_values_are_the_spec_codes() {
        let text = |value: serde_json::Value| value.as_str().unwrap().to_owned();
        assert_eq!(
            text(serde_json::to_value(TargetArch::X86_64).unwrap()),
            "x86_64"
        );
        assert_eq!(
            text(serde_json::to_value(TargetArch::Aarch64).unwrap()),
            "aarch64"
        );
        assert_eq!(
            text(serde_json::to_value(SupportReason::NoBuildForArch).unwrap()),
            "no_build_for_arch"
        );
        let capabilities: Vec<String> = ProviderCapability::ALL
            .iter()
            .map(|capability| text(serde_json::to_value(capability).unwrap()))
            .collect();
        assert_eq!(
            capabilities,
            [
                "multi_instance",
                "persistence",
                "reload",
                "metrics",
                "tls",
                "cold_backup",
                "online_backup",
                "logical_backup"
            ]
        );
    }
}
