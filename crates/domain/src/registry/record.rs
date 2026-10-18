//! Records of facts the registry keeps next to the aggregates: provider
//! revisions with their registry columns, and artifacts.

use crate::provider::ProviderRevision;
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
/// Fields later features fill (`signature_info`, `dependency_manifest`, the
/// provider revision) are not here yet and are stored empty.
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
        }
    }
}
