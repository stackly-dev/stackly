//! The handshake (spec 0006 AC-1, AC-10): the first frame must be an
//! accepted `daemon.handshake`, and nothing behind a refused one is ever
//! processed.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::codes::{ipc, protocol, request};
use stackly_protocol::{CONTRACT_DIGEST, ProtocolVersion};
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{PATIENCE, Raw, Site, error_of, handshake, request};

#[tokio::test]
async fn a_client_of_major_1_is_accepted_whatever_its_minor() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047", "AC-071"],
        kind = Kind::Fixture,
        expected = "handshakes of protocol 1.0 and 1.99 are both accepted with the daemon's version, server instance, digest, limits, and head",
    );
    let site = Site::new();
    let daemon = site.start().await;
    // The start slot published the two `daemon.reconciling` events.
    let base = daemon.bus().head().sequence.get();

    for minor in [0, 99] {
        let mut raw = Raw::connect(&site.socket()).await;
        raw.send(&handshake("h1", 1, minor, CONTRACT_DIGEST)).await;
        let frame = raw.recv().await.expect("the handshake is answered");

        assert_eq!(frame["type"], "response", "{frame}");
        assert_eq!(frame["content"]["requestId"], "h1");
        let accepted = &frame["content"]["reply"]["content"];
        assert_eq!(frame["content"]["reply"]["type"], "daemon.handshake");
        assert_eq!(accepted["protocol"]["major"], 1);
        assert_eq!(
            accepted["protocol"]["minor"],
            ProtocolVersion::CURRENT.minor
        );
        assert_eq!(
            accepted["serverInstanceId"],
            daemon.info().server_instance_id.to_string()
        );
        assert_eq!(accepted["daemon"]["pid"], std::process::id());
        assert_eq!(accepted["contractDigest"], CONTRACT_DIGEST);
        assert_eq!(accepted["contractDrift"], false);
        assert_eq!(accepted["limits"]["maxFrameBytes"], 1_048_576);
        assert_eq!(accepted["head"]["sequence"], base);
    }
    ev.pass();
}

#[tokio::test]
async fn another_major_with_broken_params_gets_major_mismatch_and_nothing_behind_it_runs() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047", "AC-071"],
        kind = Kind::Fixture,
        expected = "a major 2 handshake with broken params gets protocol.major_mismatch with the daemon version and the connection closes; a request sent behind it is never answered",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::connect(&site.socket()).await;
    let broken = serde_json::to_vec(&json!({
        "requestId": "h2",
        "call": {
            "type": "daemon.handshake",
            "content": { "protocol": { "major": 2, "minor": 0 }, "client": 7 }
        }
    }))
    .unwrap();

    raw.send(&broken).await;
    raw.send(&request("after", "daemon.get_info", None)).await;

    let frame = raw.recv().await.expect("the refusal arrives");
    let error = error_of(&frame);
    assert!(error.is(&protocol::MAJOR_MISMATCH));
    assert_eq!(frame["content"]["requestId"], "h2");
    assert_eq!(error.detail("serverMajor"), Some("1"));
    assert_eq!(
        error.detail("serverMinor"),
        Some(ProtocolVersion::CURRENT.minor.to_string().as_str())
    );
    raw.expect_end().await;
    ev.pass();
}

#[tokio::test]
async fn a_first_request_that_is_not_a_handshake_gets_handshake_required() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047"],
        kind = Kind::Fixture,
        expected = "daemon.get_info or an unknown method as the first frame gets protocol.handshake_required and the connection closes",
    );
    let site = Site::new();
    let _daemon = site.start().await;

    for method in ["daemon.get_info", "nope.nothing"] {
        let mut raw = Raw::connect(&site.socket()).await;
        raw.send(&request("first", method, None)).await;

        let frame = raw.recv().await.expect("the refusal arrives");
        let error = error_of(&frame);
        assert!(error.is(&protocol::HANDSHAKE_REQUIRED), "{method}");
        assert_eq!(error.detail("method"), Some(method));
        assert_eq!(frame["content"]["requestId"], "first");
        raw.expect_end().await;
    }
    ev.pass();
}

#[tokio::test]
async fn broken_json_or_bad_handshake_params_as_the_first_frame_get_their_request_code() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047"],
        kind = Kind::Fixture,
        expected = "broken JSON gets request.malformed and a handshake with an invalid digest gets request.invalid_params, each closing the connection",
    );
    let site = Site::new();
    let _daemon = site.start().await;

    let mut raw = Raw::connect(&site.socket()).await;
    raw.send(b"{not json").await;
    let error = error_of(&raw.recv().await.expect("the refusal arrives"));
    assert!(error.is(&request::MALFORMED));
    raw.expect_end().await;

    let mut raw = Raw::connect(&site.socket()).await;
    raw.send(&handshake("h", 1, 0, "not-a-digest")).await;
    let frame = raw.recv().await.expect("the refusal arrives");
    let error = error_of(&frame);
    assert!(error.is(&request::INVALID_PARAMS));
    assert_eq!(error.fields()[0].path(), "contractDigest");
    assert_eq!(frame["content"]["requestId"], "h");
    raw.expect_end().await;
    ev.pass();
}

