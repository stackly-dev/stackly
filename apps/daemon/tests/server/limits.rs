//! Payload, request, subscription, and connection limits (spec 0006 AC-6,
//! AC-7, AC-8): payload errors keep the connection, framing errors close
//! it, and every error matches the catalog.

use serde_json::json;
use stackly_protocol::codes::{events, internal, ipc, request};
use stackly_protocol::daemon::Capabilities;
use stackly_protocol::limits::{CONTROL_FRAME_MAX, FIELD_ERRORS_MAX};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::dispatch::{HandlerFuture, Handlers, RequestContext};
use stacklyd::state::DaemonCore;

use crate::common::{Hold, HoldInfo, Raw, Site, error_of, request};

#[tokio::test]
async fn every_payload_error_is_answered_and_the_connection_stays_up() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004", "VAL-020"],
        kind = Kind::Fixture,
        expected = "after the handshake, broken JSON, a bad requestId, depth 33, an unknown method, and bad params each get their request.* code, the error never echoes the input, and the connection still serves",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let too_deep = format!(
        r#"{{"requestId":"deep","call":{{"type":"daemon.get_info","content":{}{}}}}}"#,
        "[".repeat(33),
        "]".repeat(33)
    );
    let content_first = r#"{"requestId":"params","call":{"content":{"since":{"serverInstanceId":"secret-sentinel","sequence":1}},"type":"events.subscribe"}}"#;
    let cases: [(&[u8], &str, Option<&str>); 5] = [
        (b"{broken", "request.malformed", None),
        (
            br#"{"requestId":"has space","call":{"type":"daemon.get_info"}}"#,
            "request.malformed",
            None,
        ),
        (too_deep.as_bytes(), "request.too_deep", Some("deep")),
        (
            br#"{"requestId":"unknown","call":{"type":"nope.nothing"}}"#,
            "request.unknown_method",
            Some("unknown"),
        ),
        (
            content_first.as_bytes(),
            "request.invalid_params",
            Some("params"),
        ),
    ];

    for (payload, code, request_id) in cases {
        raw.send(payload).await;
        let frame = raw.recv().await.expect("the error arrives");
        let error = error_of(&frame);
        assert_eq!(error.code(), code);
        assert_eq!(frame["content"]["requestId"].as_str(), request_id, "{code}");
        assert!(!frame.to_string().contains("secret-sentinel"), "{frame}");
    }

    raw.send(&request("info", "daemon.get_info", None)).await;
    assert_eq!(raw.answer("info").await["type"], "response");
    ev.pass();
}

