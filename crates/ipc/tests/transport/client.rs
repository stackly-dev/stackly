//! The verified client against a small fake daemon built on the real
//! listener: handshake, matching replies by `requestId`, events with
//! unknown kinds skipped, and how each way the connection can end reaches
//! a waiting call (AC-3, AC-8).

use std::time::Duration;

use stackly_ipc::{CallOptions, Client, ConnectPolicy, IpcError, Listener};
use stackly_protocol::codes::{events, ipc, protocol, request};
use stackly_protocol::events::{
    Event, EventCursor, EventKind, ShutdownReason, ShuttingDown, SubscribeParams, Subscribed,
    UnsubscribeParams,
};
use stackly_protocol::handshake::{ClientInfo, ClientKind, DaemonIdentity, HandshakeAccepted};
use stackly_protocol::json::{DecodedFrame, is_request_id};
use stackly_protocol::{
    CONTRACT_DIGEST, Call, ErrorBody, ErrorFrame, Limits, ProtocolVersion, Reply, Sequence,
    ServerFrame,
};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::{Site, accept_authorized, read_request, respond};

const SERVER_INSTANCE: Uuid = Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d0e);

fn client_info() -> ClientInfo {
    ClientInfo {
        kind: ClientKind::Cli,
        version: "0.1.0".to_owned(),
        pid: Some(std::process::id()),
        interactive: None,
    }
}

fn accepted() -> HandshakeAccepted {
    HandshakeAccepted {
        protocol: ProtocolVersion::CURRENT,
        server_instance_id: SERVER_INSTANCE,
        daemon: DaemonIdentity {
            version: "0.1.0".to_owned(),
            pid: 4242,
        },
        contract_digest: CONTRACT_DIGEST.to_owned(),
        contract_drift: false,
        limits: Limits::CURRENT,
        head: EventCursor {
            server_instance_id: SERVER_INSTANCE,
            sequence: Sequence::ZERO,
        },
    }
}

fn unsubscribe(id: u32) -> Call {
    Call::EventsUnsubscribe(UnsubscribeParams {
        subscription_id: id,
    })
}

/// Accepts one client and answers its handshake.
async fn accept_and_handshake(listener: &Listener) -> stackly_ipc::FramedConnection {
    let (mut connection, _) = accept_authorized(listener).await;
    let request = read_request(&mut connection).await;
    assert!(matches!(request.call, Call::DaemonHandshake(_)));
    respond(
        &mut connection,
        &request.request_id,
        Reply::DaemonHandshake(accepted()),
    )
    .await;
    connection
}

async fn connect(site: &Site) -> Client {
    let (client, handshake) = Client::connect(
        &site.socket,
        ConnectPolicy::same_user(site.uid),
        client_info(),
    )
    .await
    .expect("the fake daemon accepts the handshake");
    assert_eq!(handshake, accepted());
    client
}

#[tokio::test]
async fn connect_sends_the_current_version_digest_and_a_uuid_v7_request_id() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let (mut connection, _) = accept_authorized(&listener).await;
        let request = read_request(&mut connection).await;
        respond(
            &mut connection,
            &request.request_id,
            Reply::DaemonHandshake(accepted()),
        )
        .await;
        (request, connection)
    });

    let (client, handshake) = Client::connect(
        &site.socket,
        ConnectPolicy::same_user(site.uid),
        client_info(),
    )
    .await
    .unwrap();
    let (request, _connection) = daemon.await.unwrap();

    assert_eq!(handshake, accepted());
    let Call::DaemonHandshake(params) = request.call else {
        panic!("the first request is the handshake");
    };
    assert_eq!(params.protocol, ProtocolVersion::CURRENT);
    assert_eq!(params.contract_digest, CONTRACT_DIGEST);
    assert_eq!(params.client, client_info());
    assert_eq!(request.idempotency_key, None);
    assert!(is_request_id(&request.request_id));
    let id: Uuid = request.request_id.parse().unwrap();
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(request.request_id, request.request_id.to_lowercase());
    drop(client);
}

