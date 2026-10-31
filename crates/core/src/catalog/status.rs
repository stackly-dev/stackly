//! How supported a catalog entry and each of its version and OS cells are
//! (spec 0017, *Aturan status*). The daemon computes every status; clients
//! only show them (BR-016). `supported` and `experimental` come only from
//! compatibility records of feature 18 (BR-025).

use std::collections::BTreeSet;

use stackly_domain::{Arch, ExactVersion, Os, SupportStatus};
use stackly_platform::HostFacts;

use crate::provider::{ArtifactStatus, VersionArtifact};
use crate::runtime::compatibility::support_status;

use super::install::{Installability, NotInstallableReason, installability};
use super::model::{CatalogEntry, ProviderEntry};

/// The operating systems every version row of the matrix has a cell for,
/// in order.
pub const MATRIX_OSES: [Os; 3] = [Os::Macos, Os::Linux, Os::Windows];

/// Why an entry or a cell has its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SupportReason {
    /// Available, but no compatibility record for this Mac yet.
    Untested,
    /// No artifact of this version for the OS.
    NoBuild,
    /// Artifacts for the OS exist, but not for this architecture.
    NoBuildForArch,
    /// No artifact of any version for this Mac.
    NoBuildForHost,
    /// The catalog note for the OS gives the status.
    PlatformNote,
    /// The artifact is blocked, with its recorded reason.
    ArtifactBlocked,
    /// A test fixture, never installable.
    FixtureOnly,
    /// The provider is planned.
    ProviderPlanned,
    /// The provider package failed validation.
    PackageInvalid,
    /// The artifact for this Mac needs a newer macOS than this Mac runs.
    RequiresNewerOs,
    /// A passing compatibility record of this provider revision on this
    /// macOS makes it supported.
    ProviderTested,
    /// A passing runtime record on this macOS makes it experimental; the
    /// provider integration is not tested.
    RuntimeTested,
}

impl SupportReason {
    /// Every reason.
    pub const ALL: [Self; 12] = [
        Self::Untested,
        Self::NoBuild,
        Self::NoBuildForArch,
        Self::NoBuildForHost,
        Self::PlatformNote,
        Self::ArtifactBlocked,
        Self::FixtureOnly,
        Self::ProviderPlanned,
        Self::PackageInvalid,
        Self::RequiresNewerOs,
        Self::ProviderTested,
        Self::RuntimeTested,
    ];

    /// Returns the stable code, such as `no_build_for_arch`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Untested => "untested",
            Self::NoBuild => "no_build",
            Self::NoBuildForArch => "no_build_for_arch",
            Self::NoBuildForHost => "no_build_for_host",
            Self::PlatformNote => "platform_note",
            Self::ArtifactBlocked => "artifact_blocked",
            Self::FixtureOnly => "fixture_only",
            Self::ProviderPlanned => "provider_planned",
            Self::PackageInvalid => "package_invalid",
            Self::RequiresNewerOs => "requires_newer_os",
            Self::ProviderTested => "provider_tested",
            Self::RuntimeTested => "runtime_tested",
        }
    }
}

/// The state of a whole catalog entry for this Mac.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EntryState {
    /// At least one version can be installed on this Mac.
    Available,
    /// Every artifact for this Mac is blocked.
    Blocked,
    /// No version can be installed on this Mac.
    Unsupported,
    /// Planned, without a package.
    Planned,
    /// The package failed validation.
    Invalid,
}

impl EntryState {
    /// Every state.
    pub const ALL: [Self; 5] = [
        Self::Available,
        Self::Blocked,
        Self::Unsupported,
        Self::Planned,
        Self::Invalid,
    ];

    /// Returns the stable code, such as `available`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Blocked => "blocked",
            Self::Unsupported => "unsupported",
            Self::Planned => "planned",
            Self::Invalid => "invalid",
        }
    }
}

