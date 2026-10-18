//! Runs every case in `protocols/fixtures/ipc/index.json` against the Rust
//! contract (spec 0006 AC-7, AC-8, AC-10). The Swift tests in
//! `clients/swift` read the same index.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use stackly_protocol::codes;
use stackly_protocol::events::{EventKind, GapReason, ShutdownReason};
use stackly_protocol::json::{DecodedFrame, decode_request, decode_server_frame, handshake_major};
use stackly_protocol::{
    Call, ErrorBody, ErrorFrame, Request, ResourceKind, ServerFrame, Timestamp,
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

fn as_decoded(frame: ServerFrame) -> DecodedFrame {
    match frame {
        ServerFrame::Response(response) => DecodedFrame::Response(response),
        ServerFrame::Error(error) => DecodedFrame::Error(error),
        ServerFrame::Event(event) => DecodedFrame::Event(event),
    }
}

/// AC-8: the code is in the catalog, category and `retryable` match it, and
/// every `details` key is one the catalog lists for the code.
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
}

#[test]
fn every_fixture_in_the_index_behaves_as_expected_in_rust() {
    let mut checked = 0;
    for entry in rust_entries() {
        let bytes = read(&entry.file);
        match entry.type_name.as_str() {
            "Request" => check_request(&entry, &bytes),
            "ServerFrame" => check_server_frame(&entry, &bytes),
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
fn foreign_values_of_open_enums_decode_as_unknown() {
    let request = decode_request(&read("request-handshake-newer-client.json")).unwrap();
    let Call::DaemonHandshake(params) = request.call else {
        panic!("expected a handshake");
    };
    assert_eq!(
        params.client.kind,
        stackly_protocol::handshake::ClientKind::Unknown
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
        stackly_protocol::daemon::HealthStatus::Unknown
    );
    assert_eq!(
        health.subsystems[0].status,
        stackly_protocol::daemon::SubsystemStatus::Unknown
    );

    let gap = event_kind("frame-event-gap-unknown-reason.json");
    assert!(matches!(gap, EventKind::EventsGap(g) if g.reason == GapReason::Unknown));
    let shutdown = event_kind("frame-event-shutdown-unknown-reason.json");
    assert!(
        matches!(shutdown, EventKind::DaemonShuttingDown(s) if s.reason == ShutdownReason::Unknown)
    );

    let frame: ServerFrame =
        serde_json::from_slice(&read("frame-error-unknown-resource-kind.json")).unwrap();
    let ServerFrame::Error(error) = frame else {
        panic!("expected an error frame");
    };
    assert_eq!(
        error.error.resource().map(|r| r.kind),
        Some(ResourceKind::Unknown)
    );
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
