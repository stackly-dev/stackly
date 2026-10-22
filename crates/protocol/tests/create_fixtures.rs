//! Checks the create DTOs of spec 0020 against `protocols/fixtures/`: the
//! three create forms and the command cases in `instance/`, and the frames
//! of `instance.get_create_form`, `instance.preflight`, and the new error
//! codes in `ipc/` (AC-1, AC-2, AC-17, AC-18).

use std::path::PathBuf;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use stackly_protocol::catalog::{ProviderCapability, ProviderCategory};
use stackly_protocol::instance::command::{CommandDraft, equivalent_command};
use stackly_protocol::instance::{
    CreateForm, CreateProviderSource, EndpointPurpose, EndpointTransport, FieldApply, FieldKind,
    FieldVisibility, PreflightCheckKind, PreflightStatus,
};
use stackly_protocol::json::{Validate, decode_request};
use stackly_protocol::{Call, ErrorFrame, Reply, ServerFrame, codes};

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

fn reply(file: &str) -> Reply {
    let ServerFrame::Response(response) = decode(&format!("ipc/{file}")) else {
        panic!("{file}: expected a response");
    };
    response.reply
}

fn form(name: &str) -> CreateForm {
    decode(&format!("instance/create-form-{name}.json"))
}

#[derive(Debug, Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    draft: CommandDraft,
    command: String,
}

// covers: AC-17
#[test]
fn every_create_form_fixture_roundtrips_in_the_wire_form() {
    for name in ["fake", "nats", "postgresql"] {
        let file = format!("instance/create-form-{name}.json");
        let value: CreateForm = decode(&file);
        let written = serde_json::to_value(&value).expect("a decoded value always encodes");
        let fixture: serde_json::Value = decode(&file);
        assert_eq!(written, fixture, "{file}: Rust writes another shape");
    }
}

// covers: AC-1, AC-21
#[test]
fn the_form_lists_endpoints_and_fields_in_the_order_the_package_declares() {
    let nats = form("nats");
    assert_eq!(nats.provider.name, "official/nats");
    assert_eq!(nats.provider.category, ProviderCategory::Messaging);
    assert_eq!(nats.provider.source, CreateProviderSource::BundledCatalog);
    let endpoints: Vec<_> = nats
        .endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.name.as_str(),
                endpoint.purpose,
                endpoint.default_port,
            )
        })
        .collect();
    assert_eq!(
        endpoints,
        [
            ("client", EndpointPurpose::Client, Some(4222)),
            ("monitoring", EndpointPurpose::Monitoring, Some(8222)),
        ]
    );
    assert!(
        nats.endpoints
            .iter()
            .all(|endpoint| endpoint.transport == EndpointTransport::Tcp)
    );
    let keys: Vec<_> = nats.fields.iter().map(|field| field.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "serverName",
            "jetstream",
            "jetstreamMaxMemoryBytes",
            "jetstreamMaxFileBytes",
            "maxPayloadBytes",
        ],
        "the order of `properties`, not the order of the names"
    );
    assert!(nats.fields[0].managed);
    assert_eq!(nats.fields[1].kind, FieldKind::Boolean);
    for field in &nats.fields[2..] {
        assert_eq!(field.kind, FieldKind::Integer, "{}", field.key);
        assert_eq!(field.visibility, FieldVisibility::Advanced, "{}", field.key);
        assert_eq!(field.apply, Some(FieldApply::Restart), "{}", field.key);
        assert!(
            field.minimum.is_some() && field.maximum.is_some(),
            "{}",
            field.key
        );
    }
    assert_eq!(nats.capabilities.len(), ProviderCapability::ALL.len());
    assert_eq!(nats.data_path, "instances/<new instance id>/data");
}

// covers: AC-1, AC-7
#[test]
fn the_postgresql_form_has_a_secret_field_with_its_rules() {
    let pg = form("postgresql");
    let secret = pg
        .fields
        .iter()
        .find(|field| field.secret)
        .expect("the package has a secret field");
    assert_eq!(secret.key, "superuserPassword");
    assert!(secret.printable_only);
    assert_eq!(
        (
            secret.min_length.map(|n| n.get()),
            secret.max_length.map(|n| n.get())
        ),
        (Some(8), Some(128))
    );
    assert!(secret.default.is_none());
}

#[test]
fn the_frames_carry_the_create_form_fixture() {
    let Reply::InstanceGetCreateForm(frame) = reply("frame-response-instance-get-create-form.json")
    else {
        panic!("expected an instance.get_create_form reply");
    };
    assert_eq!(*frame, form("postgresql"));
}

