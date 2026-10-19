//! AC-5: interactive exactly when stdin and stderr are terminals and
//! neither `--no-input` nor `STACKLY_NO_INPUT` is on; the handshake tells
//! the daemon (spec 0006 AC-19).

use stackly_protocol::Reply;
use stackly_protocol::daemon::DaemonHealthStatus;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{dto, envelope, run_command, stackly};

fn ready(root: &TestRoot) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(false)))],
        _ => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            DaemonHealthStatus::Ready,
        )))],
    })
}

#[test]
fn stackly_no_input_takes_1_true_0_and_empty_and_refuses_anything_else() {
    let root = TestRoot::new().unwrap();
    let _daemon = ready(&root);
    for (value, code) in [
        ("1", 0),
        ("TRUE", 0),
        ("0", 0),
        ("", 0),
        ("false", 0),
        ("yes", 2),
    ] {
        let mut command = stackly(root.path());
        command
            .args(["daemon", "status", "--json"])
            .env("STACKLY_NO_INPUT", value);
        let run = run_command(command);
        assert_eq!(run.code, Some(code), "{value:?}: {run:?}");
        if code == 2 {
            let value = envelope(&run);
            assert_eq!(value["error"]["code"], "cli.invalid_environment");
            assert_eq!(
                crate::common::detail(&value["error"], "variable").as_deref(),
                Some("STACKLY_NO_INPUT")
            );
        }
    }
}

#[test]
fn the_handshake_says_interactive_only_on_terminals_without_no_input() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "the cli handshakes with interactive true only when stdin and stderr are terminals and neither --no-input nor STACKLY_NO_INPUT is on",
    );
    let root = TestRoot::new().unwrap();
    let daemon = ready(&root);
    let tty = PtyOptions {
        stdin_tty: true,
        ..PtyOptions::default()
    };

    let cases: [(Vec<&str>, PtyOptions<'_>, bool); 4] = [
        (vec!["daemon", "status"], tty.clone(), true),
        (vec!["daemon", "status", "--no-input"], tty.clone(), false),
        (
            vec!["daemon", "status"],
            PtyOptions {
                env: vec![("STACKLY_NO_INPUT", "1")],
                ..tty.clone()
            },
            false,
        ),
        (vec!["daemon", "status"], PtyOptions::default(), false),
    ];
    for (args, options, _) in &cases {
        let run = run_in_pty(root.path(), args, options);
        assert_eq!(run.code, Some(0), "{args:?}: {run:?}");
    }
    let piped = run_command({
        let mut command = stackly(root.path());
        command.args(["daemon", "status"]);
        command
    });
    assert_eq!(piped.code, Some(0));

    let flags: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "daemon.handshake")
        .map(|logged| logged.raw["call"]["content"]["client"]["interactive"].clone())
        .collect();
    let mut expected: Vec<serde_json::Value> = cases
        .iter()
        .map(|(_, _, interactive)| serde_json::Value::Bool(*interactive))
        .collect();
    expected.push(serde_json::Value::Bool(false));
    assert_eq!(flags, expected);
    ev.pass();
}
