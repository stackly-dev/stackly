//! `stackly.lock` (spec 0045, *Schema `stackly.lock`*): the exact choice
//! for every dedicated service of a manifest, so every Mac that reads the
//! same manifest and lock runs the same binaries.
//!
//! The lock is a record of a choice, never a source of artifacts or of
//! trust: every artifact it names is looked up again in the trusted
//! catalog and passes the install gate (*Runtime dari lock*). It holds no
//! secret, reference, path, port, setting, host, PID, time, or URL
//! (AC-1), and its bytes depend only on its content.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::Deserialize;
use stackly_domain::{
    ExactVersion, MemberAlias, ProviderName, RevisionLabel, RuntimeLine, RuntimeSelector,
    Sha256Digest, Target,
};
use stackly_platform::{Filesystem, HostFacts};

use super::finding::{Finding, FindingCode, FindingLocation, Findings};
use super::manifest::{API_VERSION, load_document, safe_segment};
use super::plan::{BlockerKind, PlanBlocker};
use super::register::UnreadableReason;
use super::validate::{EnvironmentManifest, ServiceSpec};
use crate::catalog::{Catalog, CatalogEntry, ProviderEntry, TrustVerdict, require_entry_trust};
use crate::provider::raw::Ordered;
use crate::provider::{ArtifactStatus, VersionArtifact};
use crate::runtime::resolve::{ResolveError, UnresolvedReason, resolve};

/// The `kind` of a lock.
pub const LOCK_KIND: &str = "EnvironmentLock";
/// Largest lock file, in bytes.
pub const LOCK_MAX_LEN: u64 = 256 * 1024;
/// Most services one lock holds.
pub const LOCK_SERVICES_MAX: usize = 32;
/// Most artifacts one locked service holds, one per target.
pub const LOCK_ARTIFACTS_MAX: usize = 16;
/// The comment line the emitter writes first.
pub const LOCK_HEADER: &str =
    "# Written by `stackly env lock`. Commit this file. Do not edit it by hand.";

/// The stem of a manifest file name: the name without a last `.yaml` or
/// `.yml`, or the whole name (*Nama file*).
fn stem(manifest_file: &str) -> &str {
    manifest_file
        .strip_suffix(".yaml")
        .or_else(|| manifest_file.strip_suffix(".yml"))
        .unwrap_or(manifest_file)
}

/// The lock file name next to `manifest_file`: `<stem>.lock`.
#[must_use]
pub fn lock_file_name(manifest_file: &str) -> String {
    format!("{}.lock", stem(manifest_file))
}

/// The local override file name next to `manifest_file`:
/// `.<stem>.local.yaml`.
#[must_use]
pub fn local_file_name(manifest_file: &str) -> String {
    format!(".{}.local.yaml", stem(manifest_file))
}

/// `stackly.lock`, as it decodes.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "EnvironmentLock"))]
pub(crate) struct RawLock {
    /// Always `stackly/v1alpha1`.
    #[expect(
        dead_code,
        reason = "the loader checks it before decoding; the schema shows it"
    )]
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = r"^stackly/v1alpha1$"))
    )]
    pub(crate) api_version: String,
    /// Always `EnvironmentLock`.
    #[expect(
        dead_code,
        reason = "the loader checks it before decoding; the schema shows it"
    )]
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = r"^EnvironmentLock$"))
    )]
    pub(crate) kind: String,
    /// The locked services, by alias: 0 to 32.
    pub(crate) services: Ordered<RawLockedService>,
}

/// One locked service.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "LockedService"))]
pub(crate) struct RawLockedService {
    /// `<namespace>/<name>` of the provider.
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = crate::provider::json_schema::PROVIDER_NAME))
    )]
    pub(crate) provider: String,
    /// The revision of the catalog entry when it was locked.
    pub(crate) provider_revision: String,
    /// The manifest digest of that revision.
    #[cfg_attr(feature = "json-schema", schemars(regex(pattern = r"^[0-9a-f]{64}$")))]
    pub(crate) provider_manifest_sha256: String,
    /// The selector the manifest wrote, `stable` when it wrote none.
    pub(crate) selector: String,
    /// The exact version locked.
    pub(crate) version: String,
    /// Its runtime line.
    pub(crate) line: String,
    /// One artifact per target: 1 to 16.
    pub(crate) artifacts: Vec<RawLockedArtifact>,
}

