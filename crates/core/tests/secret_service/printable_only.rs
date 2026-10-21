//! The `printable_only` rule of a secret field (spec 0011 AC-22): `put`
//! checks a value's length first, then its control characters, and fails
//! `secret.invalid_value` with a `reason` that names neither the value nor
//! touches the store for a `Value`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use stackly_core::operation::OperationError;
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::secret::{
    FieldContext, InvalidValueReason, PutRequest, PutTarget, SecretError, SecretFieldRule,
    SecretInput,
};
use stackly_domain::{InstanceId, SecretOwner, SecretRef, SecretUser};
use stackly_evidence::{Kind, evidence};
use stackly_platform::WriteMode;
use stackly_testkit::SecretOp;

use crate::common::{Harness, field_name, instance_reference, reference, value};

/// A password field of 8 to 64 characters that refuses control characters.
fn printable() -> SecretFieldRule {
    SecretFieldRule::new(8, Some(64), None).with_printable_only(true)
}

/// A request that registers the reference of `purpose` for a field of
/// `instance` with `rule`.
fn new_field(
    instance: InstanceId,
    purpose: &str,
    rule: SecretFieldRule,
    input: SecretInput,
) -> PutRequest {
    PutRequest::new(
        PutTarget::New {
            reference: instance_reference(instance, purpose),
            owner: SecretOwner::Instance(instance),
            purpose: purpose.parse().expect("a valid test purpose"),
            field: Some(FieldContext::new(field_name("adminPassword"), rule)),
        },
        input,
        WriteMode::Create,
    )
}

/// The details of an error as `(key, value)` text pairs.
fn details(err: &SecretError) -> Vec<(String, String)> {
    OperationError::details(err)
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn invalid(reason: InvalidValueReason) -> SecretError {
    SecretError::InvalidValue {
        field: field_name("adminPassword"),
        min_length: 8,
        max_length: Some(64),
        reason,
    }
}

#[test]
fn a_value_with_a_control_character_fails_a_printable_only_field_before_any_store_call() {
    let harness = Harness::new();
    let instance = harness.instance(false);

    for (i, text) in [
        "line-one\nline-two",
        "tab\tseparated-value",
        "next\u{85}line-value",
    ]
    .into_iter()
    .enumerate()
    {
        let purpose = format!("admin-password-{i}");
        let err = harness
            .put(new_field(
                instance,
                &purpose,
                printable(),
                SecretInput::Value(value(text)),
            ))
            .unwrap_err();

        assert_eq!(err, invalid(InvalidValueReason::ControlCharacters));
        assert_eq!(err.code().as_str(), "secret.invalid_value");
        assert_eq!(
            details(&err),
            [
                ("field".to_owned(), "adminPassword".to_owned()),
                ("minLength".to_owned(), "8".to_owned()),
                ("maxLength".to_owned(), "64".to_owned()),
                ("reason".to_owned(), "control_characters".to_owned()),
            ]
        );
        for shown in [err.to_string(), format!("{err:?}")] {
            assert!(!shown.contains(text), "{shown}");
        }
        assert!(
            harness
                .view(&instance_reference(instance, &purpose))
                .is_none()
        );
    }
    assert!(harness.store().calls().is_empty());
    let audit = harness.audit();
    assert_eq!(audit.len(), 3);
    assert!(
        audit
            .iter()
            .all(|row| row.safe_details.get("errorCode") == Some("secret.invalid_value"))
    );
}

#[test]
fn the_length_is_checked_before_the_control_characters() {
    let harness = Harness::new();
    let instance = harness.instance(false);

    let short = harness
        .put(new_field(
            instance,
            "short",
            printable(),
            SecretInput::Value(value("a\nb")),
        ))
        .unwrap_err();
    let long = harness
        .put(new_field(
            instance,
            "long",
            printable(),
            SecretInput::Value(value(&format!("{}\n", "x".repeat(64)))),
        ))
        .unwrap_err();

    for err in [short, long] {
        assert_eq!(err, invalid(InvalidValueReason::Length));
        assert!(details(&err).contains(&("reason".to_owned(), "length".to_owned())));
    }
    assert!(harness.store().calls().is_empty());
}

#[test]
fn a_copy_from_a_source_with_a_control_character_fails_after_one_read_and_no_write() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let source = reference("local:shared/multiline");
    harness.set(&source, "line-one\nline-two");
    let (reads, writes) = (
        harness.store().count(SecretOp::Read),
        harness.store().count(SecretOp::Write),
    );

    let err = harness
        .put(new_field(
            instance,
            "copied",
            printable(),
            SecretInput::CopyFrom(source.clone()),
        ))
        .unwrap_err();

    assert_eq!(err, invalid(InvalidValueReason::ControlCharacters));
    assert_eq!(harness.store().count(SecretOp::Read), reads + 1);
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    assert!(
        harness
            .view(&instance_reference(instance, "copied"))
            .is_none()
    );
}

#[test]
fn a_generated_value_always_passes_a_printable_only_field() {
    let harness = Harness::new();
    let instance = harness.instance(false);

    harness
        .put(new_field(
            instance,
            "generated",
            printable(),
            SecretInput::Generate,
        ))
        .unwrap();

    let stored = harness
        .store()
        .stored_value(&instance_reference(instance, "generated"))
        .unwrap();
    assert_eq!(stored.len(), 32);
    assert!(!stored.chars().any(char::is_control));
}

