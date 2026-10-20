//! The `nats-readiness` state machine (spec 0023, *Algoritma satu
//! percobaan*, AC-5): the transcripts nats-server 2.15.1 sent in the step 0
//! spike, then one synthetic transcript per result and per protocol error.
//! Every transcript is fed whole, byte by byte, and in other chunk sizes,
//! and each way must write the same bytes and reach the same result.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use stackly_core::provider::probe::NativeProbeFailure;
use stackly_core::provider::probe::nats::{
    Exchange, Expectation, NATS_DETAIL_VALUE_MAX, NATS_LINE_MAX, NATS_READ_MAX, NATS_REPLY_MAX,
    Phase, connect_bytes, connect_failed, connect_timed_out, unix_target,
};
use stackly_evidence::{Kind, evidence};

/// The inbox token of every transcript here and of the step 0 spike.
const TOKEN: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];
const INBOX: &str = "_INBOX.stackly.00112233445566778899aabbccddeeff";
const ACCOUNT_INFO: &str =
    r#"{"type":"io.nats.jetstream.api.v1.account_info_response","memory":0,"storage":0}"#;

/// What one attempt wrote and how it ended.
type Outcome = (Vec<u8>, Option<Result<(), NativeProbeFailure>>);

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nats-protocol")
}

fn expect(jetstream: bool) -> Expectation {
    Expectation {
        server_name: "golden".to_owned(),
        jetstream,
    }
}

/// Feeds `bytes` in chunks whose sizes come from `sizes`, until a result.
fn run_with(expectation: &Expectation, bytes: &[u8], mut sizes: impl FnMut() -> usize) -> Outcome {
    let mut exchange = Exchange::new(expectation.clone(), TOKEN);
    let mut written = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (chunk, tail) = rest.split_at(sizes().clamp(1, rest.len()));
        rest = tail;
        let step = exchange.feed(chunk);
        written.extend_from_slice(&step.write);
        if step.done.is_some() {
            return (written, step.done);
        }
    }
    (written, exchange.result().cloned())
}

/// Feeds `bytes` whole, byte by byte, in fixed chunk sizes, and in
/// pseudo random sizes, checks that every way agrees, and returns the
/// outcome.
fn run(expectation: &Expectation, bytes: &[u8]) -> Outcome {
    let whole = run_with(expectation, bytes, || usize::MAX);
    for size in [1, 2, 3, 7, 64, 1500, 4096] {
        assert_eq!(
            run_with(expectation, bytes, || size),
            whole,
            "chunks of {size} bytes disagree with one whole feed"
        );
    }
    let mut state: u32 = 0x9e37_79b9;
    let random = run_with(expectation, bytes, || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 24) as usize + 1
    });
    assert_eq!(random, whole, "random chunks disagree with one whole feed");
    whole
}