/// One locked artifact.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "LockedArtifact"))]
pub(crate) struct RawLockedArtifact {
    /// `<os>-<arch>`.
    pub(crate) target: String,
    /// The catalog artifact ID.
    pub(crate) id: String,
    /// The digest of the download.
    #[cfg_attr(feature = "json-schema", schemars(regex(pattern = r"^[0-9a-f]{64}$")))]
    pub(crate) sha256: String,
}

/// The JSON Schema of `stackly.lock` for editors, with the file stem
/// `lock.v1alpha1` that `cargo xtask codegen` writes it under in
/// `schemas/environment/` (AC-20).
#[cfg(feature = "json-schema")]
#[must_use]
pub fn json_schemas() -> Vec<(&'static str, schemars::Schema)> {
    let generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    vec![("lock.v1alpha1", generator.into_root_schema_for::<RawLock>())]
}

/// One artifact of a locked service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedArtifact {
    /// The target it is built for.
    pub target: Target,
    /// The catalog artifact ID.
    pub id: String,
    /// The digest of the download.
    pub sha256: Sha256Digest,
}

/// One locked service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedService {
    /// The alias.
    pub alias: MemberAlias,
    /// The provider.
    pub provider: ProviderName,
    /// The revision of the catalog entry when it was locked.
    pub provider_revision: RevisionLabel,
    /// The manifest digest of that revision.
    pub provider_manifest_sha256: Sha256Digest,
    /// The selector the manifest wrote, `stable` when it wrote none.
    pub selector: String,
    /// The exact version.
    pub version: ExactVersion,
    /// Its runtime line.
    pub line: RuntimeLine,
    /// One artifact per target, by target text.
    pub artifacts: Vec<LockedArtifact>,
}

impl LockedService {
    /// Returns the artifact for `target`, if the lock has one.
    #[must_use]
    pub fn artifact_for(&self, target: Target) -> Option<&LockedArtifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
    }

    /// Returns the targets it has artifacts for, as text.
    #[must_use]
    pub fn targets(&self) -> Vec<String> {
        self.artifacts
            .iter()
            .map(|artifact| artifact.target.to_string())
            .collect()
    }
}

/// A lock: its services by alias.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentLock {
    services: Vec<LockedService>,
}

impl EnvironmentLock {
    /// Creates a lock with these services, sorted by alias and each one's
    /// artifacts by target text.
    #[must_use]
    pub fn new(mut services: Vec<LockedService>) -> Self {
        for service in &mut services {
            service
                .artifacts
                .sort_by_key(|artifact| artifact.target.to_string());
        }
        services.sort_by(|a, b| a.alias.as_str().cmp(b.alias.as_str()));
        Self { services }
    }

    /// Returns the services, by alias.
    #[must_use]
    pub fn services(&self) -> &[LockedService] {
        &self.services
    }

    /// Returns the service `alias`, if locked.
    #[must_use]
    pub fn service(&self, alias: &MemberAlias) -> Option<&LockedService> {
        self.services.iter().find(|service| &service.alias == alias)
    }

    /// Returns every target of every entry, by text, without repeats.
    #[must_use]
    pub fn targets(&self) -> Vec<String> {
        let targets: BTreeSet<String> = self
            .services
            .iter()
            .flat_map(LockedService::targets)
            .collect();
        targets.into_iter().collect()
    }
}

/// Why the loader refused a lock: the first finding of the loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockProblem {
    /// The finding code.
    pub code: FindingCode,
    /// Where.
    pub location: FindingLocation,
}

impl LockProblem {
    /// Whether the lock has another `apiVersion` or `kind`, as a newer
    /// Stackly may write: `env.lock` never replaces it (AC-3).
    #[must_use]
    pub const fn is_foreign(&self) -> bool {
        matches!(
            self.code,
            FindingCode::UnsupportedApiVersion | FindingCode::UnsupportedKind
        )
    }

    fn of(findings: &Findings) -> Self {
        findings.list().first().map_or(
            Self {
                code: FindingCode::InvalidValue,
                location: FindingLocation::File,
            },
            |finding| Self {
                code: finding.code(),
                location: finding.location().clone(),
            },
        )
    }

    fn at(code: FindingCode, path: String) -> Self {
        Self {
            code,
            location: FindingLocation::Field(path),
        }
    }
}

