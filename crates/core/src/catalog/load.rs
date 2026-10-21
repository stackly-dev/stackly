//! `load_catalog`: reads a catalog folder through the `Filesystem` port
//! into the read model (spec 0017, AC-2 and AC-3). It never writes a file,
//! runs a process, touches the network, or resolves a secret.

use std::sync::Arc;

use stackly_domain::{ProviderName, Sha256Digest, SupportStatus};
use stackly_platform::{Clock, FileKind, Filesystem, PlatformError};

use crate::provider::{
    Category, Issue, IssueCode, Location, NameCheck, PackageFiles, ValidatedPackage,
    manifest_software, read_package,
};

use super::index::{
    CATALOG_INDEX_FILE, CatalogIndex, IndexError, IndexProvider, package_dir, versions_sha256,
};
use super::model::{
    Catalog, CatalogEntry, CatalogLocation, CatalogMeta, CatalogOptions, CatalogState,
    InvalidEntry, PlannedEntry, PlatformNote, ProviderEntry, TileHint, UnavailableReason,
};
use super::rules::{
    INDEX_MAX_LEN, LimitKind, PACKAGE_BYTES_MAX, category_from_code, index_problems,
    namespace_allowed, package_limits,
};

/// Loads the catalog in `location`.
///
/// The index is read strictly and checked as a whole; each provider entry
/// is then read and validated on its own, so a broken package becomes an
/// [`InvalidEntry`] while the others load (spec 0017 AC-3).
#[must_use]
pub fn load_catalog(
    fs: &dyn Filesystem,
    clock: &dyn Clock,
    location: CatalogLocation,
    options: CatalogOptions,
) -> CatalogState {
    load_with_budget(fs, clock, location, options, PACKAGE_BYTES_MAX)
}

/// [`load_catalog`] with the package byte budget as a parameter, so a test
/// can reach it without 16 MiB of packages.
fn load_with_budget(
    fs: &dyn Filesystem,
    clock: &dyn Clock,
    location: CatalogLocation,
    options: CatalogOptions,
    budget: usize,
) -> CatalogState {
    let dir = location.path().clone();
    let index_path = dir.join(CATALOG_INDEX_FILE);
    let unavailable = |reason, detail: String| CatalogState::Unavailable { reason, detail };
    match fs.symlink_metadata(&index_path) {
        Ok(None) => {
            return unavailable(UnavailableReason::Missing, "catalog.json is missing".into());
        }
        Ok(Some(info)) if info.kind != FileKind::File => {
            return unavailable(
                UnavailableReason::IndexInvalid,
                "catalog.json is not a regular file".into(),
            );
        }
        Ok(Some(_)) => {}
        Err(err) => {
            return unavailable(
                UnavailableReason::IndexInvalid,
                format!("catalog.json cannot be read: {}", short(&err)),
            );
        }
    }
    let bytes = match fs.read_file_limited(&index_path, INDEX_MAX_LEN as u64) {
        Ok(bytes) => bytes,
        Err(PlatformError::FileTooLarge { .. }) => {
            return unavailable(
                UnavailableReason::IndexInvalid,
                "catalog.json is larger than 256 KiB".into(),
            );
        }
        Err(err) => {
            return unavailable(
                UnavailableReason::IndexInvalid,
                format!("catalog.json cannot be read: {}", short(&err)),
            );
        }
    };
    let index = match CatalogIndex::from_json(&bytes) {
        Ok(index) => index,
        Err(IndexError::UnsupportedSchema(schema)) => {
            return unavailable(
                UnavailableReason::UnsupportedSchema,
                format!("catalog schema {schema} is not supported"),
            );
        }
        Err(IndexError::Invalid(_)) => {
            return unavailable(
                UnavailableReason::IndexInvalid,
                "catalog.json does not decode as a catalog index".into(),
            );
        }
    };
    if let Some(problem) = index_problems(&index).into_iter().next() {
        return unavailable(UnavailableReason::IndexInvalid, problem);
    }
    let Ok(content_sha256) = index.content_sha256.parse::<Sha256Digest>() else {
        return unavailable(
            UnavailableReason::IndexInvalid,
            "contentSha256 is not 64 lowercase hex characters".into(),
        );
    };
    let mut entries = Vec::new();
    let mut budget = Budget {
        max: budget,
        used: 0,
        exceeded: false,
    };
    for (position, provider) in index.providers.iter().enumerate() {
        entries.push(load_provider(
            fs,
            &dir,
            position,
            provider,
            options,
            &mut budget,
        ));
    }
    for planned in &index.planned {
        // `index_problems` already checked the ID, category, and fields.
        let (Ok(id), Some(category)) = (
            planned.id.parse::<ProviderName>(),
            category_from_code(&planned.category),
        ) else {
            continue;
        };
        entries.push(CatalogEntry::Planned(PlannedEntry {
            id,
            display_name: planned.display_name.clone(),
            category,
            description: planned.description.clone(),
            note: planned.note.clone(),
            tile: TileHint {
                mark: planned.mark.clone(),
                tile: planned.tile,
            },
        }));
    }
    CatalogState::Loaded(Arc::new(Catalog::new(
        CatalogMeta {
            revision: index.revision,
            published_at: index.published_at,
            content_sha256,
            location,
            loaded_at: clock.now(),
        },
        entries,
    )))
}

