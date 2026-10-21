//! The `install_runtime` operation (spec 0018, *Alur install*): one
//! resumable work with the five stages of the mockup (download, verify
//! digest, extract, validate binary, commit), its input as the journal keeps
//! it, and the factory that builds it again after a restart.
//!
//! The stage bodies are plain functions of an [`Env`] and a [`Plan`], so the
//! operation and the test helper of [`fixture`](super::fixture) run exactly
//! the same code.

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stackly_domain::{
    ArtifactFormat, ArtifactRecord, CompatibilityRecordRow, CompatibilityResult,
    CompatibilityScope, DependencyManifestJson, DetailKey, ErrorCodeName, ExactVersion,
    InstallDetails, InstallMode, JournalPayload, JournalView, NewRuntime, Operation,
    OperationResource, OsVersion, Progress, ProviderRevisionKey, Registry, ResourceRef, Runtime,
    RuntimeId, RuntimeLine, Sha256Digest, SignatureInfoJson, SoftwareName, StageName, Target,
    WriteOp,
};
use stackly_platform::{
    EntryPath, FileMode, HostFacts, HttpsUrl, Platform, PlatformError, ReadSeekFile, StopFlag,
    TreeFile, TreeWriter,
};
use tokio::sync::mpsc;

use super::archive::limits::{ExtractLimits, required_space};
use super::archive::{ArchiveError, EntryKind, EntryRecord, ExtractReport, ExtractSpec, extract};
use super::codes;
use super::commit::{CommitError, CommitRequest, commit_tree};
use super::download::{
    CachePaths, DownloadContext, DownloadError, DownloadOutcome, DownloadPolicy, DownloadSource,
    download,
};
use super::macho;
use super::validate::{
    Marker, ValidateError, ValidateInput, ValidateReport, VersionCheckSpec, finish_tree, validate,
};
use crate::layout::PreparedLayout;
use crate::operation::{
    BoxFuture, FinalBatch, Finalize, OperationContext, OperationError, OperationOutcome,
    OperationWork, RebuildError, StepSpec, StepStart, WorkError, WorkFactory, WorkOutcome,
    WorkResult,
};
use crate::ownership::remove_owned_tree;
use crate::provider::{
    ArtifactDependency, ArtifactStatus, DependencyKind, SigningPolicy, UnpackedSize,
    VersionArtifact,
};

/// The stage of the download.
pub const DOWNLOAD: StageName = StageName::from_static("download");
/// The stage that checks the digest of the cache entry.
pub const VERIFY_DIGEST: StageName = StageName::from_static("verify_digest");
/// The stage of the extraction.
pub const EXTRACT: StageName = StageName::from_static("extract");
/// The stage of the binary validation.
pub const VALIDATE_BINARY: StageName = StageName::from_static("validate_binary");
/// The stage of the commit, the point of no return.
pub const COMMIT: StageName = StageName::from_static("commit");

/// The most compatibility records an input carries.
pub const COMPATIBILITY_MAX: usize = 8;

/// The most warnings a checkpoint keeps.
const WARNINGS_MAX: usize = 8;

/// The folder inside the scratch folder that holds the staged tree.
const STAGE_DIR: &str = "stage";

/// The staged tree inside [`STAGE_DIR`].
const TREE_DIR: &str = "tree";

/// The file that keeps the entries of the tree for a resumed validation.
const ENTRIES_FILE: &str = "entries.json";

/// Longest `entries.json` read back, in bytes.
const ENTRIES_MAX: u64 = 64 << 20;

/// The bytes read per call while hashing.
const HASH_CHUNK: usize = 64 * 1024;

// ---------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------

/// What the operation needs, frozen when it begins (spec 0018, *Alur
/// install*): the artifact as the catalog stated it, the compatibility
/// records for this host, and the version check. The work never reads the
/// catalog again, so a catalog that changes under a running or resumed
/// install changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallInput {
    /// The software the runtime runs.
    pub software: String,
    /// The provider the request named.
    pub provider: String,
    /// The selector as typed, kept only as the origin of the request.
    pub selector: String,
    /// The host the selector was resolved for.
    pub host: HostInput,
    /// The artifact the selector resolved to.
    pub artifact: ArtifactInput,
    /// The compatibility records of that artifact for this host.
    #[serde(default)]
    pub compatibility: Vec<CompatibilityInput>,
    /// The provider revision whose package holds the artifact.
    pub package_revision: RevisionInput,
    /// The version check of the package, if it declares one.
    #[serde(default)]
    pub version_check: Option<VersionCheckInput>,
}

/// The host an input was resolved for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostInput {
    /// The target, such as `macos-aarch64`.
    pub target: String,
    /// The macOS version, such as `27.0`.
    pub os_version: String,
}

/// A provider revision as the input names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionInput {
    /// The provider ID.
    pub id: String,
    /// The revision label.
    pub revision: String,
    /// The SHA-256 of the manifest.
    pub manifest_sha256: String,
}

/// The `versions.json` entry of the artifact, without its compatibility
/// records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactInput {
    /// The ID of the entry.
    pub id: String,
    /// The runtime line.
    pub line: String,
    /// The exact version.
    pub version: String,
    /// The target, such as `macos-aarch64`.
    pub target: String,
    /// The lowest macOS it needs.
    pub min_os: String,
    /// `tar_gz`, `zip`, or `binary`.
    pub format: String,
    /// The candidate `https` URLs.
    pub urls: Vec<String>,
    /// The SHA-256 of the download.
    pub sha256: String,
    /// The size of the download.
    pub size: u64,
    /// The one folder at the root of the archive, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_root: Option<String>,
    /// `keep_or_adhoc` or `adhoc_pipeline`.
    pub signing_policy: String,
    /// Executable paths inside the runtime.
    pub executables: Vec<String>,
    /// Whether `@stable` selects it.
    #[serde(default)]
    pub stable: bool,
    /// The maintainer build number, for a maintainer artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<u32>,
    /// The bytes `runtime pin` measured inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpacked_size: Option<u64>,
    /// The entries `runtime pin` measured inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_count: Option<u64>,
    /// The libraries it links, when the catalog says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<DependencyInput>>,
}

/// One library of an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyInput {
    /// The file name.
    pub name: String,
    /// `bundled`, `own`, or `system`.
    pub kind: String,
    /// The install name.
    pub install_name: String,
    /// The version, for a bundled library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The SPDX license, for a bundled library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// The source URL, for a bundled library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

/// A compatibility record as the input keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilityInput {
    /// `runtime` or `provider`.
    pub scope: String,
    /// `pass` or `fail`.
    pub result: String,
    /// The provider revision, for the `provider` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_revision: Option<RevisionInput>,
    /// The OS.
    pub os: String,
    /// The OS version.
    pub os_version: String,
    /// The OS build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_build: Option<String>,
    /// The architecture.
    pub arch: String,
    /// The checks the run covered.
    pub checks: Vec<String>,
    /// What the run did not cover.
    pub known_limitations: Vec<String>,
    /// A short summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// When the run finished.
    pub tested_at: String,
    /// The evidence folder.
    pub evidence: String,
}

/// The version check as the input keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VersionCheckInput {
    /// The literal arguments.
    pub args: Vec<String>,
    /// The pattern, with one capture group.
    pub pattern: String,
    /// The time limit in seconds.
    pub timeout_seconds: u64,
}

/// Why an input could not be built or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InputError {
    /// The artifact is not an `available` one.
    #[error("only an available artifact can be installed")]
    NotAvailable,
    /// A field of the input does not parse.
    #[error("the input field {field} is invalid")]
    Invalid {
        /// Which field.
        field: &'static str,
    },
    /// The input does not fit the 16 KiB of a journal input.
    #[error("the input is {len} bytes, more than the journal keeps")]
    TooLarge {
        /// Its size.
        len: usize,
    },
}

