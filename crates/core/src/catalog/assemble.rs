//! Turns a catalog source and its validated packages into the index and
//! the package copies of a catalog folder (spec 0017, AC-1). `cargo xtask
//! catalog build` and the test `CatalogBuilder` both call this, so a test
//! catalog follows the same rules as the bundled one.

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use stackly_domain::{Os, ProviderName};

use crate::provider::{PackageFiles, ValidatedPackage};

use super::index::{CatalogIndex, IndexPlanned, IndexPlatformNote, content_sha256, provider_entry};
use super::rules::{
    TileColor, default_mark, index_problems, is_mark, namespace_allowed, package_limits,
    platform_note_problems,
};

/// A catalog source as written by maintainers (`providers/catalog.toml`),
/// with every value still text so the rules can name what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    /// The catalog revision.
    pub revision: u64,
    /// When the revision was published.
    pub published_at: Timestamp,
    /// Provider entries, each backed by a package.
    pub providers: Vec<SourceProvider>,
    /// Planned entries, without a package.
    pub planned: Vec<SourcePlanned>,
}

/// One `[[provider]]` of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProvider {
    /// The provider ID, such as `official/nats`.
    pub id: String,
    /// The tile mark, or the default from the display name.
    pub mark: Option<String>,
    /// The tile color code, or gray.
    pub tile: Option<String>,
    /// Notes for operating systems without an artifact.
    pub platform_notes: Vec<SourceNote>,
}

/// One `[[provider.platform_note]]` of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNote {
    /// `linux` or `windows`.
    pub os: String,
    /// `planned` or `unsupported`.
    pub status: String,
    /// The note text.
    pub note: String,
}

/// One `[[planned]]` of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePlanned {
    /// The provider ID it will have.
    pub id: String,
    /// The name on its card.
    pub display_name: String,
    /// Its category code.
    pub category: String,
    /// What it is.
    pub description: String,
    /// An optional note.
    pub note: Option<String>,
    /// The tile mark, or the default from the display name.
    pub mark: Option<String>,
    /// The tile color code, or gray.
    pub tile: Option<String>,
}

/// What `assemble` checks beyond the shared rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AssembleOptions {
    /// Accept `testing` packages. Only test catalogs set it.
    pub allow_testing: bool,
    /// Reject a `published_at` after this instant (the build clock).
    pub not_after: Option<Timestamp>,
}

/// The index and package copies of a catalog folder.
#[derive(Debug, Clone)]
pub struct AssembledCatalog {
    /// The `catalog.json` to write.
    pub index: CatalogIndex,
    /// Each provider ID with the files to copy to `packages/<id>/`.
    pub packages: Vec<(String, PackageFiles)>,
}

