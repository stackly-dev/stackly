//! AC-13: one `-v` line when a request is sent and one when it ends, never
//! params, reply content, or secrets.

use std::time::Duration;

use stackly_protocol::Reply;
use stackly_protocol::daemon::DaemonHealthStatus;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{Handshake, ScriptedDaemon, Step, handshake_answer};
use crate::common::{dto, run};

#[test]
fn verbose_writes_a_line_before_each_request_and_one_after_without_reply_content() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "-v writes `-> <method> <requestId>` before each request and `<- <outcome> <ms> ms` after it, with the daemon's request ids, no reply content, and also with -q",
    );
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::start(
        &root,
        |index| Handshake::Accept(Box::new(handshake_answer(index, true))),
        |call, _| match call.method() {
            "daemon.get_info" => {
                let mut info = dto::info(false);
                info.version = "reply-sentinel".to_owned();
                vec![
                    Step::Sleep(Duration::from_millis(300)),
                    Step::Reply(Reply::DaemonGetInfo(info)),
                ]
            }
            _ => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
                DaemonHealthStatus::Ready,
            )))],
        },
    );

    let run = run(root.path(), &["daemon", "status", "-v", "-q"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    let ids: Vec<String> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method != "daemon.handshake")
        .map(|logged| logged.request_id)
        .collect();
    let lines: Vec<&str> = run.stderr.lines().collect();
    assert_eq!(lines.len(), 6, "{}", run.stderr);
    assert_eq!(lines[0], "-> daemon.handshake");
    assert!(lines[1].starts_with("<- ok ") && lines[1].ends_with(" ms"));
    assert_eq!(lines[2], format!("-> daemon.get_info {}", ids[0]));
    let ms: u64 = lines[3]
        .strip_prefix("<- ok ")
        .and_then(|rest| rest.strip_suffix(" ms"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(ms >= 300, "the end line comes after the reply: {ms}");
    assert_eq!(lines[4], format!("-> daemon.get_health {}", ids[1]));
    assert!(lines[5].starts_with("<- ok "));
    assert!(!run.stderr.contains("reply-sentinel"));
    assert!(
        !run.stderr.contains("warning:"),
        "-q still drops the drift warning"
    );
    let id: uuid::Uuid = ids[0].parse().unwrap();
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(ids[0], ids[0].to_lowercase());
    ev.pass();
}
