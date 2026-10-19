//! AC-3: text for people, colors only where asked, and daemon text escaped.

use stackly_protocol::Reply;
use stackly_protocol::daemon::{DaemonHealthStatus, SubsystemHealth, SubsystemStatus};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{dto, golden, run, stackly};

fn degraded(root: &TestRoot) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(false)))],
        _ => {
            let mut health = dto::health(DaemonHealthStatus::Degraded);
            health.subsystems[2] = SubsystemHealth {
                name: "operations".to_owned(),
                status: SubsystemStatus::Degraded,
                detail: Some("queue is slow".to_owned()),
            };
            vec![Step::Reply(Reply::DaemonGetHealth(health))]
        }
    })
}

#[test]
fn status_text_is_one_fact_per_line_in_local_time() {
    let root = TestRoot::new().unwrap();
    let _daemon = degraded(&root);

    let run = run(root.path(), &["daemon", "status"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(
        run.stdout
            .contains("Started     2026-10-18 03:04:05 +00:00 (")
    );
    golden::check("daemon_status.txt", &run.stdout);
    assert_eq!(run.stderr, "");
    let mut jakarta = stackly(root.path());
    jakarta.args(["daemon", "status"]).env("TZ", "Asia/Jakarta");
    let jakarta = crate::common::run_command(jakarta);
    assert!(
        jakarta
            .stdout
            .contains("Started     2026-10-18 10:04:05 +07:00 ("),
        "{}",
        jakarta.stdout
    );
}

#[test]
fn color_shows_only_on_a_terminal_in_auto_or_when_forced() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "text has ANSI color with --color always, CLICOLOR_FORCE=1, or a terminal in auto mode, and none with --color never, NO_COLOR, or a stdout that is not a terminal",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = degraded(&root);
    let colored = |run: &str| run.contains("\u{1b}[");

    let piped = run(root.path(), &["daemon", "status"]);
    let always = run(root.path(), &["daemon", "status", "--color", "always"]);
    let mut forced = stackly(root.path());
    forced.args(["daemon", "status"]).env("CLICOLOR_FORCE", "1");
    let forced = crate::common::run_command(forced);
    let mut never_forced = stackly(root.path());
    never_forced
        .args(["daemon", "status", "--color", "never"])
        .env("CLICOLOR_FORCE", "1");
    let never_forced = crate::common::run_command(never_forced);

    assert!(!colored(&piped.stdout), "{}", piped.stdout);
    assert!(colored(&always.stdout));
    assert!(always.stdout.contains("\u{1b}[33mdegraded\u{1b}[0m"));
    assert!(colored(&forced.stdout));
    assert!(!colored(&never_forced.stdout));

    let tty = run_in_pty(
        root.path(),
        &["daemon", "status", "--color", "always", "--nope"],
        &PtyOptions::default(),
    );
    assert_eq!(tty.code, Some(2));
    assert!(
        colored(&tty.raw_stderr),
        "clap follows --color: {}",
        tty.raw_stderr
    );
    let no_color = run_in_pty(
        root.path(),
        &["daemon", "status", "--nope"],
        &PtyOptions {
            env: vec![("NO_COLOR", "1")],
            ..PtyOptions::default()
        },
    );
    assert!(!colored(&no_color.raw_stderr), "{}", no_color.raw_stderr);
    ev.pass();
}

#[test]
fn control_characters_and_bidi_overrides_from_the_daemon_are_escaped() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "an ESC sequence, a bidi override, and a newline in daemon text reach the terminal as \\u{..} escapes, never raw",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call.method() {
        "daemon.get_info" => {
            let mut info = dto::info(false);
            info.version = "0.1.0\u{1b}[31m".to_owned();
            info.socket_path = "/tmp/\u{202e}kcos.dylkcats\nnext".to_owned();
            vec![Step::Reply(Reply::DaemonGetInfo(info))]
        }
        _ => {
            let mut health = dto::health(DaemonHealthStatus::Degraded);
            health.subsystems[0].status = SubsystemStatus::Unavailable;
            health.subsystems[0].detail = Some("\u{1b}]0;owned\u{7}".to_owned());
            vec![Step::Reply(Reply::DaemonGetHealth(health))]
        }
    });

    let run = run(root.path(), &["daemon", "status", "--color", "always"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(
        run.stdout.contains("0.1.0\\u{1b}[31m (protocol"),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("/tmp/\\u{202e}kcos.dylkcats\\u{a}next"));
    assert!(
        run.stdout
            .contains("ipc: unavailable (\\u{1b}]0;owned\\u{7})")
    );
    assert!(!run.stdout.contains('\u{202e}'));
    assert!(!run.stdout.contains("\u{1b}]"));
    assert!(!run.stdout.contains("\u{1b}[31m ("));
    ev.pass();
}
