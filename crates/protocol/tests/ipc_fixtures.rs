//! Runs every case in `protocols/fixtures/ipc/index.json` against the Rust
//! contract (spec 0006 AC-7, AC-8, AC-10). The Swift tests in
//! `clients/swift` read the same index.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stackly_protocol::codes;
use stackly_protocol::events::{EventKind, GapReason, ShutdownReason};
use stackly_protocol::json::{DecodedFrame, decode_request, decode_server_frame, handshake_major};
use stackly_protocol::operation::{
    CancelDisposition, CreatedState, OperationAccepted, OperationCursor, OperationKind,
    OperationState, PartialOutcome, RecoveryAction, RecoveryCause, RecoveryHint, StepState,
    TriggerOrigin,
};
use stackly_protocol::secret::SecretInput;
use stackly_protocol::{
    Call, ErrorBody, ErrorFrame, Reply, Request, ResourceKind, ResourceRef, ServerFrame, Timestamp,
};

#[derive(Debug, Deserialize)]
struct Index {
    fixtures: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    file: String,
    #[serde(rename = "type")]
    type_name: String,
    expect: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    langs: Option<Vec<String>>,
}

impl Entry {
    fn applies_to_rust(&self) -> bool {
        self.langs
            .as_ref()
            .is_none_or(|langs| langs.iter().any(|lang| lang == "rust"))
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures/ipc")
}

fn read(file: &str) -> Vec<u8> {
    let path = fixtures_dir().join(file);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn index() -> Vec<Entry> {
    let index: Index =
        serde_json::from_slice(&read("index.json")).expect("index.json matches its shape");
    index.fixtures
}

fn rust_entries() -> impl Iterator<Item = Entry> {
    index().into_iter().filter(Entry::applies_to_rust)
}

/// Every request decoded by the staged decoder, by file.
fn decoded_requests(expect: &str) -> Vec<(String, Request)> {
    rust_entries()
        .filter(|entry| entry.type_name == "Request" && entry.expect == expect)
        .map(|entry| {
            let request = decode_request(&read(&entry.file))
                .unwrap_or_else(|err| panic!("{}: {err:?}", entry.file));
            (entry.file, request)
        })
        .collect()
}

/// Every server frame decoded with serde, by file.
fn decoded_frames(expect: &str) -> Vec<(String, ServerFrame)> {
    rust_entries()
        .filter(|entry| entry.type_name == "ServerFrame" && entry.expect == expect)
        .map(|entry| {
            let frame = serde_json::from_slice(&read(&entry.file))
                .unwrap_or_else(|err| panic!("{}: {err}", entry.file));
            (entry.file, frame)
        })
        .collect()
}

fn check_request(entry: &Entry, bytes: &[u8]) {
    let decoded = decode_request(bytes);
    match entry.expect.as_str() {
        "roundtrip" => {
            let request = decoded.unwrap_or_else(|err| panic!("{}: {err:?}", entry.file));
            let by_serde: Request = serde_json::from_slice(bytes)
                .expect("a roundtrip Request fixture decodes with serde");
            assert_eq!(
                by_serde, request,
                "{}: serde and the staged decoder disagree",
                entry.file
            );
            let encoded = serde_json::to_vec(&request).expect("a decoded value always encodes");
            assert_eq!(
                decode_request(&encoded).expect("a decoded value decodes again"),
                request,
                "{}",
                entry.file
            );
            let fixture: Value = serde_json::from_slice(bytes).expect("a fixture is JSON");
            let written: Value = serde_json::from_slice(&encoded).expect("encoded output is JSON");
            assert_eq!(
                written, fixture,
                "{}: Rust writes another shape",
                entry.file
            );
        }
        "decode" => {
            decoded.unwrap_or_else(|err| panic!("{}: {err:?}", entry.file));
        }
        "reject" => {
            let rejection = decoded.expect_err(&entry.file);
            let code = entry
                .code
                .as_deref()
                .expect("a reject fixture names its code");
            assert_eq!(rejection.error.code(), code, "{}", entry.file);
            assert_catalog_conform(&rejection.error, &entry.file);
        }
        other => panic!("{}: expect `{other}` does not apply to Request", entry.file),
    }
}

fn check_server_frame(entry: &Entry, bytes: &[u8]) {
    let decoded = decode_server_frame(bytes);
    match entry.expect.as_str() {
        "roundtrip" => {
            let frame: ServerFrame =
                serde_json::from_slice(bytes).unwrap_or_else(|err| panic!("{}: {err}", entry.file));
            let encoded = serde_json::to_vec(&frame).expect("a decoded value always encodes");
            let again: ServerFrame =
                serde_json::from_slice(&encoded).expect("an encoded frame decodes again");
            assert_eq!(again, frame, "{}", entry.file);
            let fixture: Value = serde_json::from_slice(bytes).expect("a fixture is JSON");
            let written: Value = serde_json::from_slice(&encoded).expect("encoded output is JSON");
            assert_eq!(
                written, fixture,
                "{}: Rust writes another shape",
                entry.file
            );
            let decoded = decoded.unwrap_or_else(|err| panic!("{}: {err:?}", entry.file));
            assert_eq!(decoded, as_decoded(frame), "{}", entry.file);
        }
        "decode" => {
            let decoded = decoded.unwrap_or_else(|err| panic!("{}: {err:?}", entry.file));
            assert!(
                !matches!(decoded, DecodedFrame::SkippedEvent { .. }),
                "{}: decoded as skipped",
                entry.file
            );
        }
        "skipped" => {
            let decoded = decoded.unwrap_or_else(|err| panic!("{}: {err:?}", entry.file));
            assert!(
                matches!(decoded, DecodedFrame::SkippedEvent { .. }),
                "{}: {decoded:?}",
                entry.file
            );
        }
        "reject" => {
            let error = decoded.expect_err(&entry.file);
            if let Some(code) = &entry.code {
                assert_eq!(error.code(), code, "{}", entry.file);
            }
        }
        other => panic!("{}: unknown expect `{other}`", entry.file),
    }
}

/// A shared DTO that no method carries yet, such as `SecretInput`: its
/// fixture decodes, encodes to the same JSON, and decodes again.
fn check_dto<T>(entry: &Entry, bytes: &[u8])
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let decoded: T =
        serde_json::from_slice(bytes).unwrap_or_else(|err| panic!("{}: {err}", entry.file));
    match entry.expect.as_str() {
        "roundtrip" => {
            let encoded = serde_json::to_vec(&decoded).expect("a decoded value always encodes");
            let again: T = serde_json::from_slice(&encoded).expect("an encoded value decodes");
            assert_eq!(again, decoded, "{}", entry.file);
            let fixture: Value = serde_json::from_slice(bytes).expect("a fixture is JSON");
            let written: Value = serde_json::from_slice(&encoded).expect("encoded output is JSON");
            assert_eq!(
                written, fixture,
                "{}: Rust writes another shape",
                entry.file
            );
        }
        "decode" => {}
        other => panic!("{}: expect `{other}` does not apply to a DTO", entry.file),
    }
}

fn as_decoded(frame: ServerFrame) -> DecodedFrame {
    match frame {
        ServerFrame::Response(response) => DecodedFrame::Response(response),
        ServerFrame::Error(error) => DecodedFrame::Error(error),
        ServerFrame::Event(event) => DecodedFrame::Event(event),
    }
}

/// AC-8: the code is in the catalog, category and `retryable` match it,
/// every `details` key is one the catalog lists for the code, and `items`
/// is empty or holds only the kind and field keys the code allows, at most
/// its limit.
fn assert_catalog_conform(error: &ErrorBody, file: &str) {
    let spec = error
        .spec()
        .unwrap_or_else(|| panic!("{file}: `{}` is not in the catalog", error.code()));
    assert_eq!(error.category(), spec.category, "{file}");
    assert_eq!(error.retryable(), spec.retryable, "{file}");
    for detail in error.details() {
        assert!(
            spec.details.contains(&detail.key()),
            "{file}: detail `{}` is not listed for `{}`",
            detail.key(),
            spec.code
        );
    }
    assert!(error.message().chars().count() <= 200, "{file}");
    match spec.items {
        None => assert!(
            error.items().is_empty(),
            "{file}: `{}` allows no items",
            spec.code
        ),
        Some(items) => {
            assert!(error.items().len() <= usize::from(items.max), "{file}");
            for item in error.items() {
                assert_eq!(item.kind(), items.kind, "{file}");
                for field in item.fields() {
                    assert!(items.fields.contains(&field.key()), "{file}: {field:?}");
                }
            }
        }
    }
}

#[test]
fn every_fixture_in_the_index_behaves_as_expected_in_rust() {
    let mut checked = 0;
    for entry in rust_entries() {
        let bytes = read(&entry.file);
        match entry.type_name.as_str() {
            "Request" => check_request(&entry, &bytes),
            "ServerFrame" => check_server_frame(&entry, &bytes),
            "SecretInput" => check_dto::<SecretInput>(&entry, &bytes),
            "OperationAccepted" => check_dto::<OperationAccepted>(&entry, &bytes),
            other => panic!("{}: no decoder for type `{other}`", entry.file),
        }
        checked += 1;
    }
    assert!(checked > 30, "only {checked} fixtures ran");
}

#[test]
fn every_file_in_the_fixture_folder_is_listed_in_the_index() {
    let listed: BTreeSet<String> = index().into_iter().map(|entry| entry.file).collect();
    let on_disk: BTreeSet<String> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "index.json")
        .collect();
    assert_eq!(listed, on_disk);
}

