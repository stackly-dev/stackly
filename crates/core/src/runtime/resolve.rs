//! Turning a selector into one exact artifact (spec 0018, *Resolusi
//! selector*): `stable`, a digest, a version, or a line is matched against
//! the artifacts of a catalog entry, and the install gate of spec 0017 has
//! the last word. Matching never orders versions.

use stackly_domain::{ExactVersion, RuntimeLine, RuntimeSelector, Sha256Digest, Target};
use stackly_platform::HostFacts;

use crate::catalog::{
    Installability, NotInstallableReason, ProviderEntry, VersionNotFound, provider_installability,
};
use crate::provider::{ArtifactStatus, VersionArtifact};

/// What a selector resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Resolution<'a> {
    /// The artifact the gate lets this Mac install.
    pub artifact: &'a VersionArtifact,
}

/// Why a selector matched nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnresolvedReason {
    /// `stable` and the entry has no `stable` artifact for this Mac.
    NoStableArtifact,
    /// A digest or text that no artifact has.
    NoMatch,
}

impl UnresolvedReason {
    /// Returns the `reason` detail of `runtime.selector_unresolved`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NoStableArtifact => "no_stable_artifact",
            Self::NoMatch => "no_match",
        }
    }
}

/// Why a selector did not resolve to an installable artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveError {
    /// Nothing matched (`runtime.selector_unresolved`).
    Unresolved {
        /// Why.
        reason: UnresolvedReason,
        /// The lines of the entry's artifacts, in order, for the hint.
        known_lines: Vec<RuntimeLine>,
    },
    /// The text names both a version and a line, or a line with more than
    /// one version (`runtime.selector_ambiguous`).
    Ambiguous {
        /// The versions it could mean, in catalog order.
        versions: Vec<ExactVersion>,
    },
    /// The gate refuses the version, or a digest of another target was
    /// named while the host has its own artifact of that version
    /// (`catalog.not_installable`).
    NotInstallable {
        /// Why.
        reason: NotInstallableReason,
        /// The host target the request was for.
        target: Target,
        /// The targets the version's available artifacts are built for.
        available_targets: Vec<Target>,
    },
}

/// Resolves `selector` against the artifacts of `entry` for `host`.
///
/// 1. `stable`: the `available` artifact with `stable: true` for the host
///    target.
/// 2. `sha256:<hex>`: the artifact with that digest on any target, whose
///    version then goes to the gate. When the gate returns the host's
///    artifact and its digest is another, the request is refused: the
///    artifact of the host is never a substitute.
/// 3. Text: the artifacts whose version equals it, and the artifacts whose
///    line equals it. Both found, or one holding more than one distinct
///    version, is ambiguous.
///
/// # Errors
///
/// Returns the [`ResolveError`] of the rule that applies.
pub fn resolve<'a>(
    entry: &'a ProviderEntry,
    selector: &RuntimeSelector,
    host: &HostFacts,
) -> Result<Resolution<'a>, ResolveError> {
    let artifacts = entry.artifacts();
    let (version, wanted_digest) = match selector {
        RuntimeSelector::Stable => (stable_version(artifacts, host.target)?, None),
        RuntimeSelector::Sha256(digest) => (digest_version(artifacts, digest)?, Some(*digest)),
        RuntimeSelector::Text {
            version: text_version,
            line,
            ..
        } => (
            text_match(artifacts, text_version.as_ref(), line.as_ref())?,
            None,
        ),
        _ => return Err(unresolved(artifacts, UnresolvedReason::NoMatch)),
    };
    let verdict = provider_installability(entry, &version, host)
        .map_err(|VersionNotFound| unresolved(artifacts, UnresolvedReason::NoMatch))?;
    match verdict {
        Installability::Installable { artifact } => {
            if let Some(digest) = wanted_digest
                && artifact_digest(artifact) != Some(&digest)
            {
                let reason = if artifacts
                    .iter()
                    .find(|candidate| artifact_digest(candidate) == Some(&digest))
                    .is_some_and(|candidate| candidate.target.os() == host.target.os())
                {
                    NotInstallableReason::NoBuildForArch
                } else {
                    NotInstallableReason::NoBuildForHost
                };
                return Err(ResolveError::NotInstallable {
                    reason,
                    target: host.target,
                    available_targets: available_targets(artifacts, &version),
                });
            }
            Ok(Resolution { artifact })
        }
        Installability::NotInstallable {
            reason,
            available_targets,
        } => Err(ResolveError::NotInstallable {
            reason,
            target: host.target,
            available_targets,
        }),
    }
}

fn artifact_digest(artifact: &VersionArtifact) -> Option<&Sha256Digest> {
    match &artifact.status {
        ArtifactStatus::Available { sha256, .. } => Some(sha256),
        _ => None,
    }
}

fn available_targets(artifacts: &[VersionArtifact], version: &ExactVersion) -> Vec<Target> {
    let mut targets: Vec<Target> = artifacts
        .iter()
        .filter(|artifact| {
            artifact.version == *version
                && matches!(artifact.status, ArtifactStatus::Available { .. })
        })
        .map(|artifact| artifact.target)
        .collect();
    targets.sort_by_key(ToString::to_string);
    targets.dedup();
    targets
}

fn unresolved(artifacts: &[VersionArtifact], reason: UnresolvedReason) -> ResolveError {
    let mut known_lines: Vec<RuntimeLine> = Vec::new();
    for artifact in artifacts {
        if !known_lines.contains(&artifact.line) {
            known_lines.push(artifact.line.clone());
        }
    }
    ResolveError::Unresolved {
        reason,
        known_lines,
    }
}

fn stable_version(
    artifacts: &[VersionArtifact],
    host: Target,
) -> Result<ExactVersion, ResolveError> {
    artifacts
        .iter()
        .find(|artifact| {
            artifact.target == host
                && matches!(
                    artifact.status,
                    ArtifactStatus::Available { stable: true, .. }
                )
        })
        .map(|artifact| artifact.version.clone())
        .ok_or_else(|| unresolved(artifacts, UnresolvedReason::NoStableArtifact))
}

fn digest_version(
    artifacts: &[VersionArtifact],
    digest: &Sha256Digest,
) -> Result<ExactVersion, ResolveError> {
    artifacts
        .iter()
        .find(|artifact| artifact_digest(artifact) == Some(digest))
        .map(|artifact| artifact.version.clone())
        .ok_or_else(|| unresolved(artifacts, UnresolvedReason::NoMatch))
}

fn text_match(
    artifacts: &[VersionArtifact],
    version: Option<&ExactVersion>,
    line: Option<&RuntimeLine>,
) -> Result<ExactVersion, ResolveError> {
    let mut by_version: Vec<ExactVersion> = Vec::new();
    let mut by_line: Vec<ExactVersion> = Vec::new();
    for artifact in artifacts {
        if version.is_some_and(|wanted| *wanted == artifact.version)
            && !by_version.contains(&artifact.version)
        {
            by_version.push(artifact.version.clone());
        }
        if line.is_some_and(|wanted| *wanted == artifact.line)
            && !by_line.contains(&artifact.version)
        {
            by_line.push(artifact.version.clone());
        }
    }
    match (by_version.as_slice(), by_line.as_slice()) {
        ([], []) => Err(unresolved(artifacts, UnresolvedReason::NoMatch)),
        ([only], []) | ([], [only]) => Ok(only.clone()),
        _ => {
            let mut versions = by_version;
            for found in by_line {
                if !versions.contains(&found) {
                    versions.push(found);
                }
            }
            Err(ResolveError::Ambiguous { versions })
        }
    }
}