/// Decodes lock bytes with the strict loader of the manifest, then checks
/// every value.
///
/// # Errors
///
/// Returns the first [`LockProblem`].
pub fn load(bytes: &[u8]) -> Result<EnvironmentLock, LockProblem> {
    let raw: RawLock = load_document(bytes, LOCK_KIND, "kind must be EnvironmentLock")
        .map_err(|findings| LockProblem::of(&findings))?;
    if raw.services.len() > LOCK_SERVICES_MAX {
        return Err(LockProblem::at(
            FindingCode::InvalidValue,
            "services".into(),
        ));
    }
    let mut services = Vec::with_capacity(raw.services.len());
    for (key, service) in raw.services {
        let base = format!("services.{}", safe_segment(&key));
        let field =
            |name: &str| LockProblem::at(FindingCode::InvalidValue, format!("{base}.{name}"));
        let alias: MemberAlias = key
            .parse()
            .map_err(|_| LockProblem::at(FindingCode::InvalidName, base.clone()))?;
        let provider = service.provider.parse().map_err(|_| field("provider"))?;
        let provider_revision = service
            .provider_revision
            .parse()
            .map_err(|_| field("providerRevision"))?;
        let provider_manifest_sha256 = service
            .provider_manifest_sha256
            .parse()
            .map_err(|_| field("providerManifestSha256"))?;
        service
            .selector
            .parse::<RuntimeSelector>()
            .map_err(|_| field("selector"))?;
        let version = service.version.parse().map_err(|_| field("version"))?;
        let line = service.line.parse().map_err(|_| field("line"))?;
        if service.artifacts.is_empty() || service.artifacts.len() > LOCK_ARTIFACTS_MAX {
            return Err(field("artifacts"));
        }
        let mut artifacts: Vec<LockedArtifact> = Vec::with_capacity(service.artifacts.len());
        for (index, artifact) in service.artifacts.into_iter().enumerate() {
            let at = |name: &str| field(&format!("artifacts[{index}].{name}"));
            let target: Target = artifact.target.parse().map_err(|_| at("target"))?;
            if artifacts.iter().any(|seen| seen.target == target) {
                return Err(at("target"));
            }
            let id_fits = !artifact.id.is_empty()
                && artifact.id.len() <= 128
                && artifact
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".+_-".contains(&b));
            if !id_fits {
                return Err(at("id"));
            }
            artifacts.push(LockedArtifact {
                target,
                id: artifact.id,
                sha256: artifact.sha256.parse().map_err(|_| at("sha256"))?,
            });
        }
        services.push(LockedService {
            alias,
            provider,
            provider_revision,
            provider_manifest_sha256,
            selector: service.selector,
            version,
            line,
            artifacts,
        });
    }
    Ok(EnvironmentLock::new(services))
}

/// A string in double quotes, escaped as JSON escapes it, which YAML reads
/// the same way.
fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("a string always encodes as json")
}

/// The bytes of `lock` (*Emitter*): the comment line, two space indents,
/// every string value in double quotes, `\n` line ends, and one `\n` at
/// the end. The same lock always gives the same bytes.
#[must_use]
pub fn emit(lock: &EnvironmentLock) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(LOCK_HEADER);
    out.push('\n');
    let _ = writeln!(out, "apiVersion: {}", quoted(API_VERSION));
    let _ = writeln!(out, "kind: {}", quoted(LOCK_KIND));
    if lock.services.is_empty() {
        out.push_str("services: {}\n");
        return out.into_bytes();
    }
    out.push_str("services:\n");
    for service in &lock.services {
        let _ = writeln!(out, "  {}:", service.alias);
        let _ = writeln!(out, "    provider: {}", quoted(service.provider.as_str()));
        let _ = writeln!(
            out,
            "    providerRevision: {}",
            quoted(&service.provider_revision.to_string())
        );
        let _ = writeln!(
            out,
            "    providerManifestSha256: {}",
            quoted(&service.provider_manifest_sha256.to_string())
        );
        let _ = writeln!(out, "    selector: {}", quoted(&service.selector));
        let _ = writeln!(out, "    version: {}", quoted(&service.version.to_string()));
        let _ = writeln!(out, "    line: {}", quoted(&service.line.to_string()));
        out.push_str("    artifacts:\n");
        for artifact in &service.artifacts {
            let _ = writeln!(
                out,
                "      - target: {}",
                quoted(&artifact.target.to_string())
            );
            let _ = writeln!(out, "        id: {}", quoted(&artifact.id));
            let _ = writeln!(
                out,
                "        sha256: {}",
                quoted(&artifact.sha256.to_string())
            );
        }
    }
    out.into_bytes()
}

