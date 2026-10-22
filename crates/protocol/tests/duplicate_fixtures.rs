//! Checks the duplicate DTOs of spec 0024 against `protocols/fixtures/`: the
//! two duplicate forms in `instance/` and the request and frames of
//! `instance.get_duplicate_form` in `ipc/` (AC-10).

use std::path::PathBuf;

use serde::de::DeserializeOwned;
use stackly_protocol::instance::{CreateProviderSource, DuplicateForm, PortRequest, SecretSource};
use stackly_protocol::json::{Validate, decode_request};
use stackly_protocol::{Call, Reply, ServerFrame};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures")
}

fn read(path: &str) -> Vec<u8> {
    let path = fixtures_dir().join(path);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn decode<T: DeserializeOwned>(path: &str) -> T {
    serde_json::from_slice(&read(path)).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn duplicate(name: &str) -> DuplicateForm {
    decode(&format!("instance/duplicate-form-{name}.json"))
}

fn reply(file: &str) -> Reply {
    let ServerFrame::Response(response) = decode(&format!("ipc/{file}")) else {
        panic!("{file}: expected a response");
    };
    response.reply
}

// covers: AC-10
#[test]
fn every_duplicate_form_fixture_roundtrips_in_the_wire_form() {
    for name in ["fake", "postgresql"] {
        let file = format!("instance/duplicate-form-{name}.json");
        let value: DuplicateForm = decode(&file);
        let written = serde_json::to_value(&value).expect("a decoded value always encodes");
        let fixture: serde_json::Value = decode(&file);
        assert_eq!(written, fixture, "{file}: Rust writes another shape");
    }
}

// covers: AC-1, AC-4, AC-5, AC-10
#[test]
fn a_duplicate_form_pins_the_package_asks_auto_ports_and_generates_every_secret() {
    for name in ["fake", "postgresql"] {
        let form = duplicate(name);
        assert_eq!(form.form.provider.source, CreateProviderSource::Pinned);
        assert_eq!(form.draft.provider.name, form.form.provider.name);
        assert_eq!(
            form.draft.provider.revision.as_deref(),
            Some(form.form.provider.revision.as_str())
        );
        assert_eq!(form.draft.runtime_id, form.form.runtime.id);
        assert_eq!(form.draft.display_name, None);
        assert!(!form.draft.start);
        assert!(!form.draft.endpoints.is_empty());
        assert!(
            form.draft
                .endpoints
                .iter()
                .all(|endpoint| endpoint.port == PortRequest::Auto),
            "{name}: a duplicate draft never fixes a port"
        );
        let secret_fields: Vec<_> = form
            .form
            .fields
            .iter()
            .filter(|field| field.secret)
            .map(|field| field.key.as_str())
            .collect();
        assert!(!secret_fields.is_empty());
        assert_eq!(
            form.draft
                .secrets
                .iter()
                .map(|plan| plan.field.as_str())
                .collect::<Vec<_>>(),
            secret_fields,
            "{name}: one plan per secret field, in form order"
        );
        assert!(
            form.draft
                .secrets
                .iter()
                .all(|plan| plan.source == SecretSource::Generate),
            "{name}: a duplicate draft never copies or binds a secret"
        );
        assert_eq!(
            form.secrets
                .iter()
                .map(|secret| secret.field.as_str())
                .collect::<Vec<_>>(),
            secret_fields
        );
        assert!(
            form.secrets
                .iter()
                .all(|secret| secret.source_reference.is_some())
        );
        assert_eq!(
            form.form.suggested_name.as_deref(),
            Some(form.draft.name.as_str())
        );
        assert!(
            form.draft.validate().is_empty(),
            "{name}: the draft is valid"
        );
        let settings = form.draft.settings.as_map();
        assert!(
            secret_fields.iter().all(|key| !settings.contains_key(*key)),
            "{name}: no secret field in the copied settings"
        );
    }
}

// covers: AC-1, AC-3
#[test]
fn the_fixtures_name_the_duplicate_after_the_names_already_taken() {
    let postgresql = duplicate("postgresql");
    assert_eq!(postgresql.source.name, "pg17-dev");
    assert_eq!(postgresql.draft.name, "pg17-dev-copy");
    let fake = duplicate("fake");
    assert_eq!(fake.source.name, "fake-a");
    assert_eq!(fake.draft.name, "fake-a-copy-2");
}

// covers: AC-10
#[test]
fn the_request_frame_decodes_to_the_new_method() {
    let request = decode_request(&read("ipc/request-instance-get-duplicate-form.json"))
        .expect("the request decodes");
    let Call::InstanceGetDuplicateForm(params) = request.call else {
        panic!("expected instance.get_duplicate_form");
    };
    assert_eq!(
        params.instance_id.to_string(),
        "0192f3a6-0000-7000-8000-0000000000b2"
    );
    assert!(params.validate().is_empty());
    assert!(!Call::InstanceGetDuplicateForm(params).allowed_in_recovery());
}

// covers: AC-10
#[test]
fn the_response_frame_carries_the_postgresql_fixture_and_foreign_values_stay_decodable() {
    let Reply::InstanceGetDuplicateForm(form) =
        reply("frame-response-instance-get-duplicate-form.json")
    else {
        panic!("expected an instance.get_duplicate_form reply");
    };
    assert_eq!(*form, duplicate("postgresql"));
    let Reply::InstanceGetDuplicateForm(foreign) =
        reply("frame-response-instance-get-duplicate-form-unknown-values.json")
    else {
        panic!("expected an instance.get_duplicate_form reply");
    };
    assert_eq!(
        foreign.form.provider.source,
        CreateProviderSource::Unrecognized
    );
}