#[test]
fn every_call_reply_frame_and_event_kind_has_a_roundtrip_fixture() {
    let calls: BTreeSet<&str> = decoded_requests("roundtrip")
        .iter()
        .map(|(_, request)| request.call.method())
        .collect();
    assert_eq!(calls, Call::METHODS.iter().copied().collect());

    let frames = decoded_frames("roundtrip");
    let mut replies = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    let mut frame_types = BTreeSet::new();
    for (_, frame) in &frames {
        match frame {
            ServerFrame::Response(response) => {
                frame_types.insert("response");
                replies.insert(response.reply.method());
            }
            ServerFrame::Error(_) => {
                frame_types.insert("error");
            }
            ServerFrame::Event(event) => {
                frame_types.insert("event");
                kinds.insert(event.kind.name());
            }
        }
    }
    assert_eq!(replies, Call::METHODS.iter().copied().collect());
    assert_eq!(kinds, EventKind::KINDS.iter().copied().collect());
    assert_eq!(frame_types, BTreeSet::from(["response", "error", "event"]));
}

#[test]
fn every_request_decode_code_has_a_reject_fixture() {
    let codes: BTreeSet<String> = index()
        .into_iter()
        .filter(|entry| entry.type_name == "Request" && entry.expect == "reject")
        .filter_map(|entry| entry.code)
        .collect();
    for spec in [
        codes::request::MALFORMED,
        codes::request::TOO_DEEP,
        codes::request::UNKNOWN_METHOD,
        codes::request::INVALID_PARAMS,
    ] {
        assert!(
            codes.contains(spec.code),
            "no reject fixture for {}",
            spec.code
        );
    }
}