/// What reading the lock file gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockFile {
    /// There is no file.
    Absent,
    /// The file is there but can't be read.
    Unreadable(UnreadableReason),
    /// The loader refused it.
    Invalid(LockProblem),
    /// A valid lock, with the digest of its bytes.
    Valid {
        /// The lock.
        lock: EnvironmentLock,
        /// The SHA-256 of the bytes read.
        sha256: Sha256Digest,
    },
}

impl LockFile {
    /// The lock file from the result of a read: nothing at the path is
    /// [`LockFile::Absent`].
    #[must_use]
    pub fn from_read(read: Result<Vec<u8>, UnreadableReason>) -> Self {
        match read {
            Err(UnreadableReason::NotFound) => Self::Absent,
            Err(reason) => Self::Unreadable(reason),
            Ok(bytes) => match load(&bytes) {
                Ok(lock) => Self::Valid {
                    lock,
                    sha256: super::validate::manifest_digest(&bytes),
                },
                Err(problem) => Self::Invalid(problem),
            },
        }
    }

    /// Reads the lock at `path` through the `Filesystem` port (a regular
    /// file, at most [`LOCK_MAX_LEN`] bytes).
    #[must_use]
    pub fn read(fs: &dyn Filesystem, path: &std::path::Path) -> Self {
        Self::from_read(
            fs.read_file_limited(path, LOCK_MAX_LEN)
                .map_err(|error| UnreadableReason::of(&error)),
        )
    }

    /// Returns the lock when it is valid.
    #[must_use]
    pub const fn lock(&self) -> Option<&EnvironmentLock> {
        match self {
            Self::Valid { lock, .. } => Some(lock),
            _ => None,
        }
    }

    /// Returns the digest of a valid lock.
    #[must_use]
    pub const fn sha256(&self) -> Option<Sha256Digest> {
        match self {
            Self::Valid { sha256, .. } => Some(*sha256),
            _ => None,
        }
    }
}

/// The lock a plan reads: the file name and what reading it gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockInput {
    /// The file name, such as `stackly.lock`.
    pub file: String,
    /// What reading it gave.
    pub content: LockFile,
}

/// The state of a lock against a manifest (*State lock*). Open on the
/// wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockState {
    /// There is no lock file.
    Absent,
    /// Every dedicated service has a matching entry, and no entry is extra.
    InSync,
    /// An alias does not match.
    Stale,
    /// The loader refused the lock.
    Invalid,
    /// The file can't be read.
    Unreadable,
}

impl LockState {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::InSync => "in_sync",
            Self::Stale => "stale",
            Self::Invalid => "invalid",
            Self::Unreadable => "unreadable",
        }
    }
}

/// Why an alias does not match its lock entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockStaleReason {
    /// A dedicated service without an entry.
    NotLocked,
    /// The entry names another provider.
    ProviderChanged,
    /// The entry names another selector.
    SelectorChanged,
    /// The entry's version does not satisfy the selector.
    SelectorUnsatisfied,
    /// An entry whose alias the manifest does not declare as dedicated.
    NotInManifest,
}

impl LockStaleReason {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NotLocked => "not_locked",
            Self::ProviderChanged => "provider_changed",
            Self::SelectorChanged => "selector_changed",
            Self::SelectorUnsatisfied => "selector_unsatisfied",
            Self::NotInManifest => "not_in_manifest",
        }
    }
}

/// One alias that does not match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockStaleItem {
    /// The alias.
    pub service: MemberAlias,
    /// Why.
    pub reason: LockStaleReason,
}

/// The selector of a dedicated service as text after parsing: its
/// `Display`, `stable` when the manifest wrote none.
fn selector_text(service: &ServiceSpec) -> String {
    service
        .selector
        .as_ref()
        .map_or_else(|| RuntimeSelector::Stable.to_string(), ToString::to_string)
}

/// Why the entry of a dedicated `service` does not match, or none when it
/// is *cocok*.
#[must_use]
pub fn stale_reason(
    service: &ServiceSpec,
    entry: Option<&LockedService>,
) -> Option<LockStaleReason> {
    let Some(entry) = entry else {
        return Some(LockStaleReason::NotLocked);
    };
    if entry.provider != service.provider {
        return Some(LockStaleReason::ProviderChanged);
    }
    let written = selector_text(service);
    let locked = entry
        .selector
        .parse::<RuntimeSelector>()
        .map_or_else(|_| entry.selector.clone(), |selector| selector.to_string());
    if written != locked {
        return Some(LockStaleReason::SelectorChanged);
    }
    let satisfied = match service
        .selector
        .as_ref()
        .unwrap_or(&RuntimeSelector::Stable)
    {
        RuntimeSelector::Sha256(digest) => entry.artifacts.iter().any(|a| &a.sha256 == digest),
        RuntimeSelector::Text { text, .. } => {
            *text == entry.version.to_string() || *text == entry.line.to_string()
        }
        _ => true,
    };
    (!satisfied).then_some(LockStaleReason::SelectorUnsatisfied)
}

