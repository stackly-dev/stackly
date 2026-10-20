//! The wire contract of the secret store (spec 0011 AC-14 and AC-21): the
//! `secret` error catalog holds exactly the table of the spec, and no DTO
//! that carries a secret value prints it through `Debug`, alone or inside
//! the request and reply enums that wrap it. Nothing here starts a daemon
//! or touches a Keychain.

use stackly_protocol::codes::{self, ErrorSpec, ItemSpec};
use stackly_protocol::instance::{
    CreateInstanceParams, ProviderSelector, SecretFieldInput, SettingsDocument,
};
use stackly_protocol::secret::{
    RevealSecretParams, RevealSecretReply, SecretInput, SecretMode, SecretText, SetSecretParams,
};
use stackly_protocol::{Call, ErrorCategory, Reply};
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use uuid::Uuid;

/// One row of the table in *Kode error di wire*.
struct Row {
    code: &'static str,
    category: ErrorCategory,
    details: &'static [&'static str],
    items: Option<(&'static str, &'static [&'static str])>,
}

const fn row(
    code: &'static str,
    category: ErrorCategory,
    details: &'static [&'static str],
    items: Option<(&'static str, &'static [&'static str])>,
) -> Row {
    Row {
        code,
        category,
        details,
        items,
    }
}

/// The table of the spec, in its order.
fn table() -> [Row; 13] {
    use ErrorCategory::{
        Conflict, Failed, InvalidInput, NotFound, Timeout, Unauthorized, Unavailable, Unsupported,
    };
    [
        row(
            "secret.needs_secret",
            Unavailable,
            &["count", "omitted"],
            Some(("reference", &["reference", "reason", "generateAllowed"])),
        ),
        row("secret.not_found", NotFound, &["reference"], None),
        row(
            "secret.conflict",
            Conflict,
            &["reference", "reason", "omitted"],
            Some(("use", &["resourceKind", "resourceId", "field"])),
        ),
        row(
            "secret.confirmation_required",
            InvalidInput,
            &["reference", "omitted"],
            Some(("effect", &["resourceKind", "resourceId", "field", "effect"])),
        ),
        row(
            "secret.invalid_reference",
            InvalidInput,
            &["reference", "reason"],
            None,
        ),
        row(
            "secret.invalid_value",
            InvalidInput,
            &["field", "minLength", "maxLength", "reason", "omitted"],
            None,
        ),
        row("secret.delivery_rejected", Failed, &["reason"], None),
        row("secret.store_locked", Unavailable, &[], None),
        row(
            "secret.store_access_denied",
            Unauthorized,
            &["byUser"],
            None,
        ),
        row("secret.store_busy", Unavailable, &[], None),
        row("secret.store_outcome_unknown", Timeout, &[], None),
        row("secret.store_unsupported", Unsupported, &[], None),
        row("secret.store_backend", Failed, &["code"], None),
    ]
}

fn items_of(spec: &ErrorSpec) -> Option<(&'static str, &'static [&'static str], u16)> {
    spec.items
        .map(|items: ItemSpec| (items.kind, items.fields, items.max))
}

#[test]
fn the_secret_error_catalog_holds_exactly_the_table_of_spec_0011() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "AC-21: the secret area of the error catalog lists the thirteen codes of the spec table, each with its category, retryable false, its details keys, and its item kind, fields, and limit of 100 where the table names one, and nothing else",
    );

    let table = table();
    let listed: Vec<&str> = codes::secret::ALL.iter().map(|spec| spec.code).collect();
    let expected: Vec<&str> = table.iter().map(|row| row.code).collect();
    assert_eq!(listed, expected, "the area lists the table and no more");

    for row in &table {
        let spec =
            codes::lookup(row.code).unwrap_or_else(|| panic!("{} is in the catalog", row.code));
        assert_eq!(spec.category, row.category, "{} category", row.code);
        assert!(!spec.retryable, "{} is never retryable", row.code);
        assert_eq!(spec.details, row.details, "{} details", row.code);
        assert_eq!(
            items_of(spec),
            row.items.map(|(kind, fields)| (kind, fields, 100)),
            "{} items",
            row.code
        );
    }
    ev.pass();
}

#[test]
fn every_secret_code_with_items_lists_omitted_and_none_lists_a_value_key() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "AC-21: a secret code that carries items lists the omitted detail that counts the items above its limit, and no details key of the area names a value",
    );

    for spec in codes::secret::ALL {
        if spec.items.is_some() {
            assert!(
                spec.details.contains(&"omitted"),
                "{} carries items and must count what it drops",
                spec.code
            );
        }
        for key in spec.details {
            let lower = key.to_ascii_lowercase();
            assert!(
                !(lower == "value" || lower.ends_with("secret") || lower.contains("password")),
                "{} has a details key {key} that could hold a value",
                spec.code
            );
        }
    }
    ev.pass();
}