impl InstallInput {
    /// Freezes what an install needs from the catalog.
    ///
    /// Of `compatibility` it keeps the records whose OS and architecture are
    /// the host's, at most [`COMPATIBILITY_MAX`].
    ///
    /// # Errors
    ///
    /// Returns [`InputError::NotAvailable`] when `artifact` is not an
    /// `available` one.
    #[expect(
        clippy::too_many_arguments,
        reason = "one argument per fact the catalog hands to the operation"
    )]
    pub fn new(
        software: &SoftwareName,
        provider: &str,
        selector: &str,
        host: &HostFacts,
        artifact: &VersionArtifact,
        compatibility: &[CompatibilityRecordRow],
        revision: &ProviderRevisionKey,
        version_check: Option<&VersionCheckSpec>,
    ) -> Result<Self, InputError> {
        let ArtifactStatus::Available {
            min_os,
            format,
            urls,
            sha256,
            size,
            archive_root,
            signing_policy,
            executables,
            stable,
            build,
            unpacked,
            dependencies,
            ..
        } = &artifact.status
        else {
            return Err(InputError::NotAvailable);
        };
        Ok(Self {
            software: software.to_string(),
            provider: provider.to_owned(),
            selector: selector.to_owned(),
            host: HostInput {
                target: host.target.to_string(),
                os_version: host.os_version.to_string(),
            },
            artifact: ArtifactInput {
                id: artifact.id.clone(),
                line: artifact.line.to_string(),
                version: artifact.version.to_string(),
                target: artifact.target.to_string(),
                min_os: min_os.to_string(),
                format: format.as_code().to_owned(),
                urls: urls.clone(),
                sha256: sha256.to_string(),
                size: *size,
                archive_root: archive_root.clone(),
                signing_policy: signing_policy.as_code().to_owned(),
                executables: executables.clone(),
                stable: *stable,
                build: *build,
                unpacked_size: unpacked.map(|unpacked| unpacked.bytes),
                entry_count: unpacked.map(|unpacked| unpacked.entries),
                dependencies: dependencies.as_ref().map(|list| {
                    list.iter()
                        .map(|dependency| DependencyInput {
                            name: dependency.name.clone(),
                            kind: dependency.kind.as_code().to_owned(),
                            install_name: dependency.install_name.clone(),
                            version: dependency.version.clone(),
                            license: dependency.license.clone(),
                            source_url: dependency.source_url.clone(),
                        })
                        .collect()
                }),
            },
            compatibility: compatibility
                .iter()
                .filter(|record| record.os == host.target.os() && record.arch == host.target.arch())
                .take(COMPATIBILITY_MAX)
                .map(compatibility_input)
                .collect(),
            package_revision: revision_input(revision),
            version_check: version_check.map(|check| VersionCheckInput {
                args: check.args().to_vec(),
                pattern: check.pattern().to_owned(),
                timeout_seconds: check.timeout().as_secs(),
            }),
        })
    }

    /// Encodes the input as the journal keeps it.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::TooLarge`] when the text is over 16 KiB.
    pub fn to_payload(&self) -> Result<JournalPayload, InputError> {
        let text =
            serde_json::to_string(self).map_err(|_| InputError::Invalid { field: "input" })?;
        let len = text.len();
        JournalPayload::new(text, crate::operation::limits::INPUT_MAX)
            .map_err(|_| InputError::TooLarge { len })
    }

    /// Decodes an input the journal kept.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Invalid`] when the text is not an input.
    pub fn from_payload(payload: &JournalPayload) -> Result<Self, InputError> {
        serde_json::from_str(payload.as_str()).map_err(|_| InputError::Invalid { field: "input" })
    }
}

fn revision_input(key: &ProviderRevisionKey) -> RevisionInput {
    RevisionInput {
        id: key.provider().to_string(),
        revision: key.revision().to_string(),
        manifest_sha256: key.manifest_sha256().to_string(),
    }
}

fn compatibility_input(record: &CompatibilityRecordRow) -> CompatibilityInput {
    CompatibilityInput {
        scope: record.scope.as_code().to_owned(),
        result: record.result.as_code().to_owned(),
        provider_revision: record.provider_revision.as_ref().map(revision_input),
        os: record.os.as_code().to_owned(),
        os_version: record.os_version.to_string(),
        os_build: record.os_build.clone(),
        arch: record.arch.as_code().to_owned(),
        checks: record.checks.clone(),
        known_limitations: record.known_limitations.clone(),
        summary: record.summary.clone(),
        tested_at: record.tested_at.to_string(),
        evidence: record.evidence.clone(),
    }
}

// ---------------------------------------------------------------------
// Plan: the input, parsed once
// ---------------------------------------------------------------------

/// The input with every text parsed into its type.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The software.
    pub software: SoftwareName,
    /// The exact version.
    pub version: ExactVersion,
    /// The line of that version.
    pub line: RuntimeLine,
    /// The target of the artifact.
    pub target: Target,
    /// The ID of the catalog entry.
    pub catalog_id: String,
    /// The SHA-256 of the download.
    pub sha256: Sha256Digest,
    /// The size of the download.
    pub size: u64,
    /// How the download is packed.
    pub format: ArtifactFormat,
    /// The candidate URLs.
    pub urls: Vec<HttpsUrl>,
    /// The folder at the root of the archive.
    pub archive_root: Option<String>,
    /// The executables of the artifact.
    pub executables: Vec<EntryPath>,
    /// What happens to the signatures.
    pub signing: SigningPolicy,
    /// The lowest macOS the artifact needs.
    pub min_os: OsVersion,
    /// The maintainer build number.
    pub build: Option<u32>,
    /// What `runtime pin` measured.
    pub unpacked: Option<UnpackedSize>,
    /// The libraries of the catalog entry.
    pub dependencies: Option<Vec<ArtifactDependency>>,
    /// The compatibility records copied to the registry.
    pub compatibility: Vec<CompatibilityRecordRow>,
    /// The version check, if the package declares one.
    pub version_check: Option<VersionCheckSpec>,
}