#[test]
fn every_error_fixture_conforms_to_the_catalog() {
    for (file, frame) in decoded_frames("roundtrip") {
        if let ServerFrame::Error(ErrorFrame { error, .. }) = frame {
            assert_catalog_conform(&error, &file);
        }
    }
}

#[test]
fn a_major_2_handshake_with_broken_params_still_shows_its_major() {
    let bytes = read("reject-handshake-major-2.json");
    assert_eq!(handshake_major(&bytes), Some(2));
    assert_eq!(handshake_major(&read("request-handshake.json")), Some(1));
}

#[test]
fn an_invalid_params_error_names_the_path_but_never_the_input_value() {
    let bytes = read("reject-invalid-params-type.json");
    let rejection = decode_request(&bytes).unwrap_err();
    let field = &rejection.error.fields()[0];
    assert_eq!(field.path(), "since.sequence");
    assert_eq!(field.code(), "invalid_value");
    let frame = serde_json::to_string(&rejection.into_frame()).unwrap();
    assert!(!frame.contains("secret-sentinel"), "{frame}");
}

#[test]
fn content_before_type_decodes_like_the_usual_order() {
    let reordered = decode_request(&read("request-content-before-type.json")).unwrap();
    let usual = decode_request(&read("request-unsubscribe.json")).unwrap();
    assert_eq!(reordered.call, usual.call);
}

