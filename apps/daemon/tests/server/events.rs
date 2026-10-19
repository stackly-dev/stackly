//! Event subscriptions over a real connection (spec 0006 AC-9).

use serde_json::{Value, json};
use stackly_protocol::events::{EventKind, ShutdownReason, ShuttingDown};
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Raw, Site, request};

fn shutting_down() -> EventKind {
    EventKind::DaemonShuttingDown(ShuttingDown {
        reason: ShutdownReason::Requested,
    })
}

async fn subscribe(raw: &mut Raw, id: &str, params: Value) -> Value {
    raw.send(&request(id, "events.subscribe", Some(params)))
        .await;
    let frame = raw.recv().await.expect("the subscribe is answered");
    assert_eq!(frame["type"], "response", "{frame}");
    assert_eq!(frame["content"]["requestId"], id);
    frame["content"]["reply"]["content"].clone()
}

#[tokio::test]
async fn subscribed_comes_before_any_event_of_the_subscription() {
    let ev = evidence!(
        covers = ["AC-071", "NFR-004"],
        kind = Kind::Fixture,
        expected = "events.subscribe answers Subscribed with the head before the subscription's first event, and events then arrive in sequence order",
    );
    let site = Site::new();
    let daemon = site.start().await;
    for _ in 0..3 {
        daemon.bus().publish_daemon(shutting_down());
    }
    let mut raw = Raw::ready(&site.socket()).await;
    let since = json!({
        "since": {
            "serverInstanceId": daemon.info().server_instance_id.to_string(),
            "sequence": 1
        }
    });

    raw.send(&request("sub", "events.subscribe", Some(since)))
        .await;

    let first = raw.recv().await.expect("the reply arrives");
    assert_eq!(first["type"], "response", "{first}");
    assert_eq!(first["content"]["reply"]["content"]["head"]["sequence"], 3);
    daemon.bus().publish_daemon(shutting_down());
    let mut sequences = Vec::new();
    for _ in 0..3 {
        let event = raw.recv().await.expect("an event arrives");
        assert_eq!(event["type"], "event", "{event}");
        assert_eq!(event["content"]["subscriptionId"], 1);
        sequences.push(event["content"]["sequence"].as_u64().unwrap());
    }
    assert_eq!(sequences, [2, 3, 4]);
    ev.pass();
}

#[tokio::test]
async fn gaps_and_shutdown_events_pass_any_filter() {
    let ev = evidence!(
        covers = ["AC-071"],
        kind = Kind::Fixture,
        expected = "a subscription filtered to operation events still gets events.gap (server_restarted) and daemon.shutting_down",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let params = json!({
        "kinds": ["operation."],
        "since": { "serverInstanceId": uuid::Uuid::now_v7().to_string(), "sequence": 0 }
    });

    subscribe(&mut raw, "sub", params).await;
    daemon.bus().publish_daemon(shutting_down());

    let gap = raw.recv().await.expect("the gap arrives");
    assert_eq!(gap["content"]["kind"]["type"], "events.gap");
    assert_eq!(
        gap["content"]["kind"]["content"]["reason"],
        "server_restarted"
    );
    let event = raw.recv().await.expect("the shutdown event arrives");
    assert_eq!(event["content"]["kind"]["type"], "daemon.shutting_down");
    assert!(event["content"]["resource"].is_null());
    assert!(event["content"]["revision"].is_null());
    ev.pass();
}

#[tokio::test]
async fn an_unsubscribed_subscription_gets_no_more_events() {
    let site = Site::new();
    let daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;
    subscribe(&mut raw, "a", json!({})).await;
    subscribe(&mut raw, "b", json!({})).await;

    raw.send(&request(
        "un",
        "events.unsubscribe",
        Some(json!({ "subscriptionId": 1 })),
    ))
    .await;
    assert_eq!(raw.answer("un").await["type"], "response");
    daemon.bus().publish_daemon(shutting_down());

    let event = raw.recv().await.expect("an event arrives");
    assert_eq!(event["content"]["subscriptionId"], 2);
    raw.send(&request("info", "daemon.get_info", None)).await;
    let next = raw.recv().await.expect("the reply arrives");
    assert_eq!(
        next["content"]["requestId"], "info",
        "no second event: {next}"
    );
}

/// Reads `count` events and returns their sequences.
async fn sequences(raw: &mut Raw, count: usize) -> Vec<u64> {
    let mut sequences = Vec::new();
    for _ in 0..count {
        let event = raw.recv().await.expect("an event arrives");
        assert_eq!(event["type"], "event", "{event}");
        sequences.push(
            event["content"]["sequence"]
                .as_u64()
                .expect("an event carries its sequence"),
        );
    }
    sequences
}

#[tokio::test]
async fn a_reconnect_with_the_last_cursor_gets_the_missed_events_once_then_live_ones() {
    let ev = evidence!(
        covers = ["AC-071", "NFR-004"],
        kind = Kind::Fixture,
        expected = "a client that reconnects with the cursor of the last event it saw gets the events it missed once, in order, then live events, with no snapshot needed",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let instance = daemon.info().server_instance_id.to_string();
    let mut first = Raw::ready(&site.socket()).await;
    subscribe(&mut first, "sub", json!({})).await;
    daemon.bus().publish_daemon(shutting_down());
    daemon.bus().publish_daemon(shutting_down());
    let seen = sequences(&mut first, 2).await;
    assert_eq!(seen, [1, 2]);
    drop(first);

    daemon.bus().publish_daemon(shutting_down());
    daemon.bus().publish_daemon(shutting_down());
    let mut second = Raw::ready(&site.socket()).await;
    let subscribed = subscribe(
        &mut second,
        "sub",
        json!({ "since": { "serverInstanceId": instance, "sequence": seen[1] } }),
    )
    .await;
    assert_eq!(subscribed["head"]["sequence"], 4);
    daemon.bus().publish_daemon(shutting_down());

    assert_eq!(sequences(&mut second, 3).await, [3, 4, 5]);
    second.send(&request("info", "daemon.get_info", None)).await;
    let next = second.recv().await.expect("the reply arrives");
    assert_eq!(next["content"]["requestId"], "info", "no duplicate: {next}");
    ev.pass();
}

#[tokio::test]
async fn a_cursor_older_than_the_ring_starts_with_a_cursor_expired_gap() {
    let ev = evidence!(
        covers = ["AC-071"],
        kind = Kind::Fixture,
        expected = "a cursor of this run that fell out of the 4096 event ring starts the subscription with events.gap reason cursor_expired at the head, so the client takes a snapshot",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let instance = daemon.info().server_instance_id.to_string();
    let published = u64::from(stackly_protocol::limits::EVENT_REPLAY_LEN) + 2;
    for _ in 0..published {
        daemon.bus().publish_daemon(shutting_down());
    }
    let mut raw = Raw::ready(&site.socket()).await;

    let subscribed = subscribe(
        &mut raw,
        "sub",
        json!({ "since": { "serverInstanceId": instance, "sequence": 1 } }),
    )
    .await;

    assert_eq!(subscribed["head"]["sequence"], published);
    let gap = raw.recv().await.expect("the gap arrives");
    assert_eq!(gap["content"]["kind"]["type"], "events.gap", "{gap}");
    assert_eq!(
        gap["content"]["kind"]["content"]["reason"],
        "cursor_expired"
    );
    daemon.bus().publish_daemon(shutting_down());
    let live = raw.recv().await.expect("the live event arrives");
    assert_eq!(live["content"]["sequence"], published + 1, "{live}");
    ev.pass();
}
