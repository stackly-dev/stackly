//! AC-2: with `--json`, exactly one clean envelope line on stdout.

use stackly_protocol::Reply;
use stackly_protocol::daemon::DaemonHealthStatus;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{dto, envelope_of, run};

/// A daemon whose `get_health` fails with `registry.unavailable`.
fn half_broken(root: &TestRoot) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(true)))],
        _ => vec![Step::Fail(
            error(
                "registry.unavailable",
                "unavailable",
                "the registry cannot serve calls",
            )
            .with_hint("restart stacklyd"),
        )],
    })
}

#[test]
fn json_on_a_terminal_with_color_always_is_one_clean_line_carrying_the_daemons_error() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "with stderr on a pty and --color always, --json writes one compact line on stdout without ESC or CR, fields in order, whose error is the daemon's body unchanged and whose requestId is the failed request's",
    );
    let root = TestRoot::new().unwrap();
    let daemon = half_broken(&root);

    let run = run_in_pty(
        root.path(),
        &["--color", "always", "daemon", "status", "--json"],
        &PtyOptions::default(),
    );

    assert_eq!(run.code, Some(6), "{run:?}");
    let value = envelope_of(&run.stdout);
    assert_eq!(value["command"], "daemon status");
    assert_eq!(value["result"]["info"]["pid"], 4242);
    assert_eq!(value["result"]["health"], serde_json::Value::Null);
    let sent = serde_json::to_value(
        error(
            "registry.unavailable",
            "unavailable",
            "the registry cannot serve calls",
        )
        .with_hint("restart stacklyd"),
    )
    .unwrap();
    assert_eq!(value["error"], sent, "the body is the daemon's, unchanged");
    let health = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "daemon.get_health")
        .unwrap();
    assert_eq!(value["requestId"], health.request_id);
    assert_eq!(value["warnings"][0]["code"], "cli.root_override");
    assert_eq!(run.stderr, "", "warnings stay in the envelope");
    ev.pass();
}

#[test]
fn a_cli_side_error_has_no_request_id_and_no_cli_hint_in_the_body() {
    let root = TestRoot::new().unwrap();

    let run = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(run.code, Some(6));
    let value = envelope_of(&run.stdout);
    assert_eq!(value["error"]["code"], "ipc.endpoint_missing");
    assert_eq!(
        value["error"]["message"],
        "nothing exists at the socket path"
    );
    assert_eq!(value["error"]["hint"], serde_json::Value::Null);
    assert_eq!(value["requestId"], serde_json::Value::Null);
    assert_eq!(run.stderr, "");
}

#[test]
fn a_successful_status_has_a_null_error_and_the_result_in_struct_order() {
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(false)))],
        _ => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            DaemonHealthStatus::Ready,
        )))],
    });

    let run = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    let value = envelope_of(&run.stdout);
    assert_eq!(value["error"], serde_json::Value::Null);
    assert_eq!(value["requestId"], serde_json::Value::Null);
    assert_eq!(value["warnings"], serde_json::json!([]));
    let line = run.stdout.trim_end();
    let result = &line[line.find("\"result\":").unwrap() + 9..line.find(",\"error\":").unwrap()];
    assert_eq!(
        crate::common::key_order(result),
        ["info", "health", "client", "agent"]
    );
    crate::common::golden::check("daemon_status.json", &run.stdout);
}