#[test]
fn foreign_values_of_open_enums_decode_as_unrecognized() {
    let request = decode_request(&read("request-handshake-newer-client.json")).unwrap();
    let Call::DaemonHandshake(params) = request.call else {
        panic!("expected a handshake");
    };
    assert_eq!(
        params.client.kind,
        stackly_protocol::handshake::ClientKind::Unrecognized
    );

    let frame: ServerFrame =
        serde_json::from_slice(&read("frame-response-health-unknown-statuses.json")).unwrap();
    let ServerFrame::Response(response) = frame else {
        panic!("expected a response");
    };
    let stackly_protocol::Reply::DaemonGetHealth(health) = response.reply else {
        panic!("expected a health reply");
    };
    assert_eq!(
        health.status,
        stackly_protocol::daemon::DaemonHealthStatus::Unrecognized
    );
    assert_eq!(
        health.subsystems[0].status,
        stackly_protocol::daemon::SubsystemStatus::Unrecognized
    );

    let gap = event_kind("frame-event-gap-unknown-reason.json");
    assert!(matches!(gap, EventKind::EventsGap(g) if g.reason == GapReason::Unrecognized));
    let shutdown = event_kind("frame-event-shutdown-unknown-reason.json");
    assert!(
        matches!(shutdown, EventKind::DaemonShuttingDown(s) if s.reason == ShutdownReason::Unrecognized)
    );

    let frame: ServerFrame =
        serde_json::from_slice(&read("frame-error-unknown-resource-kind.json")).unwrap();
    let ServerFrame::Error(error) = frame else {
        panic!("expected an error frame");
    };
    assert_eq!(
        error.error.resource().map(|r| r.kind),
        Some(ResourceKind::Unrecognized)
    );
}

#[test]
fn a_handshake_without_interactive_reads_as_not_interactive() {
    let request = decode_request(&read("request-handshake-without-interactive.json")).unwrap();
    let Call::DaemonHandshake(params) = request.call else {
        panic!("expected a handshake");
    };
    assert_eq!(params.client.interactive, None);
    assert!(!params.client.is_interactive());

    let request = decode_request(&read("request-handshake.json")).unwrap();
    let Call::DaemonHandshake(params) = request.call else {
        panic!("expected a handshake");
    };
    assert!(params.client.is_interactive());
}

fn reply(file: &str) -> Reply {
    let frame: ServerFrame =
        serde_json::from_slice(&read(file)).expect("the fixture is a ServerFrame");
    let ServerFrame::Response(response) = frame else {
        panic!("{file}: expected a response");
    };
    response.reply
}

#[test]
fn info_carries_the_registry_schema_and_an_older_daemon_reads_without_it() {
    let Reply::DaemonGetInfo(info) = reply("frame-response-get-info.json") else {
        panic!("expected an info reply");
    };
    assert_eq!(info.registry_schema, Some(1));
    let Reply::DaemonGetInfo(older) = reply("frame-response-get-info-older-daemon.json") else {
        panic!("expected an info reply");
    };
    assert_eq!(older.registry_schema, None);
}

#[test]
fn health_in_recovery_mode_carries_the_report() {
    let Reply::DaemonGetHealth(health) = reply("frame-response-get-health-recovery.json") else {
        panic!("expected a health reply");
    };
    let recovery = health.recovery.expect("the fixture is in recovery mode");
    assert_eq!(recovery.reason, "schema_newer_than_app");
    assert_eq!(
        (recovery.app_schema_version, recovery.db_schema_version),
        (2, Some(999))
    );
    assert!(recovery.report_path.ends_with(".json"));
    let Reply::DaemonGetHealth(ready) = reply("frame-response-get-health.json") else {
        panic!("expected a health reply");
    };
    assert_eq!(ready.recovery, None);
}

fn error_body(file: &str) -> ErrorBody {
    let frame: ServerFrame =
        serde_json::from_slice(&read(file)).expect("the fixture is a ServerFrame");
    let ServerFrame::Error(frame) = frame else {
        panic!("{file}: expected an error frame");
    };
    frame.error
}

#[test]
fn an_error_about_the_manager_names_the_resource_self() {
    let error = error_body("frame-error-manager-resource.json");
    assert_eq!(error.resource(), Some(&ResourceRef::manager()));
}

#[test]
fn error_items_decode_whatever_their_kind_and_keep_the_envelope() {
    let error = error_body("frame-error-with-items.json");
    assert_eq!(error.items().len(), 2);
    assert_eq!(error.items()[0].kind(), "issue");
    assert_eq!(error.items()[1].field("path"), Some("metadata.name"));
    assert_eq!(error.detail("omitted"), Some("1"));

    let error = error_body("frame-error-unknown-item-kind.json");
    assert_eq!(error.code(), "secret.reference_unresolved");
    assert_eq!(error.category(), stackly_protocol::ErrorCategory::Failed);
    assert_eq!(error.message(), "2 references need a secret");
    assert_eq!(error.items()[0].kind(), "hologram");
    assert_eq!(error.items()[0].field("beam"), Some("blue"));
}

