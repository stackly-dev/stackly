//! The install gate (spec 0017, *Gate install*): whether an exact version
//! of a catalog entry can be installed on this Mac. `runtime.install` and
//! `runtime.resolve` of feature 18 call it after selector resolution and
//! before any operation (PRD AC-002, AC-005). There is no substitution of
//! architecture, OS, or emulation.

use stackly_domain::{ExactVersion, Target};
use stackly_platform::HostFacts;

use crate::provider::{ArtifactStatus, VersionArtifact};

use super::model::{CatalogEntry, ProviderEntry};
use super::status::SupportReason;

/// Why the gate refuses a version. The codes are the same as the
/// matching [`SupportReason`] codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotInstallableReason {
    /// The provider is planned.
    ProviderPlanned,
    /// The provider package failed validation.
    PackageInvalid,
    /// No artifact of the version for this Mac's OS.
    NoBuildForHost,
    /// Artifacts for this Mac's OS exist, but not for its architecture.
    NoBuildForArch,
    /// The artifact for this Mac is blocked.
    ArtifactBlocked,
    /// The artifact for this Mac is a test fixture.
    FixtureOnly,
    /// The artifact for this Mac needs a newer macOS than this Mac runs.
    RequiresNewerOs,
}

impl NotInstallableReason {
    /// Returns the [`SupportReason`] with the same code.
    #[must_use]
    pub const fn support_reason(self) -> SupportReason {
        match self {
            Self::ProviderPlanned => SupportReason::ProviderPlanned,
            Self::PackageInvalid => SupportReason::PackageInvalid,
            Self::NoBuildForHost => SupportReason::NoBuildForHost,
            Self::NoBuildForArch => SupportReason::NoBuildForArch,
            Self::ArtifactBlocked => SupportReason::ArtifactBlocked,
            Self::FixtureOnly => SupportReason::FixtureOnly,
            Self::RequiresNewerOs => SupportReason::RequiresNewerOs,
        }
    }

    /// Returns the stable code, the `reason` detail of
    /// `catalog.not_installable`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        self.support_reason().as_code()
    }
}

/// What the gate decides for one version on this Mac.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installability<'a> {
    /// The `available` artifact built for exactly this Mac's target.
    Installable {
        /// The artifact to install.
        artifact: &'a VersionArtifact,
    },
    /// The version cannot be installed here.
    NotInstallable {
        /// Why.
        reason: NotInstallableReason,
        /// The targets of the version's `available` artifacts, by their
        /// text, empty when there are none.
        available_targets: Vec<Target>,
    },
}

/// The version is not one of the entry's versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the version is not in the catalog entry")]
pub struct VersionNotFound;

/// Decides whether `version` of `entry` can be installed on `host`.
///
/// Planned and invalid entries refuse every version. For a provider entry,
/// the artifact built for exactly the host's target decides: `available`
/// installs, unless its `minOs` is above the macOS the host runs
/// (`requires_newer_os`); `blocked` and `fixture` refuse. Without an artifact
/// for the target, artifacts for the host OS on another architecture give
/// `no_build_for_arch`, and anything else `no_build_for_host`.
///
/// # Errors
///
/// Returns [`VersionNotFound`] when a provider entry has no artifact of
/// `version`.
pub fn installability<'a>(
    entry: &'a CatalogEntry,
    version: &ExactVersion,
    host: &HostFacts,
) -> Result<Installability<'a>, VersionNotFound> {
    let refuse = |reason| Installability::NotInstallable {
        reason,
        available_targets: Vec::new(),
    };
    match entry {
        CatalogEntry::Planned(_) => Ok(refuse(NotInstallableReason::ProviderPlanned)),
        CatalogEntry::Invalid(_) => Ok(refuse(NotInstallableReason::PackageInvalid)),
        CatalogEntry::Provider(provider) => provider_installability(provider, version, host),
    }
}