/// Whether `service` is dedicated, the only kind a lock holds.
const fn is_dedicated(service: &ServiceSpec) -> bool {
    service.instance_ref.is_none()
}

/// The stale items of a valid lock: dedicated services in manifest order,
/// then extra entries by alias.
#[must_use]
pub fn stale_items(manifest: &EnvironmentManifest, lock: &EnvironmentLock) -> Vec<LockStaleItem> {
    let mut items: Vec<LockStaleItem> = manifest
        .services
        .iter()
        .filter(|service| is_dedicated(service))
        .filter_map(|service| {
            stale_reason(service, lock.service(&service.alias)).map(|reason| LockStaleItem {
                service: service.alias.clone(),
                reason,
            })
        })
        .collect();
    items.extend(
        lock.services()
            .iter()
            .filter(|entry| {
                manifest
                    .service(&entry.alias)
                    .is_none_or(|service| !is_dedicated(service))
            })
            .map(|entry| LockStaleItem {
                service: entry.alias.clone(),
                reason: LockStaleReason::NotInManifest,
            }),
    );
    items
}

/// `environment::lock::state` (AC-4): a pure function of the manifest and
/// the lock file, the first matching row of *State lock*.
#[must_use]
pub fn state(manifest: &EnvironmentManifest, file: &LockFile) -> (LockState, Vec<LockStaleItem>) {
    match file {
        LockFile::Absent => (LockState::Absent, Vec::new()),
        LockFile::Unreadable(_) => (LockState::Unreadable, Vec::new()),
        LockFile::Invalid(_) => (LockState::Invalid, Vec::new()),
        LockFile::Valid { lock, .. } => {
            let stale = stale_items(manifest, lock);
            if stale.is_empty() {
                (LockState::InSync, stale)
            } else {
                (LockState::Stale, stale)
            }
        }
    }
}

/// Why a locked digest is not the catalog's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactUnknownReason {
    /// The catalog has no artifact with the digest.
    NotInCatalog,
    /// The catalog's artifact has another version.
    VersionMismatch,
    /// The catalog's artifact is for another target.
    TargetMismatch,
    /// The catalog's artifact has another ID.
    IdMismatch,
}

impl ArtifactUnknownReason {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NotInCatalog => "not_in_catalog",
            Self::VersionMismatch => "version_mismatch",
            Self::TargetMismatch => "target_mismatch",
            Self::IdMismatch => "id_mismatch",
        }
    }
}

/// Why a locked entry can't give this Mac its artifact (*Runtime dari
/// lock*, steps 3 to 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockedArtifactError {
    /// `env.lock_no_artifact`.
    NoArtifact {
        /// This Mac's target.
        target: Target,
        /// The targets the entry has.
        locked_targets: Vec<String>,
    },
    /// `env.lock_artifact_unknown`.
    Unknown {
        /// This Mac's target.
        target: Target,
        /// The locked artifact ID.
        artifact_id: String,
        /// The locked digest.
        sha256: Sha256Digest,
        /// Why.
        reason: ArtifactUnknownReason,
    },
    /// The gate refuses the artifact: `catalog.not_installable`.
    Resolve(ResolveError),
}

impl LockedArtifactError {
    /// The blocker of the error for a service of `provider` whose
    /// manifest selector is `selector`.
    #[must_use]
    pub fn blocker(self, provider: ProviderName, selector: String) -> BlockerKind {
        match self {
            Self::NoArtifact {
                target,
                locked_targets,
            } => BlockerKind::LockNoArtifact {
                target,
                locked_targets,
            },
            Self::Unknown {
                target,
                artifact_id,
                sha256,
                reason,
            } => BlockerKind::LockArtifactUnknown {
                target,
                artifact_id,
                sha256,
                reason,
            },
            Self::Resolve(error) => BlockerKind::Resolve {
                provider,
                error,
                selector,
            },
        }
    }
}

fn digest_of(artifact: &VersionArtifact) -> Option<&Sha256Digest> {
    match &artifact.status {
        ArtifactStatus::Available { sha256, .. } => Some(sha256),
        _ => None,
    }
}

