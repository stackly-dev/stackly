//! Checks the Open In DTOs of spec 0070 against `protocols/fixtures/`:
//! `instance/detail-open-actions.json` (labels and rows, shared with the
//! CLI and the Swift `OpenActionLabel`), `instance/prepare-terminal.json`,
//! and the `openActions` of the four detail triples (AC-3, AC-4, AC-14).

use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde_json::Value;
use stackly_protocol::codes::instance;
use stackly_protocol::instance::open_with::open_action_label;
use stackly_protocol::instance::{
    ConnectionAvailability, InstanceDetail, OpenAction, OpenActionKind, PrepareTerminalParams,
    PrepareTerminalReply,
};
use stackly_protocol::json::Validate;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures")
}

fn decode<T: DeserializeOwned>(path: &str) -> T {
    let path = fixtures_dir().join(path);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

#[derive(Debug, serde::Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

#[derive(Debug, serde::Deserialize)]
struct Case {
    action: OpenAction,
    label: String,
    row: [String; 4],
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Triple {
    detail: InstanceDetail,
}

// covers: AC-3
#[test]
fn every_label_case_gives_the_label_of_its_kind_and_title() {
    let cases: Cases = decode("instance/detail-open-actions.json");
    assert!(cases.cases.len() >= 7);
    for case in &cases.cases {
        assert_eq!(
            open_action_label(case.action.kind, &case.action.title),
            case.label,
            "{}",
            case.action.name
        );
        assert_eq!(case.row[0], case.action.name);
        assert_eq!(case.row[1], case.label);
    }
}

// covers: AC-3
#[test]
fn an_unknown_kind_reads_as_unrecognized() {
    let cases: Cases = decode("instance/detail-open-actions.json");
    let kinds: Vec<OpenActionKind> = cases.cases.iter().map(|case| case.action.kind).collect();
    for kind in [
        OpenActionKind::App,
        OpenActionKind::Terminal,
        OpenActionKind::Browser,
        OpenActionKind::Reveal,
        OpenActionKind::Unrecognized,
    ] {
        assert!(kinds.contains(&kind), "{kind:?}");
    }
}

/// Whether the fields of `action` agree with its state and kind: an
/// available action holds the value of its kind only, an unavailable one a
/// reason and no value or secret.
fn consistent(action: &OpenAction) -> bool {
    match action.state {
        ConnectionAvailability::Available => {
            action.reason.is_none()
                && match action.kind {
                    OpenActionKind::App => {
                        action.url.is_some()
                            && !action.app_bundle_ids.is_empty()
                            && action.command.is_none()
                            && action.path.is_none()
                    }
                    OpenActionKind::Browser => {
                        action.url.is_some() && action.command.is_none() && action.path.is_none()
                    }
                    OpenActionKind::Terminal => {
                        action.command.is_some() && action.url.is_none() && action.path.is_none()
                    }
                    OpenActionKind::Reveal => {
                        action.path.is_some() && action.url.is_none() && action.command.is_none()
                    }
                    OpenActionKind::Unrecognized => true,
                }
        }
        ConnectionAvailability::Unavailable => {
            action.reason.is_some()
                && action.url.is_none()
                && action.command.is_none()
                && action.path.is_none()
                && action.copy_secret.is_none()
        }
        ConnectionAvailability::Unrecognized => true,
    }
}

// covers: AC-3, AC-14
#[test]
fn every_open_action_of_the_triples_is_consistent_and_holds_no_secret_value() {
    for name in ["postgresql", "nats", "unavailable", "package-unreadable"] {
        let triple: Triple = decode(&format!("instance/detail-{name}.json"));
        for action in &triple.detail.open_actions {
            assert!(consistent(action), "{name} {}", action.name);
        }
        let text = serde_json::to_string(&triple.detail.open_actions).expect("encodes");
        assert!(!text.contains("fixture-secret"), "{name}");
    }
    let unreadable: Triple = decode("instance/detail-package-unreadable.json");
    assert!(unreadable.detail.open_actions.is_empty());
}

// covers: AC-3
#[test]
fn a_detail_without_open_actions_decodes_to_an_empty_list() {
    let mut value: Value = decode("instance/detail-postgresql.json");
    value["detail"]
        .as_object_mut()
        .expect("an object")
        .remove("openActions");
    let triple: Triple = serde_json::from_value(value).expect("an older daemon's detail decodes");
    assert!(triple.detail.open_actions.is_empty());
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareCases {
    request: PrepareTerminalParams,
    reply: PrepareTerminalReply,
    invalid_requests: Vec<PrepareTerminalParams>,
    errors: Vec<ErrorCase>,
}

#[derive(Debug, serde::Deserialize)]
struct ErrorCase {
    code: String,
    category: String,
    retryable: bool,
    details: serde_json::Map<String, Value>,
}

// covers: AC-4
#[test]
fn the_prepare_terminal_fixture_matches_the_params_rules_and_the_error_codes() {
    let cases: PrepareCases = decode("instance/prepare-terminal.json");
    assert!(cases.request.validate().is_empty());
    assert!(cases.reply.script_path.ends_with("/open-psql.command"));
    for invalid in &cases.invalid_requests {
        assert!(!invalid.validate().is_empty(), "{invalid:?}");
    }
    let specs = [
        instance::OPEN_ACTION_NOT_FOUND,
        instance::OPEN_ACTION_UNAVAILABLE,
        instance::OPEN_SCRIPT_UNWRITABLE,
    ];
    assert_eq!(cases.errors.len(), specs.len());
    for (case, spec) in cases.errors.iter().zip(specs) {
        assert_eq!(case.code, spec.code);
        assert_eq!(
            serde_json::to_value(spec.category).expect("encodes"),
            Value::String(case.category.clone())
        );
        assert_eq!(case.retryable, spec.retryable, "{}", case.code);
        let keys: Vec<&str> = case.details.keys().map(String::as_str).collect();
        let mut expected: Vec<&str> = spec.details.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected, "{}", case.code);
    }
}
