//! The real `stacklyd` binary serving clients (spec 0006 milestone 4): no
//! TCP or UDP socket, the socket coming back after it was removed, the stop
//! on SIGTERM, a cursor from an earlier run, the request log lines, and the
//! round trip time of `daemon.get_info`.
#![cfg(target_os = "macos")]

mod common;

use std::process::Command;
use std::time::{Duration, Instant};

use common::{client, connects, daemon_paths, field, json_lines, runtime, start_daemon};
use serde_json::Value;
use stackly_ipc::{ConnectPolicy, connect_verified};
use stackly_protocol::codes::ipc;
use stackly_protocol::events::{EventKind, GapReason, SubscribeParams};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::{CONTRACT_DIGEST, Call, ProtocolVersion, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

fn lsof(pid: u32, selector: &str) -> String {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), selector])
        .output()
        .expect("lsof runs");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_daemon_never_opens_a_tcp_or_udp_socket_while_it_serves() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-070"],
        kind = Kind::Fixture,
        expected = "with three clients connected, lsof -i lists nothing for the daemon while lsof -U shows its unix socket",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let rt = runtime();
    let clients: Vec<_> = (0..3)
        .map(|_| rt.block_on(client(daemon.socket())))
        .collect();

    let network = lsof(daemon.pid(), "-i");
    let unix = lsof(daemon.pid(), "-U");

    assert_eq!(network, "", "network sockets: {network}");
    assert!(unix.contains("stacklyd.sock"), "unix sockets: {unix}");
    drop(clients);
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn a_removed_socket_and_runtime_root_come_back_within_12_seconds() {
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);

    std::fs::remove_dir_all(&paths.runtime).unwrap();
    let removed = Instant::now();
    while !connects(&paths.socket) {
        assert!(
            removed.elapsed() < Duration::from_secs(12),
            "the socket did not come back in time"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    let rt = runtime();
    let health = rt.block_on(async {
        let (client, _) = client(&paths.socket).await;
        client.call(Call::DaemonGetHealth).await.unwrap()
    });
    let Reply::DaemonGetHealth(health) = health else {
        panic!("not a health reply");
    };
    assert_eq!(health.counters.socket_rebinds.get(), 1);
    let lines = json_lines(&daemon.stderr());
    assert!(
        lines.iter().any(|line| field(line, "message")
            .as_str()
            .is_some_and(|m| m.starts_with("socket_rebound"))),
        "{lines:?}"
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
}

#[test]
fn sigterm_tells_subscribers_removes_the_socket_and_exits_0_so_a_new_daemon_starts_at_once() {
    let ev = evidence!(
        covers = ["BR-011"],
        kind = Kind::Fixture,
        expected = "on SIGTERM a subscriber gets daemon.shutting_down, the process exits 0 within 7 seconds, the socket is gone, and the next daemon takes the lock right away",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);
    let rt = runtime();
    let (client, _) = rt.block_on(client(&paths.socket));
    let (_, mut stream) = rt
        .block_on(client.subscribe(SubscribeParams::default()))
        .unwrap();

    let started = Instant::now();
    let stop = std::thread::spawn(move || daemon.stop());
    let event = rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(5), stream.next()).await })
        .expect("the shutdown event arrives in time")
        .unwrap()
        .expect("the stream still runs");
    let status = stop.join().unwrap().unwrap();

    match event {
        DecodedFrame::Event(event) => assert!(
            matches!(event.kind, EventKind::DaemonShuttingDown(_)),
            "{event:?}"
        ),
        other => panic!("expected an event, got {other:?}"),
    }
    assert_eq!(status.code(), Some(0));
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "{:?}",
        started.elapsed()
    );
    let refused = rt
        .block_on(connect_verified(
            &paths.socket,
            ConnectPolicy::same_user(common::uid()),
        ))
        .unwrap_err();
    assert!(
        refused.to_error_body().is(&ipc::ENDPOINT_MISSING),
        "{refused}"
    );
    drop(client);

    let next = start_daemon(&root);
    assert_eq!(next.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn a_cursor_from_an_earlier_run_starts_with_server_restarted() {
    let ev = evidence!(
        covers = ["AC-071"],
        kind = Kind::Fixture,
        expected = "subscribing with the cursor of a daemon run that ended gives events.gap with reason server_restarted as the first event",
    );
    let root = TestRoot::new().unwrap();
    let paths = daemon_paths(&root);
    let rt = runtime();
    let first = start_daemon(&root);
    let (_, accepted) = rt.block_on(client(&paths.socket));
    assert_eq!(first.stop().unwrap().code(), Some(0));

    let second = start_daemon(&root);
    let (client, now) = rt.block_on(client(&paths.socket));
    assert_ne!(now.server_instance_id, accepted.server_instance_id);
    let params = SubscribeParams {
        since: Some(accepted.head),
        ..SubscribeParams::default()
    };
    let (subscribed, mut stream) = rt.block_on(client.subscribe(params)).unwrap();
    let first_event = rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(5), stream.next()).await })
        .expect("the gap arrives in time")
        .unwrap()
        .unwrap();

    match first_event {
        DecodedFrame::Event(event) => {
            assert_eq!(
                event.kind,
                EventKind::EventsGap(stackly_protocol::events::Gap {
                    reason: GapReason::ServerRestarted
                })
            );
            assert_eq!(event.sequence, subscribed.head.sequence);
        }
        other => panic!("expected the gap, got {other:?}"),
    }
    drop(client);
    assert_eq!(second.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn request_lines_carry_the_request_fields_and_never_the_params() {
    let ev = evidence!(
        covers = ["NFR-011"],
        kind = Kind::Fixture,
        expected = "each request closes with a line holding connection_id, request_id, method, client_kind, duration_ms, and outcome, at debug on success and info on failure, in stderr and the log file, and no params value appears in the log",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);
    let rt = runtime();
    rt.block_on(async {
        let (client, _) = client(&paths.socket).await;
        client.call(Call::DaemonGetInfo).await.unwrap();
        let params = SubscribeParams {
            kinds: vec!["params_sentinel.".to_owned()],
            ..SubscribeParams::default()
        };
        let _ = client.subscribe(params).await.unwrap();
        client
            .call(Call::EventsUnsubscribe(
                stackly_protocol::events::UnsubscribeParams {
                    subscription_id: 77,
                },
            ))
            .await
            .unwrap_err();
    });
    let log_file = paths.layout.daemon_log_dir().join("stacklyd.log");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));

    for text in [
        std::fs::read_to_string(&log_file).unwrap(),
        daemon_stderr(&root),
    ] {
        assert!(
            !text.contains("params_sentinel"),
            "a params value was logged"
        );
        let lines = json_lines(&text);
        let requests: Vec<&Value> = lines
            .iter()
            .filter(|line| field(line, "message") == "request")
            .collect();
        let info = requests
            .iter()
            .find(|line| field(line, "method") == "daemon.get_info")
            .expect("the get_info line");
        assert_eq!(info["level"], "DEBUG");
        assert_eq!(field(info, "outcome"), "ok");
        assert_eq!(field(info, "client_kind"), "cli");
        assert!(field(info, "connection_id").as_u64().is_some());
        assert!(field(info, "duration_ms").as_u64().is_some());
        assert!(
            field(info, "request_id")
                .as_str()
                .is_some_and(|id| !id.is_empty())
        );
        let failed = requests
            .iter()
            .find(|line| field(line, "method") == "events.unsubscribe")
            .expect("the unsubscribe line");
        assert_eq!(failed["level"], "INFO");
        assert_eq!(field(failed, "outcome"), "error");
        assert_eq!(field(failed, "code"), "events.unknown_subscription");
    }
    ev.pass();
}