#[test]
fn foreign_values_of_the_create_enums_decode_as_unrecognized() {
    let Reply::InstanceGetCreateForm(form) =
        reply("frame-response-instance-get-create-form-unknown-values.json")
    else {
        panic!("expected an instance.get_create_form reply");
    };
    assert_eq!(form.provider.category, ProviderCategory::Unrecognized);
    assert_eq!(form.provider.source, CreateProviderSource::Unrecognized);
    assert_eq!(form.endpoints[0].transport, EndpointTransport::Unrecognized);
    assert_eq!(form.endpoints[0].purpose, EndpointPurpose::Unrecognized);
    assert_eq!(form.fields[0].kind, FieldKind::Unrecognized);
    assert_eq!(form.fields[1].visibility, FieldVisibility::Unrecognized);
    assert_eq!(form.fields[2].apply, Some(FieldApply::Unrecognized));
    assert_eq!(
        form.capabilities[0].capability,
        ProviderCapability::Unrecognized
    );

    let Reply::InstancePreflight(preflight) =
        reply("frame-response-instance-preflight-unknown-values.json")
    else {
        panic!("expected an instance.preflight reply");
    };
    assert_eq!(preflight.checks[0].kind, PreflightCheckKind::Unrecognized);
    assert_eq!(preflight.checks[1].status, PreflightStatus::Unrecognized);
}

// covers: AC-2
#[test]
fn a_preflight_is_ready_exactly_when_no_row_is_an_error() {
    for file in [
        "frame-response-instance-preflight-ready.json",
        "frame-response-instance-preflight-port-conflict.json",
        "frame-response-instance-preflight-no-provider.json",
    ] {
        let Reply::InstancePreflight(preflight) = reply(file) else {
            panic!("{file}: expected an instance.preflight reply");
        };
        let failing = preflight
            .checks
            .iter()
            .any(|check| check.status == PreflightStatus::Error);
        assert_eq!(preflight.ready, !failing, "{file}");
        for check in &preflight.checks {
            assert_eq!(
                check.error.is_some(),
                check.status == PreflightStatus::Error,
                "{file}: {}",
                check.id
            );
            assert!(check.title.chars().count() <= 200 && check.detail.chars().count() <= 200);
        }
    }
    let Reply::InstancePreflight(blocked) =
        reply("frame-response-instance-preflight-port-conflict.json")
    else {
        panic!("expected an instance.preflight reply");
    };
    let row = blocked
        .checks
        .iter()
        .find(|check| check.id == "endpoint.postgres")
        .expect("the conflict row");
    assert_eq!(row.suggested_port, Some(5433));
    let error = row.error.as_ref().expect("an error row carries its error");
    assert_eq!(error.code(), "supervisor.endpoint_conflict");
    assert_eq!(error.fields(), blocked.fields.as_slice());
}

#[test]
fn a_preflight_request_carries_no_secret_value_and_a_draft_is_checked_for_shape() {
    let bytes = read("ipc/request-instance-preflight.json");
    let request = decode_request(&bytes).expect("the fixture decodes");
    let Call::InstancePreflight(params) = request.call else {
        panic!("expected instance.preflight");
    };
    assert!(params.start);
    assert_eq!(params.secrets.len(), 4);
    assert!(params.validate().is_empty());
    let text = String::from_utf8(bytes).expect("a fixture is UTF-8");
    assert!(!text.contains("fixture-secret"));

    let without = decode_request(&read("ipc/request-instance-preflight-without-fields.json"))
        .expect("the fixture decodes");
    let Call::InstancePreflight(params) = without.call else {
        panic!("expected instance.preflight");
    };
    assert!(params.endpoints.is_empty() && params.secrets.is_empty());
    assert_eq!(params.display_name, None);
    assert!(!params.start);

    for (file, path) in [
        (
            "ipc/reject-instance-preflight-bad-reference.json",
            "secrets[0].source.content",
        ),
        ("ipc/reject-instance-preflight-empty-name.json", "name"),
        (
            "ipc/reject-instance-get-create-form-provider.json",
            "provider",
        ),
    ] {
        let rejection = decode_request(&read(file)).expect_err(file);
        assert_eq!(rejection.error.code(), "request.invalid_params", "{file}");
        assert_eq!(rejection.error.fields()[0].path(), path, "{file}");
    }
}