/// Builds the index of a catalog from its source and the validated
/// packages of its provider entries, keyed by provider ID.
///
/// # Errors
///
/// Returns every problem as one lowercase line: a namespace outside
/// `official`, a provider entry without its package or with another ID, an
/// ID that repeats, a mark, tile, or note outside its rules, a platform
/// note for an OS the package has artifacts for, an artifact `minOs` above
/// the ceiling, a broken catalog limit, a publish time after `not_after`,
/// or an index rule (spec 0017 AC-1).
pub fn assemble(
    spec: &SourceSpec,
    packages: &BTreeMap<String, ValidatedPackage>,
    options: AssembleOptions,
) -> Result<AssembledCatalog, Vec<String>> {
    let mut problems = Vec::new();
    if spec.revision == 0 || spec.revision > (1 << 53) - 1 {
        problems.push("revision must be 1 to 9007199254740991".to_owned());
    }
    if let Some(now) = options.not_after
        && spec.published_at > now
    {
        problems.push(format!(
            "published_at {} is in the future",
            spec.published_at
        ));
    }
    let mut seen = BTreeSet::new();
    let mut providers = Vec::new();
    let mut copies = Vec::new();
    for source in &spec.providers {
        let id = &source.id;
        if !seen.insert(id.as_str()) {
            problems.push(format!("{id} appears more than once"));
            continue;
        }
        match id.parse::<ProviderName>() {
            Ok(name) if namespace_allowed(&name, options.allow_testing) => {}
            Ok(_) => {
                problems.push(format!("{id} is not in the official namespace"));
                continue;
            }
            Err(_) => {
                problems.push(format!("{id} is not a provider ID"));
                continue;
            }
        }
        let Some(package) = packages.get(id) else {
            problems.push(format!("{id} has no valid package"));
            continue;
        };
        if package.key().provider().as_str() != id {
            problems.push(format!("{id}: metadata.id is {}", package.key().provider()));
            continue;
        }
        let mark = source
            .mark
            .clone()
            .unwrap_or_else(|| default_mark(package.manifest().display_name()));
        let tile = tile_of(id, source.tile.as_deref(), &mut problems);
        let notes = notes_of(id, &source.platform_notes, package, &mut problems);
        if !is_mark(&mark) {
            problems.push(format!("{id}: mark must be 1 or 2 letters or digits"));
        }
        for violation in package_limits(package) {
            problems.push(format!(
                "{id}: {} {}: {}",
                violation.file, violation.location, violation.message
            ));
        }
        providers.push(provider_entry(package, mark, tile, notes));
        copies.push((id.clone(), package.files().clone()));
    }
    let mut planned = Vec::new();
    for source in &spec.planned {
        let id = &source.id;
        if !seen.insert(id.as_str()) {
            problems.push(format!("{id} appears more than once"));
            continue;
        }
        let tile = tile_of(id, source.tile.as_deref(), &mut problems);
        planned.push(IndexPlanned {
            id: id.clone(),
            display_name: source.display_name.clone(),
            category: source.category.clone(),
            description: source.description.clone(),
            note: source.note.clone(),
            mark: source
                .mark
                .clone()
                .unwrap_or_else(|| default_mark(&source.display_name)),
            tile,
        });
    }
    let index = CatalogIndex::new(spec.revision, spec.published_at, providers, planned);
    problems.extend(index_problems(&index));
    if problems.is_empty() {
        copies.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(AssembledCatalog {
            index,
            packages: copies,
        })
    } else {
        problems.sort();
        problems.dedup();
        Err(problems)
    }
}

/// Recomputes the content digest an index should carry.
#[must_use]
pub fn expected_content_sha256(index: &CatalogIndex) -> String {
    content_sha256(&index.providers, &index.planned)
}

fn tile_of(id: &str, code: Option<&str>, problems: &mut Vec<String>) -> TileColor {
    match code {
        None => TileColor::Gray,
        Some(code) => TileColor::from_code(code).unwrap_or_else(|| {
            problems.push(format!("{id}: tile {code} is not a tile color"));
            TileColor::Gray
        }),
    }
}