#[tokio::test]
async fn a_major_mismatch_comes_back_as_the_daemons_error() {
    let site = Site::new();
    let listener = site.listen();
    tokio::spawn(async move {
        let (mut connection, _) = accept_authorized(&listener).await;
        let request = read_request(&mut connection).await;
        let error = ErrorBody::new(&protocol::MAJOR_MISMATCH, "the daemon speaks protocol 2")
            .with_detail("serverMajor", "2")
            .with_detail("serverMinor", "0");
        connection
            .close_with_error(Some(request.request_id), error)
            .await
            .unwrap();
    });

    let error = Client::connect(
        &site.socket,
        ConnectPolicy::same_user(site.uid),
        client_info(),
    )
    .await
    .unwrap_err();

    let IpcError::Wire(body) = &error else {
        panic!("expected the daemon's error, got {error:?}");
    };
    assert!(body.is(&protocol::MAJOR_MISMATCH));
    assert_eq!(body.detail("serverMajor"), Some("2"));
    assert_eq!(error.to_error_body(), **body);
}

#[tokio::test]
async fn a_peer_rejection_is_reported_instead_of_connection_closed() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047", "BR-012"],
        kind = Kind::Fixture,
        expected = "a client refused by the daemon's uid check reports the daemon's ipc.peer_rejected frame, read after its own write, not ipc.connection_closed",
    );
    let site = Site::new();
    let listener = Listener::bind(&site.socket, site.uid + 1).unwrap();
    let daemon = tokio::spawn(async move {
        for _ in 0..20 {
            listener.accept().await.unwrap();
        }
    });

    for _ in 0..20 {
        let error = Client::connect(
            &site.socket,
            ConnectPolicy::same_user(site.uid),
            client_info(),
        )
        .await
        .unwrap_err();

        let body = error.to_error_body();
        assert!(body.is(&ipc::PEER_REJECTED), "{error:?}");
    }
    daemon.await.unwrap();
    ev.pass();
}

#[tokio::test]
async fn replies_are_matched_by_request_id_even_out_of_order() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        let first = read_request(&mut connection).await;
        let second = read_request(&mut connection).await;
        let (ok, missing) = match (&first.call, &second.call) {
            (Call::EventsUnsubscribe(a), _) if a.subscription_id == 1 => (first, second),
            _ => (second, first),
        };
        let error = ErrorBody::new(&events::UNKNOWN_SUBSCRIPTION, "no such subscription")
            .with_detail("subscriptionId", "2");
        connection
            .write_server_frame(&ServerFrame::Error(ErrorFrame {
                request_id: Some(missing.request_id),
                error,
            }))
            .await
            .unwrap();
        respond(&mut connection, &ok.request_id, Reply::EventsUnsubscribe).await;
        connection
    });
    let client = connect(&site).await;

    let (one, two) = tokio::join!(client.call(unsubscribe(1)), client.call(unsubscribe(2)));

    assert_eq!(one.unwrap(), Reply::EventsUnsubscribe);
    let error = two.unwrap_err();
    assert!(error.is(&events::UNKNOWN_SUBSCRIPTION));
    assert_eq!(error.detail("subscriptionId"), Some("2"));
    drop(daemon.await.unwrap());
}

#[tokio::test]
async fn a_reply_with_another_tag_is_a_protocol_violation() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        let request = read_request(&mut connection).await;
        respond(
            &mut connection,
            &request.request_id,
            Reply::DaemonHandshake(accepted()),
        )
        .await;
        connection
    });
    let client = connect(&site).await;

    let error = client.call(unsubscribe(1)).await.unwrap_err();

    assert!(error.is(&ipc::PROTOCOL_VIOLATION), "{error:?}");
    drop(daemon.await.unwrap());
}

