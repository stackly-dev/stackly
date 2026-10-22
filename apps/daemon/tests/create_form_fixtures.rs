//! The create form of the real packages (spec 0020 AC-1, AC-17): the form
//! the daemon builds for `providers/fake`, `providers/nats`, and
//! `providers/postgresql` is the fixture `protocols/fixtures/instance/
//! create-form-<name>.json`, which the Rust and Swift contract tests and the
//! form model of the sheet read. A package change that moves a field, a
//! bound, or a default shows up here; rewrite the files with
//! `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use stackly_core::instance::create::{PackageSource, build_form_from_package};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{InstanceName, RuntimeId, RuntimeState, Sha256Digest};
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

fn package(name: &str) -> ValidatedPackage {
    let dir = repo().join("providers").join(name);
    let files = read_package(&MacosFilesystem, &dir).expect("a shipped package reads");
    ValidatedPackage::from_files(files, NameCheck::FolderName).expect("a shipped package is valid")
}

fn form(name: &str, display_name: &str, version: &str, line: &str) -> CreateForm {
    let package = package(name);
    let runtime = RuntimeBuilder::new()
        .with_id(
            RUNTIME_ID
                .parse::<RuntimeId>()
                .expect("a fixture runtime ID"),
        )
        .software(package.software().as_str())
        .version(version, line)
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
        display_name.to_owned(),
        (SupportStatus::Unverified, SupportReason::Untested),
        Vec::new(),
        Vec::new(),
    );
    let capabilities = map::create::capabilities(&model.capabilities);
    map::create::create_form(&model, summary, capabilities)
}

// covers: AC-1, AC-17
#[test]
fn the_create_form_of_each_real_package_is_its_fixture() {
    let update = std::env::var_os("STACKLY_UPDATE_GOLDEN").is_some();
    for (name, display, version, line) in [
        ("fake", "Fake", "1.0.0", "1"),
        ("nats", "NATS", "2.15.1", "2.15"),
        ("postgresql", "PostgreSQL", "18.6", "18"),
    ] {
        let path = repo().join(format!(
            "protocols/fixtures/instance/create-form-{name}.json"
        ));
        let built =
            serde_json::to_value(form(name, display, version, line)).expect("a form encodes");
        if update {
            let mut text = serde_json::to_string_pretty(&built).expect("a form encodes");
            text.push('\n');
            std::fs::write(&path, text).expect("the fixture is written");
        }
        let on_disk: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display())),
        )
        .expect("a fixture is JSON");
        assert_eq!(built, on_disk, "{name}: rerun with STACKLY_UPDATE_GOLDEN=1");
    }
}