impl Plan {
    /// Parses an input.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Invalid`] naming the first field that does not
    /// parse.
    pub fn from_input(input: &InstallInput) -> Result<Self, InputError> {
        fn field<T: std::str::FromStr>(text: &str, name: &'static str) -> Result<T, InputError> {
            text.parse()
                .map_err(|_| InputError::Invalid { field: name })
        }
        let artifact = &input.artifact;
        let urls = artifact
            .urls
            .iter()
            .map(|url| HttpsUrl::new(url).map_err(|_| InputError::Invalid { field: "urls" }))
            .collect::<Result<Vec<_>, _>>()?;
        let executables = artifact
            .executables
            .iter()
            .map(|path| {
                EntryPath::new(path).map_err(|_| InputError::Invalid {
                    field: "executables",
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let signing = match artifact.signing_policy.as_str() {
            "keep_or_adhoc" => SigningPolicy::KeepOrAdhoc,
            "adhoc_pipeline" => SigningPolicy::AdhocPipeline,
            _ => {
                return Err(InputError::Invalid {
                    field: "signingPolicy",
                });
            }
        };
        let unpacked = match (artifact.unpacked_size, artifact.entry_count) {
            (Some(bytes), Some(entries)) => Some(UnpackedSize { bytes, entries }),
            _ => None,
        };
        let dependencies = artifact
            .dependencies
            .as_ref()
            .map(|list| {
                list.iter()
                    .map(dependency_of)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let version_check = input
            .version_check
            .as_ref()
            .map(|check| {
                VersionCheckSpec::new(
                    check.args.clone(),
                    &check.pattern,
                    Duration::from_secs(check.timeout_seconds),
                )
                .map_err(|_| InputError::Invalid {
                    field: "versionCheck",
                })
            })
            .transpose()?;
        Ok(Self {
            software: field(&input.software, "software")?,
            version: field(&artifact.version, "version")?,
            line: field(&artifact.line, "line")?,
            target: field(&artifact.target, "target")?,
            catalog_id: artifact.id.clone(),
            sha256: field(&artifact.sha256, "sha256")?,
            size: artifact.size,
            format: field(&artifact.format, "format")?,
            urls,
            archive_root: artifact.archive_root.clone(),
            executables,
            signing,
            min_os: field(&artifact.min_os, "minOs")?,
            build: artifact.build,
            unpacked,
            dependencies,
            compatibility: input
                .compatibility
                .iter()
                .map(compatibility_of)
                .collect::<Result<Vec<_>, _>>()?,
            version_check,
        })
    }

    fn expected_libraries(&self) -> Option<Vec<String>> {
        self.dependencies.as_ref().map(|list| {
            list.iter()
                .filter(|dependency| dependency.kind != DependencyKind::System)
                .map(|dependency| dependency.name.clone())
                .collect()
        })
    }
}

fn dependency_of(input: &DependencyInput) -> Result<ArtifactDependency, InputError> {
    let kind = match input.kind.as_str() {
        "bundled" => DependencyKind::Bundled,
        "own" => DependencyKind::Own,
        "system" => DependencyKind::System,
        _ => {
            return Err(InputError::Invalid {
                field: "dependencies",
            });
        }
    };
    Ok(ArtifactDependency {
        name: input.name.clone(),
        kind,
        install_name: input.install_name.clone(),
        version: input.version.clone(),
        license: input.license.clone(),
        source_url: input.source_url.clone(),
    })
}

fn compatibility_of(input: &CompatibilityInput) -> Result<CompatibilityRecordRow, InputError> {
    let invalid = || InputError::Invalid {
        field: "compatibility",
    };
    let revision = |input: &RevisionInput| -> Result<ProviderRevisionKey, InputError> {
        Ok(ProviderRevisionKey::new(
            input.id.parse().map_err(|_| invalid())?,
            input.revision.parse().map_err(|_| invalid())?,
            input.manifest_sha256.parse().map_err(|_| invalid())?,
        ))
    };
    CompatibilityRecordRow::new(
        input
            .scope
            .parse::<CompatibilityScope>()
            .map_err(|_| invalid())?,
        input.provider_revision.as_ref().map(revision).transpose()?,
        input.os.parse().map_err(|_| invalid())?,
        input.os_version.parse().map_err(|_| invalid())?,
        input.os_build.clone(),
        input.arch.parse().map_err(|_| invalid())?,
        input
            .result
            .parse::<CompatibilityResult>()
            .map_err(|_| invalid())?,
        input.checks.clone(),
        input.known_limitations.clone(),
        input.summary.clone(),
        input.tested_at.parse().map_err(|_| invalid())?,
        input.evidence.clone(),
    )
    .map_err(|_| invalid())
}

// ---------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------

/// Why an install failed, with the catalog code and the details of that
/// code (spec 0018, *Kode error*).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InstallError {
    /// The download failed.
    #[error("{source}")]
    Download {
        /// The catalog ID of the artifact.
        artifact: String,
        /// What went wrong.
        source: DownloadError,
    },
    /// The SHA-256 of the cache entry or the stream is not the catalog's.
    #[error("the digest of the artifact differs from the catalog")]
    DigestMismatch {
        /// The catalog ID of the artifact.
        artifact: String,
        /// What the catalog says.
        expected: String,
        /// What the bytes hash to.
        actual: String,
    },
    /// The size of the cache entry is not the catalog's.
    #[error("the size of the artifact differs from the catalog")]
    SizeMismatch {
        /// The catalog ID of the artifact.
        artifact: String,
        /// What the catalog says.
        expected: u64,
        /// What the file holds.
        actual: u64,
    },
    /// The magic number of the file does not match the format.
    #[error("the artifact is not a {format} file")]
    FormatMismatch {
        /// The catalog ID of the artifact.
        artifact: String,
        /// The format the catalog says.
        format: &'static str,
        /// What the first bytes say: `gzip`, `zip`, `mach-o`, or `unknown`.
        kind: &'static str,
    },
    /// A volume has no room.
    #[error("no room left on the {root} volume")]
    InsufficientSpace {
        /// The root, `cache` or `data`.
        root: &'static str,
        /// What the step needs.
        needed: u64,
        /// What the volume has.
        available: u64,
    },
    /// A platform call failed.
    #[error("{op} failed")]
    Io {
        /// The step.
        op: &'static str,
        /// The root, `cache` or `data`.
        root: &'static str,
    },
    /// The extractor refused the archive.
    #[error("the archive was rejected: {reason}")]
    ArchiveRejected {
        /// The catalog ID of the artifact.
        artifact: String,
        /// The rule that was broken.
        reason: &'static str,
        /// The member that broke it.
        entry: String,
    },
    /// The staged runtime failed validation.
    #[error("{0}")]
    Validate(ValidateError),
    /// The target folder belongs to something else.
    #[error("the runtime folder belongs to something else")]
    InstallConflict {
        /// The folder.
        path: String,
    },
    /// Something that should not happen.
    #[error("internal error: {0}")]
    Internal(String),
}

impl InstallError {
    /// Returns the catalog code.
    #[must_use]
    pub fn catalog_code(&self) -> ErrorCodeName {
        match self {
            Self::Download { source, .. } => match source {
                DownloadError::Offline { .. } => codes::DOWNLOAD_OFFLINE,
                DownloadError::Failed { .. } => codes::DOWNLOAD_FAILED,
                DownloadError::Untrusted { .. } => codes::DOWNLOAD_UNTRUSTED,
                DownloadError::TooLarge { .. } => codes::DOWNLOAD_TOO_LARGE,
                DownloadError::DigestMismatch { .. } => codes::DIGEST_MISMATCH,
                DownloadError::SizeMismatch { .. } => codes::SIZE_MISMATCH,
                DownloadError::InsufficientSpace { .. } => codes::INSUFFICIENT_SPACE,
                _ => codes::IO_FAILED,
            },
            Self::DigestMismatch { .. } => codes::DIGEST_MISMATCH,
            Self::SizeMismatch { .. } => codes::SIZE_MISMATCH,
            Self::FormatMismatch { .. } => codes::FORMAT_MISMATCH,
            Self::InsufficientSpace { .. } => codes::INSUFFICIENT_SPACE,
            Self::Io { .. } => codes::IO_FAILED,
            Self::ArchiveRejected { .. } => codes::ARCHIVE_REJECTED,
            Self::Validate(error) => error.code(),
            Self::InstallConflict { .. } => codes::INSTALL_CONFLICT,
            Self::Internal(_) => crate::operation::codes::INTERNAL_UNEXPECTED,
        }
    }
}

fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("every detail key of spec 0018 is camelCase")
}

impl OperationError for InstallError {
    fn code(&self) -> ErrorCodeName {
        self.catalog_code()
    }

    fn retryable(&self) -> bool {
        match self {
            Self::Download { source, .. } => matches!(
                source,
                DownloadError::Offline { .. }
                    | DownloadError::Failed { .. }
                    | DownloadError::DigestMismatch { .. }
                    | DownloadError::SizeMismatch { .. }
                    | DownloadError::InsufficientSpace { .. }
                    | DownloadError::Io { .. }
            ),
            Self::DigestMismatch { .. }
            | Self::SizeMismatch { .. }
            | Self::InsufficientSpace { .. }
            | Self::Io { .. } => true,
            Self::Validate(error) => matches!(
                error,
                ValidateError::BinaryCheckTimeout { .. }
                    | ValidateError::InsufficientSpace
                    | ValidateError::Io { .. }
            ),
            _ => false,
        }
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        match self {
            Self::Download { artifact, source } => download_details(artifact, source),
            Self::DigestMismatch {
                artifact,
                expected,
                actual,
            } => vec![
                (key("artifact"), artifact.clone()),
                (key("expected"), expected.clone()),
                (key("actual"), actual.clone()),
            ],
            Self::SizeMismatch {
                artifact,
                expected,
                actual,
            } => vec![
                (key("artifact"), artifact.clone()),
                (key("expected"), expected.to_string()),
                (key("actual"), actual.to_string()),
            ],
            Self::FormatMismatch {
                artifact,
                format,
                kind,
            } => vec![
                (key("artifact"), artifact.clone()),
                (key("format"), (*format).to_owned()),
                (key("kind"), (*kind).to_owned()),
            ],
            Self::InsufficientSpace {
                root,
                needed,
                available,
            } => vec![
                (key("root"), (*root).to_owned()),
                (key("needed"), needed.to_string()),
                (key("available"), available.to_string()),
            ],
            Self::Io { op, root } => vec![
                (key("op"), (*op).to_owned()),
                (key("root"), (*root).to_owned()),
            ],
            Self::ArchiveRejected {
                artifact,
                reason,
                entry,
            } => vec![
                (key("artifact"), artifact.clone()),
                (key("reason"), (*reason).to_owned()),
                (key("entry"), entry.clone()),
            ],
            Self::Validate(error) => validate_details(error),
            Self::InstallConflict { path } => vec![(key("path"), path.clone())],
            Self::Internal(_) => Vec::new(),
        }
    }
}

fn download_details(artifact: &str, error: &DownloadError) -> Vec<(DetailKey, String)> {
    let artifact = (key("artifact"), artifact.to_owned());
    match error {
        DownloadError::Offline { hosts } => {
            vec![artifact, (key("hosts"), hosts.join(","))]
        }
        DownloadError::Failed {
            host,
            status,
            reason,
        } => {
            let mut details = vec![artifact, (key("host"), host.clone())];
            details.extend(status.map(|status| (key("status"), status.to_string())));
            details.push((key("reason"), reason.clone()));
            details
        }
        DownloadError::Untrusted { host, reason } => vec![
            artifact,
            (key("host"), host.clone()),
            (key("reason"), reason.clone()),
        ],
        DownloadError::TooLarge { expected, received } => vec![
            artifact,
            (key("expected"), expected.to_string()),
            (key("received"), received.to_string()),
        ],
        DownloadError::DigestMismatch { expected, actual } => vec![
            artifact,
            (key("expected"), expected.to_string()),
            (key("actual"), actual.to_string()),
        ],
        DownloadError::SizeMismatch { expected, actual } => vec![
            artifact,
            (key("expected"), expected.to_string()),
            (key("actual"), actual.to_string()),
        ],
        DownloadError::InsufficientSpace { needed, available } => vec![
            (key("root"), "cache".to_owned()),
            (key("needed"), needed.to_string()),
            (key("available"), available.to_string()),
        ],
        DownloadError::Io { op, .. } => vec![
            (key("op"), (*op).to_owned()),
            (key("root"), "cache".to_owned()),
        ],
        DownloadError::Stopped => Vec::new(),
    }
}

fn validate_details(error: &ValidateError) -> Vec<(DetailKey, String)> {
    match error {
        ValidateError::ExecutableMissing { path, reason } => {
            vec![
                (key("path"), path.clone()),
                (key("reason"), (*reason).to_owned()),
            ]
        }
        ValidateError::ArchitectureMismatch { path, arches } => vec![
            (key("path"), path.clone()),
            (key("arches"), arches.join(",")),
        ],
        ValidateError::OsTooOld {
            path,
            required,
            host,
        } => vec![
            (key("path"), path.clone()),
            (key("required"), required.to_string()),
            (key("host"), host.to_string()),
        ],
        ValidateError::DependencyOutsideRuntime { path, library } => vec![
            (key("path"), path.clone()),
            (key("library"), library.clone()),
        ],
        ValidateError::DependencyMismatch {
            missing,
            unexpected,
        } => vec![
            (key("missing"), missing.join(",")),
            (key("unexpected"), unexpected.join(",")),
        ],
        ValidateError::SignatureInvalid {
            path,
            policy,
            detail,
        } => vec![
            (key("path"), path.clone()),
            (key("policy"), (*policy).to_owned()),
            (key("detail"), detail.clone()),
        ],
        ValidateError::Quarantined { path } => vec![(key("path"), path.clone())],
        ValidateError::BinaryRejectedByOs { path, signal } => vec![
            (key("path"), path.clone()),
            (key("signal"), signal.to_string()),
        ],
        ValidateError::BinaryCheckTimeout { path, seconds } => vec![
            (key("path"), path.clone()),
            (key("seconds"), seconds.to_string()),
        ],
        ValidateError::BinaryCheckFailed {
            path,
            exit_code,
            signal,
            errno,
            ..
        } => {
            let mut details = vec![(key("path"), path.clone())];
            details.extend(exit_code.map(|code| (key("exitCode"), code.to_string())));
            details.extend(signal.map(|signal| (key("signal"), signal.to_string())));
            details.extend(errno.map(|errno| (key("errno"), errno.to_string())));
            details
        }
        ValidateError::VersionMismatch { expected, observed } => {
            let mut details = vec![(key("expected"), expected.clone())];
            details.extend(observed.clone().map(|observed| (key("observed"), observed)));
            details
        }
        ValidateError::InsufficientSpace => vec![
            (key("root"), "data".to_owned()),
            (key("needed"), "0".to_owned()),
            (key("available"), "0".to_owned()),
        ],
        ValidateError::Io { op, .. } => vec![
            (key("op"), (*op).to_owned()),
            (key("root"), "data".to_owned()),
        ],
        ValidateError::Stopped => Vec::new(),
    }
}

impl From<ValidateError> for InstallError {
    fn from(error: ValidateError) -> Self {
        match error {
            ValidateError::InsufficientSpace => Self::InsufficientSpace {
                root: "data",
                needed: 0,
                available: 0,
            },
            other => Self::Validate(other),
        }
    }
}

fn platform_error(op: &'static str, root: &'static str, error: &PlatformError) -> InstallError {
    match error {
        PlatformError::NoSpace { .. } => InstallError::InsufficientSpace {
            root,
            needed: 0,
            available: 0,
        },
        _ => {
            tracing::warn!(op, %error, "a platform call of a runtime install failed");
            InstallError::Io { op, root }
        }
    }
}

// ---------------------------------------------------------------------
// Environment and stage bodies
// ---------------------------------------------------------------------

/// What every stage body needs besides the plan.
#[derive(Clone)]
pub struct Env {
    /// The platform ports.
    pub platform: Arc<Platform>,
    /// The checked layout.
    pub layout: Arc<PreparedLayout>,
    /// The retry settings of the download.
    pub policy: DownloadPolicy,
}

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env").finish_non_exhaustive()
    }
}

/// What the download stage hands on: where the entry is and where it came
/// from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCheckpoint {
    /// The name of the cache entry, the digest.
    pub cache_file: String,
    /// The URL the bytes came from, when this run fetched them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// What the extract stage hands on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractCheckpoint {
    /// Regular files written.
    pub files: u64,
    /// Bytes of regular files.
    pub bytes: u64,
    /// Symlinks created.
    pub symlinks: u64,
    /// AppleDouble members skipped.
    pub skipped_metadata: u64,
    /// Files and folders whose special bits were dropped.
    pub stripped_special_bits: u64,
}

/// What the validate stage hands on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateCheckpoint {
    /// Whether the install signed every Mach-O file ad hoc.
    pub adhoc_signed: bool,
    /// How many Mach-O files the tree holds.
    pub mach_o_files: u64,
    /// The signature of each executable.
    pub executables: Vec<SignedExecutable>,
    /// Findings that did not fail the install.
    pub warnings: Vec<String>,
    /// When validation finished.
    pub validated_at: String,
}