/// *Runtime dari lock*, steps 3 to 5: the catalog artifact the entry
/// locked for this Mac, exactly, through the digest path of
/// `runtime::resolve`. Never another artifact, version, or target.
///
/// # Errors
///
/// Returns the [`LockedArtifactError`] of the first step that fails.
pub fn locked_artifact<'e>(
    entry: &'e ProviderEntry,
    locked: &LockedService,
    host: &HostFacts,
    trust: &TrustVerdict,
) -> Result<&'e VersionArtifact, LockedArtifactError> {
    // 3. The locked artifact for this Mac.
    let Some(wanted) = locked.artifact_for(host.target) else {
        return Err(LockedArtifactError::NoArtifact {
            target: host.target,
            locked_targets: locked.targets(),
        });
    };
    // 4. The digest in the catalog, with the same version, target, and ID.
    let unknown = |reason| LockedArtifactError::Unknown {
        target: host.target,
        artifact_id: wanted.id.clone(),
        sha256: wanted.sha256,
        reason,
    };
    let Some(found) = entry
        .artifacts()
        .iter()
        .find(|artifact| digest_of(artifact) == Some(&wanted.sha256))
    else {
        return Err(unknown(ArtifactUnknownReason::NotInCatalog));
    };
    if found.version != locked.version {
        return Err(unknown(ArtifactUnknownReason::VersionMismatch));
    }
    if found.target != wanted.target {
        return Err(unknown(ArtifactUnknownReason::TargetMismatch));
    }
    if found.id != wanted.id {
        return Err(unknown(ArtifactUnknownReason::IdMismatch));
    }
    // 5. The digest path of resolve: the gate, and no host substitute.
    match resolve(entry, &RuntimeSelector::Sha256(wanted.sha256), host, trust) {
        Ok(resolution) => Ok(resolution.artifact),
        Err(ResolveError::Unresolved { .. }) => Err(unknown(ArtifactUnknownReason::NotInCatalog)),
        Err(error) => Err(LockedArtifactError::Resolve(error)),
    }
}

/// What `env.lock` re-picks (`refresh`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockRefresh {
    /// Keep every entry that is still good.
    None,
    /// Pick every entry again.
    All,
    /// Pick these aliases again; keep the rest that are still good.
    Services(Vec<MemberAlias>),
}

/// How one service of `env.lock` changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockChange {
    /// A new alias.
    Added,
    /// The same version.
    Kept,
    /// Another version.
    Updated,
    /// The entry was dropped.
    Removed,
}

impl LockChange {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Kept => "kept",
            Self::Updated => "updated",
            Self::Removed => "removed",
        }
    }
}

/// One service of the reply of `env.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedServiceChange {
    /// The alias.
    pub alias: MemberAlias,
    /// The provider.
    pub provider: ProviderName,
    /// The selector.
    pub selector: String,
    /// The version locked now; none for a removed entry or a blocked one.
    pub version: Option<ExactVersion>,
    /// The artifact for this Mac, if the entry has one.
    pub artifact_id: Option<String>,
    /// How it changed.
    pub change: LockChange,
    /// The version before, when it changed.
    pub previous_version: Option<ExactVersion>,
}

/// What `env.lock` picked: the new lock when nothing blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockResolution {
    /// The lock to write, when `blockers` is empty.
    pub lock: Option<EnvironmentLock>,
    /// One per service, in manifest order, then removed entries.
    pub services: Vec<LockedServiceChange>,
    /// What stops the write.
    pub blockers: Vec<PlanBlocker>,
}

/// Why `env.lock` could not pick at all.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LockError {
    /// `refresh` names an alias that is not a dedicated service.
    #[error("the manifest has no dedicated service of that alias")]
    ServiceNotFound(MemberAlias),
    /// The registry could not be read for the trust gate.
    #[error(transparent)]
    Registry(#[from] stackly_domain::RegistryError),
}

/// The first available artifact of `version` for each target, in catalog
/// order (the rule of the install gate of spec 0017), skipping `skip`.
fn other_targets<'e>(
    entry: &'e ProviderEntry,
    version: &ExactVersion,
    skip: &[Target],
) -> Vec<&'e VersionArtifact> {
    let mut seen: Vec<Target> = skip.to_vec();
    let mut found = Vec::new();
    for artifact in entry.artifacts() {
        if artifact.version != *version || seen.contains(&artifact.target) {
            continue;
        }
        seen.push(artifact.target);
        if digest_of(artifact).is_some() {
            found.push(artifact);
        }
    }
    found
}

