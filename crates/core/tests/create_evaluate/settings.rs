//! The `settings` row and the field issues of a settings document (spec
//! 0020 AC-7 and AC-9, *Field error*).

use stackly_core::instance::create::{CheckStatus, SecretSourceKind};
use stackly_evidence::{Kind, evidence};

use crate::common::{Site, error_codes, has_row, paths, row, source};

fn evaluate_with(
    site: &Site,
    settings: serde_json::Value,
) -> stackly_core::instance::create::Evaluation {
    let mut draft = site.draft("orders");
    draft.settings = settings;
    site.evaluate(&draft)
}

#[test]
fn every_kind_of_settings_problem_maps_to_its_field_code_and_path() {
    let ev = evidence!(
        covers = ["VAL-019"],
        kind = Kind::Fixture,
        expected = "an unknown key, a wrong type, an out of range number, and a text over its length each make instance.settings_invalid with the code of the Field error table at the path settings.<field>",
    );
    let site = Site::new();

    let unknown = evaluate_with(&site, serde_json::json!({ "nope": 1 }));
    assert_eq!(error_codes(&unknown), ["instance.settings_invalid"]);
    assert_eq!(paths(&unknown), [("settings.nope", "unknown_value")]);
    assert_eq!(
        row(&unknown, "settings").detail,
        "1 settings need attention"
    );

    let wrong_type = evaluate_with(&site, serde_json::json!({ "readyDelayMs": "abc" }));
    assert_eq!(
        paths(&wrong_type),
        [("settings.readyDelayMs", "invalid_value")]
    );

    let range = evaluate_with(&site, serde_json::json!({ "readyDelayMs": 700_000 }));
    assert_eq!(paths(&range), [("settings.readyDelayMs", "out_of_range")]);

    let long = evaluate_with(&site, serde_json::json!({ "greeting": "x".repeat(257) }));
    assert_eq!(paths(&long), [("settings.greeting", "too_long")]);

    let fraction = evaluate_with(&site, serde_json::json!({ "readyDelayMs": 5.0 }));
    assert_eq!(
        paths(&fraction),
        [("settings.readyDelayMs", "invalid_value")]
    );

    let control = evaluate_with(&site, serde_json::json!({ "greeting": "a\nb" }));
    assert_eq!(paths(&control), [("settings.greeting", "invalid_value")]);

    let not_object = evaluate_with(&site, serde_json::json!([1]));
    assert_eq!(paths(&not_object), [("settings", "invalid_value")]);
    ev.pass();
}

#[test]
fn several_problems_are_listed_ordered_by_path() {
    let site = Site::new();

    let evaluation = evaluate_with(
        &site,
        serde_json::json!({ "zzz": 1, "readyDelayMs": "x", "aaa": 2 }),
    );

    assert_eq!(
        paths(&evaluation),
        [
            ("settings.aaa", "unknown_value"),
            ("settings.readyDelayMs", "invalid_value"),
            ("settings.zzz", "unknown_value")
        ]
    );
    assert_eq!(
        row(&evaluation, "settings").detail,
        "3 settings need attention"
    );
}

#[test]
fn a_managed_field_that_is_filled_is_rejected() {
    let site = Site::with_provider("nats");

    let evaluation = evaluate_with(&site, serde_json::json!({ "serverName": "mine" }));

    assert_eq!(error_codes(&evaluation), ["instance.settings_invalid"]);
    assert_eq!(
        paths(&evaluation),
        [("settings.serverName", "invalid_value")]
    );
}

#[test]
fn a_secret_field_in_settings_and_a_bad_secrets_entry_are_settings_invalid() {
    let ev = evidence!(
        covers = ["VAL-012"],
        kind = Kind::Fixture,
        expected = "a secret field inside settings, a secrets entry for a field the schema lacks or that is not a secret, and a field sent twice are instance.settings_invalid with the paths settings.<field> and secrets[i].field",
    );
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.settings = serde_json::json!({ "authToken": "local:instance/x/y" });
    draft.secrets = vec![
        source("nope", SecretSourceKind::Generate),
        source("greeting", SecretSourceKind::Generate),
        source("authToken", SecretSourceKind::Generate),
        source("authToken", SecretSourceKind::Value),
    ];

    let evaluation = site.evaluate(&draft);

    assert_eq!(error_codes(&evaluation), ["instance.settings_invalid"]);
    assert_eq!(
        paths(&evaluation),
        [
            ("secrets[0].field", "unknown_value"),
            ("secrets[1].field", "unknown_value"),
            ("secrets[3].field", "invalid_value"),
            ("settings.authToken", "invalid_value"),
        ]
    );
    ev.pass();
}

#[test]
fn a_bind_reference_that_does_not_parse_does_not_add_a_settings_problem() {
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.secrets = vec![source(
        "authToken",
        SecretSourceKind::Bind("nope".to_owned()),
    )];

    let evaluation = site.evaluate(&draft);

    assert!(!has_row(&evaluation, "settings"));
    assert_eq!(error_codes(&evaluation), ["secret.invalid_reference"]);
    assert_eq!(
        row(&evaluation, "secret.authToken").status,
        CheckStatus::Error
    );
}

#[test]
fn the_validated_settings_hold_the_reference_of_each_secret_and_the_defaults() {
    let site = Site::new();

    let evaluation = evaluate_with(&site, serde_json::json!({ "greeting": "hi" }));

    let resolved = evaluation.resolved.expect("ready");
    let json = resolved.settings.to_json();
    assert_eq!(json["greeting"], "hi");
    assert_eq!(json["readyDelayMs"], 0, "defaults fill in");
    let reference = json["authToken"].as_str().expect("a reference text");
    assert!(reference.starts_with("local:instance/00000000-0000-7000-8000-000000000000/"));
    assert_eq!(resolved.secrets[0].reference.as_str(), reference);
}

#[test]
fn evaluating_for_a_real_id_derives_the_references_from_that_id() {
    let site = Site::new();
    let draft = site.draft("orders");
    let facts = site.facts(&draft);
    let id = stackly_domain::InstanceId::generate();

    let evaluation = stackly_core::instance::create::evaluate_for(&draft, &facts, id);

    let resolved = evaluation.resolved.expect("ready");
    assert!(
        resolved.secrets[0]
            .reference
            .as_str()
            .contains(&id.to_string())
    );
}
