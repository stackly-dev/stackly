//! Checks the detail DTOs of spec 0021 against `protocols/fixtures/`: the
//! four `detail-*.json` triples in `instance/` and the requests, replies,
//! and errors of `instance.get_detail` and `instance.get_data_usage` in
//! `ipc/` (AC-17).

use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde_json::Value;
use stackly_protocol::codes::instance;
use stackly_protocol::instance::connection::{ConnectionPartKind, compose};
use stackly_protocol::instance::{
    ConnectionAvailability, ConnectionString, ConnectionUnavailableReason, DataUsage,
    DataUsageReply, DependentOwnership, InstanceDetail, InstanceSummary, StartupPolicyKind,
};
use stackly_protocol::json::{Validate, decode_request};
use stackly_protocol::secret::SecretStatus;
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

/// One `detail-*.json` file: the same three values `stackly instance
/// inspect --json` writes as its `result`.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Triple {
    instance: InstanceSummary,
    detail: InstanceDetail,
    data_usage: Option<DataUsage>,
}

const TRIPLES: [&str; 4] = ["postgresql", "nats", "unavailable", "package-unreadable"];

fn triple(name: &str) -> Triple {
    decode(&format!("instance/detail-{name}.json"))
}

fn reply(file: &str) -> Reply {
    let ServerFrame::Response(response) = decode(&format!("ipc/{file}")) else {
        panic!("{file}: expected a response");
    };
    response.reply
}

// covers: AC-17
#[test]
fn every_detail_triple_roundtrips_in_the_wire_form() {
    for name in TRIPLES {
        let file = format!("instance/detail-{name}.json");
        let value: Triple = decode(&file);
        let written = serde_json::to_value(&value).expect("a decoded value always encodes");
        let fixture: Value = decode(&file);
        assert_eq!(written, fixture, "{file}: Rust writes another shape");
    }
}

// covers: AC-17
#[test]
fn every_triple_belongs_to_one_instance_at_one_revision() {
    for name in TRIPLES {
        let triple = triple(name);
        assert_eq!(triple.instance.id, triple.detail.instance_id, "{name}");
        assert_eq!(triple.instance.revision, triple.detail.revision, "{name}");
        assert_eq!(
            triple.instance.provider.name, triple.detail.provider.name,
            "{name}"
        );
        assert_eq!(
            triple.instance.runtime_id, triple.detail.runtime.id,
            "{name}"
        );
    }
}

// covers: AC-17
#[test]
fn a_connection_string_is_available_only_with_masked_parts_and_no_reason() {
    for name in TRIPLES {
        for connection in &triple(name).detail.connections {
            assert!(connection.is_consistent(), "{name}: {}", connection.name);
        }
    }
}

// covers: AC-4, AC-7
#[test]
fn an_unreadable_package_has_no_name_and_no_connection_and_the_others_are_readable() {
    for name in TRIPLES {
        let detail = triple(name).detail;
        if detail.provider.package_readable {
            assert!(detail.provider.display_name.is_some(), "{name}");
        } else {
            assert_eq!(name, "package-unreadable");
            assert_eq!(detail.provider.display_name, None);
            assert!(detail.connections.is_empty());
            assert!(
                detail
                    .secrets
                    .iter()
                    .all(|secret| secret.title == secret.field),
                "the title falls back to the field name"
            );
        }
    }
    assert!(
        !triple("package-unreadable")
            .detail
            .provider
            .package_readable
    );
}

// covers: AC-2, AC-3, AC-17
#[test]
fn the_masked_string_is_the_composed_one_with_every_secret_as_four_stars() {
    let detail = triple("postgresql").detail;
    for connection in &detail.connections {
        let stars: std::collections::BTreeMap<String, String> = connection
            .parts
            .iter()
            .filter_map(|part| part.secret.as_ref())
            .map(|secret| (secret.reference.clone(), "****".to_owned()))
            .collect();
        // percent encoding leaves four stars as %2A%2A%2A%2A, so compare
        // with the encoded form of the mask
        let composed = compose(&connection.parts, &stars).expect("every reference has a value");
        assert_eq!(
            composed.replace("%2A%2A%2A%2A", "****"),
            connection.masked.clone().expect("available"),
            "{}",
            connection.name
        );
    }
    let secret_fields: Vec<_> = detail.secrets.iter().map(|s| s.field.as_str()).collect();
    assert_eq!(secret_fields, ["superuserPassword"]);
}

// covers: AC-17
#[test]
fn the_triples_cover_available_unavailable_and_port_may_change() {
    let postgresql = triple("postgresql").detail;
    let nats = triple("nats").detail;
    let retained = triple("unavailable").detail;

    assert!(
        postgresql
            .connections
            .iter()
            .all(|c| c.state == ConnectionAvailability::Available && !c.port_may_change)
    );
    assert!(nats.connections.iter().all(|c| {
        c.state == ConnectionAvailability::Unavailable
            && c.reason == Some(ConnectionUnavailableReason::PortNotAssigned)
            && c.port_may_change
    }));
    assert!(
        retained
            .connections
            .iter()
            .all(|c| c.reason == Some(ConnectionUnavailableReason::Retained))
    );
    assert_eq!(
        postgresql.dependents.len(),
        2,
        "one dedicated with a lease, one shared without"
    );
    assert_eq!(
        triple("unavailable").data_usage.map(|u| u.complete),
        Some(false)
    );
    assert_eq!(triple("nats").data_usage, None);
}