fn locked_of(artifact: &VersionArtifact) -> Option<LockedArtifact> {
    digest_of(artifact).map(|sha256| LockedArtifact {
        target: artifact.target,
        id: artifact.id.clone(),
        sha256: *sha256,
    })
}

/// Picks the entry of `service` again: the trust gate, `runtime::resolve`,
/// then one artifact per target of that version.
fn pick(
    entry: &ProviderEntry,
    service: &ServiceSpec,
    host: &HostFacts,
    verdict: &TrustVerdict,
) -> Result<LockedService, BlockerKind> {
    match verdict {
        TrustVerdict::Trusted => {}
        TrustVerdict::ApprovalRequired => {
            return Err(BlockerKind::ProviderApprovalRequired {
                provider: service.provider.clone(),
            });
        }
        TrustVerdict::Revoked(_) => {
            return Err(BlockerKind::ProviderRevoked {
                provider: service.provider.clone(),
            });
        }
    }
    let selector = service.selector.clone().unwrap_or(RuntimeSelector::Stable);
    let written = service
        .selector_text
        .clone()
        .unwrap_or_else(|| "stable".to_owned());
    let artifact = resolve(entry, &selector, host, verdict)
        .map_err(|error| BlockerKind::Resolve {
            provider: service.provider.clone(),
            error,
            selector: written.clone(),
        })?
        .artifact;
    let Some(own) = locked_of(artifact) else {
        return Err(BlockerKind::Resolve {
            provider: service.provider.clone(),
            error: ResolveError::Unresolved {
                reason: UnresolvedReason::NoMatch,
                known_lines: Vec::new(),
            },
            selector: written,
        });
    };
    let mut artifacts = vec![own];
    artifacts.extend(
        other_targets(entry, &artifact.version, &[host.target])
            .into_iter()
            .filter_map(locked_of),
    );
    let key = entry.package.key();
    Ok(LockedService {
        alias: service.alias.clone(),
        provider: service.provider.clone(),
        provider_revision: key.revision().clone(),
        provider_manifest_sha256: *key.manifest_sha256(),
        selector: selector.to_string(),
        version: artifact.version.clone(),
        line: artifact.line.clone(),
        artifacts,
    })
}

/// Keeps a matching entry when it is still good (*Entri yang
/// dipertahankan*): the provider key of the catalog now, its artifacts as
/// they are, and the targets it lacks added. None when it must be picked
/// again.
fn keep(
    entry: &ProviderEntry,
    locked: &LockedService,
    host: &HostFacts,
    verdict: &TrustVerdict,
) -> Option<LockedService> {
    let mut kept = locked.clone();
    if locked.artifact_for(host.target).is_some() {
        locked_artifact(entry, locked, host, verdict).ok()?;
    } else {
        let selector: RuntimeSelector = locked.version.to_string().parse().ok()?;
        let artifact = resolve(entry, &selector, host, verdict).ok()?.artifact;
        if artifact.version != locked.version || artifact.target != host.target {
            return None;
        }
        kept.artifacts.push(locked_of(artifact)?);
    }
    let have: Vec<Target> = kept.artifacts.iter().map(|a| a.target).collect();
    kept.artifacts.extend(
        other_targets(entry, &locked.version, &have)
            .into_iter()
            .filter_map(locked_of),
    );
    kept.artifacts.truncate(LOCK_ARTIFACTS_MAX);
    let key = entry.package.key();
    kept.provider_revision = key.revision().clone();
    kept.provider_manifest_sha256 = *key.manifest_sha256();
    Some(kept)
}

