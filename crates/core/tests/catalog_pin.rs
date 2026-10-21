//! Pinning a provider revision and recording the catalog source (spec 0020
//! AC-13 and AC-14; K-1 and K-2 of spec 0017).
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_core::catalog::{
    BUNDLED_LOCATION, CatalogLocation, CatalogMeta, PinError, ProviderEntry, TileColor, TileHint,
    ensure_source, pin_revision,
};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{
    ProviderSourceId, ProviderSourceKind, ProviderSourceStatus, Registry, RegistryError,
    Sha256Digest,
};
use stackly_evidence::{Kind, evidence};
use stackly_platform::{Clock, Platform};
use stackly_testkit::{FakePlatform, FakeRegistry, FsOp, TestRoot};

struct Site {
    _root: TestRoot,
    fake: FakePlatform,
    platform: Platform,
    prepared: PreparedLayout,
    entry: ProviderEntry,
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the target folder can be made");
    for item in fs::read_dir(from).expect("the source folder reads") {
        let item = item.expect("an entry reads");
        let target = to.join(item.file_name());
        if item.file_type().expect("the type reads").is_dir() {
            copy_dir(&item.path(), &target);
        } else {
            fs::copy(item.path(), &target).expect("a file copies");
        }
    }
}

impl Site {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).expect("the fake roots prepare");
        let copy = root.path().join("input/nats");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats"),
            &copy,
        );
        let files = read_package(platform.fs(), &copy).expect("the package reads");
        let package =
            Arc::new(ValidatedPackage::from_files(files, NameCheck::Skip).expect("it validates"));
        Self {
            _root: root,
            fake,
            platform,
            prepared,
            entry: ProviderEntry {
                package,
                tile: TileHint {
                    mark: "N".to_owned(),
                    tile: TileColor::Cyan,
                },
                notes: Vec::new(),
            },
        }
    }

    fn dir(&self) -> PathBuf {
        self.prepared
            .layout()
            .provider_revision_dir(self.entry.package.key())
    }

    fn pin(&self) -> Result<stackly_core::catalog::PinnedRevision, PinError> {
        pin_revision(
            &self.platform,
            &self.prepared,
            &self.entry,
            ProviderSourceId::generate(),
        )
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("metadata reads").mode() & 0o7777
    }
}

#[test]
fn pinning_copies_the_package_privately_and_returns_the_record_with_its_source() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "pin_revision writes the files of the package into providers/<namespace>/<name>/<revision>/<digest>/ with folders 0700 and files 0600, the copy reads back with the digest of the key, and the record names the source",
    );
    let site = Site::new();
    let source = ProviderSourceId::generate();

    let pinned = pin_revision(&site.platform, &site.prepared, &site.entry, source)
        .unwrap_or_else(|error| panic!("{error}"));

    let dir = site.dir();
    assert!(pinned.wrote_files);
    assert_eq!(Site::mode(&dir), 0o700);
    assert_eq!(Site::mode(&dir.join("schemas")), 0o700);
    assert_eq!(Site::mode(&dir.join("templates")), 0o700);
    assert_eq!(Site::mode(&dir.join("provider.yaml")), 0o600);
    assert_eq!(Site::mode(&dir.join("schemas/settings.schema.json")), 0o600);
    let files = read_package(site.platform.fs(), &dir).expect("the copy reads");
    let read_back = ValidatedPackage::from_files(files, NameCheck::Skip).expect("it validates");
    assert_eq!(read_back.key(), site.entry.package.key());
    assert_eq!(pinned.record.source_id, Some(source));
    assert_eq!(
        pinned.record.revision.key(),
        site.entry.package.key(),
        "the record is the revision of the package"
    );
    assert_eq!(
        pinned.record.schema_version.as_deref(),
        Some("stackly/v1alpha1")
    );
    assert_eq!(pinned.record.publisher.as_deref(), Some("Stackly"));
    ev.pass();
}