/// Every stderr file the daemons of `root` wrote, joined.
fn daemon_stderr(root: &TestRoot) -> String {
    let mut text = String::new();
    for entry in std::fs::read_dir(root.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "stderr") {
            text.push_str(&std::fs::read_to_string(path).unwrap());
        }
    }
    text
}

#[test]
fn a_client_with_another_contract_digest_connects_with_one_drift_warning() {
    let ev = evidence!(
        covers = ["AC-071"],
        kind = Kind::Fixture,
        expected = "a handshake with the daemon's protocol version and another contract digest is accepted with contractDrift true and the daemon logs one contract_drift warning",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);
    let rt = runtime();
    let accepted = rt.block_on(async {
        let mut connection =
            connect_verified(&paths.socket, ConnectPolicy::same_user(common::uid()))
                .await
                .unwrap();
        let current = ProtocolVersion::CURRENT;
        let other_digest = if CONTRACT_DIGEST.starts_with('a') {
            "b"
        } else {
            "a"
        }
        .repeat(64);
        let request = serde_json::json!({
            "requestId": "drift",
            "call": { "type": "daemon.handshake", "content": {
                "protocol": { "major": current.major, "minor": current.minor },
                "client": { "kind": "gui", "version": "0.1.0" },
                "contractDigest": other_digest,
            }}
        });
        connection
            .write_frame(&serde_json::to_vec(&request).unwrap())
            .await
            .unwrap();
        let reply = connection.read_frame(None).await.unwrap().unwrap();
        serde_json::from_slice::<Value>(&reply).unwrap()
    });
    assert_eq!(daemon.stop().unwrap().code(), Some(0));

    assert_eq!(
        accepted["content"]["reply"]["content"]["contractDrift"],
        true
    );
    let lines = json_lines(&daemon_stderr(&root));
    let warnings: Vec<_> = lines
        .iter()
        .filter(|line| field(line, "contract_drift") == true)
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0]["level"], "WARN");
    ev.pass();
}