/// The package bytes a catalog may hold together, and how many it holds.
struct Budget {
    max: usize,
    used: usize,
    exceeded: bool,
}

/// Reads and checks one provider entry; any failure gives an invalid entry.
fn load_provider(
    fs: &dyn Filesystem,
    dir: &std::path::Path,
    position: usize,
    provider: &IndexProvider,
    options: CatalogOptions,
    budget: &mut Budget,
) -> CatalogEntry {
    let tile = TileHint {
        mark: provider.mark.clone(),
        tile: provider.tile,
    };
    let id: ProviderName = provider
        .id
        .parse()
        .expect("index_problems checks that every provider ID parses before entries load");
    let invalid = |category, issues| {
        CatalogEntry::Invalid(InvalidEntry {
            id: id.clone(),
            category,
            tile: tile.clone(),
            issues,
        })
    };
    let at = |field: &str| Location::Field(format!("providers[{position}].{field}"));
    if !namespace_allowed(&id, options.allow_testing_namespace) {
        return invalid(
            None,
            vec![Issue::new(
                CATALOG_INDEX_FILE,
                at("id"),
                IssueCode::NamespaceNotAllowed,
                format!(
                    "namespace {} is not allowed in this catalog",
                    id.namespace()
                ),
            )],
        );
    }
    let files = match read_package(fs, &dir.join(package_dir(id.as_str()))) {
        Ok(files) => files,
        Err(err) => return invalid(None, err.issues),
    };
    let category = metadata_category(&files);
    let package_bytes: usize = files.iter().map(|(_, bytes)| bytes.len()).sum();
    budget.used = budget.used.saturating_add(package_bytes);
    if budget.exceeded || budget.used > budget.max {
        budget.exceeded = true;
        return invalid(
            category,
            vec![
                Issue::new(
                    CATALOG_INDEX_FILE,
                    Location::Field(format!("providers[{position}]")),
                    IssueCode::CatalogLimit,
                    "the catalog packages are larger than 16 MiB together",
                )
                .with_reason(LimitKind::TotalBytes.as_code()),
            ],
        );
    }
    let versions_digest = versions_sha256(&files);
    let package = match ValidatedPackage::from_files(files, NameCheck::FolderName) {
        Ok(package) => package,
        Err(err) => return invalid(category, err.issues),
    };
    let mut issues = Vec::new();
    if package.key().provider() != &id {
        issues.push(Issue::new(
            "provider.yaml",
            Location::Field("metadata.id".into()),
            IssueCode::CatalogIdMismatch,
            format!(
                "metadata.id is {}, the index says {id}",
                package.key().provider()
            ),
        ));
    }
    if package.key().manifest_sha256().to_string() != provider.manifest_sha256 {
        issues.push(Issue::new(
            CATALOG_INDEX_FILE,
            at("manifestSha256"),
            IssueCode::CatalogDigestMismatch,
            "manifestSha256 does not match the package files",
        ));
    }
    if versions_digest.as_deref() != Some(provider.versions_sha256.as_str()) {
        issues.push(Issue::new(
            CATALOG_INDEX_FILE,
            at("versionsSha256"),
            IssueCode::CatalogDigestMismatch,
            "versionsSha256 does not match versions.json",
        ));
    }
    for violation in package_limits(&package) {
        issues.push(
            Issue::new(
                violation.file,
                Location::Field(violation.location),
                IssueCode::CatalogLimit,
                violation.message,
            )
            .with_reason(violation.kind.as_code()),
        );
    }
    if !issues.is_empty() {
        return invalid(category, issues);
    }
    let notes = provider
        .platform_notes
        .iter()
        .filter_map(|note| {
            let status = match note.status.as_str() {
                "planned" => SupportStatus::Planned,
                "unsupported" => SupportStatus::Unsupported,
                // `index_problems` already refused other statuses.
                _ => return None,
            };
            Some(PlatformNote {
                os: note.os,
                status,
                note: note.note.clone(),
            })
        })
        .collect();
    CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package),
        tile,
        notes,
    })
}

