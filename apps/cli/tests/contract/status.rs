//! AC-6 against a scripted daemon: warnings, a partial result, another
//! major, and the recovery block.

use stackly_protocol::Reply;
use stackly_protocol::codes::protocol;
use stackly_protocol::daemon::DaemonHealthStatus;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{Handshake, ScriptedDaemon, Step, error, handshake_answer};
use crate::common::{dto, envelope, golden, listing, run};

fn ready(
    root_override: bool,
) -> impl Fn(&stackly_protocol::Call, crate::common::scripted::Ctx) -> Vec<Step> + Send + Sync + 'static
{
    move |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(root_override)))],
        _ => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            DaemonHealthStatus::Ready,
        )))],
    }
}

#[test]
fn contract_drift_and_the_root_override_are_warnings_in_text_and_json() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a handshake with contractDrift and an active root override give the cli.contract_drift and cli.root_override warnings on stderr in text and only in warnings with --json, and still exit 0",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::start(
        &root,
        |index| Handshake::Accept(Box::new(handshake_answer(index, true))),
        ready(true),
    );

    let text = run(root.path(), &["daemon", "status"]);
    let json = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(text.stderr.contains(
        "warning: the STACKLY_ROOT override is active, with the data root at /stackly/data\n"
    ));
    assert!(
        text.stderr
            .contains("warning: the daemon has another contract digest")
    );
    assert!(text.stdout.contains("Override    yes (STACKLY_ROOT)\n"));
    assert_eq!(json.code, Some(0));
    let value = envelope(&json);
    let codes: Vec<&str> = value["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|warning| warning["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["cli.root_override", "cli.contract_drift"]);
    assert_eq!(value["result"]["client"]["contractDrift"], true);
    assert_eq!(json.stderr, "");
    ev.pass();
}

#[test]
fn a_failed_health_call_gives_info_with_null_health_and_the_exit_of_its_category() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "when daemon.get_info works and daemon.get_health fails, the result keeps the info with health null, error is set, and the exit code follows the error's category",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(false)))],
        _ => vec![Step::Fail(error(
            "test.denied",
            "unauthorized",
            "no health for you",
        ))],
    });

    let text = run(root.path(), &["daemon", "status"]);
    let json = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(text.code, Some(4));
    assert!(
        text.stdout.starts_with("Status     unknown\n"),
        "{}",
        text.stdout
    );
    assert!(
        text.stderr
            .starts_with("error: no health for you\n  code: test.denied\n")
    );
    assert_eq!(json.code, Some(4));
    let value = envelope(&json);
    assert_eq!(
        value["result"]["info"]["serverInstanceId"],
        dto::info(false).server_instance_id.to_string()
    );
    assert_eq!(value["result"]["health"], serde_json::Value::Null);
    assert_eq!(value["error"]["code"], "test.denied");
    ev.pass();
}

#[test]
fn a_daemon_of_another_major_exits_5() {
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::start(
        &root,
        |_| {
            Handshake::Refuse(Box::new(
                stackly_protocol::ErrorBody::new(
                    &protocol::MAJOR_MISMATCH,
                    "the daemon speaks protocol 2.0",
                )
                .with_detail("serverMajor", "2")
                .with_detail("serverMinor", "0"),
            ))
        },
        ready(false),
    );

    let text = run(root.path(), &["daemon", "status"]);
    let json = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(text.code, Some(5), "{text:?}");
    assert!(text.stderr.contains("code: protocol.major_mismatch"));
    assert_eq!(json.code, Some(5));
    assert_eq!(envelope(&json)["error"]["code"], "protocol.major_mismatch");
}

#[test]
fn recovery_mode_shows_the_recovery_block_and_hint_and_the_whole_object_in_json() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a daemon in recovery mode shows Recovery, Report, and Schema lines and the restore hint in text, the whole recovery object in --json, and exits 0",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(false)))],
        _ => vec![Step::Reply(Reply::DaemonGetHealth(dto::recovery_health()))],
    });
    let before = listing(root.path());

    let text = run(root.path(), &["daemon", "status"]);
    let json = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("daemon_status_recovery.txt", &text.stdout);
    assert_eq!(
        text.stderr,
        "hint: restore the registry from a snapshot or backup, then restart stacklyd\n"
    );
    assert_eq!(json.code, Some(0));
    let value = envelope(&json);
    assert_eq!(
        value["result"]["health"]["recovery"],
        serde_json::to_value(dto::recovery_health().recovery).unwrap()
    );
    let quiet = run(root.path(), &["daemon", "status", "-q"]);
    assert_eq!(quiet.stderr, "", "--quiet drops the hint");
    assert_eq!(listing(root.path()), before, "the cli wrote nothing");
    ev.pass();
}