/// An entry's state with why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryStatus {
    /// The state.
    pub state: EntryState,
    /// Why, empty for `available`.
    pub reason: Option<SupportReason>,
    /// A note that goes with the reason, such as a blocked reason.
    pub note: Option<String>,
}

/// The status of one artifact on this Mac.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellStatus {
    /// The status.
    pub status: SupportStatus,
    /// Why.
    pub reason: Option<SupportReason>,
    /// A note that goes with the reason.
    pub note: Option<String>,
}

/// One cell of the version matrix: a version on an OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSupport {
    /// The OS of the column.
    pub os: Os,
    /// The architectures of the artifacts of this version for the OS.
    pub arches: Vec<Arch>,
    /// The status.
    pub status: SupportStatus,
    /// Why.
    pub reason: Option<SupportReason>,
    /// A note that goes with the reason.
    pub note: Option<String>,
}

/// Returns the state of an entry for `host`: from the install gate over
/// every version of a provider entry (spec 0017, *Aturan status*).
#[must_use]
pub fn entry_status(entry: &CatalogEntry, host: &HostFacts) -> EntryStatus {
    let provider = match entry {
        CatalogEntry::Planned(_) => {
            return EntryStatus {
                state: EntryState::Planned,
                reason: Some(SupportReason::ProviderPlanned),
                note: None,
            };
        }
        CatalogEntry::Invalid(_) => {
            return EntryStatus {
                state: EntryState::Invalid,
                reason: Some(SupportReason::PackageInvalid),
                note: None,
            };
        }
        CatalogEntry::Provider(provider) => provider,
    };
    let mut first_refusal = None;
    for version in provider.versions() {
        match installability(entry, version, host) {
            Ok(Installability::Installable { .. }) => {
                return EntryStatus {
                    state: EntryState::Available,
                    reason: None,
                    note: None,
                };
            }
            Ok(Installability::NotInstallable { reason, .. }) => {
                first_refusal.get_or_insert(reason);
            }
            Err(_) => {}
        }
    }
    let blocked = provider
        .artifacts()
        .iter()
        .find_map(|artifact| match &artifact.status {
            ArtifactStatus::Blocked { reason, .. } if artifact.target == host.target => {
                Some(reason.clone())
            }
            _ => None,
        });
    if let Some(note) = blocked {
        return EntryStatus {
            state: EntryState::Blocked,
            reason: Some(SupportReason::ArtifactBlocked),
            note: Some(note),
        };
    }
    EntryStatus {
        state: EntryState::Unsupported,
        reason: Some(first_refusal.map_or(
            SupportReason::NoBuildForHost,
            NotInstallableReason::support_reason,
        )),
        note: None,
    }
}

/// Returns the three cells of one version row, in [`MATRIX_OSES`] order.
#[must_use]
pub fn version_support(
    entry: &ProviderEntry,
    version: &ExactVersion,
    host: &HostFacts,
) -> Vec<TargetSupport> {
    MATRIX_OSES
        .into_iter()
        .map(|os| cell(entry, version, os, host))
        .collect()
}

/// The status of an artifact for the host OS (spec 0017, *Aturan status*
/// step 3), from [`support_status`] over the artifact's compatibility
/// records.
fn host_cell_status(
    entry: &ProviderEntry,
    artifact: &VersionArtifact,
    host: &HostFacts,
) -> CellStatus {
    let verdict = support_status(
        artifact,
        artifact.compatibility(),
        Some(entry.package.key()),
        host,
    );
    let note = match &artifact.status {
        ArtifactStatus::Blocked { reason, .. } => Some(reason.clone()),
        _ => None,
    };
    CellStatus {
        status: verdict.status,
        reason: Some(verdict.reason),
        note,
    }
}