#[tokio::test(start_paused = true)]
async fn a_handshake_that_never_completes_times_out_after_5_seconds() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047"],
        kind = Kind::Fixture,
        expected = "a frame header without its payload gets ipc.handshake_timeout once 5 seconds pass on the paused clock, and the connection closes",
    );
    let site = Site::new();
    let (_daemon, _gate) = site.start_held().await;
    let mut raw = Raw::connect(&site.socket()).await;

    raw.send_bytes(&100_u32.to_be_bytes()).await;
    let started = tokio::time::Instant::now();
    let frame = raw.read_frame().await.expect("the timeout is reported");

    let error = error_of(&frame);
    assert!(error.is(&ipc::HANDSHAKE_TIMEOUT));
    assert!(frame["content"]["requestId"].is_null());
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_secs(4) && waited <= Duration::from_secs(6),
        "{waited:?}"
    );
    assert!(raw.read_frame().await.is_none());
    ev.pass();
}

#[tokio::test]
async fn a_digest_that_differs_at_the_same_version_connects_with_contract_drift() {
    let ev = evidence!(
        covers = ["AC-071"],
        kind = Kind::Fixture,
        expected = "a handshake with the daemon's version but another contract digest is accepted with contractDrift true",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::connect(&site.socket()).await;
    let current = ProtocolVersion::CURRENT;

    raw.send(&handshake(
        "h",
        current.major,
        current.minor,
        &"a".repeat(64),
    ))
    .await;

    let frame = raw.recv().await.expect("the handshake is answered");
    assert_eq!(frame["content"]["reply"]["content"]["contractDrift"], true);
    ev.pass();
}

#[tokio::test]
async fn a_second_handshake_is_refused_and_the_connection_stays_up() {
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;

    raw.send(&handshake("again", 1, 0, CONTRACT_DIGEST)).await;
    let error = error_of(&raw.answer("again").await);
    assert!(error.is(&protocol::HANDSHAKE_REPEATED));

    raw.send(&request("info", "daemon.get_info", None)).await;
    assert_eq!(raw.answer("info").await["type"], "response");
}

#[tokio::test]
async fn every_refused_handshake_counts_once_in_connections_rejected() {
    let ev = evidence!(
        covers = ["NFR-004"],
        kind = Kind::Fixture,
        expected = "a first frame that is not a handshake and a handshake of another major each add one to counters.connectionsRejected, and an accepted handshake adds nothing",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let rejected = || daemon.health().counters.connections_rejected.get();
    let settles_at = |count: u64| async move {
        let deadline = std::time::Instant::now() + PATIENCE;
        while rejected() != count {
            assert!(
                std::time::Instant::now() < deadline,
                "connectionsRejected is {}, not {count}",
                rejected()
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    assert_eq!(rejected(), 0);

    let mut raw = Raw::connect(&site.socket()).await;
    raw.send(&request("r1", "daemon.get_info", None)).await;
    let frame = raw.recv().await.expect("the refusal arrives");
    assert!(
        error_of(&frame).is(&protocol::HANDSHAKE_REQUIRED),
        "{frame}"
    );
    raw.expect_end().await;
    settles_at(1).await;

    let mut raw = Raw::connect(&site.socket()).await;
    raw.send(&handshake("h2", 2, 0, CONTRACT_DIGEST)).await;
    let frame = raw.recv().await.expect("the refusal arrives");
    assert!(error_of(&frame).is(&protocol::MAJOR_MISMATCH), "{frame}");
    raw.expect_end().await;
    settles_at(2).await;

    let _accepted = Raw::ready(&site.socket()).await;
    assert_eq!(rejected(), 2);
    ev.pass();
}

#[tokio::test]
async fn a_major_given_in_another_json_shape_is_still_refused() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047"],
        kind = Kind::Fixture,
        expected = "a handshake whose protocol is the array [2, 0], which the early major reader skips, still gets protocol.major_mismatch once decoded",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::connect(&site.socket()).await;

    raw.send(&request(
        "h-array",
        "daemon.handshake",
        Some(json!({
            "protocol": [2, 0],
            "client": { "kind": "cli", "version": "0.1.0", "pid": std::process::id() },
            "contractDigest": CONTRACT_DIGEST,
        })),
    ))
    .await;

    let frame = raw.recv().await.expect("the refusal arrives");
    let error = error_of(&frame);
    assert!(error.is(&protocol::MAJOR_MISMATCH), "{frame}");
    assert_eq!(frame["content"]["requestId"], "h-array");
    assert_eq!(
        error.detail("serverMajor"),
        Some(ProtocolVersion::CURRENT.major.to_string().as_str())
    );
    raw.expect_end().await;
    ev.pass();
}