fn failure(outcome: &Outcome) -> (&str, &str) {
    match &outcome.1 {
        Some(Err(failure)) => (failure.reason.as_str(), failure.detail.as_str()),
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn assert_failure(outcome: &Outcome, reason: &str, detail: &str) {
    assert_eq!(failure(outcome), (reason, detail));
}

fn assert_ready(outcome: &Outcome) {
    assert_eq!(outcome.1, Some(Ok(())), "expected ready");
}

/// An `INFO` line with a trailing space before `\r\n`, as nats-server
/// writes it.
fn info(json: &str) -> String {
    format!("INFO {json} \r\n")
}

fn good_info(jetstream: bool) -> String {
    info(&format!(
        r#"{{"server_id":"NABC","server_name":"golden","version":"2.15.1","proto":1,"headers":true,"max_payload":1048576,"jetstream":{jetstream}}}"#
    ))
}

fn msg(payload: &str) -> String {
    format!("MSG {INBOX} 1 {}\r\n{payload}\r\n", payload.len())
}

fn jetstream_request() -> Vec<u8> {
    format!("SUB {INBOX} 1\r\nPUB $JS.API.INFO {INBOX} 0\r\n\r\n").into_bytes()
}

/// The server's side up to the `PONG`, with JetStream as `jetstream`.
fn to_pong(jetstream: bool) -> String {
    format!("{}PONG\r\n", good_info(jetstream))
}

/// One transcript of `transcripts.json` after its replay.
struct Replayed {
    file: String,
    jetstream: bool,
    outcome: Outcome,
}

/// Replays every transcript in `transcripts.json` whose file name starts
/// with `prefix` against its expectation, and checks its recorded result.
fn replay_transcripts(prefix: &str) -> Vec<Replayed> {
    let index: Value = serde_json::from_slice(
        &fs::read(fixtures_dir().join("transcripts.json")).expect("transcripts.json"),
    )
    .expect("transcripts.json is JSON");
    let transcripts = index["transcripts"].as_array().expect("transcripts list");
    let mut replayed = Vec::new();
    for entry in transcripts {
        let file = entry["file"].as_str().expect("file");
        if !file.starts_with(prefix) {
            continue;
        }
        let jetstream = entry["jetstream"].as_bool().expect("jetstream");
        assert_eq!(entry["serverName"], "golden");
        assert_eq!(entry["inbox"], "00112233445566778899aabbccddeeff");
        let bytes = fs::read(fixtures_dir().join(file)).expect("transcript file");
        let outcome = run(&expect(jetstream), &bytes);
        match entry["reason"].as_str() {
            None => assert_ready(&outcome),
            Some(reason) => assert_eq!(
                failure(&outcome).0,
                reason,
                "{file} with jetstream {jetstream}"
            ),
        }
        replayed.push(Replayed {
            file: file.to_owned(),
            jetstream,
            outcome,
        });
    }
    replayed
}

#[test]
fn real_transcripts_from_nats_2_15_1_give_the_recorded_results() {
    let ev = evidence!(
        covers = ["REQ-036"],
        kind = Kind::Fixture,
        expected = "every byte nats-server 2.15.1 sent in the step 0 spike gives the recorded nats-readiness result, fed whole and in any chunk size",
    );
    let replayed = replay_transcripts("real-");
    assert_eq!(replayed.len(), 7);
    for Replayed {
        file,
        jetstream,
        outcome,
    } in replayed
    {
        if outcome.1 == Some(Ok(())) {
            let mut sent = connect_bytes();
            if jetstream {
                sent.extend_from_slice(&jetstream_request());
            }
            assert_eq!(outcome.0, sent, "{file} with jetstream {jetstream}");
        }
    }
    ev.pass();
}

#[test]
fn synthetic_seed_transcripts_give_their_recorded_results() {
    let replayed = replay_transcripts("synthetic-");
    assert_eq!(replayed.len(), 4);
    let ping = replayed
        .iter()
        .find(|replay| replay.file == "synthetic-ping-ok-info.bin")
        .expect("the PING transcript");
    let mut sent = connect_bytes();
    sent.extend_from_slice(b"PONG\r\n");
    sent.extend_from_slice(&jetstream_request());
    sent.extend_from_slice(b"PONG\r\n");
    assert_eq!(ping.outcome.0, sent);
    let error = replayed
        .iter()
        .find(|replay| replay.file == "synthetic-js-api-error.bin")
        .expect("the API error transcript");
    assert_eq!(
        failure(&error.outcome).1,
        "$JS.API.INFO error 503: JetStream not enabled for account"
    );
}

#[test]
fn real_transcripts_carry_the_details_of_the_spec() {
    let read = |file: &str| fs::read(fixtures_dir().join(file)).expect("transcript file");
    assert_failure(
        &run(&expect(false), &read("real-foreign.bin")),
        "identity_mismatch",
        r#"server_name is "foreign", expected "golden""#,
    );
    assert_failure(
        &run(&expect(false), &read("real-auth.bin")),
        "auth_required",
        "the server requires authentication",
    );
    assert_failure(
        &run(&expect(false), &read("real-lame-duck.bin")),
        "lame_duck",
        "the server is in lame duck mode",
    );
    assert_failure(
        &run(&expect(true), &read("real-js-off.bin")),
        "jetstream_disabled",
        "JetStream is off, the instance asks for it",
    );
    assert_failure(
        &run(&expect(false), &read("real-js-on.bin")),
        "jetstream_unexpected",
        "JetStream is on, the instance asks for it off",
    );
}

#[test]
fn connect_is_the_exact_line_of_the_spec_without_credentials() {
    let expected = format!(
        "CONNECT {{\"verbose\":false,\"pedantic\":false,\"tls_required\":false,\"name\":\"stackly-readiness\",\"lang\":\"rust\",\"version\":\"{}\",\"protocol\":1,\"echo\":false,\"headers\":false,\"no_responders\":false}}\r\nPING\r\n",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(String::from_utf8(connect_bytes()).unwrap(), expected);

    let mut exchange = Exchange::new(expect(false), TOKEN);
    assert_eq!(exchange.phase(), Phase::Info);
    let step = exchange.feed(good_info(false).as_bytes());
    assert_eq!(step.write, connect_bytes());
    assert_eq!(step.done, None);
    assert_eq!(exchange.phase(), Phase::Pong);
}

#[test]
fn jetstream_request_subscribes_and_publishes_to_the_random_inbox() {
    let mut exchange = Exchange::new(expect(true), TOKEN);
    assert_eq!(exchange.inbox(), INBOX);
    exchange.feed(good_info(true).as_bytes());
    let step = exchange.feed(b"PONG\r\n");
    assert_eq!(
        String::from_utf8(step.write).unwrap(),
        format!("SUB {INBOX} 1\r\nPUB $JS.API.INFO {INBOX} 0\r\n\r\n")
    );
    assert_eq!(step.done, None);
    assert_eq!(exchange.phase(), Phase::JetStreamReply);

    let other = Exchange::new(expect(true), [0xa5; 16]);
    assert_eq!(
        other.inbox(),
        "_INBOX.stackly.a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5"
    );
}

#[test]
fn pong_without_jetstream_is_ready() {
    let outcome = run(&expect(false), to_pong(false).as_bytes());
    assert_ready(&outcome);
    assert_eq!(outcome.0, connect_bytes());
}

#[test]
fn account_info_reply_with_jetstream_is_ready() {
    let server = format!("{}{}", to_pong(true), msg(ACCOUNT_INFO));
    let outcome = run(&expect(true), server.as_bytes());
    assert_ready(&outcome);
    let mut sent = connect_bytes();
    sent.extend_from_slice(&jetstream_request());
    assert_eq!(outcome.0, sent);
}

#[test]
fn missing_boolean_fields_read_as_false() {
    let server = format!("{}PONG\r\n", info(r#"{"server_name":"golden"}"#));
    assert_ready(&run(&expect(false), server.as_bytes()));
    assert_failure(
        &run(&expect(true), server.as_bytes()),
        "jetstream_disabled",
        "JetStream is off, the instance asks for it",
    );
}

#[test]
fn ping_from_the_server_is_answered_with_pong_in_both_waits() {
    let server = format!(
        "{}PING\r\nPONG\r\nPING\r\n{}",
        good_info(true),
        msg(ACCOUNT_INFO)
    );
    let outcome = run(&expect(true), server.as_bytes());
    assert_ready(&outcome);
    let mut sent = connect_bytes();
    sent.extend_from_slice(b"PONG\r\n");
    sent.extend_from_slice(&jetstream_request());
    sent.extend_from_slice(b"PONG\r\n");
    assert_eq!(outcome.0, sent);
}

#[test]
fn info_and_ok_in_the_middle_are_ignored() {
    let later_info = info(r#"{"server_name":"other","ldm":true,"jetstream":"not checked"}"#);
    let server = format!(
        "{}+OK\r\n{later_info}PONG\r\n+OK\r\n{later_info}{}",
        good_info(true),
        msg(ACCOUNT_INFO)
    );
    assert_ready(&run(&expect(true), server.as_bytes()));
}

#[test]
fn operation_names_ignore_case_and_allow_tabs() {
    let server = format!(
        "info\t{{\"server_name\":\"golden\",\"jetstream\":true}}\t\r\n+ok\r\nping\r\npong\r\nmsg\t{INBOX}\t1\t{}\r\n{ACCOUNT_INFO}\r\n",
        ACCOUNT_INFO.len()
    );
    assert_ready(&run(&expect(true), server.as_bytes()));
}

#[test]
fn msg_with_a_reply_subject_is_accepted() {
    let server = format!(
        "{}MSG {INBOX} 1 _INBOX.reply {}\r\n{ACCOUNT_INFO}\r\n",
        to_pong(true),
        ACCOUNT_INFO.len()
    );
    assert_ready(&run(&expect(true), server.as_bytes()));
}

#[test]
fn identity_is_checked_byte_for_byte() {
    for (name, detail) in [
        ("foreign", r#"server_name is "foreign", expected "golden""#),
        ("Golden", r#"server_name is "Golden", expected "golden""#),
        ("golden ", r#"server_name is "golden ", expected "golden""#),
        ("", r#"server_name is "", expected "golden""#),
    ] {
        let server = format!(
            "{}PONG\r\n",
            info(&format!(r#"{{"server_name":"{name}"}}"#))
        );
        assert_failure(
            &run(&expect(false), server.as_bytes()),
            "identity_mismatch",
            detail,
        );
    }
    let server = info(r#"{"server_id":"NABC","version":"2.15.1"}"#);
    assert_failure(
        &run(&expect(false), server.as_bytes()),
        "identity_mismatch",
        r#"server_name is missing, expected "golden""#,
    );
}

#[test]
fn info_checks_run_in_the_order_of_the_spec() {
    let cases = [
        (
            r#"{"server_name":"x","tls_required":true,"auth_required":true,"ldm":true,"jetstream":true}"#,
            "identity_mismatch",
        ),
        (
            r#"{"server_name":"golden","tls_required":true,"auth_required":true,"ldm":true,"jetstream":true}"#,
            "tls_required",
        ),
        (
            r#"{"server_name":"golden","auth_required":true,"ldm":true,"jetstream":true}"#,
            "auth_required",
        ),
        (
            r#"{"server_name":"golden","ldm":true,"jetstream":true}"#,
            "lame_duck",
        ),
        (
            r#"{"server_name":"golden","jetstream":true}"#,
            "jetstream_unexpected",
        ),
    ];
    for (json, reason) in cases {
        let outcome = run(&expect(false), info(json).as_bytes());
        assert_eq!(failure(&outcome).0, reason, "{json}");
        assert!(outcome.0.is_empty(), "a refused INFO sends nothing");
    }
    assert_failure(
        &run(
            &expect(false),
            info(r#"{"server_name":"golden","tls_required":true}"#).as_bytes(),
        ),
        "tls_required",
        "the server requires TLS",
    );
}

#[test]
fn false_flags_pass() {
    let server = format!(
        "{}PONG\r\n",
        info(
            r#"{"server_name":"golden","tls_required":false,"auth_required":false,"ldm":false,"jetstream":false}"#
        )
    );
    assert_ready(&run(&expect(false), server.as_bytes()));
}

#[test]
fn malformed_info_is_a_protocol_error() {
    let cases = [
        ("HELLO\r\n".to_owned(), r#"expected INFO, got "HELLO""#),
        ("PING\r\n".to_owned(), r#"expected INFO, got "PING""#),
        (
            "INFO{\"server_name\":\"golden\"}\r\n".to_owned(),
            r#"expected INFO, got "INFO{"server_name":"golden"}""#,
        ),
        ("INFO\r\n".to_owned(), "INFO is not one valid JSON value"),
        (info("[1,2]"), "INFO is not a JSON object"),
        (info("\"golden\""), "INFO is not a JSON object"),
        (
            info("{\"server_name\":"),
            "INFO is not one valid JSON value",
        ),
        (
            info(r#"{"server_name":"golden"} {}"#),
            "INFO is not one valid JSON value",
        ),
        (
            info(r#"{"server_name":"golden"}x"#),
            "INFO is not one valid JSON value",
        ),
        (
            info(r#"{"server_name":"golden","jetstream":"x"}"#),
            "INFO field jetstream is not a boolean",
        ),
        (
            info(r#"{"server_name":"golden","ldm":null}"#),
            "INFO field ldm is not a boolean",
        ),
        (
            info(r#"{"server_name":7}"#),
            "INFO field server_name is not a string",
        ),
        (
            info(r#"{"server_name":null}"#),
            "INFO field server_name is not a string",
        ),
        (
            info(r#"{"server_name":"golden","server_id":1}"#),
            "INFO field server_id is not a string",
        ),
        (
            info(r#"{"server_name":"golden","version":true}"#),
            "INFO field version is not a string",
        ),
    ];
    for (server, detail) in cases {
        assert_failure(
            &run(&expect(false), server.as_bytes()),
            "protocol_error",
            detail,
        );
    }
}

#[test]
fn lines_longer_than_16_kib_are_refused_and_16_kib_is_accepted() {
    let line = |len: usize| {
        let head = r#"INFO {"server_name":"golden","pad":""#;
        let tail = r#""}"#;
        format!(
            "{head}{}{tail}\r\n",
            "a".repeat(len - head.len() - tail.len())
        )
    };
    let fits = format!("{}PONG\r\n", line(NATS_LINE_MAX));
    assert_ready(&run(&expect(false), fits.as_bytes()));
    let too_long = format!("{}PONG\r\n", line(NATS_LINE_MAX + 1));
    assert_failure(
        &run(&expect(false), too_long.as_bytes()),
        "protocol_error",
        "a line from the server is longer than 16384 bytes",
    );
    let endless = "a".repeat(NATS_LINE_MAX + 2);
    assert_failure(
        &run(&expect(false), endless.as_bytes()),
        "protocol_error",
        "a line from the server is longer than 16384 bytes",
    );
    let later = format!("{}{}", good_info(false), "+".repeat(NATS_LINE_MAX + 2));
    assert_failure(
        &run(&expect(false), later.as_bytes()),
        "protocol_error",
        "a line from the server is longer than 16384 bytes",
    );
}

#[test]
fn err_from_the_server_is_a_protocol_error_with_its_text() {
    for (server, jetstream) in [
        (
            format!("{}-ERR 'Authorization Violation'\r\n", good_info(false)),
            false,
        ),
        (
            format!("{}-err 'Authorization Violation'\r\n", to_pong(true)),
            true,
        ),
    ] {
        assert_failure(
            &run(&expect(jetstream), server.as_bytes()),
            "protocol_error",
            "server error: 'Authorization Violation'",
        );
    }
}

#[test]
fn unexpected_lines_are_protocol_errors() {
    let cases = [
        (
            format!("{}MSG {INBOX} 1 2\r\n{{}}\r\n", good_info(true)),
            true,
            format!(r#"unexpected line "MSG {INBOX} 1 2""#),
        ),
        (
            format!("{}PONG\r\nPONG\r\n", good_info(true)),
            true,
            r#"unexpected line "PONG""#.to_owned(),
        ),
        (
            format!("{}HMSG {INBOX} 1 0 2\r\n", to_pong(true)),
            true,
            format!(r#"unexpected line "HMSG {INBOX} 1 0 2""#),
        ),
        (
            format!("{}PING now\r\n", good_info(false)),
            false,
            r#"unexpected line "PING now""#.to_owned(),
        ),
        (
            format!("{}+OK more\r\n", good_info(false)),
            false,
            r#"unexpected line "+OK more""#.to_owned(),
        ),
        (
            format!("{}\r\n", good_info(false)),
            false,
            r#"unexpected line """#.to_owned(),
        ),
    ];
    for (server, jetstream, detail) in cases {
        assert_failure(
            &run(&expect(jetstream), server.as_bytes()),
            "protocol_error",
            &detail,
        );
    }
}

#[test]
fn msg_for_another_subscription_or_with_bad_arguments_is_a_protocol_error() {
    let cases = [
        (
            "MSG _INBOX.other 1 2\r\n".to_owned(),
            "a MSG arrived for another subscription",
        ),
        (
            format!("MSG {INBOX} 2 2\r\n"),
            "a MSG arrived for another subscription",
        ),
        (
            format!("MSG {INBOX} 1\r\n"),
            "a MSG line does not have 3 or 4 arguments",
        ),
        (
            format!("MSG {INBOX} 1 a b 2\r\n"),
            "a MSG line does not have 3 or 4 arguments",
        ),
        (
            format!("MSG {INBOX} 1 -2\r\n"),
            "a MSG line has an invalid size",
        ),
        (
            format!("MSG {INBOX} 1 +2\r\n"),
            "a MSG line has an invalid size",
        ),
        (
            format!("MSG {INBOX} 1 99999999999\r\n"),
            "a MSG line has an invalid size",
        ),
    ];
    for (line, detail) in cases {
        let server = format!("{}{line}", to_pong(true));
        assert_failure(
            &run(&expect(true), server.as_bytes()),
            "protocol_error",
            detail,
        );
    }
}

#[test]
fn reply_payload_above_64_kib_is_refused_and_64_kib_is_accepted() {
    let head = r#"{"type":"io.nats.jetstream.api.v1.account_info_response","pad":""#;
    let payload = format!("{head}{}\"}}", "a".repeat(NATS_REPLY_MAX - head.len() - 2));
    assert_eq!(payload.len(), NATS_REPLY_MAX);
    let server = format!("{}{}", to_pong(true), msg(&payload));
    assert_ready(&run(&expect(true), server.as_bytes()));

    let server = format!("{}MSG {INBOX} 1 {}\r\n", to_pong(true), NATS_REPLY_MAX + 1);
    assert_failure(
        &run(&expect(true), server.as_bytes()),
        "protocol_error",
        "the $JS.API.INFO reply is larger than 65536 bytes",
    );
}

#[test]
fn reply_payload_must_end_with_crlf_and_be_a_json_object() {
    let cases = [
        (
            format!("MSG {INBOX} 1 2\r\n{{}}xx"),
            "the $JS.API.INFO reply does not end with CRLF",
        ),
        (
            msg("not json"),
            "the $JS.API.INFO reply is not one valid JSON value",
        ),
        (msg("[1]"), "the $JS.API.INFO reply is not a JSON object"),
    ];
    for (reply, detail) in cases {
        let server = format!("{}{reply}", to_pong(true));
        assert_failure(
            &run(&expect(true), server.as_bytes()),
            "protocol_error",
            detail,
        );
    }
}

#[test]
fn jetstream_api_error_or_other_type_is_unavailable() {
    let cases = [
        (
            r#"{"type":"io.nats.jetstream.api.v1.account_info_response","error":{"code":503,"err_code":10039,"description":"jetstream not enabled for account"}}"#,
            "$JS.API.INFO error 503: jetstream not enabled for account",
        ),
        (r#"{"error":{}}"#, "$JS.API.INFO error : "),
        (
            r#"{"error":{"code":"x","description":{"a":1}}}"#,
            r#"$JS.API.INFO error x: {"a":1}"#,
        ),
        (
            r#"{"type":"io.nats.jetstream.api.v1.stream_info_response"}"#,
            r#"$JS.API.INFO replied with type "io.nats.jetstream.api.v1.stream_info_response""#,
        ),
        (r#"{"memory":0}"#, "$JS.API.INFO replied without a type"),
        (r#"{"type":7}"#, "$JS.API.INFO replied without a type"),
    ];
    for (payload, detail) in cases {
        let server = format!("{}{}", to_pong(true), msg(payload));
        assert_failure(
            &run(&expect(true), server.as_bytes()),
            "jetstream_unavailable",
            detail,
        );
    }
}

#[test]
fn reading_more_than_128_kib_without_a_result_is_a_protocol_error() {
    let mut server = good_info(false).into_bytes();
    while server.len() < NATS_READ_MAX {
        server.extend_from_slice(b"+OK\r\n");
    }
    server.truncate(NATS_READ_MAX);
    assert_eq!(
        run(&expect(false), &server).1,
        None,
        "exactly the limit waits"
    );

    server.push(b'\n');
    assert_failure(
        &run(&expect(false), &server),
        "protocol_error",
        "the server sent more than 131072 bytes",
    );
}

#[test]
fn bytes_after_the_result_are_ignored() {
    let mut server = to_pong(false).into_bytes();
    server.extend(std::iter::repeat_n(b'x', NATS_READ_MAX * 2));
    assert_ready(&run(&expect(false), &server));

    let mut exchange = Exchange::new(expect(false), TOKEN);
    let first = exchange.feed(to_pong(false).as_bytes());
    assert_eq!(first.done, Some(Ok(())));
    let again = exchange.feed(b"PING\r\n-ERR 'late'\r\n");
    assert!(again.write.is_empty());
    assert_eq!(again.done, Some(Ok(())));
}

#[test]
fn closed_and_reset_name_the_phase() {
    let info_line = good_info(true);
    let mut exchange = Exchange::new(expect(true), TOKEN);
    let phases = [
        ("", "INFO"),
        (info_line.as_str(), "PONG"),
        ("PONG\r\n", "the $JS.API.INFO reply"),
    ];
    for (bytes, awaited) in phases {
        exchange.feed(bytes.as_bytes());
        let closed = exchange.closed();
        assert_eq!(closed.reason.as_str(), "protocol_error");
        assert_eq!(
            closed.detail,
            format!("connection closed while waiting for {awaited}")
        );
        let reset = exchange.reset();
        assert_eq!(reset.reason.as_str(), "protocol_error");
        assert_eq!(
            reset.detail,
            format!("connection reset while waiting for {awaited}")
        );
    }
}

#[test]
fn timeouts_name_the_phase_and_the_attempt() {
    let attempt = Duration::from_millis(2000);
    let info_line = good_info(true);
    let mut exchange = Exchange::new(expect(true), TOKEN);
    let phases = [
        ("", "no INFO from the server within 2000 ms"),
        (info_line.as_str(), "no PONG from the server within 2000 ms"),
        ("PONG\r\n", "no reply to $JS.API.INFO within 2000 ms"),
    ];
    for (bytes, detail) in phases {
        exchange.feed(bytes.as_bytes());
        let timeout = exchange.timed_out(attempt);
        assert_eq!(timeout.reason.as_str(), "timeout");
        assert_eq!(timeout.detail, detail);
    }
}

#[test]
fn connect_failures_and_unix_targets_have_the_texts_of_the_spec() {
    let failed = connect_failed("127.0.0.1:4222", "connection refused");
    assert_eq!(failed.reason.as_str(), "connect_failed");
    assert_eq!(
        failed.detail,
        "connect to 127.0.0.1:4222 failed: connection refused"
    );
    let slow = connect_timed_out("127.0.0.1:4222", Duration::from_millis(1500));
    assert_eq!(slow.reason.as_str(), "timeout");
    assert_eq!(
        slow.detail,
        "connect to 127.0.0.1:4222 timed out after 1500 ms"
    );
    let unix = unix_target();
    assert_eq!(unix.reason.as_str(), "protocol_error");
    assert_eq!(unix.detail, "nats-readiness supports tcp endpoints only");
}

#[test]
fn server_values_are_cut_to_64_characters_and_lose_control_characters() {
    assert_eq!(NATS_DETAIL_VALUE_MAX, 64);
    let exact = "é".repeat(64);
    let server = info(&format!(r#"{{"server_name":"{exact}"}}"#));
    assert_failure(
        &run(&expect(false), server.as_bytes()),
        "identity_mismatch",
        &format!(r#"server_name is "{exact}", expected "golden""#),
    );

    let long = "n".repeat(65);
    let server = info(&format!(r#"{{"server_name":"{long}"}}"#));
    assert_failure(
        &run(&expect(false), server.as_bytes()),
        "identity_mismatch",
        &format!(
            r#"server_name is "{}...", expected "golden""#,
            "n".repeat(61)
        ),
    );

    let server = info(r#"{"server_name":"a\u0000b\u0007c\u007fd\u0085e\u009ff g"}"#);
    assert_failure(
        &run(&expect(false), server.as_bytes()),
        "identity_mismatch",
        "server_name is \"a?b?c?d?e?f\u{a0}g\", expected \"golden\"",
    );

    let server = format!("{}-ERR '\x1b[31mred\x07'\r\n", good_info(false));
    assert_failure(
        &run(&expect(false), server.as_bytes()),
        "protocol_error",
        "server error: '?[31mred?'",
    );

    let server = b"\x00\xff\x1bHELLO\r\n";
    assert_failure(
        &run(&expect(false), server),
        "protocol_error",
        "expected INFO, got \"?\u{fffd}?HELLO\"",
    );

    let garbage = format!("{}\r\n", "z".repeat(NATS_LINE_MAX));
    assert_failure(
        &run(&expect(false), garbage.as_bytes()),
        "protocol_error",
        &format!(r#"expected INFO, got "{}...""#, "z".repeat(61)),
    );
}

#[test]
fn every_detail_fits_the_observation_limit_before_redaction() {
    let long = "x".repeat(NATS_LINE_MAX - 64);
    let servers = [
        (info(&format!(r#"{{"server_name":"{long}"}}"#)), false),
        (format!("{}-ERR {long}\r\n", good_info(false)), false),
        (
            format!(
                "{}{}",
                to_pong(true),
                msg(&format!(
                    r#"{{"error":{{"code":"{long}","description":"{long}"}}}}"#
                ))
            ),
            true,
        ),
        (
            format!(
                "{}{}",
                to_pong(true),
                msg(&format!(r#"{{"type":"{long}"}}"#))
            ),
            true,
        ),
    ];
    for (server, jetstream) in servers {
        let outcome = run(&expect(jetstream), server.as_bytes());
        let (_, detail) = failure(&outcome);
        assert!(detail.chars().count() <= 256, "{detail}");
        assert!(!detail.chars().any(char::is_control), "{detail}");
    }
}