#[tokio::test]
async fn a_huge_bad_kinds_list_and_positional_requests_are_answered_and_the_connection_stays_up() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004", "VAL-020"],
        kind = Kind::Fixture,
        expected = "a subscribe request just under 1 MiB with 349000 bad kinds gets request.invalid_params with 32 fields and details.omitted, positional arrays get request.malformed or request.invalid_params, and the connection still serves",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let kinds = vec![r#""""#; 349_000].join(",");
    let huge = format!(
        r#"{{"requestId":"huge","call":{{"type":"events.subscribe","content":{{"kinds":[{kinds}]}}}}}}"#
    );
    assert!(huge.len() < CONTROL_FRAME_MAX as usize, "{}", huge.len());

    raw.send(huge.as_bytes()).await;
    let error = error_of(&raw.answer("huge").await);
    assert!(error.is(&request::INVALID_PARAMS));
    assert_eq!(error.fields().len(), FIELD_ERRORS_MAX);
    assert_eq!(error.detail("omitted"), Some("1"));

    raw.send(br#"["positional",null,{"type":"daemon.get_info"}]"#)
        .await;
    let frame = raw.recv().await.expect("the error arrives");
    assert!(error_of(&frame).is(&request::MALFORMED));
    assert!(frame["content"]["requestId"].is_null(), "{frame}");
    raw.send(br#"{"requestId":"params","call":{"type":"events.unsubscribe","content":[3]}}"#)
        .await;
    assert!(error_of(&raw.answer("params").await).is(&request::INVALID_PARAMS));

    raw.send(&request("info", "daemon.get_info", None)).await;
    assert_eq!(raw.answer("info").await["type"], "response");
    ev.pass();
}

#[tokio::test]
async fn invalid_params_name_the_field_path_relative_to_the_params() {
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;

    raw.send(&request(
        "kinds",
        "events.subscribe",
        Some(json!({ "kinds": ["ok.kind", "NOPE"] })),
    ))
    .await;

    let error = error_of(&raw.answer("kinds").await);
    assert!(error.is(&request::INVALID_PARAMS));
    assert_eq!(error.detail("method"), Some("events.subscribe"));
    let field = &error.fields()[0];
    assert_eq!((field.path(), field.code()), ("kinds[1]", "invalid_format"));
}

#[tokio::test]
async fn the_33rd_running_request_and_a_repeated_request_id_are_refused() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004"],
        kind = Kind::Fixture,
        expected = "with 32 held requests the 33rd gets request.too_many_in_flight and a requestId still running gets request.duplicate_id; released, all 32 are answered",
    );
    let site = Site::new();
    let hold = Hold::new();
    let _daemon = site.start_with(HoldInfo(hold.clone())).await;
    let mut raw = Raw::ready(&site.socket()).await;

    for index in 0..32 {
        raw.send(&request(&format!("held-{index}"), "daemon.get_info", None))
            .await;
    }
    hold.wait_entered(32).await;
    raw.send(&request("one-more", "daemon.get_info", None))
        .await;
    let error = error_of(&raw.answer("one-more").await);
    assert!(error.is(&request::TOO_MANY_IN_FLIGHT));
    assert_eq!(error.detail("max"), Some("32"));
    assert!(error.retryable());
    raw.send(&request("held-0", "daemon.get_info", None)).await;
    let error = error_of(&raw.answer("held-0").await);
    assert!(error.is(&request::DUPLICATE_ID));

    hold.release();
    let mut answered = 0;
    while answered < 32 {
        let frame = raw.recv().await.expect("the held requests are answered");
        assert_eq!(frame["type"], "response", "{frame}");
        answered += 1;
    }
    ev.pass();
}

#[tokio::test]
async fn the_9th_subscription_and_an_unknown_unsubscribe_are_refused() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004"],
        kind = Kind::Fixture,
        expected = "subscriptions 1 to 8 get ids 1 to 8, the 9th gets events.too_many_subscriptions, and unsubscribing an unknown id gets events.unknown_subscription",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;

    for index in 1..=8 {
        let id = format!("sub-{index}");
        raw.send(&request(&id, "events.subscribe", Some(json!({}))))
            .await;
        let frame = raw.answer(&id).await;
        assert_eq!(
            frame["content"]["reply"]["content"]["subscriptionId"], index,
            "{frame}"
        );
    }
    raw.send(&request("sub-9", "events.subscribe", Some(json!({}))))
        .await;
    let error = error_of(&raw.answer("sub-9").await);
    assert!(error.is(&events::TOO_MANY_SUBSCRIPTIONS));
    assert_eq!(error.detail("max"), Some("8"));

    raw.send(&request(
        "unsub",
        "events.unsubscribe",
        Some(json!({ "subscriptionId": 42 })),
    ))
    .await;
    let error = error_of(&raw.answer("unsub").await);
    assert!(error.is(&events::UNKNOWN_SUBSCRIPTION));
    assert_eq!(error.detail("subscriptionId"), Some("42"));

    raw.send(&request(
        "unsub-1",
        "events.unsubscribe",
        Some(json!({ "subscriptionId": 1 })),
    ))
    .await;
    assert_eq!(raw.answer("unsub-1").await["type"], "response");
    raw.send(&request("sub-again", "events.subscribe", Some(json!({}))))
        .await;
    let frame = raw.answer("sub-again").await;
    assert_eq!(frame["content"]["reply"]["content"]["subscriptionId"], 9);
    ev.pass();
}