#[test]
fn the_form_and_preflight_methods_read_the_registry_and_start_no_operation() {
    for method in ["instance.get_create_form", "instance.preflight"] {
        assert!(Call::METHODS.contains(&method), "{method}");
        assert!(!stackly_protocol::operation::starts_operation(method));
    }
}

// covers: AC-17
#[test]
fn the_new_error_codes_have_a_fixture_with_their_catalog_details_and_static_hint() {
    type Hint = fn(&str, &[stackly_protocol::error::ErrorDetail]) -> Option<&'static str>;
    let instance: Hint = codes::instance::hint;
    let cases: [(&str, &codes::ErrorSpec, Hint); 10] = [
        (
            "instance-name-invalid",
            &codes::instance::NAME_INVALID,
            instance,
        ),
        (
            "instance-provider-unavailable",
            &codes::instance::PROVIDER_UNAVAILABLE,
            instance,
        ),
        (
            "instance-capability-unsupported",
            &codes::instance::CAPABILITY_UNSUPPORTED,
            instance,
        ),
        (
            "instance-runtime-incompatible",
            &codes::instance::RUNTIME_INCOMPATIBLE,
            instance,
        ),
        (
            "instance-storage-unsupported",
            &codes::instance::STORAGE_UNSUPPORTED,
            instance,
        ),
        (
            "instance-insufficient-space",
            &codes::instance::INSUFFICIENT_SPACE,
            instance,
        ),
        (
            "instance-data-not-empty",
            &codes::instance::DATA_NOT_EMPTY,
            instance,
        ),
        (
            "instance-data-overlap",
            &codes::instance::DATA_OVERLAP,
            instance,
        ),
        (
            "instance-short-key-taken",
            &codes::instance::SHORT_KEY_TAKEN,
            instance,
        ),
        (
            "catalog-pin-failed",
            &codes::catalog::PIN_FAILED,
            codes::catalog::hint,
        ),
    ];
    for (name, code, hint) in cases {
        let file = format!("frame-error-{name}.json");
        let ServerFrame::Error(ErrorFrame { error, .. }) = decode(&format!("ipc/{file}")) else {
            panic!("{file}: expected an error frame");
        };
        assert_eq!(error.code(), code.code, "{file}");
        let keys: Vec<&str> = error.details().iter().map(|detail| detail.key()).collect();
        assert_eq!(keys, code.details, "{file}");
        assert_eq!(error.hint(), hint(code.code, error.details()), "{file}");
        assert!(error.hint().is_some(), "{file}");
    }
}

#[test]
fn a_name_error_lists_its_field_issues_and_registry_not_deletable_names_the_instance() {
    let ServerFrame::Error(ErrorFrame { error, .. }) =
        decode("ipc/frame-error-instance-name-invalid.json")
    else {
        panic!("expected an error frame");
    };
    assert_eq!(error.fields()[0].path(), "name");
    let ServerFrame::Error(ErrorFrame { error, .. }) =
        decode("ipc/frame-error-registry-not-deletable.json")
    else {
        panic!("expected an error frame");
    };
    assert_eq!(
        error.resource().map(|resource| resource.kind),
        Some(stackly_protocol::ResourceKind::Instance)
    );
    assert_eq!(codes::registry::NOT_DELETABLE.details, ["reason"]);
}

// covers: AC-18
#[test]
fn every_command_case_writes_the_command_of_its_draft_and_roundtrips() {
    let cases: Cases = decode("instance/commands.json");
    assert!(cases.cases.len() >= 7);
    for case in cases.cases {
        assert_eq!(
            equivalent_command(&case.draft),
            case.command,
            "{}",
            case.name
        );
        let written = serde_json::to_value(&case.draft).expect("a draft encodes");
        let again: CommandDraft = serde_json::from_value(written).expect("a draft decodes");
        assert_eq!(again, case.draft, "{}", case.name);
        assert!(!case.command.contains("fixture-secret"), "{}", case.name);
    }
}

#[test]
fn the_commands_fixture_is_in_the_wire_form() {
    let fixture: serde_json::Value = decode("instance/commands.json");
    let cases: Cases = decode("instance/commands.json");
    for (case, raw) in cases
        .cases
        .iter()
        .zip(fixture["cases"].as_array().expect("cases is a list"))
    {
        assert_eq!(
            serde_json::to_value(&case.draft).expect("a draft encodes"),
            raw["draft"],
            "{}",
            case.name
        );
    }
}
