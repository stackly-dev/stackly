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
    load_package(name, None)
}

/// The validated package of `providers/<name>` with its `versions.json`
/// changed by `edit`, and `macos-x86_64` and `linux-aarch64` added to its
/// platforms.
pub fn package_with(name: &str, edit: impl FnOnce(&mut serde_json::Value)) -> ValidatedPackage {
    load_package(name, Some(Box::new(edit)))
}

type Edit<'a> = Box<dyn FnOnce(&mut serde_json::Value) + 'a>;

fn load_package(name: &str, edit: Option<Edit<'_>>) -> ValidatedPackage {
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
    if let Some(edit) = edit {
        edit_files(&mut files, edit);
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

/// `testing/single`: the fake package under another name, without
/// `multiInstance`, the one package of the test catalog that runs one
/// instance at a time (spec 0044, *Provider satu instance*).
pub fn single_instance_package() -> ValidatedPackage {
    let dir = repo().join("providers/fake");
    let mut files = BTreeMap::new();
    for path in ["versions.json", "schemas/settings.schema.json"] {
        files.insert(
            path.to_owned(),
            fs::read(dir.join(path)).expect("package file"),
        );
    }
    let manifest = fs::read_to_string(dir.join("provider.yaml"))
        .expect("provider.yaml")
        .replace("id: testing/fake", "id: testing/single")
        .replace("multiInstance: true", "multiInstance: false");
    files.insert("provider.yaml".to_owned(), manifest.into_bytes());
    for entry in fs::read_dir(dir.join("templates")).expect("templates folder") {
        let entry = entry.expect("template entry");
        let file = entry.file_name().into_string().expect("UTF-8 name");
        files.insert(
            format!("templates/{file}"),
            fs::read(entry.path()).expect("template"),
        );
    }
    ValidatedPackage::from_files(PackageFiles::new("single", files), NameCheck::FolderName)
        .unwrap_or_else(|err| panic!("single validates: {:#?}", err.issues))
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

/// The test catalog: [`PACKAGES`], `testing/single` (no `multiInstance`),
/// plus `testing/broken`, whose package does not load.
pub fn catalog() -> Catalog {
    catalog_with(Vec::new())
}

/// The test catalog with these packages in place of the official ones of
/// the same name.
pub fn catalog_with(replaced: Vec<(&str, ValidatedPackage)>) -> Catalog {
    let mut replaced = replaced;
    let mut entries: Vec<CatalogEntry> = PACKAGES
        .iter()
        .map(|name| {
            let at = replaced.iter().position(|(own, _)| own == name);
            entry(at.map_or_else(|| package(name), |at| replaced.remove(at).1))
        })
        .collect();
    entries.push(entry(single_instance_package()));
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

fn edit_files(files: &mut BTreeMap<String, Vec<u8>>, edit: Edit<'_>) {
    // Every platform an edit may add artifacts for.
    let manifest = files
        .get_mut("provider.yaml")
        .expect("provider.yaml is read");
    *manifest = String::from_utf8(manifest.clone())
        .expect("UTF-8")
        .replace(
            "    - { os: macos, arch: aarch64 }\n",
            "    - { os: macos, arch: aarch64 }\n    - { os: macos, arch: x86_64 }\n    - { os: linux, arch: aarch64 }\n",
        )
        .into_bytes();
    let versions = files
        .get_mut("versions.json")
        .expect("versions.json is read");
    let mut value: serde_json::Value = serde_json::from_slice(versions).expect("versions.json");
    edit(&mut value);
    *versions = serde_json::to_vec_pretty(&value).expect("versions.json encodes");
}