#[tokio::test]
async fn subscribe_streams_events_and_skips_kinds_this_build_does_not_know() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        let request = read_request(&mut connection).await;
        let head = EventCursor {
            server_instance_id: SERVER_INSTANCE,
            sequence: Sequence::new(1).unwrap(),
        };
        respond(
            &mut connection,
            &request.request_id,
            Reply::EventsSubscribe(Subscribed {
                subscription_id: 1,
                head,
            }),
        )
        .await;
        connection
            .write_server_frame(&ServerFrame::Event(Event {
                subscription_id: 1,
                sequence: Sequence::new(2).unwrap(),
                at: "2026-10-18T03:04:05.123Z".parse().unwrap(),
                resource: None,
                revision: None,
                kind: EventKind::DaemonShuttingDown(ShuttingDown {
                    reason: ShutdownReason::Signal,
                }),
            }))
            .await
            .unwrap();
        connection
            .write_frame(
                br#"{"type":"event","content":{"subscriptionId":1,"sequence":3,"at":"2026-10-18T03:04:06.000Z","resource":null,"revision":null,"kind":{"type":"instance.paused","content":{"why":"later minor"}}}}"#,
            )
            .await
            .unwrap();
        connection
            .write_frame(
                br#"{"type":"event","content":{"subscriptionId":9,"sequence":4,"at":"2026-10-18T03:04:07.000Z","resource":null,"revision":null,"kind":{"type":"daemon.shutting_down","content":{"reason":"signal"}}}}"#,
            )
            .await
            .unwrap();
    });
    let client = connect(&site).await;

    let (subscribed, mut stream) = client.subscribe(SubscribeParams::default()).await.unwrap();

    assert_eq!(subscribed.subscription_id, 1);
    assert_eq!(stream.subscription_id(), 1);
    match stream.next().await.unwrap() {
        Some(DecodedFrame::Event(event)) => {
            assert_eq!(event.sequence.get(), 2);
            assert_eq!(event.kind.name(), "daemon.shutting_down");
        }
        other => panic!("expected the shutdown event, got {other:?}"),
    }
    match stream.next().await.unwrap() {
        Some(DecodedFrame::SkippedEvent {
            subscription_id,
            sequence,
            kind,
        }) => {
            assert_eq!(subscription_id, 1);
            assert_eq!(sequence.get(), 3);
            assert_eq!(kind, "instance.paused");
        }
        other => panic!("expected a skipped event, got {other:?}"),
    }
    daemon.await.unwrap();
    assert_eq!(
        stream.next().await.unwrap(),
        None,
        "the event of another subscription is never handed out, and the stream ends cleanly"
    );
    let error = client.call(unsubscribe(1)).await.unwrap_err();
    assert!(error.is(&ipc::CONNECTION_CLOSED), "{error:?}");
}

#[tokio::test]
async fn a_daemon_that_closes_before_replying_gives_connection_closed() {
    let site = Site::new();
    let listener = site.listen();
    tokio::spawn(async move {
        let (mut connection, _) = accept_authorized(&listener).await;
        read_request(&mut connection).await;
    });

    let error = Client::connect(
        &site.socket,
        ConnectPolicy::same_user(site.uid),
        client_info(),
    )
    .await
    .unwrap_err();

    let body = error.to_error_body();
    assert!(body.is(&ipc::CONNECTION_CLOSED), "{error:?}");
    assert!(!body.retryable(), "the request may have run");
}

#[tokio::test]
async fn a_connection_level_error_fails_every_waiting_call() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        read_request(&mut connection).await;
        let error = ErrorBody::new(&ipc::TOO_MANY_CONNECTIONS, "the daemon is full")
            .with_detail("max", "64");
        connection.close_with_error(None, error).await.unwrap();
    });
    let client = connect(&site).await;

    let waiting = client.call(unsubscribe(1)).await.unwrap_err();
    daemon.await.unwrap();
    let later = client.call(unsubscribe(2)).await.unwrap_err();

    assert!(waiting.is(&ipc::TOO_MANY_CONNECTIONS), "{waiting:?}");
    assert!(later.is(&ipc::TOO_MANY_CONNECTIONS), "{later:?}");
}

#[tokio::test]
async fn a_daemon_frame_that_does_not_decode_is_a_protocol_violation() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        read_request(&mut connection).await;
        connection
            .write_frame(br#"{"type":"bogus","content":{}}"#)
            .await
            .unwrap();
        connection
    });
    let client = connect(&site).await;

    let error = client.call(unsubscribe(1)).await.unwrap_err();

    assert!(error.is(&ipc::PROTOCOL_VIOLATION), "{error:?}");
    drop(daemon.await.unwrap());
}

#[tokio::test]
async fn a_call_dropped_by_a_timeout_does_not_disturb_later_calls() {
    let site = Site::new();
    let listener = site.listen();
    let (late_tx, late_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        let abandoned = read_request(&mut connection).await;
        late_rx.await.unwrap();
        respond(
            &mut connection,
            &abandoned.request_id,
            Reply::EventsUnsubscribe,
        )
        .await;
        let next = read_request(&mut connection).await;
        respond(&mut connection, &next.request_id, Reply::EventsUnsubscribe).await;
        connection
    });
    let client = connect(&site).await;

    let timed_out =
        tokio::time::timeout(Duration::from_millis(100), client.call(unsubscribe(1))).await;
    assert!(timed_out.is_err());
    late_tx.send(()).unwrap();

    assert_eq!(
        client.call(unsubscribe(2)).await.unwrap(),
        Reply::EventsUnsubscribe
    );
    drop(daemon.await.unwrap());
}