/// [`installability`] for an entry already known to be a provider.
///
/// # Errors
///
/// Returns [`VersionNotFound`] when the entry has no artifact of `version`.
pub fn provider_installability<'a>(
    provider: &'a ProviderEntry,
    version: &ExactVersion,
    host: &HostFacts,
) -> Result<Installability<'a>, VersionNotFound> {
    let artifacts: Vec<&VersionArtifact> = provider
        .artifacts()
        .iter()
        .filter(|artifact| artifact.version == *version)
        .collect();
    if artifacts.is_empty() {
        return Err(VersionNotFound);
    }
    let mut available_targets: Vec<Target> = artifacts
        .iter()
        .filter(|artifact| matches!(artifact.status, ArtifactStatus::Available { .. }))
        .map(|artifact| artifact.target)
        .collect();
    available_targets.sort_by_key(ToString::to_string);
    available_targets.dedup();
    let reason = match artifacts
        .iter()
        .find(|artifact| artifact.target == host.target)
    {
        Some(artifact) => match &artifact.status {
            ArtifactStatus::Available { min_os, .. } if *min_os > host.os_version => {
                NotInstallableReason::RequiresNewerOs
            }
            ArtifactStatus::Available { .. } => {
                return Ok(Installability::Installable { artifact });
            }
            ArtifactStatus::Blocked { .. } => NotInstallableReason::ArtifactBlocked,
            ArtifactStatus::Fixture { .. } => NotInstallableReason::FixtureOnly,
        },
        None if artifacts
            .iter()
            .any(|artifact| artifact.target.os() == host.target.os()) =>
        {
            NotInstallableReason::NoBuildForArch
        }
        None => NotInstallableReason::NoBuildForHost,
    };
    Ok(Installability::NotInstallable {
        reason,
        available_targets,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;

    use stackly_domain::ProviderName;

    use super::super::model::{PlannedEntry, ProviderEntry, TileHint};
    use super::super::rules::TileColor;
    use super::super::testing::CatalogFixture;
    use super::*;
    use crate::provider::{Category, NameCheck, PackageFiles, ValidatedPackage};

    fn host() -> HostFacts {
        HostFacts::new(
            "macos-aarch64".parse().unwrap(),
            stackly_domain::OsVersion::major_minor(27, 0),
            None,
        )
    }

    fn provider(package: ValidatedPackage) -> CatalogEntry {
        CatalogEntry::Provider(ProviderEntry {
            package: Arc::new(package),
            tile: TileHint {
                mark: "Na".into(),
                tile: TileColor::Gray,
            },
            notes: Vec::new(),
        })
    }

    fn version() -> ExactVersion {
        "2.15.1".parse().unwrap()
    }

    fn refusal(entry: &CatalogEntry) -> (NotInstallableReason, Vec<String>) {
        match installability(entry, &version(), &host()).unwrap() {
            Installability::NotInstallable {
                reason,
                available_targets,
            } => (
                reason,
                available_targets.iter().map(ToString::to_string).collect(),
            ),
            Installability::Installable { artifact } => panic!("installable: {}", artifact.id),
        }
    }

    #[test]
    fn a_provider_for_another_platform_is_refused_with_its_available_targets() {
        assert_eq!(
            refusal(&provider(CatalogFixture::LinuxOnly.package())),
            (
                NotInstallableReason::NoBuildForHost,
                vec!["linux-x86_64".to_owned()]
            )
        );
        assert_eq!(
            refusal(&provider(CatalogFixture::IntelOnly.package())),
            (
                NotInstallableReason::NoBuildForArch,
                vec!["macos-x86_64".to_owned()]
            )
        );
        assert_eq!(
            refusal(&provider(CatalogFixture::BlockedHost.package())),
            (NotInstallableReason::ArtifactBlocked, Vec::new())
        );
    }

    #[test]
    fn only_the_available_artifact_for_exactly_the_host_installs() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats");
        let files: BTreeMap<String, Vec<u8>> = [
            "provider.yaml",
            "versions.json",
            "schemas/settings.schema.json",
            "templates/nats.conf.tmpl",
        ]
        .into_iter()
        .map(|path| (path.to_owned(), std::fs::read(dir.join(path)).unwrap()))
        .collect();
        let nats = provider(
            ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
                .unwrap(),
        );

        match installability(&nats, &version(), &host()).unwrap() {
            Installability::Installable { artifact } => {
                assert_eq!(artifact.id, "nats-2.15.1-macos-aarch64");
            }
            other => panic!("not installable: {other:?}"),
        }
        assert_eq!(
            installability(&nats, &"9.9.9".parse().unwrap(), &host()),
            Err(VersionNotFound)
        );
    }

    #[test]
    fn planned_entries_refuse_every_version() {
        let planned = CatalogEntry::Planned(PlannedEntry {
            id: "official/mysql".parse::<ProviderName>().unwrap(),
            display_name: "MySQL".into(),
            category: Category::Database,
            description: "Relational server.".into(),
            note: None,
            tile: TileHint {
                mark: "My".into(),
                tile: TileColor::Teal,
            },
        });

        assert_eq!(
            installability(&planned, &"9.9.9".parse().unwrap(), &host()),
            Ok(Installability::NotInstallable {
                reason: NotInstallableReason::ProviderPlanned,
                available_targets: Vec::new()
            })
        );
    }

    #[test]
    fn refusal_codes_are_the_support_reason_codes() {
        for (reason, code) in [
            (NotInstallableReason::ProviderPlanned, "provider_planned"),
            (NotInstallableReason::PackageInvalid, "package_invalid"),
            (NotInstallableReason::NoBuildForHost, "no_build_for_host"),
            (NotInstallableReason::NoBuildForArch, "no_build_for_arch"),
            (NotInstallableReason::ArtifactBlocked, "artifact_blocked"),
            (NotInstallableReason::FixtureOnly, "fixture_only"),
            (NotInstallableReason::RequiresNewerOs, "requires_newer_os"),
        ] {
            assert_eq!(reason.as_code(), code);
            assert_eq!(reason.support_reason().as_code(), code);
        }
    }
}
