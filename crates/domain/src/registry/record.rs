//! Records of facts the registry keeps next to the aggregates: provider
//! revisions with their registry columns, and artifacts.

use jiff::Timestamp;

use crate::artifact_format::ArtifactFormat;
use crate::code::code_enum;
use crate::id::ProviderSourceId;
use crate::provider::ProviderRevision;
use crate::registry::install::{CompatibilityRecordRow, DependencyManifestJson, SignatureInfoJson};
use crate::target::Target;
use crate::value::{ExactVersion, ResourceRevision, Sha256Digest, SoftwareName};

code_enum! {
    /// What kind of place a provider source is, an open code set in the
    /// registry. Feature 20 writes the two of the catalog the daemon
    /// loads; feature 37 adds `local`. A stored code this build does not
    /// know reads as `Corrupt`.
    pub enum ProviderSourceKind: ProviderSourceKind {
        /// The catalog inside the app bundle.
        Bundled => "bundled",
        /// The catalog of `STACKLY_CATALOG_DIR`, for development and tests.
        Development => "development",
    }
}

code_enum! {
    /// Where a provider source stands, an open code set in the registry.
    /// Feature 20 writes only `active`; feature 37 adds the others. A
    /// stored code this build does not know reads as `Corrupt`.
    pub enum ProviderSourceStatus: ProviderSourceStatus {
        /// The source the daemon loads providers from.
        Active => "active",
    }
}

/// A place provider packages come from: one `provider_sources` row (spec
/// 0005, *Tambahan fitur 20*, and K-2 of spec 0017).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderSourceRecord {
    /// The row's ID.
    pub id: ProviderSourceId,
    /// What kind of place it is.
    pub kind: ProviderSourceKind,
    /// Where it is: the canonical catalog folder for the catalog kinds.
    pub location: String,
    /// The trust root, once a source has one.
    pub trust_root: Option<String>,
    /// When it was last synced, empty for a local catalog.
    pub last_sync_at: Option<Timestamp>,
    /// Where it stands.
    pub status: ProviderSourceStatus,
    /// The revision, 1 for a new row.
    pub revision: ResourceRevision,
    /// When the row was written.
    pub created_at: Timestamp,
    /// When the row last changed.
    pub updated_at: Timestamp,
}

impl ProviderSourceRecord {
    /// A new source: `active`, never synced, without a trust root, at
    /// revision 1, written `at`.
    #[must_use]
    pub const fn new(
        id: ProviderSourceId,
        kind: ProviderSourceKind,
        location: String,
        at: Timestamp,
    ) -> Self {
        Self {
            id,
            kind,
            location,
            trust_root: None,
            last_sync_at: None,
            status: ProviderSourceStatus::Active,
            revision: ResourceRevision::INITIAL,
            created_at: at,
            updated_at: at,
        }
    }
}

/// A provider revision with the columns the registry keeps for it.
///
/// A field a later feature fills (`approved_at`) is not here yet and is
/// stored empty.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderRevisionRecord {
    /// The revision itself.
    pub revision: ProviderRevision,
    /// The manifest's `apiVersion` (feature 9).
    pub schema_version: Option<String>,
    /// Who published it (feature 37).
    pub publisher: Option<String>,
    /// Its approval code (feature 37).
    pub approval: Option<String>,
    /// The source it was pinned from (feature 20).
    pub source_id: Option<ProviderSourceId>,
}

impl ProviderRevisionRecord {
    /// Bundles a revision with its registry columns.
    #[must_use]
    pub const fn new(
        revision: ProviderRevision,
        schema_version: Option<String>,
        publisher: Option<String>,
        approval: Option<String>,
    ) -> Self {
        Self {
            revision,
            schema_version,
            publisher,
            approval,
            source_id: None,
        }
    }

    /// Names the source the revision was pinned from.
    #[must_use]
    pub const fn with_source_id(mut self, source_id: ProviderSourceId) -> Self {
        self.source_id = Some(source_id);
        self
    }
}

/// One concrete artifact: an exact version of a software for one target,
/// named by its digest (filled by feature 18).
///
/// The provider revision column is not here: managed runtimes are shared by
/// every revision of a provider, so it stays empty (spec 0018).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ArtifactRecord {
    /// The artifact's SHA-256.
    pub sha256: Sha256Digest,
    /// The software.
    pub software: SoftwareName,
    /// The exact version, never a selector.
    pub version: ExactVersion,
    /// The OS and architecture.
    pub target: Target,
    /// The lowest OS version it runs on, if known.
    pub minimum_os: Option<String>,
    /// Where it was downloaded from, if known.
    pub source: Option<String>,
    /// The ID of the catalog entry it came from, such as
    /// `nats-2.15.1-macos-aarch64`.
    pub catalog_id: Option<String>,
    /// The maintainer build number, for a maintainer artifact.
    pub build: Option<u32>,
    /// How the downloaded file is packed.
    pub format: Option<ArtifactFormat>,
    /// The download size in bytes.
    pub size: Option<u64>,
    /// What the install found about the signatures.
    pub signature_info: Option<SignatureInfoJson>,
    /// The dependencies of the catalog entry and the validation warnings.
    pub dependency_manifest: Option<DependencyManifestJson>,
    /// The compatibility records copied from the catalog, in order.
    pub compatibility: Vec<CompatibilityRecordRow>,
}

impl ArtifactRecord {
    /// Describes an artifact.
    #[must_use]
    pub const fn new(
        sha256: Sha256Digest,
        software: SoftwareName,
        version: ExactVersion,
        target: Target,
        minimum_os: Option<String>,
        source: Option<String>,
    ) -> Self {
        Self {
            sha256,
            software,
            version,
            target,
            minimum_os,
            source,
            catalog_id: None,
            build: None,
            format: None,
            size: None,
            signature_info: None,
            dependency_manifest: None,
            compatibility: Vec::new(),
        }
    }

    /// Adds what an install of the artifact records beyond its identity.
    #[must_use]
    pub fn with_install_details(mut self, details: InstallDetails) -> Self {
        self.catalog_id = details.catalog_id;
        self.build = details.build;
        self.format = details.format;
        self.size = details.size;
        self.signature_info = details.signature_info;
        self.dependency_manifest = details.dependency_manifest;
        self.compatibility = details.compatibility;
        self
    }
}

/// The columns of an [`ArtifactRecord`] that only an install fills (spec
/// 0018, "Data model"), for [`ArtifactRecord::with_install_details`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallDetails {
    /// The ID of the catalog entry.
    pub catalog_id: Option<String>,
    /// The maintainer build number.
    pub build: Option<u32>,
    /// How the downloaded file is packed.
    pub format: Option<ArtifactFormat>,
    /// The download size in bytes.
    pub size: Option<u64>,
    /// What the install found about the signatures.
    pub signature_info: Option<SignatureInfoJson>,
    /// The dependencies and the validation warnings.
    pub dependency_manifest: Option<DependencyManifestJson>,
    /// The compatibility records, in order.
    pub compatibility: Vec<CompatibilityRecordRow>,
}
