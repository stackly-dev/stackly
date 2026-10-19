//! AC-9: the connect and request time limits and the `daemon.starting`
//! retry.

use std::time::Duration;

use stackly_protocol::Reply;
use stackly_protocol::codes::{daemon, request};
use stackly_protocol::daemon::DaemonHealthStatus;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{Ctx, Handshake, ScriptedDaemon, Step};
use crate::common::{detail, dto, envelope, run};

fn starting() -> stackly_protocol::ErrorBody {
    stackly_protocol::ErrorBody::new(&daemon::STARTING, "the daemon is starting")
        .with_detail("subsystem", "registry")
}

fn healthy(call: &stackly_protocol::Call) -> Vec<Step> {
    match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(false)))],
        _ => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            DaemonHealthStatus::Ready,
        )))],
    }
}

#[test]
fn a_request_without_an_answer_times_out_after_10_seconds_with_its_own_request_id() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a daemon that never answers daemon.get_info gives ipc.call_timeout after 10 seconds, exit 8, with the requestId the -v line printed and the daemon logged",
    );
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::serve(&root, |_, _| Vec::new());

    let run = run(root.path(), &["daemon", "status", "--json", "-v"]);

    assert_eq!(run.code, Some(8), "{run:?}");
    assert!(run.elapsed >= Duration::from_secs(10), "{:?}", run.elapsed);
    assert!(run.elapsed < Duration::from_secs(14), "{:?}", run.elapsed);
    let value = envelope(&run);
    assert_eq!(value["error"]["code"], "ipc.call_timeout");
    assert_eq!(
        detail(&value["error"], "method").as_deref(),
        Some("daemon.get_info")
    );
    assert_eq!(detail(&value["error"], "seconds").as_deref(), Some("10"));
    assert_eq!(detail(&value["error"], "idempotencyKey"), None);
    let logged = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "daemon.get_info")
        .unwrap();
    assert_eq!(value["requestId"], logged.request_id);
    assert!(
        run.stderr
            .contains(&format!("-> daemon.get_info {}\n", logged.request_id)),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("<- timeout "), "{}", run.stderr);
    ev.pass();
}

#[test]
fn a_handshake_without_an_answer_times_out_after_5_seconds() {
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::start(&root, |_| Handshake::Hang, healthy_handler);

    let run = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(run.code, Some(8), "{run:?}");
    assert!(run.elapsed >= Duration::from_secs(5));
    assert!(run.elapsed < Duration::from_secs(9), "{:?}", run.elapsed);
    let value = envelope(&run);
    assert_eq!(value["error"]["code"], "ipc.call_timeout");
    assert_eq!(
        detail(&value["error"], "method").as_deref(),
        Some("daemon.handshake")
    );
    assert_eq!(detail(&value["error"], "seconds").as_deref(), Some("5"));
    assert_eq!(value["requestId"], serde_json::Value::Null);
    assert_eq!(daemon.methods(), ["daemon.handshake"]);
}

fn healthy_handler(call: &stackly_protocol::Call, _: Ctx) -> Vec<Step> {
    healthy(call)
}

#[test]
fn daemon_starting_is_retried_until_the_daemon_is_ready() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "three daemon.starting answers are retried with backoff and the fourth answer gives exit 0",
    );
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::serve(&root, |call, ctx| {
        if call.method() == "daemon.get_info" && ctx.nth < 3 {
            vec![Step::Fail(starting())]
        } else {
            healthy(call)
        }
    });

    let run = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    let methods = daemon.methods();
    assert_eq!(
        methods
            .iter()
            .filter(|method| *method == "daemon.get_info")
            .count(),
        4
    );
    assert!(
        run.elapsed >= Duration::from_millis(700),
        "{:?}",
        run.elapsed
    );
    ev.pass();
}

#[test]
fn a_daemon_that_keeps_starting_exits_6_after_about_10_seconds() {
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |_, _| vec![Step::Fail(starting())]);

    let run = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(run.code, Some(6), "{run:?}");
    assert!(run.elapsed >= Duration::from_secs(10), "{:?}", run.elapsed);
    assert!(run.elapsed < Duration::from_secs(13), "{:?}", run.elapsed);
    assert_eq!(envelope(&run)["error"]["code"], "daemon.starting");
}

#[test]
fn other_retryable_codes_are_never_retried() {
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::serve(&root, |_, _| {
        vec![Step::Fail(
            stackly_protocol::ErrorBody::new(&request::TOO_MANY_IN_FLIGHT, "too many")
                .with_detail("max", "64"),
        )]
    });

    let run = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(run.code, Some(6), "{run:?}");
    assert_eq!(daemon.methods(), ["daemon.handshake", "daemon.get_info"]);
}