/// One executable of a [`ValidateCheckpoint`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedExecutable {
    /// Where it lies in the tree.
    pub path: String,
    /// `developer_id`, `adhoc`, `linker_signed`, `other`, or `unsigned`.
    pub kind: String,
    /// The team ID of a Developer ID signature.
    pub team_id: Option<String>,
}

/// The paths of one install's scratch folder.
#[derive(Debug, Clone)]
pub struct ScratchPaths {
    /// `operations/<id>`.
    pub scratch: PathBuf,
    /// `scratch/stage`.
    pub stage: PathBuf,
    /// `scratch/stage/tree`.
    pub tree: PathBuf,
    /// `scratch/stage/entries.json`.
    pub entries: PathBuf,
    /// `scratch/check`.
    pub check: PathBuf,
}

impl ScratchPaths {
    /// The paths below `scratch`.
    #[must_use]
    pub fn new(scratch: &Path) -> Self {
        let stage = scratch.join(STAGE_DIR);
        Self {
            scratch: scratch.to_path_buf(),
            tree: stage.join(TREE_DIR),
            entries: stage.join(ENTRIES_FILE),
            stage,
            check: scratch.join("check"),
        }
    }
}

/// The download stage: brings the artifact into the cache.
///
/// # Errors
///
/// Returns [`InstallError::Download`] with what [`download`] reports.
pub fn download_stage(
    env: &Env,
    plan: &Plan,
    stop: &StopFlag,
    progress: &dyn Fn(u64, u64),
) -> Result<DownloadCheckpoint, InstallError> {
    let downloads = env.layout.layout().downloads_dir();
    let context = DownloadContext {
        platform: &env.platform,
        downloads_dir: &downloads,
        stop,
        policy: &env.policy,
    };
    let source = DownloadSource {
        sha256: plan.sha256,
        size: plan.size,
        urls: &plan.urls,
    };
    let outcome = download(&context, &source, &mut |done, total| progress(done, total)).map_err(
        |source| InstallError::Download {
            artifact: plan.catalog_id.clone(),
            source,
        },
    )?;
    let source = match outcome {
        DownloadOutcome::Cached => None,
        DownloadOutcome::Downloaded { .. } => cache_source(env, plan),
    };
    Ok(DownloadCheckpoint {
        cache_file: plan.sha256.to_string(),
        source,
    })
}

