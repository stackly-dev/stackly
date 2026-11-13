//! The test catalog and fixture paths.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, InvalidEntry, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};

/// The official packages in the test catalog, with the fake one.
pub const PACKAGES: &[&str] = &[
    "postgresql",
    "mysql",
    "nats",
    "clickhouse",
    "valkey",
    "traefik",
    "fake",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The fixture folder of environment manifests.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/environments")
}

/// Reads one fixture manifest.
pub fn fixture(name: &str) -> Vec<u8> {
    fs::read(fixtures().join(name)).expect("the fixture exists")
}

/// The validated package of `providers/<name>`.
pub fn package(name: &str) -> ValidatedPackage {
    let dir = repo().join("providers").join(name);
    let mut files = BTreeMap::new();
    for path in [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
    ] {
        files.insert(
            path.to_owned(),
            fs::read(dir.join(path)).expect("package file"),
        );
    }
    for entry in fs::read_dir(dir.join("templates")).expect("templates folder") {
        let entry = entry.expect("template entry");
        let file = entry.file_name().into_string().expect("UTF-8 name");
        files.insert(
            format!("templates/{file}"),
            fs::read(entry.path()).expect("template"),
        );
    }
    ValidatedPackage::from_files(PackageFiles::new(name, files), NameCheck::FolderName)
        .unwrap_or_else(|err| panic!("{name} validates: {:#?}", err.issues))
}

/// A catalog entry for a package.
pub fn entry(package: ValidatedPackage) -> CatalogEntry {
    CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package),
        tile: TileHint {
            mark: "Te".into(),
            tile: TileColor::Blue,
        },
        notes: Vec::new(),
        origin: EntryOrigin::Catalog,
    })
}

/// The test catalog: [`PACKAGES`], plus `testing/broken`, whose package
/// does not load.
pub fn catalog() -> Catalog {
    let mut entries: Vec<CatalogEntry> = PACKAGES.iter().map(|name| entry(package(name))).collect();
    entries.push(CatalogEntry::Invalid(InvalidEntry {
        id: "testing/broken".parse().expect("a provider name"),
        category: None,
        tile: TileHint {
            mark: "Br".into(),
            tile: TileColor::Blue,
        },
        issues: Vec::new(),
        local: None,
    }));
    Catalog::for_tests(&entries)
}