#[test]
fn an_offset_time_is_read_as_the_same_instant_in_utc() {
    let frame: ServerFrame =
        serde_json::from_slice(&read("frame-event-time-with-offset.json")).unwrap();
    let ServerFrame::Event(event) = frame else {
        panic!("expected an event");
    };
    let expected: Timestamp = "2026-10-18T03:04:05Z".parse().unwrap();
    assert_eq!(event.at, expected);
    assert_eq!(event.at.to_string(), "2026-10-18T03:04:05.000Z");
}

#[test]
fn an_uppercase_uuid_reads_as_the_same_uuid() {
    let request = decode_request(&read("request-subscribe-uppercase-uuid.json")).unwrap();
    let Call::EventsSubscribe(params) = request.call else {
        panic!("expected a subscribe");
    };
    let id = params.since.unwrap().server_instance_id;
    assert_eq!(id.to_string(), "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e");
}

fn event_kind(file: &str) -> EventKind {
    let frame: ServerFrame =
        serde_json::from_slice(&read(file)).expect("the fixture is a ServerFrame");
    let ServerFrame::Event(event) = frame else {
        panic!("{file}: expected an event");
    };
    event.kind
}

fn dto<T: DeserializeOwned>(file: &str) -> T {
    serde_json::from_slice(&read(file)).unwrap_or_else(|err| panic!("{file}: {err}"))
}

#[test]
fn every_secret_input_source_has_a_fixture_and_a_value_is_read_as_secret_text() {
    let SecretInput::Value(text) = dto("dto-secret-input-value.json") else {
        panic!("expected a value input");
    };
    assert_eq!(text.expose(), "fixture-secret-0001");
    assert_eq!(format!("{text:?}"), "SecretText(****)");
    assert_eq!(
        dto::<SecretInput>("dto-secret-input-generate.json"),
        SecretInput::Generate
    );
    assert_eq!(
        dto::<SecretInput>("dto-secret-input-copy-from.json"),
        SecretInput::CopyFrom("local:erp/postgres/admin".to_owned())
    );
    let SecretInput::Bind(reference) = dto("dto-secret-input-bind.json") else {
        panic!("expected a bind input");
    };
    assert!(stackly_protocol::secret::is_secret_reference(&reference));
    for file in [
        "dto-secret-input-value.json",
        "dto-secret-input-generate.json",
        "dto-secret-input-copy-from.json",
        "dto-secret-input-bind.json",
    ] {
        use stackly_protocol::json::Validate as _;
        assert!(dto::<SecretInput>(file).validate().is_empty(), "{file}");
    }
}

#[test]
fn an_operation_list_request_carries_a_cursor_the_daemon_wrote() {
    let request = decode_request(&read("request-operation-list.json")).unwrap();
    let Call::OperationList(params) = request.call else {
        panic!("expected operation.list");
    };
    assert_eq!(params.limit_or_default(), 20);
    let before = params.before.expect("the fixture pages backwards");
    assert!(OperationCursor::parse(&before).is_some(), "{before}");

    let request = decode_request(&read("request-operation-list-without-fields.json")).unwrap();
    let Call::OperationList(params) = request.call else {
        panic!("expected operation.list");
    };
    assert!(!params.active_only);
    assert_eq!(params.limit_or_default(), 50);

    let Reply::OperationList(page) = reply("frame-response-operation-list.json") else {
        panic!("expected an operation.list reply");
    };
    let next = page.next.expect("the fixture has a next page");
    let cursor = OperationCursor::parse(&next).expect("the next cursor is valid");
    assert_eq!(cursor.id(), page.items[1].id);
}

#[test]
fn a_limit_above_200_is_refused_on_the_limit_field() {
    let rejection = decode_request(&read("reject-operation-list-limit.json")).unwrap_err();
    assert_eq!(rejection.error.code(), "request.invalid_params");
    let field = &rejection.error.fields()[0];
    assert_eq!((field.path(), field.code()), ("limit", "out_of_range"));
}