/// Reads the URL of `<sha256>.json`, if the note is there.
fn cache_source(env: &Env, plan: &Plan) -> Option<String> {
    let paths = CachePaths::new(&env.layout.layout().downloads_dir(), &plan.sha256);
    let bytes = env
        .platform
        .fs()
        .read_file_limited(&paths.source_note, 4096)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("url")?.as_str().map(str::to_owned)
}

/// Hashes the file from its start, checking `stop` between chunks, and
/// returns the digest. `progress` gets the bytes read and the total.
fn hash_file(
    file: &mut dyn ReadSeekFile,
    stop: &StopFlag,
    progress: &dyn Fn(u64, u64),
) -> Result<Sha256Digest, std::io::Error> {
    file.seek(SeekFrom::Start(0))?;
    let total = file.len();
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; HASH_CHUNK];
    let mut done = 0_u64;
    loop {
        if stop.is_stopped() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "stopped",
            ));
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        done += read as u64;
        progress(done, total);
    }
    Ok(Sha256Digest::from_bytes(hasher.finalize().into()))
}

/// What the first 8 bytes of a file say it is.
fn classify(head: &[u8]) -> &'static str {
    if head.starts_with(&[0x1f, 0x8b]) {
        "gzip"
    } else if head.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
        "zip"
    } else if macho::classify(head).is_some() {
        "mach-o"
    } else {
        "unknown"
    }
}

const fn format_kind(format: ArtifactFormat) -> &'static str {
    match format {
        ArtifactFormat::TarGz => "gzip",
        ArtifactFormat::Zip => "zip",
        ArtifactFormat::Binary => "mach-o",
    }
}

/// The verify stage: recomputes the size and digest of the cache entry and
/// matches its magic number with the format. A wrong size or digest removes
/// the entry, a wrong format leaves it (the bytes are right, the catalog is
/// not).
///
/// # Errors
///
/// Returns [`InstallError::SizeMismatch`], [`InstallError::DigestMismatch`],
/// or [`InstallError::FormatMismatch`] for what it finds, and
/// [`InstallError::Io`] when the entry cannot be read.
pub fn verify_stage(
    env: &Env,
    plan: &Plan,
    stop: &StopFlag,
    progress: &dyn Fn(u64, u64),
) -> Result<(), InstallError> {
    let paths = CachePaths::new(&env.layout.layout().downloads_dir(), &plan.sha256);
    let mut file = env
        .platform
        .fs()
        .open_read(&paths.entry)
        .map_err(|error| platform_error("open the cache entry", "cache", &error))?;
    let len = file.len();
    let mut head = [0_u8; 8];
    let head_len = file.read(&mut head).unwrap_or(0);
    let digest = hash_file(&mut *file, stop, progress).map_err(|error| {
        if error.kind() == std::io::ErrorKind::Interrupted {
            InstallError::Internal("stopped".to_owned())
        } else {
            InstallError::Io {
                op: "read the cache entry",
                root: "cache",
            }
        }
    })?;
    drop(file);
    if len != plan.size || digest != plan.sha256 {
        remove_cache_entry(env, &paths.entry);
    }
    if len != plan.size {
        return Err(InstallError::SizeMismatch {
            artifact: plan.catalog_id.clone(),
            expected: plan.size,
            actual: len,
        });
    }
    if digest != plan.sha256 {
        return Err(InstallError::DigestMismatch {
            artifact: plan.catalog_id.clone(),
            expected: plan.sha256.to_string(),
            actual: digest.to_string(),
        });
    }
    let kind = classify(&head[..head_len]);
    if kind != format_kind(plan.format) {
        return Err(InstallError::FormatMismatch {
            artifact: plan.catalog_id.clone(),
            format: plan.format.as_code(),
            kind,
        });
    }
    Ok(())
}

fn remove_cache_entry(env: &Env, path: &Path) {
    if let Err(error) = remove_owned_tree(&env.platform, &env.layout, path) {
        tracing::warn!(%error, "a wrong cache entry could not be removed");
    }
}

/// Counts the bytes written to regular files for progress.
struct CountingWriter<'a> {
    inner: Box<dyn TreeWriter>,
    done: u64,
    total: u64,
    progress: &'a (dyn Fn(u64, u64) + Sync),
}

impl TreeWriter for CountingWriter<'_> {
    fn create_dir(&mut self, path: &EntryPath) -> Result<(), PlatformError> {
        self.inner.create_dir(path)
    }

    fn create_file(&mut self, path: &EntryPath) -> Result<Box<dyn TreeFile + '_>, PlatformError> {
        // The progress reaches the caller after each file: the extractor
        // copies a file in one call per 64 KiB chunk, but only the writer
        // knows the bytes, so the file reports as it finishes.
        let file = self.inner.create_file(path)?;
        let (done, total, progress) = (&mut self.done, self.total, self.progress);
        Ok(Box::new(ProgressFile {
            inner: file,
            done,
            total,
            progress,
        }))
    }

    fn create_symlink(&mut self, path: &EntryPath, target: &str) -> Result<(), PlatformError> {
        self.inner.create_symlink(path, target)
    }

    fn set_mode(&mut self, path: &EntryPath, mode: FileMode) -> Result<(), PlatformError> {
        self.inner.set_mode(path, mode)
    }

    fn sync(&mut self) -> Result<(), PlatformError> {
        self.inner.sync()
    }
}

struct ProgressFile<'w> {
    inner: Box<dyn TreeFile + 'w>,
    done: &'w mut u64,
    total: u64,
    progress: &'w (dyn Fn(u64, u64) + Sync),
}

impl Write for ProgressFile<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        *self.done += written as u64;
        (self.progress)(*self.done, self.total);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl TreeFile for ProgressFile<'_> {
    fn finish(self: Box<Self>) -> Result<u64, PlatformError> {
        self.inner.finish()
    }
}