fn cell(entry: &ProviderEntry, version: &ExactVersion, os: Os, host: &HostFacts) -> TargetSupport {
    let artifacts: Vec<&VersionArtifact> = entry
        .artifacts()
        .iter()
        .filter(|artifact| artifact.version == *version && artifact.target.os() == os)
        .collect();
    let arches: Vec<Arch> = artifacts
        .iter()
        .map(|artifact| artifact.target.arch())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let support = |status, reason, note| TargetSupport {
        os,
        arches: arches.clone(),
        status,
        reason: Some(reason),
        note,
    };
    if artifacts.is_empty() {
        return match entry.notes.iter().find(|note| note.os == os) {
            Some(note) => support(
                note.status,
                SupportReason::PlatformNote,
                Some(note.note.clone()),
            ),
            None => support(SupportStatus::Unsupported, SupportReason::NoBuild, None),
        };
    }
    if os == host.target.os() {
        return match artifacts
            .iter()
            .find(|artifact| artifact.target.arch() == host.target.arch())
        {
            Some(artifact) => {
                let cell = host_cell_status(entry, artifact, host);
                TargetSupport {
                    os,
                    arches,
                    status: cell.status,
                    reason: cell.reason,
                    note: cell.note,
                }
            }
            None => support(
                SupportStatus::Unsupported,
                SupportReason::NoBuildForArch,
                None,
            ),
        };
    }
    let available = artifacts
        .iter()
        .any(|artifact| matches!(artifact.status, ArtifactStatus::Available { .. }));
    let fixture = artifacts
        .iter()
        .any(|artifact| matches!(artifact.status, ArtifactStatus::Fixture { .. }));
    if available {
        support(SupportStatus::Unverified, SupportReason::Untested, None)
    } else if fixture {
        support(SupportStatus::Unverified, SupportReason::FixtureOnly, None)
    } else {
        let note = artifacts
            .iter()
            .find_map(|artifact| match &artifact.status {
                ArtifactStatus::Blocked { reason, .. } => Some(reason.clone()),
                _ => None,
            });
        support(SupportStatus::Blocked, SupportReason::ArtifactBlocked, note)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;

    use stackly_domain::ProviderName;

    use super::super::model::{InvalidEntry, PlannedEntry, PlatformNote, TileHint};
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

    fn tile() -> TileHint {
        TileHint {
            mark: "Na".into(),
            tile: TileColor::Gray,
        }
    }

    fn repo_files(name: &str) -> BTreeMap<String, Vec<u8>> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../providers")
            .join(name);
        let mut files = BTreeMap::new();
        for path in [
            "provider.yaml",
            "versions.json",
            "schemas/settings.schema.json",
        ] {
            files.insert(path.to_owned(), std::fs::read(dir.join(path)).unwrap());
        }
        for entry in std::fs::read_dir(dir.join("templates")).unwrap() {
            let entry = entry.unwrap();
            let file = entry.file_name().into_string().unwrap();
            files.insert(
                format!("templates/{file}"),
                std::fs::read(entry.path()).unwrap(),
            );
        }
        files
    }

    fn entry(package: ValidatedPackage, notes: Vec<PlatformNote>) -> ProviderEntry {
        ProviderEntry {
            package: Arc::new(package),
            tile: tile(),
            notes,
        }
    }

    fn repo(name: &str) -> ValidatedPackage {
        ValidatedPackage::from_files(
            PackageFiles::new(name, repo_files(name)),
            NameCheck::FolderName,
        )
        .unwrap()
    }

    /// The NATS package with Linux artifacts for both architectures, one
    /// available and one blocked.
    fn linux_mixed() -> ValidatedPackage {
        let mut files = repo_files("nats");
        let manifest = String::from_utf8(files["provider.yaml"].clone()).unwrap().replace(
            "    - { os: macos, arch: aarch64 }",
            "    - { os: macos, arch: aarch64 }\n    - { os: linux, arch: x86_64 }\n    - { os: linux, arch: aarch64 }",
        );
        let versions = String::from_utf8(files["versions.json"].clone()).unwrap();
        let available = versions
            .split_once("\"artifacts\": [")
            .unwrap()
            .1
            .rsplit_once(']')
            .unwrap()
            .0
            .trim()
            .to_owned();
        let linux = available
            .replace("nats-2.15.1-macos-aarch64", "nats-2.15.1-linux-x86_64")
            .replace("\"os\": \"macos\"", "\"os\": \"linux\"")
            .replace("\"arch\": \"aarch64\"", "\"arch\": \"x86_64\"");
        let blocked = r#"{ "id": "nats-2.15.1-linux-aarch64", "line": "2.15", "version": "2.15.1", "os": "linux", "arch": "aarch64", "status": "blocked", "blocked": { "reason": "no arm64 build upstream", "owner": "runtime maintainer", "since": "2026-10-01" } }"#;
        let rebuilt = format!(
            "{{ \"schema\": \"stackly.provider-versions/v1\", \"software\": \"nats\", \"artifacts\": [{available}, {linux}, {blocked}] }}"
        );
        files.insert("provider.yaml".into(), manifest.into_bytes());
        files.insert("versions.json".into(), rebuilt.into_bytes());
        ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
            .unwrap()
    }

    fn windows_planned() -> PlatformNote {
        PlatformNote {
            os: Os::Windows,
            status: SupportStatus::Planned,
            note: "Windows releases exist upstream.".into(),
        }
    }

    /// One matrix cell as a tuple: OS, arches, status, reason, note.
    type Cell = (
        Os,
        Vec<Arch>,
        SupportStatus,
        Option<SupportReason>,
        Option<String>,
    );

    fn cells(entry: &ProviderEntry) -> Vec<Cell> {
        let version = entry.versions()[0].clone();
        version_support(entry, &version, &host())
            .into_iter()
            .map(|cell| (cell.os, cell.arches, cell.status, cell.reason, cell.note))
            .collect()
    }

    fn status_of(entry: ProviderEntry) -> EntryStatus {
        entry_status(&CatalogEntry::Provider(entry), &host())
    }

    #[test]
    fn an_available_host_artifact_takes_its_support_from_its_records_and_notes_fill_empty_columns()
    {
        let nats = entry(repo("nats"), vec![windows_planned()]);

        assert_eq!(status_of(nats.clone()).state, EntryState::Available);
        assert_eq!(
            cells(&nats),
            [
                (
                    Os::Macos,
                    vec![Arch::Aarch64],
                    SupportStatus::Supported,
                    Some(SupportReason::ProviderTested),
                    None
                ),
                (
                    Os::Linux,
                    vec![],
                    SupportStatus::Unsupported,
                    Some(SupportReason::NoBuild),
                    None
                ),
                (
                    Os::Windows,
                    vec![],
                    SupportStatus::Planned,
                    Some(SupportReason::PlatformNote),
                    Some("Windows releases exist upstream.".into())
                ),
            ]
        );
    }

    #[test]
    fn a_provider_for_another_platform_is_unsupported_with_its_reason() {
        let linux = entry(CatalogFixture::LinuxOnly.package(), vec![]);

        assert_eq!(
            status_of(linux.clone()),
            EntryStatus {
                state: EntryState::Unsupported,
                reason: Some(SupportReason::NoBuildForHost),
                note: None
            }
        );
        assert_eq!(
            cells(&linux),
            [
                (
                    Os::Macos,
                    vec![],
                    SupportStatus::Unsupported,
                    Some(SupportReason::NoBuild),
                    None
                ),
                (
                    Os::Linux,
                    vec![Arch::X86_64],
                    SupportStatus::Unverified,
                    Some(SupportReason::Untested),
                    None
                ),
                (
                    Os::Windows,
                    vec![],
                    SupportStatus::Unsupported,
                    Some(SupportReason::NoBuild),
                    None
                ),
            ]
        );
    }

    #[test]
    fn another_architecture_on_the_host_os_is_no_build_for_arch() {
        let intel = entry(CatalogFixture::IntelOnly.package(), vec![]);

        assert_eq!(
            status_of(intel.clone()).reason,
            Some(SupportReason::NoBuildForArch)
        );
        assert_eq!(
            cells(&intel)[0],
            (
                Os::Macos,
                vec![Arch::X86_64],
                SupportStatus::Unsupported,
                Some(SupportReason::NoBuildForArch),
                None
            )
        );
    }

    #[test]
    fn a_blocked_host_artifact_blocks_the_entry_with_its_reason() {
        let blocked = entry(CatalogFixture::BlockedHost.package(), vec![]);
        let reason = Some("the upstream build fails the closure audit".to_owned());

        assert_eq!(
            status_of(blocked.clone()),
            EntryStatus {
                state: EntryState::Blocked,
                reason: Some(SupportReason::ArtifactBlocked),
                note: reason.clone()
            }
        );
        assert_eq!(
            cells(&blocked)[0],
            (
                Os::Macos,
                vec![Arch::Aarch64],
                SupportStatus::Blocked,
                Some(SupportReason::ArtifactBlocked),
                reason
            )
        );
    }

    #[test]
    fn a_fixture_is_unverified_and_never_available() {
        let fake = entry(repo("fake"), vec![]);

        assert_eq!(
            status_of(fake.clone()).reason,
            Some(SupportReason::FixtureOnly)
        );
        assert_eq!(status_of(fake.clone()).state, EntryState::Unsupported);
        assert_eq!(cells(&fake)[0].2, SupportStatus::Unverified);
        assert_eq!(cells(&fake)[0].3, Some(SupportReason::FixtureOnly));
    }

    #[test]
    fn another_os_column_takes_the_best_status_of_its_artifacts() {
        let mixed = entry(linux_mixed(), vec![]);

        assert_eq!(
            cells(&mixed)[1],
            (
                Os::Linux,
                vec![Arch::Aarch64, Arch::X86_64],
                SupportStatus::Unverified,
                Some(SupportReason::Untested),
                None
            )
        );
    }

    #[test]
    fn planned_and_invalid_entries_have_their_own_state() {
        let id: ProviderName = "official/mysql".parse().unwrap();
        let planned = CatalogEntry::Planned(PlannedEntry {
            id: id.clone(),
            display_name: "MySQL".into(),
            category: Category::Database,
            description: "Relational server.".into(),
            note: None,
            tile: tile(),
        });
        let invalid = CatalogEntry::Invalid(InvalidEntry {
            id,
            category: None,
            tile: tile(),
            issues: Vec::new(),
        });

        assert_eq!(entry_status(&planned, &host()).state, EntryState::Planned);
        assert_eq!(
            entry_status(&planned, &host()).reason,
            Some(SupportReason::ProviderPlanned)
        );
        assert_eq!(entry_status(&invalid, &host()).state, EntryState::Invalid);
        assert_eq!(
            entry_status(&invalid, &host()).reason,
            Some(SupportReason::PackageInvalid)
        );
    }

    #[test]
    fn a_cell_without_compatibility_records_is_never_supported_or_experimental() {
        let entries = [entry(repo("fake"), vec![])]
            .into_iter()
            .chain(CatalogFixture::ALL.map(|fixture| entry(fixture.package(), vec![])));

        for entry in entries {
            for (_, _, status, _, _) in cells(&entry) {
                assert_ne!(status, SupportStatus::Supported);
                assert_ne!(status, SupportStatus::Experimental);
            }
        }
    }

    #[test]
    fn reason_and_state_codes_are_stable_snake_case() {
        assert_eq!(SupportReason::NoBuildForArch.as_code(), "no_build_for_arch");
        assert_eq!(SupportReason::PlatformNote.as_code(), "platform_note");
        assert_eq!(EntryState::Unsupported.as_code(), "unsupported");
    }
}