#[test]
fn both_operation_events_are_about_the_operation_at_its_revision() {
    for file in [
        "frame-event-operation-state-changed.json",
        "frame-event-operation-progress.json",
    ] {
        let frame: ServerFrame = serde_json::from_slice(&read(file)).unwrap();
        let ServerFrame::Event(event) = frame else {
            panic!("{file}: expected an event");
        };
        let (EventKind::OperationStateChanged(summary) | EventKind::OperationProgress(summary)) =
            &event.kind
        else {
            panic!("{file}: expected an operation event");
        };
        let resource = event
            .resource
            .as_ref()
            .expect("an operation event names it");
        assert_eq!(resource.kind, ResourceKind::Operation, "{file}");
        assert_eq!(resource.id, summary.id.to_string(), "{file}");
        assert_eq!(event.revision, Some(summary.revision), "{file}");
    }
}

#[test]
fn an_abandoned_operation_lists_its_leftovers_in_the_reply_and_the_result() {
    let Reply::OperationAbandon(reply) = reply("frame-response-operation-abandon.json") else {
        panic!("expected an operation.abandon reply");
    };
    assert_eq!(reply.operation.state, OperationState::Failed);
    let failure = reply
        .operation
        .failure
        .as_ref()
        .expect("a failed operation");
    assert_eq!(failure.reason, "abandoned");
    assert!(failure.error.is(&codes::operation::ABANDONED));
    assert_eq!(failure.error.operation_id(), Some(reply.operation.id));
    let partial = &reply.operation.result.as_ref().expect("a result").partial;
    assert!(
        partial
            .iter()
            .any(|item| item.outcome == PartialOutcome::LeftOver)
    );
    assert_eq!(reply.leftovers[0].state, CreatedState::Created);
}

#[test]
fn an_operation_that_needs_recovery_carries_its_hint_cause_and_actions() {
    let Reply::OperationGet(detail) = reply("frame-response-operation-get.json") else {
        panic!("expected an operation.get reply");
    };
    let recovery = detail.operation.recovery.as_ref().expect("needs recovery");
    assert_eq!(detail.operation.state, OperationState::NeedsRecovery);
    assert_eq!(
        (recovery.hint, recovery.cause),
        (RecoveryHint::Abandon, RecoveryCause::CompensationFailed)
    );
    assert_eq!(recovery.actions, [RecoveryAction::Abandon]);
    assert_eq!(detail.operation.failure, None);
    assert_eq!(detail.steps[1].state, StepState::Failed);
    assert_eq!(detail.created[0].state, CreatedState::CompensationFailed);
}

#[test]
fn foreign_values_of_the_operation_enums_decode_as_unrecognized() {
    let Reply::OperationGet(detail) = reply("frame-response-operation-get-unknown-values.json")
    else {
        panic!("expected an operation.get reply");
    };
    let operation = &detail.operation;
    assert_eq!(operation.kind, OperationKind::Unrecognized);
    assert_eq!(operation.trigger.origin, TriggerOrigin::Unrecognized);
    assert_eq!(
        operation.trigger.client,
        Some(stackly_protocol::handshake::ClientKind::Unrecognized)
    );
    let recovery = operation.recovery.as_ref().expect("needs recovery");
    assert_eq!(recovery.hint, RecoveryHint::Unrecognized);
    assert_eq!(recovery.cause, RecoveryCause::Unrecognized);
    assert_eq!(
        recovery.actions,
        [RecoveryAction::Unrecognized, RecoveryAction::Abandon]
    );
    let partial = &operation.result.as_ref().expect("a result").partial;
    assert_eq!(partial[0].outcome, PartialOutcome::Unrecognized);
    assert_eq!(detail.steps[1].state, StepState::Unrecognized);
    assert_eq!(detail.created[0].state, CreatedState::Unrecognized);

    let Reply::OperationCancel(cancel) =
        reply("frame-response-operation-cancel-unknown-disposition.json")
    else {
        panic!("expected an operation.cancel reply");
    };
    assert_eq!(cancel.disposition, CancelDisposition::Unrecognized);
}

#[test]
fn an_operation_accepted_reply_names_the_operation_and_whether_it_replayed() {
    let accepted: OperationAccepted = dto("dto-operation-accepted.json");
    assert!(!accepted.replayed);
    assert_eq!(accepted.operation.state, OperationState::Queued);
    assert_eq!(accepted.operation.revision.get(), 1);
}

#[test]
fn a_reused_idempotency_key_names_the_first_operation() {
    let error = error_body("frame-error-idempotency-key-reused.json");
    assert!(error.is(&codes::operation::IDEMPOTENCY_KEY_REUSED));
    assert_eq!(
        error.detail("operationId"),
        Some("0192f3a4-0000-7000-8000-000000000001")
    );
}