/// The extract stage: hashes the cache entry through the one descriptor it
/// then reads the archive from, so bytes swapped after the verify stage
/// never reach the tree, and extracts into `stage/tree/`.
///
/// # Errors
///
/// Returns [`InstallError::InsufficientSpace`] before any write when the
/// volume lacks room, [`InstallError::DigestMismatch`] when the entry
/// changed since the verify stage, [`InstallError::ArchiveRejected`] for an
/// archive that breaks a rule, and [`InstallError::Io`] otherwise.
pub fn extract_stage(
    env: &Env,
    plan: &Plan,
    scratch: &ScratchPaths,
    operation_id: &str,
    stop: &StopFlag,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> Result<(ExtractReport, ExtractCheckpoint), InstallError> {
    let fs = env.platform.fs();
    let needed = required_space(plan.size, plan.unpacked.map(|unpacked| unpacked.bytes));
    let available = fs
        .available_space(&scratch.scratch)
        .map_err(|error| platform_error("read the free space", "data", &error))?;
    if available < needed {
        return Err(InstallError::InsufficientSpace {
            root: "data",
            needed,
            available,
        });
    }
    if fs
        .symlink_metadata(&scratch.stage)
        .map_err(|error| platform_error("inspect the stage folder", "data", &error))?
        .is_some()
    {
        remove_owned_tree(&env.platform, &env.layout, &scratch.stage).map_err(|_| {
            InstallError::Io {
                op: "remove the stage folder",
                root: "data",
            }
        })?;
    }
    for dir in [&scratch.stage, &scratch.tree] {
        fs.create_dir_private(dir)
            .map_err(|error| platform_error("create the stage folder", "data", &error))?;
    }
    let paths = CachePaths::new(&env.layout.layout().downloads_dir(), &plan.sha256);
    let mut file = fs
        .open_read(&paths.entry)
        .map_err(|error| platform_error("open the cache entry", "cache", &error))?;
    let digest = hash_file(&mut *file, stop, &|_, _| {}).map_err(|error| {
        if error.kind() == std::io::ErrorKind::Interrupted {
            InstallError::Internal("stopped".to_owned())
        } else {
            InstallError::Io {
                op: "read the cache entry",
                root: "cache",
            }
        }
    })?;
    if digest != plan.sha256 {
        remove_cache_entry(env, &paths.entry);
        return Err(InstallError::DigestMismatch {
            artifact: plan.catalog_id.clone(),
            expected: plan.sha256.to_string(),
            actual: digest.to_string(),
        });
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| InstallError::Io {
            op: "rewind the cache entry",
            root: "cache",
        })?;
    let limits = ExtractLimits::for_artifact(
        plan.size,
        plan.unpacked.map(|unpacked| unpacked.bytes),
        plan.unpacked.map(|unpacked| unpacked.entries),
    );
    let spec = ExtractSpec::new(plan.archive_root.clone(), plan.executables.clone(), limits);
    let writer = fs
        .open_tree_writer(&scratch.tree)
        .map_err(|error| platform_error("open the tree writer", "data", &error))?;
    let mut writer = CountingWriter {
        inner: writer,
        done: 0,
        total: plan.unpacked.map_or(0, |unpacked| unpacked.bytes),
        progress,
    };
    let report =
        extract(plan.format, &mut *file, &spec, &mut writer, stop).map_err(
            |error| match error {
                ArchiveError::Rejected { reason, entry } => InstallError::ArchiveRejected {
                    artifact: plan.catalog_id.clone(),
                    reason: reason.as_code(),
                    entry,
                },
                ArchiveError::Write(PlatformError::NoSpace { .. }) => {
                    InstallError::InsufficientSpace {
                        root: "data",
                        needed,
                        available: 0,
                    }
                }
                ArchiveError::Stopped => InstallError::Internal("stopped".to_owned()),
                ArchiveError::InvalidSpec { reason } => InstallError::Internal(reason.to_owned()),
                other => {
                    tracing::warn!(error = %other, "a runtime archive could not be read");
                    InstallError::Io {
                        op: "extract the archive",
                        root: "data",
                    }
                }
            },
        )?;
    let marker = Marker {
        operation_id: operation_id.to_owned(),
        sha256: plan.sha256.to_string(),
        software: plan.software.to_string(),
        version: plan.version.to_string(),
        target: plan.target.to_string(),
    };
    finish_tree(&mut writer, &report.entries, &marker)
        .map_err(|error| platform_error("finish the tree", "data", &error))?;
    drop(writer);
    let text = serde_json::to_vec(&entries_json(&report.entries)).map_err(|error| {
        InstallError::Internal(format!("the entries cannot be encoded: {error}"))
    })?;
    fs.write_file_atomic(&scratch.entries, &text)
        .map_err(|error| platform_error("write the entry list", "data", &error))?;
    let checkpoint = ExtractCheckpoint {
        files: report.files,
        bytes: report.file_bytes,
        symlinks: report.symlinks,
        skipped_metadata: report.skipped_metadata,
        stripped_special_bits: report.stripped_special_bits,
    };
    Ok((report, checkpoint))
}

#[derive(Serialize, Deserialize)]
struct StoredEntry {
    path: String,
    kind: String,
    mode: Option<u32>,
    size: u64,
    target: Option<String>,
}

fn entries_json(entries: &[EntryRecord]) -> Vec<StoredEntry> {
    entries
        .iter()
        .map(|entry| StoredEntry {
            path: entry.path.to_string(),
            kind: match entry.kind {
                EntryKind::Dir => "dir",
                EntryKind::File => "file",
                EntryKind::Symlink => "symlink",
            }
            .to_owned(),
            mode: entry.mode.map(FileMode::bits),
            size: entry.size,
            target: entry.target.clone(),
        })
        .collect()
}

/// Reads the entry list the extract stage wrote, for a validation that runs
/// in a later daemon run than the extraction.
fn read_entries(env: &Env, scratch: &ScratchPaths) -> Result<Vec<EntryRecord>, InstallError> {
    let bytes = env
        .platform
        .fs()
        .read_file_limited(&scratch.entries, ENTRIES_MAX)
        .map_err(|error| platform_error("read the entry list", "data", &error))?;
    let stored: Vec<StoredEntry> = serde_json::from_slice(&bytes)
        .map_err(|_| InstallError::Internal("the entry list is not readable".to_owned()))?;
    stored
        .into_iter()
        .map(|entry| {
            let invalid = || InstallError::Internal("the entry list is not valid".to_owned());
            Ok(EntryRecord {
                path: EntryPath::new(&entry.path).map_err(|_| invalid())?,
                kind: match entry.kind.as_str() {
                    "dir" => EntryKind::Dir,
                    "file" => EntryKind::File,
                    "symlink" => EntryKind::Symlink,
                    _ => return Err(invalid()),
                },
                mode: entry.mode.and_then(FileMode::new),
                size: entry.size,
                target: entry.target,
            })
        })
        .collect()
}

/// The validate stage.
///
/// # Errors
///
/// Returns the [`InstallError`] of the [`ValidateError`] it finds.
pub async fn validate_stage(
    env: &Env,
    plan: &Plan,
    scratch: &ScratchPaths,
    entries: Option<Vec<EntryRecord>>,
    stop: &StopFlag,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<ValidateCheckpoint, InstallError> {
    let entries = match entries {
        Some(entries) => entries,
        None => {
            let env = env.clone();
            let scratch = scratch.clone();
            tokio::task::spawn_blocking(move || read_entries(&env, &scratch))
                .await
                .map_err(|error| InstallError::Internal(error.to_string()))??
        }
    };
    let expected = plan.expected_libraries();
    let input = ValidateInput {
        platform: &env.platform,
        tree: &scratch.tree,
        check_dir: &scratch.check,
        entries: &entries,
        executables: &plan.executables,
        min_os: plan.min_os,
        signing: plan.signing,
        maintainer: plan.build.is_some(),
        expected_libraries: expected.as_deref(),
        version: &plan.version,
        version_check: plan.version_check.as_ref(),
        stop,
    };
    let report = validate(input, progress).await?;
    Ok(checkpoint_of(&report, env.platform.clock().now()))
}

fn checkpoint_of(report: &ValidateReport, now: jiff::Timestamp) -> ValidateCheckpoint {
    ValidateCheckpoint {
        adhoc_signed: report.adhoc_signed,
        mach_o_files: report.mach_o_files,
        executables: report
            .executables
            .iter()
            .map(|executable| SignedExecutable {
                path: executable.path.clone(),
                kind: executable.kind.as_code().to_owned(),
                team_id: executable.team_id.as_ref().map(ToString::to_string),
            })
            .collect(),
        warnings: report.warnings.iter().take(WARNINGS_MAX).cloned().collect(),
        validated_at: now.to_string(),
    }
}

/// The commit stage: moves the staged tree to its place with one rename
/// that refuses an existing target.
///
/// # Errors
///
/// Returns [`InstallError::InstallConflict`] when the target folder belongs
/// to something else, and [`InstallError::Io`] otherwise.
pub fn commit_stage(
    env: &Env,
    plan: &Plan,
    scratch: &ScratchPaths,
    operation_id: &str,
    registry: &dyn Registry,
) -> Result<(), InstallError> {
    let request = CommitRequest {
        platform: &env.platform,
        layout: &env.layout,
        registry,
        software: &plan.software,
        version: &plan.version,
        target: plan.target,
        sha256: &plan.sha256,
        stage_tree: &scratch.tree,
        scratch: &scratch.scratch,
        operation_id,
    };
    commit_tree(&request)
        .map(|_| ())
        .map_err(|error| match error {
            CommitError::Conflict { path } => InstallError::InstallConflict {
                path: path.display().to_string(),
            },
            CommitError::Platform(error) => {
                platform_error("move the runtime into place", "data", &error)
            }
            CommitError::Registry(error) => {
                tracing::warn!(%error, "the registry could not be read during a commit");
                InstallError::Io {
                    op: "read the registry",
                    root: "data",
                }
            }
        })
}

/// Everything the final batch writes, built from the checkpoints.
pub struct FinalFacts {
    /// The ID of the runtime the install creates.
    pub runtime_id: RuntimeId,
    /// What validation reported.
    pub validate: ValidateCheckpoint,
    /// What the extraction wrote.
    pub extract: ExtractCheckpoint,
    /// The URL the bytes came from, if known.
    pub source: Option<String>,
}

/// Builds the final batch: `InsertArtifact` with its signature, dependency,
/// and compatibility records, and `InsertRuntime` already `installed`.
///
/// It reads the registry first. A runtime for the artifact that is already
/// there is a conflict that slipped past the artifact lock, so the commit
/// stage has already moved the folder and the operation ends
/// `needs_recovery`.
///
/// # Errors
///
/// Returns [`InstallError::InstallConflict`] for that runtime, and
/// [`InstallError::Internal`] when a record cannot be built.
pub fn final_batch(
    env: &Env,
    plan: &Plan,
    facts: &FinalFacts,
    registry: &dyn Registry,
) -> Result<FinalBatch, InstallError> {
    let read = registry.runtimes().map_err(|error| {
        tracing::warn!(%error, "the registry could not be read for the final batch");
        InstallError::Io {
            op: "read the registry",
            root: "data",
        }
    })?;
    let path =
        env.layout
            .layout()
            .runtime_dir(&plan.software, &plan.version, plan.target, &plan.sha256);
    if read.value.iter().any(|runtime| {
        matches!(runtime.install(), InstallMode::Managed { artifact_sha256 } if *artifact_sha256 == plan.sha256)
    }) {
        return Err(InstallError::InstallConflict {
            path: path.display().to_string(),
        });
    }
    let now = env.platform.clock().now();
    let validated_at = facts
        .validate
        .validated_at
        .parse()
        .map_err(|_| InstallError::Internal("the validation time is not valid".to_owned()))?;
    let runtime = Runtime::committed(
        NewRuntime {
            id: facts.runtime_id,
            software: plan.software.clone(),
            version: plan.version.clone(),
            line: plan.line.clone(),
            target: plan.target,
            install: InstallMode::Managed {
                artifact_sha256: plan.sha256,
            },
            depends_on: BTreeSet::new(),
        },
        validated_at,
        now,
    )
    .map_err(|error| InstallError::Internal(error.to_string()))?;
    let signature = serde_json::json!({
        "policy": plan.signing.as_code(),
        "adhocSignedAtInstall": facts.validate.adhoc_signed,
        "machOFiles": facts.validate.mach_o_files,
        "executables": facts.validate.executables.iter().map(|executable| serde_json::json!({
            "path": executable.path,
            "kind": executable.kind,
            "teamId": executable.team_id,
        })).collect::<Vec<_>>(),
    });
    let dependencies = serde_json::json!({
        "dependencies": plan.dependencies.as_ref().map(|list| list.iter().map(|dependency| {
            let mut value = serde_json::json!({
                "name": dependency.name,
                "kind": dependency.kind.as_code(),
                "installName": dependency.install_name,
            });
            for (name, text) in [
                ("version", &dependency.version),
                ("license", &dependency.license),
                ("sourceUrl", &dependency.source_url),
            ] {
                if let Some(text) = text {
                    value[name] = serde_json::Value::String(text.clone());
                }
            }
            value
        }).collect::<Vec<_>>()).unwrap_or_default(),
        "warnings": facts.validate.warnings,
        "skippedMetadata": facts.extract.skipped_metadata,
        "strippedSpecialBits": facts.extract.stripped_special_bits,
    });
    let json = |value: &serde_json::Value| {
        serde_json::to_string(value).map_err(|error| InstallError::Internal(error.to_string()))
    };
    let record = ArtifactRecord::new(
        plan.sha256,
        plan.software.clone(),
        plan.version.clone(),
        plan.target,
        Some(plan.min_os.to_string()),
        facts.source.clone(),
    )
    .with_install_details(InstallDetails {
        catalog_id: Some(plan.catalog_id.clone()),
        build: plan.build,
        format: Some(plan.format),
        size: Some(plan.size),
        signature_info: Some(
            SignatureInfoJson::new(json(&signature)?)
                .map_err(|error| InstallError::Internal(error.to_string()))?,
        ),
        dependency_manifest: Some(
            DependencyManifestJson::new(json(&dependencies)?)
                .map_err(|error| InstallError::Internal(error.to_string()))?,
        ),
        compatibility: plan.compatibility.clone(),
    });
    Ok(FinalBatch {
        expected_generation: Some(read.generation),
        ops: vec![
            WriteOp::InsertArtifact(record),
            WriteOp::InsertRuntime(runtime),
        ],
    })
}

// ---------------------------------------------------------------------
// The operation
// ---------------------------------------------------------------------

/// What the work needs from the daemon.
#[derive(Clone)]
pub struct InstallDeps {
    /// The platform ports.
    pub platform: Arc<Platform>,
    /// The checked layout.
    pub layout: Arc<PreparedLayout>,
    /// The retry settings of the download.
    pub policy: DownloadPolicy,
}

impl std::fmt::Debug for InstallDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallDeps").finish_non_exhaustive()
    }
}

