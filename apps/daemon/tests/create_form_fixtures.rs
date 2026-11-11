//! The create form of the real packages (spec 0020 AC-1, AC-17; spec 0036
//! AC-8): the form the daemon builds for every folder in `providers/` is the
//! fixture `protocols/fixtures/instance/create-form-<folder>.json`, which the
//! Rust and Swift contract tests and the form model of the sheet read. The
//! folders come from the `providers/` listing, never from a list in code,
//! and the runtime facts of each form come from its package: the display
//! name from `metadata.displayName`, the version and line from the first
//! `versions.json` artifact for the host target. A package change that moves
//! a field, a bound, or a default shows up here, and a new package without
//! its fixture fails; write the files with
//! `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use stackly_core::instance::create::{PackageSource, build_form_from_package};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{InstanceName, RuntimeId, RuntimeState, Sha256Digest};
use stackly_platform::host_target;
use stackly_platform_macos::MacosFilesystem;
use stackly_protocol::catalog::{SupportReason, SupportStatus};
use stackly_protocol::instance::CreateForm;
use stackly_testkit::RuntimeBuilder;
use stacklyd::map;

const RUNTIME_ID: &str = "0192f3a6-0000-7000-8000-0000000000a1";
const ARTIFACT_SHA256: [u8; 32] = [0x88; 32];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn package(dir: &Path) -> ValidatedPackage {
    let files = read_package(&MacosFilesystem, dir).expect("a shipped package reads");
    ValidatedPackage::from_files(files, NameCheck::FolderName).expect("a shipped package is valid")
}

/// Every package folder in `providers/`, sorted by name.
fn package_folders() -> Vec<(String, PathBuf)> {
    let mut folders: Vec<(String, PathBuf)> = std::fs::read_dir(repo().join("providers"))
        .expect("the providers folder lists")
        .map(|entry| entry.expect("a providers entry reads"))
        .filter(|entry| entry.file_type().expect("the entry has a type").is_dir())
        .map(|entry| {
            let name = entry
                .file_name()
                .into_string()
                .expect("a package folder name is UTF-8");
            (name, entry.path())
        })
        .collect();
    folders.sort();
    folders
}

/// The create form of the package in `dir`, for a runtime of its first
/// artifact for the host target.
fn form(dir: &Path) -> CreateForm {
    let package = package(dir);
    let target = host_target().expect("the test host is a modeled target");
    let artifact = package
        .versions()
        .artifacts()
        .iter()
        .find(|artifact| artifact.target == target)
        .unwrap_or_else(|| panic!("{} has an artifact for {target:?}", dir.display()));
    let version = artifact.version.to_string();
    let line = artifact.line.to_string();
    let runtime = RuntimeBuilder::new()
        .with_id(
            RUNTIME_ID
                .parse::<RuntimeId>()
                .expect("a fixture runtime ID"),
        )
        .software(package.software().as_str())
        .version(&version, &line)
        .managed(Sha256Digest::from_bytes(ARTIFACT_SHA256))
        .state(RuntimeState::Installed)
        .build();
    let suggested = format!("{}-{}", package.software().as_str(), line.replace('.', "-"))
        .parse::<InstanceName>()
        .ok();
    let model =
        build_form_from_package(&package, PackageSource::BundledCatalog, &runtime, suggested);
    let summary = map::runtime::runtime_summary(
        &runtime,
        package.manifest().display_name().to_owned(),
        (SupportStatus::Unverified, SupportReason::Untested),
        Vec::new(),
        Vec::new(),
    );
    let capabilities = map::create::capabilities(&model.capabilities);
    map::create::create_form(&model, summary, capabilities, &fixture_locations())
}

/// The defaults the golden fixtures were written with.
fn fixture_locations() -> stackly_core::instance::create::FormLocations {
    stackly_core::instance::create::FormLocations {
        startup_default: stackly_domain::StartupPolicy::Manual,
        data: "/Users/dev/Library/Application Support/Stackly".into(),
        logs: "/Users/dev/Library/Logs/Stackly".into(),
    }
}

// covers: AC-1, AC-17 (spec 0020), AC-8 (spec 0036)
#[test]
fn the_create_form_of_each_package_folder_is_its_fixture() {
    let update = std::env::var_os("STACKLY_UPDATE_GOLDEN").is_some();
    let folders = package_folders();
    assert!(
        folders.len() >= 5,
        "only {} package folders found",
        folders.len()
    );
    for (name, dir) in folders {
        let path = repo().join(format!(
            "protocols/fixtures/instance/create-form-{name}.json"
        ));
        let built = serde_json::to_value(form(&dir)).expect("a form encodes");
        if update {
            let mut text = serde_json::to_string_pretty(&built).expect("a form encodes");
            text.push('\n');
            std::fs::write(&path, text).expect("the fixture is written");
        }
        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|err| {
                panic!(
                    "{}: {err}; write it with STACKLY_UPDATE_GOLDEN=1",
                    path.display()
                )
            }))
            .expect("a fixture is JSON");
        assert_eq!(built, on_disk, "{name}: rerun with STACKLY_UPDATE_GOLDEN=1");
    }
}