#[test]
fn pinning_again_writes_nothing() {
    let site = Site::new();
    site.pin().expect("the first pin works");
    let before = site.fake.fs().mutations();

    let again = site.pin().expect("the second pin works");

    assert!(!again.wrote_files);
    assert_eq!(
        site.fake.fs().mutations(),
        before,
        "no folder or file is written"
    );
}

#[test]
fn a_pinned_file_that_differs_is_an_error_and_is_left_alone_but_versions_json_is_not_compared() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "a pinned file covered by the digest that differs from the catalog is existing_differs and stays as it is, while a different versions.json is accepted and the first copy wins",
    );
    let site = Site::new();
    site.pin().expect("the first pin works");
    let dir = site.dir();

    let versions = dir.join("versions.json");
    let mut changed = fs::read(&versions).expect("it reads");
    changed.push(b'\n');
    fs::write(&versions, &changed).expect("it writes");
    site.pin()
        .expect("a different versions.json is not compared");
    assert_eq!(
        fs::read(&versions).expect("it reads"),
        changed,
        "the first copy wins"
    );

    let manifest = dir.join("provider.yaml");
    fs::write(&manifest, b"apiVersion: other\n").expect("it writes");
    let error = site.pin().expect_err("a differing manifest is refused");
    assert_eq!(
        error,
        PinError::ExistingDiffers {
            path: manifest.clone()
        }
    );
    assert_eq!(error.reason(), "existing_differs");
    assert_eq!(
        fs::read(&manifest).expect("it reads"),
        b"apiVersion: other\n",
        "the file is not overwritten"
    );
    ev.pass();
}

#[test]
fn a_symlink_or_a_foreign_entry_stops_the_pin_without_touching_anything() {
    let ev = evidence!(
        covers = ["VAL-007"],
        kind = Kind::Fixture,
        expected = "a symlink in place of a package file or folder, and a foreign entry in the pinned folder, make pin_revision fail unsafe_path without overwriting or removing anything",
    );
    let site = Site::new();
    site.pin().expect("the first pin works");
    let dir = site.dir();
    let outside = site._root.path().join("outside-file");
    fs::write(&outside, b"keep").expect("it writes");

    // A symlink in place of a file.
    let manifest = dir.join("provider.yaml");
    let original = fs::read(&manifest).expect("it reads");
    fs::remove_file(&manifest).expect("it removes");
    symlink(&outside, &manifest).expect("it links");
    let error = site.pin().expect_err("a symlink is refused");
    assert_eq!(error.reason(), "unsafe_path");
    assert_eq!(fs::read(&outside).expect("it reads"), b"keep");
    assert!(
        fs::symlink_metadata(&manifest)
            .expect("it stats")
            .is_symlink()
    );
    fs::remove_file(&manifest).expect("it removes");
    fs::write(&manifest, original).expect("it restores");

    // A foreign entry.
    let foreign = dir.join("notes.txt");
    fs::write(&foreign, b"mine").expect("it writes");
    let error = site.pin().expect_err("a foreign entry is refused");
    assert_eq!(
        error,
        PinError::UnsafePath {
            path: foreign.clone()
        }
    );
    assert_eq!(fs::read(&foreign).expect("it reads"), b"mine");
    fs::remove_file(&foreign).expect("it removes");

    // A symlink in place of a folder of the path.
    let templates = dir.join("templates");
    let entries: Vec<_> = fs::read_dir(&templates)
        .expect("it lists")
        .map(|e| e.expect("an entry").path())
        .collect();
    let moved = site._root.path().join("moved-templates");
    fs::rename(&templates, &moved).expect("it moves");
    symlink(&moved, &templates).expect("it links");
    let error = site.pin().expect_err("a symlinked folder is refused");
    assert_eq!(error.reason(), "unsafe_path");
    assert!(!entries.is_empty());
    assert!(
        fs::symlink_metadata(&templates)
            .expect("it stats")
            .is_symlink()
    );
    fs::remove_file(&templates).expect("it removes");
    fs::rename(&moved, &templates).expect("it moves back");

    // A symlink above the digest folder.
    let namespace = site.prepared.layout().providers_dir().join("official");
    let real = site._root.path().join("real-official");
    fs::rename(&namespace, &real).expect("it moves");
    symlink(&real, &namespace).expect("it links");
    let error = site.pin().expect_err("a symlinked folder is refused");
    assert_eq!(error.reason(), "unsafe_path");
    ev.pass();
}