/// Builds the work of one install: the resource of the operation is the new
/// runtime, `runtime_id`.
///
/// # Errors
///
/// Returns [`InputError`] when the input does not parse.
pub fn install_runtime_work(
    deps: InstallDeps,
    input: &InstallInput,
    runtime_id: RuntimeId,
) -> Result<Box<dyn OperationWork>, InputError> {
    Ok(Box::new(InstallRuntimeWork {
        env: Env {
            platform: deps.platform,
            layout: deps.layout,
            policy: deps.policy,
        },
        plan: Arc::new(Plan::from_input(input)?),
        runtime_id,
    }))
}

/// Builds the work of an `install_runtime` operation again from its journal
/// input, after a restart (spec 0007).
#[derive(Debug)]
pub struct InstallRuntimeFactory {
    deps: InstallDeps,
}

impl InstallRuntimeFactory {
    /// A factory over `deps`.
    #[must_use]
    pub const fn new(deps: InstallDeps) -> Self {
        Self { deps }
    }
}

impl WorkFactory for InstallRuntimeFactory {
    fn rebuild(
        &self,
        operation: &Operation,
        journal: &JournalView,
    ) -> Result<Box<dyn OperationWork>, RebuildError> {
        let OperationResource::Runtime(runtime_id) = operation.resource() else {
            return Err(RebuildError::InvalidInput);
        };
        let input = InstallInput::from_payload(journal.journal.input())
            .map_err(|_| RebuildError::InvalidInput)?;
        install_runtime_work(self.deps.clone(), &input, runtime_id)
            .map_err(|_| RebuildError::InvalidInput)
    }
}

struct InstallRuntimeWork {
    env: Env,
    plan: Arc<Plan>,
    runtime_id: RuntimeId,
}

impl OperationWork for InstallRuntimeWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_install(*self, ctx).await })
    }
}

/// Runs a blocking stage on the blocking pool with a stop flag the context
/// raises, and the progress it reports sent to the context.
async fn staged<T: Send + 'static>(
    ctx: &mut OperationContext,
    work: impl FnOnce(&StopFlag, &(dyn Fn(u64, u64) + Sync)) -> T + Send + 'static,
) -> Result<T, WorkError> {
    let stop = StopFlag::new();
    let (sender, mut receiver) = mpsc::unbounded_channel::<(u64, u64)>();
    let flag = stop.clone();
    let mut handle = tokio::task::spawn_blocking(move || {
        let report = move |done: u64, total: u64| {
            let _ = sender.send((done, total));
        };
        work(&flag, &report)
    });
    let mut stop_sent = false;
    loop {
        enum Event<T> {
            Done(Result<T, tokio::task::JoinError>),
            Progress(Option<(u64, u64)>),
            Stop,
        }
        let event = tokio::select! {
            biased;
            done = &mut handle => Event::Done(done),
            progress = receiver.recv() => Event::Progress(progress),
            () = ctx.stopped(), if !stop_sent => Event::Stop,
        };
        match event {
            Event::Done(done) => {
                while let Ok((done, total)) = receiver.try_recv() {
                    ctx.set_progress(progress_of(done, total));
                }
                return done.map_err(|_| {
                    WorkError::Engine(crate::operation::EngineError::Internal {
                        reason: "an install stage task stopped",
                    })
                });
            }
            Event::Progress(Some((done, total))) => ctx.set_progress(progress_of(done, total)),
            Event::Progress(None) => {}
            Event::Stop => {
                stop.stop();
                stop_sent = true;
            }
        }
    }
}