#[tokio::test]
async fn the_65th_connection_gets_too_many_connections() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004"],
        kind = Kind::Fixture,
        expected = "with 64 connections served the 65th gets ipc.too_many_connections and is closed, and the counters show it",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let mut open = Vec::new();
    for _ in 0..64 {
        open.push(Raw::ready(&site.socket()).await);
    }

    let mut extra = Raw::connect(&site.socket()).await;
    let frame = extra.recv().await.expect("the refusal arrives");

    let error = error_of(&frame);
    assert!(error.is(&ipc::TOO_MANY_CONNECTIONS));
    assert_eq!(error.detail("max"), Some("64"));
    extra.expect_end().await;
    let counters = daemon.health().counters;
    assert_eq!(counters.connections_open.get(), 64);
    assert!(counters.connections_rejected.get() >= 1);
    ev.pass();
}

#[tokio::test]
async fn framing_errors_after_the_handshake_send_the_error_and_close() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004"],
        kind = Kind::Fixture,
        expected = "a frame of length 0 gets ipc.frame_empty and one above 1 MiB gets ipc.frame_too_large as soon as the header arrives, each closing the connection",
    );
    let site = Site::new();
    let daemon = site.start().await;

    let mut raw = Raw::ready(&site.socket()).await;
    raw.send_bytes(&0_u32.to_be_bytes()).await;
    let error = error_of(&raw.recv().await.expect("the error arrives"));
    assert!(error.is(&ipc::FRAME_EMPTY));
    raw.expect_end().await;

    let mut raw = Raw::ready(&site.socket()).await;
    raw.send_bytes(&(1_048_577_u32).to_be_bytes()).await;
    let error = error_of(&raw.recv().await.expect("the error arrives"));
    assert!(error.is(&ipc::FRAME_TOO_LARGE));
    assert_eq!(error.detail("max"), Some("1048576"));
    raw.expect_end().await;
    assert!(daemon.health().counters.frames_rejected.get() >= 2);
    ev.pass();
}

/// `daemon.get_capabilities` answers with more than a frame holds.
struct HugeCapabilities;

impl Handlers for HugeCapabilities {
    fn get_capabilities<'a>(
        &'a self,
        _core: &'a DaemonCore,
        _context: &'a RequestContext,
    ) -> HandlerFuture<'a, Capabilities> {
        Box::pin(async move {
            Ok(Capabilities {
                platform: Vec::new(),
                methods: vec!["x".repeat(1024); 1100],
                event_kinds: Vec::new(),
            })
        })
    }
}

#[tokio::test]
async fn a_reply_above_the_frame_limit_becomes_reply_too_large() {
    let site = Site::new();
    let _daemon = site.start_with(HugeCapabilities).await;
    let mut raw = Raw::ready(&site.socket()).await;

    raw.send(&request("caps", "daemon.get_capabilities", None))
        .await;

    let error = error_of(&raw.answer("caps").await);
    assert!(error.is(&ipc::REPLY_TOO_LARGE));
    assert_eq!(error.detail("method"), Some("daemon.get_capabilities"));
    let length: usize = error.detail("length").unwrap().parse().unwrap();
    assert!(length > 1_048_576);
    raw.send(&request("info", "daemon.get_info", None)).await;
    assert_eq!(raw.answer("info").await["type"], "response");
}

/// `daemon.get_health` panics.
struct PanickingHealth;

impl Handlers for PanickingHealth {
    fn get_health<'a>(
        &'a self,
        _core: &'a DaemonCore,
        _context: &'a RequestContext,
    ) -> HandlerFuture<'a, stackly_protocol::daemon::DaemonHealth> {
        Box::pin(async move { panic!("a handler bug for the test") })
    }
}

#[tokio::test]
async fn a_handler_that_panics_answers_internal_unexpected_and_the_daemon_keeps_serving() {
    let site = Site::new();
    let _daemon = site.start_with(PanickingHealth).await;
    let mut raw = Raw::ready(&site.socket()).await;

    raw.send(&request("health", "daemon.get_health", None))
        .await;

    let error = error_of(&raw.answer("health").await);
    assert!(error.is(&internal::UNEXPECTED));
    raw.send(&request("info", "daemon.get_info", None)).await;
    assert_eq!(raw.answer("info").await["type"], "response");
}