/// `env.lock` without the file (AC-2, AC-3): every dedicated service of
/// `manifest` picked or kept against the catalog in memory, compared with
/// the existing lock. It reads the registry only for the trust gate, and
/// installs, downloads, spawns, and writes nothing.
///
/// # Errors
///
/// Returns [`LockError::ServiceNotFound`] when `refresh` names an alias
/// that is not a dedicated service, and [`LockError::Registry`] when the
/// trust gate can't read the registry.
pub fn resolve_entries(
    manifest: &EnvironmentManifest,
    existing: &LockFile,
    refresh: &LockRefresh,
    catalog: &Catalog,
    registry: &dyn stackly_domain::Registry,
    host: &HostFacts,
) -> Result<LockResolution, LockError> {
    if let LockRefresh::Services(aliases) = refresh {
        for alias in aliases {
            if manifest
                .service(alias)
                .is_none_or(|service| !is_dedicated(service))
            {
                return Err(LockError::ServiceNotFound(alias.clone()));
            }
        }
    }
    let mut blockers = Vec::new();
    let previous: Option<&EnvironmentLock> = match existing {
        LockFile::Absent => None,
        LockFile::Valid { lock, .. } => Some(lock),
        LockFile::Invalid(problem) if problem.is_foreign() || *refresh != LockRefresh::All => {
            blockers.push(PlanBlocker {
                service: None,
                kind: BlockerKind::LockInvalid {
                    code: problem.code,
                    location: problem.location.clone(),
                },
            });
            None
        }
        LockFile::Unreadable(reason) if *refresh != LockRefresh::All => {
            blockers.push(PlanBlocker {
                service: None,
                kind: BlockerKind::LockUnreadable { reason: *reason },
            });
            None
        }
        LockFile::Invalid(_) | LockFile::Unreadable(_) => None,
    };
    let repick = |alias: &MemberAlias| match refresh {
        LockRefresh::None => false,
        LockRefresh::All => true,
        LockRefresh::Services(aliases) => aliases.contains(alias),
    };
    let mut locked = Vec::new();
    let mut services = Vec::new();
    for service in manifest.services.iter().filter(|s| is_dedicated(s)) {
        let before = previous.and_then(|lock| lock.service(&service.alias));
        let Some(CatalogEntry::Provider(entry)) = catalog.entry(&service.provider) else {
            // A valid manifest names a catalog provider; nothing to lock.
            continue;
        };
        let verdict = require_entry_trust(catalog, registry, entry)?;
        let kept = before
            .filter(|before| {
                !repick(&service.alias) && stale_reason(service, Some(before)).is_none()
            })
            .and_then(|before| keep(entry, before, host, &verdict));
        let chosen = match kept {
            Some(kept) => Ok(kept),
            None => pick(entry, service, host, &verdict),
        };
        match chosen {
            Ok(chosen) => {
                let (change, previous_version) = match before {
                    None => (LockChange::Added, None),
                    Some(before) if before.version == chosen.version => (LockChange::Kept, None),
                    Some(before) => (LockChange::Updated, Some(before.version.clone())),
                };
                services.push(LockedServiceChange {
                    alias: service.alias.clone(),
                    provider: service.provider.clone(),
                    selector: chosen.selector.clone(),
                    version: Some(chosen.version.clone()),
                    artifact_id: chosen.artifact_for(host.target).map(|a| a.id.clone()),
                    change,
                    previous_version,
                });
                locked.push(chosen);
            }
            Err(kind) => {
                services.push(LockedServiceChange {
                    alias: service.alias.clone(),
                    provider: service.provider.clone(),
                    selector: selector_text(service),
                    version: None,
                    artifact_id: None,
                    change: if before.is_some() {
                        LockChange::Kept
                    } else {
                        LockChange::Added
                    },
                    previous_version: before.map(|before| before.version.clone()),
                });
                blockers.push(PlanBlocker {
                    service: Some(service.alias.clone()),
                    kind,
                });
            }
        }
    }
    if let Some(previous) = previous {
        for entry in previous.services() {
            if manifest
                .service(&entry.alias)
                .is_none_or(|service| !is_dedicated(service))
            {
                services.push(LockedServiceChange {
                    alias: entry.alias.clone(),
                    provider: entry.provider.clone(),
                    selector: entry.selector.clone(),
                    version: None,
                    artifact_id: None,
                    change: LockChange::Removed,
                    previous_version: Some(entry.version.clone()),
                });
            }
        }
    }
    let lock = blockers.is_empty().then(|| EnvironmentLock::new(locked));
    Ok(LockResolution {
        lock,
        services,
        blockers,
    })
}

/// A lock problem as a finding, for a message with a fixed template.
#[must_use]
pub fn problem_finding(problem: &LockProblem) -> Finding {
    Finding::error(problem.code, problem.location.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_follow_the_stem_of_the_manifest() {
        assert_eq!(lock_file_name("stackly.yaml"), "stackly.lock");
        assert_eq!(lock_file_name("dev.yml"), "dev.lock");
        assert_eq!(lock_file_name("manifest"), "manifest.lock");
        assert_eq!(local_file_name("stackly.yaml"), ".stackly.local.yaml");
        assert_eq!(local_file_name("dev.yml"), ".dev.local.yaml");
    }
}