#[test]
fn five_hundred_get_info_calls_on_one_connection_have_a_p95_of_at_most_20_ms() {
    let ev = evidence!(
        covers = ["NFR-001", "NFR-006"],
        kind = Kind::Fixture,
        expected = "500 sequential daemon.get_info calls over one connection to a real stacklyd on this Mac have a p95 of at most 20 ms",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let rt = runtime();
    let mut times = rt.block_on(async {
        let (client, _) = client(daemon.socket()).await;
        let mut times = Vec::with_capacity(500);
        for _ in 0..500 {
            let started = Instant::now();
            let reply = client.call(Call::DaemonGetInfo).await.unwrap();
            times.push(started.elapsed());
            assert!(matches!(reply, Reply::DaemonGetInfo(_)));
        }
        times
    });
    assert_eq!(daemon.stop().unwrap().code(), Some(0));

    times.sort();
    let p50 = times[249];
    let p95 = times[474];
    assert!(p95 <= Duration::from_millis(20), "p50 {p50:?}, p95 {p95:?}");
    ev.pass_with(&format!(
        "500 calls: p50 {} us, p95 {} us",
        p50.as_micros(),
        p95.as_micros()
    ));
}

#[test]
fn the_daemon_binary_answers_with_its_own_socket_path_and_the_stackly_root() {
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);
    let rt = runtime();

    let info = rt.block_on(async {
        let (client, _) = client(&paths.socket).await;
        client.call(Call::DaemonGetInfo).await.unwrap()
    });

    let Reply::DaemonGetInfo(info) = info else {
        panic!("not an info reply");
    };
    assert_eq!(info.pid, daemon.pid());
    assert_eq!(info.socket_path, paths.socket.display().to_string());
    assert_eq!(
        info.roots.data,
        root.path().join("data").display().to_string()
    );
    assert!(info.roots.root_override);
    assert_eq!(info.uid, Some(common::uid()));
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
}