/// The category of a package whose `metadata` decodes strictly, the way
/// `manifest_software` reads it (spec 0009), for an invalid entry.
fn metadata_category(files: &PackageFiles) -> Option<Category> {
    let text = std::str::from_utf8(files.get("provider.yaml")?).ok()?;
    manifest_software(text).ok()?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).ok()?;
    category_from_code(value.get("metadata")?.get("category")?.as_str()?)
}

/// A short, safe description of a platform error, without paths.
fn short(err: &PlatformError) -> &'static str {
    match err {
        PlatformError::PermissionDenied { .. } => "permission denied",
        PlatformError::NotARegularFile { .. } => "not a regular file",
        PlatformError::NotFound { .. } => "not found",
        _ => "an os error",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use jiff::{SignedDuration, Timestamp};
    use stackly_testkit::{FakePlatform, TestRoot};

    use super::super::index::content_sha256;
    use super::super::model::CATALOG_OUTDATED_AFTER;
    use super::super::testing::{CatalogBuilder, CatalogFixture};
    use super::*;

    /// A repo package read into memory with std (it lives outside the test
    /// root, where the fake filesystem may not read).
    fn repo_package(name: &str) -> PackageFiles {
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
        PackageFiles::new(name, files)
    }

    struct Setup {
        _root: TestRoot,
        fake: FakePlatform,
        dir: PathBuf,
    }

    fn setup(builder: &CatalogBuilder) -> Setup {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let dir = root.path().join("catalog");
        builder.write(fake.fs(), &dir).unwrap();
        Setup {
            _root: root,
            fake,
            dir,
        }
    }

    fn load(setup: &Setup, options: CatalogOptions) -> CatalogState {
        load_catalog(
            setup.fake.fs(),
            setup.fake.clock(),
            CatalogLocation::Development(setup.dir.clone()),
            options,
        )
    }

    fn loaded(state: CatalogState) -> Arc<Catalog> {
        match state {
            CatalogState::Loaded(catalog) => catalog,
            other => panic!("not loaded: {other:?}"),
        }
    }

    fn unavailable(state: CatalogState) -> (UnavailableReason, String) {
        match state {
            CatalogState::Unavailable { reason, detail } => (reason, detail),
            other => panic!("not unavailable: {other:?}"),
        }
    }

    fn invalid_codes(
        catalog: &Catalog,
        id: &str,
    ) -> Vec<(String, String, IssueCode, Option<&'static str>)> {
        match catalog.entry(&id.parse().unwrap()) {
            Some(CatalogEntry::Invalid(entry)) => entry
                .issues
                .iter()
                .map(|issue| {
                    (
                        issue.file.clone(),
                        issue.location.to_string(),
                        issue.code,
                        issue.reason,
                    )
                })
                .collect(),
            other => panic!("{id} is not invalid: {other:?}"),
        }
    }

    fn base() -> CatalogBuilder {
        CatalogBuilder::new()
            .package("official/nats", repo_package("nats"))
            .planned("official/mysql", "MySQL", "database", "Relational server.")
            .planned(
                "official/clickhouse",
                "ClickHouse",
                "analytics",
                "Columnar database.",
            )
    }

    #[test]
    fn loading_reads_through_the_port_without_writing_or_spawning() {
        let setup = setup(&base());
        let before = setup.fake.fs().mutations().len();
        let now: Timestamp = "2026-10-20T08:00:00Z".parse().unwrap();
        setup.fake.clock().set(now);

        let catalog = loaded(load(&setup, CatalogOptions::default()));

        assert_eq!(setup.fake.fs().mutations().len(), before);
        assert_eq!(setup.fake.fs().violations(), 0);
        assert!(setup.fake.processes().spawns().is_empty());
        assert_eq!(catalog.meta().revision, 1);
        assert_eq!(catalog.meta().loaded_at, now);
        assert_eq!(
            catalog.meta().published_at.to_string(),
            "2026-10-01T00:00:00Z"
        );
        assert_eq!(catalog.invalid_count(), 0);
        let ids: Vec<String> = catalog
            .entries()
            .iter()
            .map(|e| e.id().to_string())
            .collect();
        assert_eq!(
            ids,
            ["official/nats", "official/mysql", "official/clickhouse"]
        );
        let CatalogEntry::Provider(nats) = &catalog.entries()[0] else {
            panic!("nats is a provider entry");
        };
        assert_eq!(nats.tile.mark, "Na");
        assert_eq!(nats.versions().len(), 1);
    }

    #[test]
    fn a_broken_package_becomes_one_invalid_entry_and_the_rest_loads() {
        let setup = setup(&base().fixture(CatalogFixture::LinuxOnly));
        let template = setup
            .dir
            .join("packages/official/nats/templates/nats.conf.tmpl");
        let mut bytes = std::fs::read(&template).unwrap();
        bytes.extend_from_slice(b"# changed\n");
        std::fs::write(&template, bytes).unwrap();

        let catalog = loaded(load(&setup, CatalogOptions::default()));

        assert_eq!(
            invalid_codes(&catalog, "official/nats"),
            [(
                "catalog.json".to_owned(),
                "providers[1].manifestSha256".to_owned(),
                IssueCode::CatalogDigestMismatch,
                None
            )]
        );
        assert!(matches!(
            catalog.entry(&"official/linuxonly".parse().unwrap()),
            Some(CatalogEntry::Provider(_))
        ));
        assert_eq!(catalog.invalid_count(), 1);
        let CatalogEntry::Invalid(entry) =
            catalog.entry(&"official/nats".parse().unwrap()).unwrap()
        else {
            panic!("nats is invalid");
        };
        assert_eq!(entry.category, Some(Category::Messaging));
    }

    #[test]
    fn a_changed_versions_file_is_a_digest_mismatch() {
        let setup = setup(&base());
        let versions = setup.dir.join("packages/official/nats/versions.json");
        let text = std::fs::read_to_string(&versions).unwrap();
        std::fs::write(&versions, text.replace("6785953", "6785954")).unwrap();

        let catalog = loaded(load(&setup, CatalogOptions::default()));

        assert_eq!(
            invalid_codes(&catalog, "official/nats"),
            [(
                "catalog.json".to_owned(),
                "providers[0].versionsSha256".to_owned(),
                IssueCode::CatalogDigestMismatch,
                None
            )]
        );
    }

    #[test]
    fn testing_packages_load_only_when_tests_allow_them() {
        let setup = setup(
            &CatalogBuilder::new()
                .allow_testing()
                .package("testing/fake", repo_package("fake")),
        );

        let refused = loaded(load(&setup, CatalogOptions::default()));
        let allowed = loaded(load(
            &setup,
            CatalogOptions {
                allow_testing_namespace: true,
            },
        ));

        assert_eq!(
            invalid_codes(&refused, "testing/fake"),
            [(
                "catalog.json".to_owned(),
                "providers[0].id".to_owned(),
                IssueCode::NamespaceNotAllowed,
                None
            )]
        );
        assert!(matches!(allowed.entries()[0], CatalogEntry::Provider(_)));
    }

    #[test]
    fn a_package_with_another_namespace_is_an_id_mismatch() {
        let setup = setup(&CatalogBuilder::new().fixture(CatalogFixture::LinuxOnly));
        let manifest = setup.dir.join("packages/official/linuxonly/provider.yaml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(
            &manifest,
            text.replace("id: official/linuxonly", "id: community/linuxonly"),
        )
        .unwrap();

        let catalog = loaded(load(&setup, CatalogOptions::default()));

        let codes = invalid_codes(&catalog, "official/linuxonly");
        assert!(
            codes.contains(&(
                "provider.yaml".to_owned(),
                "metadata.id".to_owned(),
                IssueCode::CatalogIdMismatch,
                None
            )),
            "{codes:?}"
        );
    }

    #[test]
    fn a_package_over_a_catalog_limit_names_the_limit_in_its_reason() {
        let setup = setup(&base());
        let versions = setup.dir.join("packages/official/nats/versions.json");
        let text = std::fs::read_to_string(&versions).unwrap();
        let long = "L".repeat(65);
        std::fs::write(
            &versions,
            text.replace("\"Apache-2.0\"", &format!("\"{long}\"")),
        )
        .unwrap();

        let catalog = loaded(load(&setup, CatalogOptions::default()));

        assert!(invalid_codes(&catalog, "official/nats").contains(&(
            "versions.json".to_owned(),
            "artifacts[0].license".to_owned(),
            IssueCode::CatalogLimit,
            Some("text_length")
        )));
    }

    #[test]
    fn packages_past_the_byte_budget_are_invalid_from_the_one_that_crosses_it() {
        let setup = setup(
            &base()
                .fixture(CatalogFixture::LinuxOnly)
                .fixture(CatalogFixture::IntelOnly),
        );
        // One package is the size of a fixture package, which no longer
        // tracks the nats package once its catalog carries the records.
        let one = CatalogFixture::IntelOnly
            .files()
            .iter()
            .map(|(_, bytes)| bytes.len())
            .sum::<usize>();

        let state = load_with_budget(
            setup.fake.fs(),
            setup.fake.clock(),
            CatalogLocation::Development(setup.dir.clone()),
            CatalogOptions::default(),
            one + one / 2,
        );

        let catalog = loaded(state);
        // Index order is by ID: intelonly, linuxonly, nats.
        assert!(matches!(
            catalog.entry(&"official/intelonly".parse().unwrap()),
            Some(CatalogEntry::Provider(_))
        ));
        for id in ["official/linuxonly", "official/nats"] {
            let codes = invalid_codes(&catalog, id);
            assert_eq!(codes.len(), 1, "{id}");
            assert_eq!(codes[0].2, IssueCode::CatalogLimit);
            assert_eq!(codes[0].3, Some("total_bytes"));
            assert_eq!(codes[0].0, "catalog.json");
        }
    }

    #[test]
    fn index_problems_make_the_whole_catalog_unavailable() {
        let setup = setup(&base());
        let index = setup.dir.join("catalog.json");
        let text = std::fs::read_to_string(&index).unwrap();
        let cases = [
            (
                text.replace("stackly.catalog/v1", "stackly.catalog/v2"),
                UnavailableReason::UnsupportedSchema,
            ),
            (
                text.replacen("{", "{\n  \"path\": \"../x\",", 1),
                UnavailableReason::IndexInvalid,
            ),
            (
                text.replace("official/mysql", "official/clickhouse"),
                UnavailableReason::IndexInvalid,
            ),
            (
                text.replace("official/mysql", "official/nats"),
                UnavailableReason::IndexInvalid,
            ),
            (
                "x".repeat(INDEX_MAX_LEN + 1),
                UnavailableReason::IndexInvalid,
            ),
            ("not json".to_owned(), UnavailableReason::IndexInvalid),
        ];

        for (body, reason) in cases {
            std::fs::write(&index, &body).unwrap();

            assert_eq!(
                unavailable(load(&setup, CatalogOptions::default())).0,
                reason
            );
        }
        std::fs::remove_file(&index).unwrap();
        assert_eq!(
            unavailable(load(&setup, CatalogOptions::default())),
            (
                UnavailableReason::Missing,
                "catalog.json is missing".to_owned()
            )
        );
        std::fs::create_dir(&index).unwrap();
        assert_eq!(
            unavailable(load(&setup, CatalogOptions::default())).0,
            UnavailableReason::IndexInvalid
        );
    }

    #[test]
    fn entries_follow_the_display_order() {
        let setup = setup(
            &CatalogBuilder::new()
                .package("official/nats", repo_package("nats"))
                .fixture(CatalogFixture::BlockedHost)
                .planned("official/valkey", "Valkey", "cache", "Key value store.")
                .planned(
                    "official/postgresql",
                    "PostgreSQL",
                    "database",
                    "Relational database.",
                )
                .planned("official/traefik", "Traefik", "proxy", "Reverse proxy."),
        );
        let manifest = setup
            .dir
            .join("packages/official/blockedhost/provider.yaml");
        std::fs::write(&manifest, "broken: [").unwrap();

        let catalog = loaded(load(&setup, CatalogOptions::default()));

        let ids: Vec<String> = catalog
            .entries()
            .iter()
            .map(|e| e.id().to_string())
            .collect();
        assert_eq!(
            ids,
            [
                "official/nats",
                "official/blockedhost",
                "official/postgresql",
                "official/valkey",
                "official/traefik"
            ]
        );
        let CatalogEntry::Invalid(entry) = &catalog.entries()[1] else {
            panic!("blockedhost is invalid");
        };
        assert_eq!(entry.category, None);
    }

    #[test]
    fn find_by_name_returns_the_ids_with_that_name() {
        let catalog = loaded(load(&setup(&base()), CatalogOptions::default()));

        let found: Vec<String> = catalog
            .find_by_name("nats")
            .iter()
            .map(ToString::to_string)
            .collect();

        assert_eq!(found, ["official/nats"]);
        assert!(catalog.find_by_name("postgres").is_empty());
    }

    #[test]
    fn a_catalog_is_outdated_only_after_180_days() {
        let catalog = Catalog::for_tests(&[]);
        let published = catalog.meta().published_at;

        assert!(!catalog.outdated(published + SignedDuration::from_hours(179 * 24)));
        assert!(!catalog.outdated(published + CATALOG_OUTDATED_AFTER));
        assert!(catalog.outdated(published + SignedDuration::from_hours(181 * 24)));
    }

    #[test]
    fn the_empty_test_catalog_carries_the_digest_of_empty_content() {
        let catalog = Catalog::for_tests(&[]);

        assert!(catalog.entries().is_empty());
        assert_eq!(
            catalog.meta().content_sha256.to_string(),
            content_sha256(&[], &[])
        );
        assert_eq!(
            catalog.meta().published_at.to_string(),
            "2026-10-01T00:00:00Z"
        );
    }

    #[test]
    fn the_builder_refuses_testing_packages_unless_allowed() {
        let builder = CatalogBuilder::new().package("testing/fake", repo_package("fake"));

        assert!(builder.assemble().is_err());
        assert!(builder.allow_testing().assemble().is_ok());
    }

    #[test]
    fn the_testkit_empty_catalog_loads_with_its_real_digest() {
        let index =
            CatalogIndex::from_json(stackly_testkit::EMPTY_CATALOG_JSON.as_bytes()).unwrap();
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let dir = root.path().join("empty");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("catalog.json"),
            stackly_testkit::EMPTY_CATALOG_JSON,
        )
        .unwrap();

        let catalog = loaded(load_catalog(
            fake.fs(),
            fake.clock(),
            CatalogLocation::Development(dir),
            CatalogOptions::default(),
        ));

        assert_eq!(index.content_sha256, content_sha256(&[], &[]));
        assert!(catalog.entries().is_empty());
    }

    #[test]
    fn every_fixture_package_validates_with_its_id() {
        for fixture in CatalogFixture::ALL {
            assert_eq!(fixture.package().key().provider().as_str(), fixture.id());
        }
    }
}