fn progress_of(done: u64, total: u64) -> Progress {
    if total == 0 {
        Progress::Indeterminate
    } else {
        Progress::Counted {
            done: done.min(total),
            total,
        }
    }
}

/// An error from a stage, as the work ends with it: a stop is the cancel or
/// park the context reports, anything else is the failure.
fn stage_failure(ctx: &OperationContext, error: InstallError) -> WorkError {
    let stopped = matches!(&error, InstallError::Internal(text) if text == "stopped")
        || matches!(
            &error,
            InstallError::Download {
                source: DownloadError::Stopped,
                ..
            } | InstallError::Validate(ValidateError::Stopped)
        );
    if stopped {
        return ctx.check_cancel().err().unwrap_or(WorkError::Cancelled);
    }
    WorkError::Failed(Box::new(error))
}

fn encode<T: Serialize>(value: &T) -> Result<JournalPayload, WorkError> {
    let text = serde_json::to_string(value).map_err(|_| {
        WorkError::Engine(crate::operation::EngineError::Internal {
            reason: "a checkpoint cannot be encoded",
        })
    })?;
    JournalPayload::new(text, crate::operation::limits::CHECKPOINT_MAX).map_err(|_| {
        WorkError::Engine(crate::operation::EngineError::PayloadTooLarge {
            field: "checkpoint",
            max: crate::operation::limits::CHECKPOINT_MAX,
        })
    })
}

fn decode<T: for<'de> Deserialize<'de>>(payload: Option<&JournalPayload>) -> Option<T> {
    serde_json::from_str(payload?.as_str()).ok()
}

async fn existing_runtime(
    ctx: &OperationContext,
    sha256: Sha256Digest,
) -> Result<Option<RuntimeId>, WorkError> {
    let registry = Arc::clone(ctx.registry());
    let found = tokio::task::spawn_blocking(move || registry.runtimes())
        .await
        .map_err(|_| {
            WorkError::Engine(crate::operation::EngineError::Internal {
                reason: "the registry read task stopped",
            })
        })?
        .map_err(|error| WorkError::Engine(crate::operation::EngineError::Registry(error)))?;
    Ok(found.value.iter().find_map(|runtime| {
        matches!(runtime.install(), InstallMode::Managed { artifact_sha256 } if *artifact_sha256 == sha256)
            .then(|| runtime.id())
    }))
}

async fn run_install(work: InstallRuntimeWork, mut ctx: OperationContext) -> WorkResult {
    let InstallRuntimeWork {
        env,
        plan,
        runtime_id,
    } = work;
    // An install queued behind another for the same artifact finds the
    // runtime already there: nothing is left to do.
    if let Some(existing) = existing_runtime(&ctx, plan.sha256).await? {
        return Ok(WorkOutcome {
            result: OperationOutcome {
                resources: vec![ResourceRef::Runtime(existing)],
                partial: Vec::new(),
            },
            finalize: None,
        });
    }
    let operation_id = ctx.id().to_string();

    // Download.
    let downloaded: DownloadCheckpoint =
        match ctx.begin_step(StepSpec::new(DOWNLOAD).repeatable()).await? {
            StepStart::AlreadyDone { checkpoint } => {
                decode(checkpoint.as_ref()).unwrap_or(DownloadCheckpoint {
                    cache_file: plan.sha256.to_string(),
                    source: None,
                })
            }
            StepStart::Run { .. } => {
                ctx.set_stage(DOWNLOAD).await?;
                let (env, plan) = (env.clone(), Arc::clone(&plan));
                let done = staged(&mut ctx, move |stop, progress| {
                    download_stage(&env, &plan, stop, progress)
                })
                .await?
                .map_err(|error| stage_failure(&ctx, error))?;
                ctx.finish_step(Some(encode(&done)?)).await?;
                done
            }
        };

    // Verify digest.
    if let StepStart::Run { .. } = ctx
        .begin_step(StepSpec::new(VERIFY_DIGEST).repeatable())
        .await?
    {
        ctx.set_stage(VERIFY_DIGEST).await?;
        let (env, plan) = (env.clone(), Arc::clone(&plan));
        staged(&mut ctx, move |stop, progress| {
            verify_stage(&env, &plan, stop, progress)
        })
        .await?
        .map_err(|error| stage_failure(&ctx, error))?;
        ctx.finish_step(None).await?;
    }

    // Extract.
    let scratch = ScratchPaths::new(&ctx.scratch_dir().await?);
    let mut entries: Option<Vec<EntryRecord>> = None;
    let extracted: ExtractCheckpoint = match ctx
        .begin_step(StepSpec::new(EXTRACT).repeatable())
        .await?
    {
        StepStart::AlreadyDone { checkpoint } => decode(checkpoint.as_ref()).ok_or_else(|| {
            stage_failure(
                &ctx,
                InstallError::Internal("the extract checkpoint is not readable".to_owned()),
            )
        })?,
        StepStart::Run { .. } => {
            ctx.set_stage(EXTRACT).await?;
            let (env, plan, paths, id) = (
                env.clone(),
                Arc::clone(&plan),
                scratch.clone(),
                operation_id.clone(),
            );
            let (report, checkpoint) = staged(&mut ctx, move |stop, progress| {
                extract_stage(&env, &plan, &paths, &id, stop, progress)
            })
            .await?
            .map_err(|error| stage_failure(&ctx, error))?;
            entries = Some(report.entries);
            ctx.finish_step(Some(encode(&checkpoint)?)).await?;
            checkpoint
        }
    };

    // Validate binary.
    let validated: ValidateCheckpoint = match ctx
        .begin_step(StepSpec::new(VALIDATE_BINARY).repeatable())
        .await?
    {
        StepStart::AlreadyDone { checkpoint } => decode(checkpoint.as_ref()).ok_or_else(|| {
            stage_failure(
                &ctx,
                InstallError::Internal("the validate checkpoint is not readable".to_owned()),
            )
        })?,
        StepStart::Run { .. } => {
            ctx.set_stage(VALIDATE_BINARY).await?;
            let stop = StopFlag::new();
            let (sender, mut receiver) = mpsc::unbounded_channel::<(u64, u64)>();
            let progress = move |done: u64, total: u64| {
                let _ = sender.send((done, total));
            };
            let result = {
                let validation =
                    validate_stage(&env, &plan, &scratch, entries.take(), &stop, &progress);
                tokio::pin!(validation);
                let mut stop_sent = false;
                loop {
                    tokio::select! {
                        biased;
                        result = &mut validation => break result,
                        Some((done, total)) = receiver.recv() => {
                            ctx.set_progress(progress_of(done, total));
                        }
                        () = ctx.stopped(), if !stop_sent => {
                            stop.stop();
                            stop_sent = true;
                        }
                    }
                }
            };
            let done = result.map_err(|error| stage_failure(&ctx, error))?;
            ctx.finish_step(Some(encode(&done)?)).await?;
            done
        }
    };

    // Commit, the point of no return.
    if let StepStart::Run { .. } = ctx
        .begin_step(StepSpec::new(COMMIT).repeatable().irreversible())
        .await?
    {
        ctx.set_stage(COMMIT).await?;
        ctx.set_progress(Progress::Indeterminate);
        let (env, plan, paths, id) = (
            env.clone(),
            Arc::clone(&plan),
            scratch.clone(),
            operation_id.clone(),
        );
        let registry = Arc::clone(ctx.registry());
        tokio::task::spawn_blocking(move || commit_stage(&env, &plan, &paths, &id, &*registry))
            .await
            .map_err(|_| {
                WorkError::Engine(crate::operation::EngineError::Internal {
                    reason: "the commit task stopped",
                })
            })?
            .map_err(|error| stage_failure(&ctx, error))?;
        ctx.finish_step(None).await?;
    }

    let facts = FinalFacts {
        runtime_id,
        validate: validated,
        extract: extracted,
        source: downloaded.source,
    };
    let finalize_env = env.clone();
    let finalize_plan = Arc::clone(&plan);
    let finalize: Finalize = Box::new(move |registry: &dyn Registry| {
        final_batch(&finalize_env, &finalize_plan, &facts, registry)
            .map_err(|error| WorkError::Failed(Box::new(error)))
    });
    Ok(WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Runtime(runtime_id)],
            partial: Vec::new(),
        },
        finalize: Some(finalize),
    })
}