#[tokio::test]
async fn a_request_above_the_frame_limit_is_refused_before_sending() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        let next = read_request(&mut connection).await;
        respond(&mut connection, &next.request_id, Reply::EventsUnsubscribe).await;
        connection
    });
    let client = connect(&site).await;
    let huge = SubscribeParams {
        kinds: vec!["daemon.".to_owned(); 200_000],
        ..SubscribeParams::default()
    };

    let error = client.subscribe(huge).await.unwrap_err();

    assert!(error.is(&ipc::FRAME_TOO_LARGE), "{error:?}");
    assert_eq!(
        client.call(unsubscribe(1)).await.unwrap(),
        Reply::EventsUnsubscribe,
        "nothing of the refused request reached the daemon"
    );
    drop(daemon.await.unwrap());
}

#[tokio::test]
async fn call_detailed_sends_the_callers_request_id_and_idempotency_key() {
    let site = Site::new();
    let listener = site.listen();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        let call = read_request(&mut connection).await;
        respond(&mut connection, &call.request_id, Reply::EventsUnsubscribe).await;
        let subscribe = read_request(&mut connection).await;
        let head = EventCursor {
            server_instance_id: SERVER_INSTANCE,
            sequence: Sequence::ZERO,
        };
        respond(
            &mut connection,
            &subscribe.request_id,
            Reply::EventsSubscribe(Subscribed {
                subscription_id: 1,
                head,
            }),
        )
        .await;
        (call, subscribe, connection)
    });
    let client = connect(&site).await;

    let reply = client
        .call_detailed(
            unsubscribe(1),
            CallOptions::default()
                .with_request_id("cli-0192f3a0")
                .with_idempotency_key("key-1"),
        )
        .await
        .unwrap();
    let (subscribed, _stream) = client
        .subscribe_detailed(
            SubscribeParams::default(),
            CallOptions::default().with_request_id("cli-sub-1"),
        )
        .await
        .unwrap();
    let (call, subscribe, _connection) = daemon.await.unwrap();

    assert_eq!(reply, Reply::EventsUnsubscribe);
    assert_eq!(call.request_id, "cli-0192f3a0");
    assert_eq!(call.idempotency_key.as_deref(), Some("key-1"));
    assert_eq!(subscribe.request_id, "cli-sub-1");
    assert_eq!(subscribe.idempotency_key, None);
    assert_eq!(subscribed.subscription_id, 1);
}

#[tokio::test]
async fn a_bad_request_id_or_key_is_refused_before_sending_and_a_waiting_id_cannot_be_reused() {
    let site = Site::new();
    let listener = site.listen();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(async move {
        let mut connection = accept_and_handshake(&listener).await;
        let first = read_request(&mut connection).await;
        release_rx.await.unwrap();
        respond(&mut connection, &first.request_id, Reply::EventsUnsubscribe).await;
        let next = read_request(&mut connection).await;
        respond(&mut connection, &next.request_id, Reply::EventsUnsubscribe).await;
        (first, next, connection)
    });
    let client = connect(&site).await;

    for (options, field) in [
        (
            CallOptions::default().with_request_id("has space"),
            "requestId",
        ),
        (CallOptions::default().with_request_id(""), "requestId"),
        (
            CallOptions::default().with_idempotency_key("bad key"),
            "idempotencyKey",
        ),
    ] {
        let error = client
            .call_detailed(unsubscribe(1), options)
            .await
            .unwrap_err();
        assert!(error.is(&request::MALFORMED), "{error:?}");
        assert_eq!(error.fields()[0].path(), field);
    }
    let waiting = client.call_detailed(
        unsubscribe(1),
        CallOptions::default().with_request_id("same-id"),
    );
    let duplicate = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let error = client
            .call_detailed(
                unsubscribe(2),
                CallOptions::default().with_request_id("same-id"),
            )
            .await
            .unwrap_err();
        release_tx.send(()).unwrap();
        error
    };
    let (first, duplicate) = tokio::join!(waiting, duplicate);

    assert_eq!(first.unwrap(), Reply::EventsUnsubscribe);
    assert!(duplicate.is(&request::DUPLICATE_ID), "{duplicate:?}");
    assert_eq!(
        client.call(unsubscribe(3)).await.unwrap(),
        Reply::EventsUnsubscribe
    );
    let (first, next, _connection) = daemon.await.unwrap();
    assert_eq!(first.request_id, "same-id");
    assert_ne!(next.request_id, "same-id", "the refused calls sent nothing");
}