#[test]
fn a_field_without_the_flag_takes_the_same_values() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let open = SecretFieldRule::new(8, Some(64), None);
    let source = reference("local:shared/multiline");
    harness.set(&source, "line-one\nline-two");

    for (purpose, input) in [
        ("entered", SecretInput::Value(value("tab\tseparated-value"))),
        ("copied", SecretInput::CopyFrom(source.clone())),
    ] {
        harness
            .put(new_field(instance, purpose, open, input))
            .unwrap();
    }

    assert_eq!(
        harness
            .store()
            .stored_value(&instance_reference(instance, "entered"))
            .as_deref(),
        Some("tab\tseparated-value")
    );
    assert_eq!(
        harness
            .store()
            .stored_value(&instance_reference(instance, "copied"))
            .as_deref(),
        Some("line-one\nline-two")
    );
}

#[test]
fn an_existing_reference_a_printable_only_field_uses_refuses_control_characters_too() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let shared: SecretRef = reference("local:shared/password");
    harness.set(&shared, "the-first-shared-value");
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &shared)],
    );
    harness.rules.set(instance, "adminPassword", printable());
    let before = harness.view(&shared).unwrap();
    let writes = harness.store().count(SecretOp::Write);
    let replace = |text: &str| {
        harness.put(PutRequest::new(
            PutTarget::Existing(shared.clone()),
            SecretInput::Value(value(text)),
            WriteMode::Replace,
        ))
    };

    let err = replace("the-second\nshared-value").unwrap_err();

    assert_eq!(err, invalid(InvalidValueReason::ControlCharacters));
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    assert_eq!(harness.view(&shared).unwrap().record, before.record);
    replace("the-second-shared-value").unwrap();
    assert_eq!(
        harness.store().stored_value(&shared).as_deref(),
        Some("the-second-shared-value")
    );
}

/// The rule `SecretFieldRule::from_setting` reads from the secret field
/// `authToken` of `providers/fake`, whose schema `x-stackly` object is
/// `x_stackly` instead of the committed one.
fn rule_of_the_fake_token(x_stackly: &str) -> SecretFieldRule {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let mut files = BTreeMap::new();
    for path in [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
    ] {
        files.insert(
            path.to_owned(),
            fs::read(dir.join(path)).expect("fake package file"),
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
    let schema = String::from_utf8(files["schemas/settings.schema.json"].clone())
        .expect("UTF-8 schema")
        .replacen(
            r#""x-stackly": { "apply": "immutableAfterInit", "secret": true }"#,
            x_stackly,
            1,
        );
    files.insert(
        "schemas/settings.schema.json".to_owned(),
        schema.into_bytes(),
    );
    let package = ValidatedPackage::from_files(
        PackageFiles::new("fake".to_owned(), files),
        NameCheck::FolderName,
    )
    .expect("the edited fake package is valid");
    let field = package
        .settings_schema()
        .field("authToken")
        .expect("the fake package has an authToken field");
    SecretFieldRule::from_setting(field).expect("authToken is a secret text field")
}

#[test]
fn from_setting_fills_printable_only_from_the_settings_schema() {
    let ev = evidence!(
        covers = ["REQ-035"],
        kind = Kind::Fixture,
        expected = "SecretFieldRule::from_setting sets printable_only exactly when the secret \
                    field has x-stackly.printableOnly true",
    );
    let plain =
        rule_of_the_fake_token(r#""x-stackly": { "apply": "immutableAfterInit", "secret": true }"#);
    let printable = rule_of_the_fake_token(
        r#""x-stackly": { "apply": "immutableAfterInit", "secret": true, "printableOnly": true }"#,
    );
    let explicit_false = rule_of_the_fake_token(
        r#""x-stackly": { "apply": "immutableAfterInit", "secret": true, "printableOnly": false }"#,
    );

    assert!(!plain.printable_only);
    assert!(printable.printable_only);
    assert!(!explicit_false.printable_only);
    ev.pass();
}

#[test]
fn create_with_a_schema_printable_only_field_refuses_control_characters() {
    let ev = evidence!(
        covers = ["REQ-035"],
        kind = Kind::Fixture,
        expected = "a put into a printableOnly field of a settings schema refuses control \
                    characters with secret.invalid_value before any store call, and accepts \
                    spaces, quotes, and backslashes",
    );
    let rule = rule_of_the_fake_token(
        r#""x-stackly": { "apply": "immutableAfterInit", "secret": true, "printableOnly": true }"#,
    );
    let harness = Harness::new();
    let instance = harness.instance(false);

    let err = harness
        .put(new_field(
            instance,
            "from-schema",
            rule,
            SecretInput::Value(value("pass\nword-with-newline")),
        ))
        .unwrap_err();

    assert!(matches!(
        err,
        SecretError::InvalidValue {
            reason: InvalidValueReason::ControlCharacters,
            ..
        }
    ));
    assert_eq!(err.code().as_str(), "secret.invalid_value");
    assert!(harness.store().calls().is_empty());
    harness
        .put(new_field(
            instance,
            "from-schema",
            rule,
            SecretInput::Value(value("pass word's \"quoted\" \\ ok")),
        ))
        .unwrap();
    ev.pass();
}