fn assert_no_secret_value(detail: &InstanceDetail) {
    let text = serde_json::to_string(detail).expect("encodes");
    assert!(
        !text.contains("fixture-secret"),
        "no secret value in a detail"
    );
}

// covers: AC-18
#[test]
fn no_detail_fixture_holds_a_secret_value() {
    for name in TRIPLES {
        assert_no_secret_value(&triple(name).detail);
    }
}

// covers: AC-17
#[test]
fn the_requests_decode_and_the_conflicting_flags_are_rejected() {
    for (file, refresh, cached_only) in [
        ("request-instance-get-data-usage.json", false, true),
        ("request-instance-get-data-usage-refresh.json", true, false),
    ] {
        let request = decode_request(&read(&format!("ipc/{file}"))).expect(file);
        let Call::InstanceGetDataUsage(params) = request.call else {
            panic!("{file}: expected get_data_usage");
        };
        assert_eq!((params.refresh, params.cached_only), (refresh, cached_only));
        assert!(params.validate().is_empty());
    }
    let rejection = decode_request(&read(
        "ipc/reject-instance-get-data-usage-refresh-and-cached-only.json",
    ))
    .expect_err("both flags together");
    assert_eq!(rejection.error.code(), "request.invalid_params");
    assert!(
        rejection
            .error
            .fields()
            .iter()
            .any(|field| field.path() == "cachedOnly" && field.code() == "invalid_value")
    );
    let Call::InstanceGetDetail(params) =
        decode_request(&read("ipc/request-instance-get-detail.json"))
            .expect("decodes")
            .call
    else {
        panic!("expected get_detail");
    };
    assert!(params.validate().is_empty());
}

// covers: AC-17
#[test]
fn the_reply_fixtures_decode_to_the_detail_and_the_usage() {
    let Reply::InstanceGetDetail(detail) = reply("frame-response-instance-get-detail.json") else {
        panic!("expected a detail");
    };
    assert_eq!(*detail, triple("postgresql").detail);
    let Reply::InstanceGetDetail(unreadable) =
        reply("frame-response-instance-get-detail-package-unreadable.json")
    else {
        panic!("expected a detail");
    };
    assert_eq!(*unreadable, triple("package-unreadable").detail);
    let usage = |file: &str| match reply(file) {
        Reply::InstanceGetDataUsage(DataUsageReply { usage }) => usage,
        other => panic!("{file}: {other:?}"),
    };
    assert_eq!(
        usage("frame-response-instance-get-data-usage.json"),
        triple("postgresql").data_usage
    );
    assert_eq!(
        usage("frame-response-instance-get-data-usage-incomplete.json"),
        triple("unavailable").data_usage
    );
    assert_eq!(
        usage("frame-response-instance-get-data-usage-empty.json"),
        None
    );
}

// covers: AC-17
#[test]
fn every_open_enum_of_the_detail_reads_an_unknown_value_as_unrecognized() {
    let Reply::InstanceGetDetail(detail) =
        reply("frame-response-instance-get-detail-unknown-values.json")
    else {
        panic!("expected a detail");
    };
    assert_eq!(detail.startup, StartupPolicyKind::Unrecognized);
    assert_eq!(
        detail.dependents[0].ownership,
        DependentOwnership::Unrecognized
    );
    assert_eq!(detail.secrets[0].status, SecretStatus::Unrecognized);
    assert_eq!(
        detail.runtime.install_mode,
        stackly_protocol::runtime::InstallModeKind::Unrecognized
    );
    assert_eq!(
        detail.connections[0].state,
        ConnectionAvailability::Unrecognized
    );
    assert_eq!(
        detail.connections[1].parts[1].kind,
        ConnectionPartKind::Unrecognized
    );
    let later: &ConnectionString = &detail.connections[2];
    assert_eq!(
        later.reason,
        Some(ConnectionUnavailableReason::Unrecognized)
    );
}

// covers: AC-17
#[test]
fn the_data_usage_errors_carry_the_static_hint_of_their_reason() {
    for (file, reason) in [
        ("missing", "missing"),
        ("not-private", "not_private"),
        ("io", "io"),
    ] {
        let ServerFrame::Error(frame) = decode(&format!(
            "ipc/frame-error-instance-data-usage-unavailable-{file}.json"
        )) else {
            panic!("{file}: expected an error");
        };
        let error = frame.error;
        assert_eq!(error.code(), instance::DATA_USAGE_UNAVAILABLE.code);
        assert_eq!(
            error
                .details()
                .iter()
                .find(|d| d.key() == "reason")
                .map(|d| d.value()),
            Some(reason)
        );
        assert_eq!(error.hint(), instance::hint(error.code(), error.details()));
        assert!(error.hint().is_some());
    }
}
