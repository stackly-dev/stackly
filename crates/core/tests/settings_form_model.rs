//! The settings model the create form renders (spec 0009, spec 0020 AC-1,
//! AC-9, AC-21): fields in declaration order, and a fixed kind on every
//! problem with a settings document.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::provider::{
    IssueCode, NameCheck, PackageFiles, SettingsIssueKind, ValidatedPackage,
};
use stackly_evidence::{Kind, evidence};

/// The secret reference of the settings documents here, in the daemon's
/// form for their instance (spec 0011).
const SECRET_REF: &str = "local:instance/0192f000-0000-7000-8000-00000000000b/auth-token";

fn package_dir(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    match name {
        "fake-command" => root.join("tests/fixtures/packages/fake-command"),
        other => root.join("../../providers").join(other),
    }
}

fn files_of(dir: &Path) -> BTreeMap<String, Vec<u8>> {
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
        let name = entry.file_name().into_string().expect("UTF-8 name");
        files.insert(
            format!("templates/{name}"),
            fs::read(entry.path()).expect("template"),
        );
    }
    files
}

fn validated(name: &str, files: BTreeMap<String, Vec<u8>>) -> ValidatedPackage {
    ValidatedPackage::from_files(PackageFiles::new(name.to_owned(), files), NameCheck::Skip)
        .unwrap_or_else(|err| panic!("{name} should validate: {:#?}", err.issues))
}

/// The root property names of a schema file as written, found by their
/// indentation (the files of this repository indent with two spaces).
fn written_root_order(dir: &Path) -> Vec<String> {
    let text =
        fs::read_to_string(dir.join("schemas/settings.schema.json")).expect("settings schema");
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("    \"")?;
            let (name, tail) = rest.split_once('"')?;
            tail.trim_start()
                .starts_with(": {")
                .then(|| name.to_owned())
        })
        .collect()
}

#[test]
fn fields_follow_the_order_properties_declares_them() {
    let ev = evidence!(
        covers = ["VAL-012"],
        kind = Kind::Fixture,
        expected = "SettingsSchema::fields returns the fields of fake, fake-command, nats, and postgresql in the order their schema file declares them, not in name order",
    );
    for name in ["fake", "fake-command", "nats", "postgresql"] {
        let dir = package_dir(name);
        let package = validated(name, files_of(&dir));
        let actual: Vec<String> = package
            .settings_schema()
            .fields()
            .map(|field| field.name().to_string())
            .collect();

        assert_eq!(actual, written_root_order(&dir), "{name}");
    }
    let mut sorted: Vec<String> = written_root_order(&package_dir("fake-command"));
    sorted.sort();
    assert_ne!(
        sorted,
        written_root_order(&package_dir("fake-command")),
        "the fixture would not tell declaration order from name order"
    );
    ev.pass();
}

#[test]
fn item_fields_follow_the_order_the_items_declare_them() {
    let dir = package_dir("fake-command");
    let mut files = files_of(&dir);
    let schema = String::from_utf8(files["schemas/settings.schema.json"].clone()).expect("UTF-8");
    let name = r#""name": { "type": "string", "title": "Name", "maxLength": 32, "pattern": "[a-z][a-z0-9-]*" },"#;
    let port = r#""port": { "type": "integer", "title": "Port", "minimum": 1, "maximum": 65535, "default": 4222 }"#;
    assert!(schema.contains(&format!("{name}\n          {port}")));
    let swapped = schema.replacen(
        &format!("{name}\n          {port}"),
        &format!("{port},\n          {}", name.trim_end_matches(',')),
        1,
    );
    files.insert(
        "schemas/settings.schema.json".to_owned(),
        swapped.into_bytes(),
    );
    let package = validated("fake-command", files);

    let stackly_core::provider::FieldKind::Array(rules) = package
        .settings_schema()
        .field("peers")
        .expect("peers exists")
        .kind()
    else {
        panic!("peers is an array");
    };
    let order: Vec<String> = rules
        .item_fields()
        .map(|field| field.name.to_string())
        .collect();

    assert_eq!(order, ["port", "name"]);
}

