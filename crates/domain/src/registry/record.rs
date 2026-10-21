//! Records of facts the registry keeps next to the aggregates: provider
//! revisions with their registry columns, and artifacts.

use crate::artifact_format::ArtifactFormat;
use crate::provider::ProviderRevision;
use crate::registry::install::{CompatibilityRecordRow, DependencyManifestJson, SignatureInfoJson};
use crate::target::Target;
use crate::value::{ExactVersion, Sha256Digest, SoftwareName};

/// A provider revision with the columns the registry keeps for it.
///
/// Fields later features fill (`source_id`, `approved_at`) are not here yet
/// and are stored empty.
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
        }
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