fn notes_of(
    id: &str,
    sources: &[SourceNote],
    package: &ValidatedPackage,
    problems: &mut Vec<String>,
) -> Vec<IndexPlatformNote> {
    let artifact_oses: BTreeSet<Os> = package
        .versions()
        .artifacts()
        .iter()
        .map(|artifact| artifact.target.os())
        .collect();
    let mut notes = Vec::new();
    for source in sources {
        let Ok(os) = source.os.parse::<Os>() else {
            problems.push(format!("{id}: platform note os {} is not an os", source.os));
            continue;
        };
        if artifact_oses.contains(&os) {
            problems.push(format!("{id}: platform note for {os}, which has artifacts"));
        }
        notes.push(IndexPlatformNote {
            os,
            status: source.status.clone(),
            note: source.note.clone(),
        });
    }
    for problem in platform_note_problems(&notes) {
        problems.push(format!("{id}: {problem}"));
    }
    notes.sort_by_key(|note| note.os);
    notes
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::provider::NameCheck;

    use super::*;

    /// Reads a repo package into memory the way `read_package` would.
    fn package(name: &str) -> ValidatedPackage {
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
        ValidatedPackage::from_files(PackageFiles::new(name, files), NameCheck::FolderName).unwrap()
    }

    fn packages(list: &[(&str, &str)]) -> BTreeMap<String, ValidatedPackage> {
        list.iter()
            .map(|(id, folder)| ((*id).to_owned(), package(folder)))
            .collect()
    }

    fn provider(id: &str) -> SourceProvider {
        SourceProvider {
            id: id.to_owned(),
            mark: None,
            tile: None,
            platform_notes: Vec::new(),
        }
    }

    fn planned(id: &str) -> SourcePlanned {
        SourcePlanned {
            id: id.to_owned(),
            display_name: "MySQL".into(),
            category: "database".into(),
            description: "Relational server.".into(),
            note: None,
            mark: Some("My".into()),
            tile: Some("teal".into()),
        }
    }

    fn spec(providers: Vec<SourceProvider>, planned: Vec<SourcePlanned>) -> SourceSpec {
        SourceSpec {
            revision: 1,
            published_at: "2026-10-19T14:05:00Z".parse().unwrap(),
            providers,
            planned,
        }
    }

    #[test]
    fn a_provider_entry_carries_its_digests_and_default_tile() {
        let nats = packages(&[("official/nats", "nats")]);

        let catalog = assemble(
            &spec(
                vec![provider("official/nats")],
                vec![planned("official/mysql")],
            ),
            &nats,
            AssembleOptions::default(),
        )
        .unwrap();

        let entry = &catalog.index.providers[0];
        let package = &nats["official/nats"];
        assert_eq!(entry.id, "official/nats");
        assert_eq!(
            entry.manifest_sha256,
            package.key().manifest_sha256().to_string()
        );
        assert_eq!(entry.versions_sha256.len(), 64);
        assert_eq!(entry.mark, "Na");
        assert_eq!(entry.tile, TileColor::Gray);
        assert_eq!(catalog.index.planned[0].tile, TileColor::Teal);
        assert_eq!(catalog.packages[0].0, "official/nats");
        assert_eq!(
            catalog.index.content_sha256,
            expected_content_sha256(&catalog.index)
        );
    }

    #[test]
    fn only_official_packages_enter_unless_tests_allow_testing() {
        let fake = packages(&[("testing/fake", "fake")]);
        let source = spec(vec![provider("testing/fake")], vec![]);

        assert_eq!(
            assemble(&source, &fake, AssembleOptions::default()).unwrap_err(),
            ["testing/fake is not in the official namespace"]
        );
        assert!(
            assemble(
                &source,
                &fake,
                AssembleOptions {
                    allow_testing: true,
                    ..AssembleOptions::default()
                }
            )
            .is_ok()
        );
        assert_eq!(
            assemble(
                &spec(vec![provider("community/x")], vec![]),
                &fake,
                AssembleOptions::default()
            )
            .unwrap_err(),
            ["community/x is not in the official namespace"]
        );
    }

    #[test]
    fn a_missing_package_or_another_id_is_refused() {
        let nats = packages(&[("official/nats", "nats"), ("official/other", "nats")]);

        assert_eq!(
            assemble(
                &spec(vec![provider("official/mysql")], vec![]),
                &nats,
                AssembleOptions::default()
            )
            .unwrap_err(),
            ["official/mysql has no valid package"]
        );
        assert_eq!(
            assemble(
                &spec(vec![provider("official/other")], vec![]),
                &nats,
                AssembleOptions::default()
            )
            .unwrap_err(),
            ["official/other: metadata.id is official/nats"]
        );
    }

    #[test]
    fn platform_notes_only_cover_operating_systems_without_artifacts() {
        let nats = packages(&[("official/nats", "nats")]);
        let mut with_notes = provider("official/nats");
        with_notes.platform_notes = vec![
            SourceNote {
                os: "windows".into(),
                status: "planned".into(),
                note: "Windows releases exist upstream.".into(),
            },
            SourceNote {
                os: "macos".into(),
                status: "planned".into(),
                note: "x".into(),
            },
        ];

        let problems = assemble(
            &spec(vec![with_notes], vec![]),
            &nats,
            AssembleOptions::default(),
        )
        .unwrap_err();

        assert_eq!(
            problems,
            [
                "official/nats: platform note for macos, which has artifacts",
                "official/nats: platform notes are only for linux and windows"
            ]
        );
    }

    #[test]
    fn a_future_publish_time_repeats_and_bad_tiles_are_refused() {
        let none = BTreeMap::new();
        let mut bad_tile = planned("official/valkey");
        bad_tile.tile = Some("purple".into());

        let problems = assemble(
            &spec(
                vec![],
                vec![
                    planned("official/mysql"),
                    planned("official/mysql"),
                    bad_tile,
                ],
            ),
            &none,
            AssembleOptions {
                not_after: Some("2026-01-01T00:00:00Z".parse().unwrap()),
                ..AssembleOptions::default()
            },
        )
        .unwrap_err();

        assert_eq!(
            problems,
            [
                "official/mysql appears more than once",
                "official/valkey: tile purple is not a tile color",
                "published_at 2026-10-19T14:05:00Z is in the future",
            ]
        );
    }
}