fn create_params(secrets: Vec<SecretFieldInput>) -> CreateInstanceParams {
    CreateInstanceParams {
        name: "erp".to_owned(),
        display_name: None,
        provider: ProviderSelector {
            name: "testing/fake".to_owned(),
            revision: None,
        },
        runtime_id: Uuid::nil(),
        endpoints: Vec::new(),
        settings: SettingsDocument::new(serde_json::Map::new()),
        secrets,
    }
}

#[test]
fn debug_of_every_dto_and_enum_that_carries_a_value_hides_it() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "AC-14: the Debug text of SecretText, SecretInput, SetSecretParams, RevealSecretReply, CreateInstanceParams with a secret field, and the Call and Reply enums that wrap them never holds the fixture credential, while the reference stays readable",
    );
    let credential = FixtureCredential::new();
    let value = || SecretText::new(credential.value().to_owned());

    let set = SetSecretParams {
        reference: "local:erp/postgres/admin".to_owned(),
        input: SecretInput::Value(value()),
        mode: SecretMode::Upsert,
        expected_revision: None,
        confirm_stored_only: false,
    };
    let reveal = RevealSecretReply {
        reference: "local:erp/postgres/admin".to_owned(),
        value: value(),
    };
    let create = create_params(vec![SecretFieldInput {
        field: "adminPassword".to_owned(),
        input: SecretInput::Value(value()),
    }]);

    let texts = [
        ("SecretText", format!("{:?}", value())),
        ("SecretInput", format!("{:?}", SecretInput::Value(value()))),
        ("SetSecretParams", format!("{set:?}")),
        (
            "Call::SecretSet",
            format!("{:?}", Call::SecretSet(set.clone())),
        ),
        ("RevealSecretReply", format!("{reveal:?}")),
        (
            "Reply::SecretReveal",
            format!("{:?}", Reply::SecretReveal(reveal.clone())),
        ),
        ("CreateInstanceParams", format!("{create:?}")),
        (
            "Call::InstanceCreate",
            format!("{:?}", Call::InstanceCreate(create.clone())),
        ),
        (
            "pretty Debug of Call::SecretSet",
            format!("{:#?}", Call::SecretSet(set)),
        ),
    ];
    for (what, text) in &texts {
        credential.assert_absent(text, what);
        assert!(
            !text.contains(credential.value()),
            "{what} printed the value"
        );
    }
    assert!(
        texts[2].1.contains("local:erp/postgres/admin"),
        "{}",
        texts[2].1
    );
    assert!(texts[0].1.contains("****"), "{}", texts[0].1);
    ev.pass();
}

#[test]
fn a_params_type_that_only_names_a_reference_has_no_value_to_print() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "AC-14: the reveal request names a reference and a purpose and its Debug text has no value field",
    );
    let params = RevealSecretParams {
        reference: "local:erp/postgres/admin".to_owned(),
        purpose: stackly_protocol::secret::RevealPurpose::Show,
    };

    let text = format!("{params:?}");

    assert!(text.contains("local:erp/postgres/admin"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("value"), "{text}");
    ev.pass();
}