#[test]
fn eight_threads_pinning_one_revision_all_succeed_with_the_same_content() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "eight threads that pin the same revision at once all succeed and leave one folder whose files read back with the digest of the key",
    );
    let site = Site::new();

    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8).map(|_| scope.spawn(|| site.pin())).collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a pin does not panic"))
            .collect()
    });

    for result in &results {
        assert!(result.is_ok(), "{result:?}");
    }
    let files = read_package(site.platform.fs(), &site.dir()).expect("the copy reads");
    let read_back = ValidatedPackage::from_files(files, NameCheck::Skip).expect("it validates");
    assert_eq!(read_back.key(), site.entry.package.key());
    assert!(
        site.fake
            .fs()
            .mutations()
            .iter()
            .all(|mutation| mutation.op != FsOp::RemoveTree),
        "a pin never deletes"
    );
    ev.pass();
}

fn meta(location: CatalogLocation, site: &Site) -> CatalogMeta {
    let at = site.fake.clock().now();
    CatalogMeta {
        revision: 1,
        published_at: at,
        content_sha256: Sha256Digest::from_bytes([7; 32]),
        location,
        loaded_at: at,
    }
}

#[test]
fn the_catalog_source_is_recorded_once_with_the_columns_of_k_2() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "ensure_source writes one provider_sources row for the loaded catalog, a second call and another meta of the same place find it, last_sync_at stays empty, and a bundled catalog that moved keeps its row",
    );
    let site = Site::new();
    let registry = FakeRegistry::new(Arc::new(site.fake.clock().clone()), site.fake.clock().now());
    let clock = site.fake.clock();
    let development = meta(CatalogLocation::Development("/tmp/catalog".into()), &site);

    let id = ensure_source(&registry, clock, &development).expect("the row is written");
    let again = ensure_source(&registry, clock, &development).expect("the row is found");

    assert_eq!(id, again);
    let row = registry
        .provider_source_by_location(ProviderSourceKind::Development, "/tmp/catalog")
        .expect("it reads")
        .value
        .expect("the row exists");
    assert_eq!(row.id, id);
    assert_eq!(row.trust_root, None);
    assert_eq!(row.last_sync_at, None);
    assert_eq!(row.status, ProviderSourceStatus::Active);

    let other = meta(CatalogLocation::Development("/tmp/other".into()), &site);
    assert_ne!(
        ensure_source(&registry, clock, &other).expect("another row"),
        id
    );

    let bundled = ensure_source(
        &registry,
        clock,
        &meta(
            CatalogLocation::Bundled("/Applications/A.app/x".into()),
            &site,
        ),
    )
    .expect("the bundled row is written");
    let moved = ensure_source(
        &registry,
        clock,
        &meta(CatalogLocation::Bundled("/Users/me/A.app/x".into()), &site),
    )
    .expect("the bundled row is found");
    assert_eq!(bundled, moved, "an app that moved keeps its row");
    let row = registry
        .provider_source_by_location(ProviderSourceKind::Bundled, BUNDLED_LOCATION)
        .expect("it reads")
        .value
        .expect("the row exists");
    assert_eq!(row.trust_root.as_deref(), Some("app_bundle"));
    ev.pass();
}

#[test]
fn a_registry_that_cannot_write_the_source_is_an_error() {
    let site = Site::new();
    let registry = FakeRegistry::new(Arc::new(site.fake.clock().clone()), site.fake.clock().now());
    registry.fail_next_commit(RegistryError::Busy);

    let result = ensure_source(
        &registry,
        site.fake.clock(),
        &meta(CatalogLocation::Development("/tmp/catalog".into()), &site),
    );

    assert_eq!(result, Err(RegistryError::Busy));
}
