//! The duplicate form of the real packages (spec 0024 AC-10): the form and
//! the draft the daemon builds for a source of `providers/fake` and of
//! `providers/postgresql` is the fixture `protocols/fixtures/instance/
//! duplicate-form-<name>.json`, which the Rust and Swift contract tests and
//! the form model of the sheet read. A package change that moves a field,
//! a bound, or a default shows up here; rewrite the files with
//! `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test duplicate_form_fixtures`.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use stackly_core::instance::duplicate::{DuplicateFacts, build_duplicate};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{
    InstanceId, InstanceLifecycle, InstanceName, RuntimeId, RuntimeState, SecretFieldName,
    SecretPurpose, SecretRef, SecretUse, SecretUser, Sha256Digest,
};
use stackly_platform_macos::MacosFilesystem;
use stackly_protocol::catalog::{SupportReason, SupportStatus};
use stackly_protocol::instance::DuplicateForm;
use stackly_testkit::{InstanceBuilder, RuntimeBuilder};
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

struct Scenario {
    package: &'static str,
    display_name: &'static str,
    version: &'static str,
    line: &'static str,
    source_id: &'static str,
    source_name: &'static str,
    taken: &'static [&'static str],
    secret_field: &'static str,
    values: serde_json::Value,
}

fn duplicate_form(scenario: &Scenario) -> DuplicateForm {
    let package = package(scenario.package);
    let runtime = RuntimeBuilder::new()
        .with_id(
            RUNTIME_ID
                .parse::<RuntimeId>()
                .expect("a fixture runtime ID"),
        )
        .software(package.software().as_str())
        .version(scenario.version, scenario.line)
        .managed(Sha256Digest::from_bytes(ARTIFACT_SHA256))
        .state(RuntimeState::Installed)
        .build();
    let source_id = scenario
        .source_id
        .parse::<InstanceId>()
        .expect("a fixture instance ID");
    let source = InstanceBuilder::new(&runtime, package.key())
        .with_id(source_id)
        .name(scenario.source_name)
        .lifecycle(InstanceLifecycle::Stopped)
        .build();
    let purpose = SecretPurpose::from_field_name(scenario.secret_field).expect("a field purpose");
    let facts = DuplicateFacts {
        source,
        package,
        runtime: runtime.clone(),
        values: Some(scenario.values.clone()),
        uses: vec![SecretUse::new(
            SecretRef::for_instance(source_id, &purpose),
            SecretUser::Instance(source_id),
            scenario
                .secret_field
                .parse::<SecretFieldName>()
                .expect("a field name"),
        )],
        taken: scenario
            .taken
            .iter()
            .chain(&[scenario.source_name])
            .map(|name| name.parse::<InstanceName>().expect("a fixture name"))
            .collect(),
    };
    let model = build_duplicate(&facts);
    let summary = map::runtime::runtime_summary(
        &runtime,
        scenario.display_name.to_owned(),
        (SupportStatus::Unverified, SupportReason::Untested),
        Vec::new(),
        Vec::new(),
    );
    let capabilities = map::create::capabilities(&model.form.capabilities);
    map::duplicate::duplicate_form(&model, summary, capabilities)
}

// covers: AC-1, AC-10
#[test]
fn the_duplicate_form_of_each_real_package_is_its_fixture() {
    let update = std::env::var_os("STACKLY_UPDATE_GOLDEN").is_some();
    let scenarios = [
        (
            "fake",
            Scenario {
                package: "fake",
                display_name: "Fake",
                version: "1.0.0",
                line: "1",
                source_id: "0192f3a6-0000-7000-8000-0000000000b1",
                source_name: "fake-a",
                taken: &["fake-a-copy"],
                secret_field: "authToken",
                values: serde_json::json!({
                    "greeting": "hello from fake-a",
                    "label": "fake",
                    "readyDelayMs": 0,
                    "crashAfterMs": 0,
                    "crashExitCode": 70,
                    "logLinesPerSecond": 0,
                    "ignoreTerm": false,
                    "failInit": false,
                    "initDelayMs": 0,
                }),
            },
        ),
        (
            "postgresql",
            Scenario {
                package: "postgresql",
                display_name: "PostgreSQL",
                version: "17.11",
                line: "17",
                source_id: "0192f3a6-0000-7000-8000-0000000000b2",
                source_name: "pg17-dev",
                taken: &[],
                secret_field: "superuserPassword",
                values: serde_json::json!({
                    "maxConnections": 200,
                    "sharedBuffersMb": 128,
                    "logStatement": "ddl",
                    "logMinDurationStatementMs": -1,
                }),
            },
        ),
    ];
    for (name, scenario) in &scenarios {
        let path = repo().join(format!(
            "protocols/fixtures/instance/duplicate-form-{name}.json"
        ));
        let built = serde_json::to_value(duplicate_form(scenario)).expect("a form encodes");
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