#[test]
fn every_problem_with_a_settings_document_has_a_fixed_kind() {
    let ev = evidence!(
        covers = ["VAL-019"],
        kind = Kind::Fixture,
        expected = "each problem SettingsSchema::validate finds carries the SettingsIssueKind of spec 0009: unknown, type, managed_filled, control_characters, duplicate_key, pattern, too_short, too_long, out_of_range, enum, too_many_items, too_few_items, and required",
    );
    let mut files = files_of(&package_dir("fake-command"));
    let schema = String::from_utf8(files["schemas/settings.schema.json"].clone()).expect("UTF-8");
    // A minimum length and a minimum item count that the fixture lacks.
    let schema = schema
        .replacen(
            "\"title\": \"Greeting\",\n      \"maxLength\": 256,",
            "\"title\": \"Greeting\",\n      \"minLength\": 3,\n      \"maxLength\": 30,",
            1,
        )
        .replacen(
            "\"maxItems\": 8,",
            "\"minItems\": 1,\n      \"maxItems\": 2,",
            1,
        )
        .replacen("\"default\": [],", "\"default\": [{ \"name\": \"a\" }],", 1);
    files.insert(
        "schemas/settings.schema.json".to_owned(),
        schema.into_bytes(),
    );
    let package = validated("fake-command", files);
    let schema = package.settings_schema();
    let kinds = |value: serde_json::Value| -> Vec<(String, IssueCode, SettingsIssueKind)> {
        schema
            .validate(&value)
            .expect_err("the document is rejected")
            .issues
            .into_iter()
            .map(|issue| {
                (
                    issue.location.to_string(),
                    issue.code,
                    issue.settings_kind.expect("a settings issue has a kind"),
                )
            })
            .collect()
    };
    let token = SECRET_REF;
    let value = IssueCode::SettingsValue;
    let case = |path: &str, kind| (path.to_owned(), value, kind);

    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "extra": 1 })),
        [(
            "extra".to_owned(),
            IssueCode::UnknownSetting,
            SettingsIssueKind::Unknown
        )]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "serverName": "x" })),
        [case("serverName", SettingsIssueKind::ManagedFilled)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": 5 })),
        [case("authToken", SettingsIssueKind::Type)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "greeting": "a\nbcd" })),
        [case("greeting", SettingsIssueKind::ControlCharacters)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "greeting": "ab" })),
        [case("greeting", SettingsIssueKind::TooShort)]
    );
    assert_eq!(
        kinds(
            serde_json::json!({ "authToken": token, "greeting": "abcdefghijklmnopqrstuvwxyzabcde" })
        ),
        [case("greeting", SettingsIssueKind::TooLong)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "mode": "silent" })),
        [case("mode", SettingsIssueKind::Enum)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "peers": [{ "name": "A" }] })),
        [case("peers[0].name", SettingsIssueKind::Pattern)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "peers": [{ "name": "a", "port": 70000 }] })),
        [case("peers[0].port", SettingsIssueKind::OutOfRange)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "peers": [{ "name": "a", "port": 5.0 }] })),
        [case("peers[0].port", SettingsIssueKind::Type)]
    );
    assert_eq!(
        kinds(
            serde_json::json!({ "authToken": token, "peers": [{ "name": "a" }, { "name": "a" }] })
        ),
        [case("peers[1].name", SettingsIssueKind::DuplicateKey)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "peers": [] })),
        [case("peers", SettingsIssueKind::TooFewItems)]
    );
    assert_eq!(
        kinds(serde_json::json!({
            "authToken": token,
            "peers": [{ "name": "a" }, { "name": "b" }, { "name": "c" }]
        })),
        [case("peers", SettingsIssueKind::TooManyItems)]
    );
    assert_eq!(
        kinds(serde_json::json!({ "authToken": token, "peers": [{ "port": 4222 }] })),
        [case("peers[0].name", SettingsIssueKind::Required)]
    );
    assert_eq!(
        kinds(serde_json::json!({})),
        [case("authToken", SettingsIssueKind::Required)]
    );
    for kind in SettingsIssueKind::ALL {
        assert!(!kind.as_code().is_empty());
    }
    ev.pass();
}
