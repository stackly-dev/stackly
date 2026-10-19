//! AC-8, AC-9, AC-4: `--wait` ends on a final state or `needs_recovery`,
//! maps it to the exit of its mode, stops on `--timeout` or Ctrl+C without
//! ever cancelling, reconnects, and shows progress on stderr only.

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use stackly_protocol::codes::daemon;
use stackly_protocol::events::EventKind;
use stackly_protocol::operation::{
    CancelDisposition, CountedProgress, OperationProgress, OperationState,
};
use stackly_protocol::{Call, Count, ErrorBody, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{Handshake, ScriptedDaemon, Step};
use crate::common::{detail, dto, envelope, envelope_of, run, stackly};
use crate::play::{got, playing, subscribed};

fn id() -> String {
    dto::OPERATION.to_string()
}

fn running(revision: u64) -> stackly_protocol::operation::OperationSummary {
    dto::summary(dto::OPERATION, OperationState::Running, revision)
}

fn queued() -> stackly_protocol::operation::OperationSummary {
    dto::summary(dto::OPERATION, OperationState::Queued, 1)
}

/// No request the daemon read was `operation.cancel`.
fn never_cancelled(daemon: &ScriptedDaemon) {
    assert!(
        !daemon
            .methods()
            .iter()
            .any(|method| method == "operation.cancel"),
        "{:?}",
        daemon.methods()
    );
}

#[test]
fn inspect_wait_plays_queued_and_running_then_exits_by_the_end_state() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "inspect --wait follows queued and running to the end: succeeded exits 0, failed and needs_recovery exit by their error's category or the cli.operation_* fallback, cancelled exits 9, and the result is the last snapshot",
    );
    let cases = [
        (OperationState::Succeeded, true, 0, None),
        (
            OperationState::Failed,
            true,
            7,
            Some("instance.start_failed"),
        ),
        (
            OperationState::Failed,
            false,
            7,
            Some("cli.operation_failed"),
        ),
        (
            OperationState::Cancelled,
            false,
            9,
            Some("cli.operation_cancelled"),
        ),
        (
            OperationState::NeedsRecovery,
            true,
            7,
            Some("instance.start_failed"),
        ),
        (
            OperationState::NeedsRecovery,
            false,
            7,
            Some("cli.operation_needs_recovery"),
        ),
    ];
    for (state, with_error, code, error) in cases {
        let root = TestRoot::new().unwrap();
        let daemon = ScriptedDaemon::serve(
            &root,
            playing(
                vec![queued(), running(2), dto::ended(state, with_error, 3)],
                None,
            ),
        );

        let run = run(
            root.path(),
            &["operation", "inspect", &id(), "--wait", "--json"],
        );

        assert_eq!(run.code, Some(code), "{state:?} {with_error}: {run:?}");
        let value = envelope(&run);
        assert_eq!(
            value["result"]["operation"]["state"],
            serde_json::to_value(state).unwrap()
        );
        assert_eq!(value["result"]["operation"]["revision"], 3);
        match error {
            Some(error) => assert_eq!(value["error"]["code"], error, "{state:?}"),
            None => assert_eq!(value["error"], serde_json::Value::Null),
        }
        if let Some(error) = error.filter(|error| error.starts_with("cli.")) {
            assert_eq!(value["error"]["operationId"], id(), "{error}");
        }
        assert!(
            run.stderr.starts_with(&format!(
                "waiting for operation {} (Ctrl+C stops waiting; the operation keeps running)\n",
                id()
            )),
            "{}",
            run.stderr
        );
        assert!(run.stderr.contains(" queued\n") && run.stderr.contains(" running\n"));
        never_cancelled(&daemon);
        assert_eq!(
            daemon.methods()[1],
            "events.subscribe",
            "subscribe comes before get"
        );
    }
    ev.pass();
}

#[test]
fn cancel_wait_exits_0_only_for_cancelled() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "cancel --wait exits 0 when the operation ends cancelled, 3 with cli.cancel_not_applied and the end state when it ends succeeded or failed, and by its error when it needs recovery",
    );
    for (state, code, error) in [
        (OperationState::Cancelled, 0, None),
        (OperationState::Succeeded, 3, Some("cli.cancel_not_applied")),
        (OperationState::Failed, 3, Some("cli.cancel_not_applied")),
        (
            OperationState::NeedsRecovery,
            7,
            Some("instance.start_failed"),
        ),
    ] {
        let root = TestRoot::new().unwrap();
        let mut cancelling = running(2);
        cancelling.state = OperationState::CancelRequested;
        let daemon = ScriptedDaemon::serve(
            &root,
            playing(
                vec![cancelling, dto::ended(state, true, 4)],
                Some(CancelDisposition::Requested),
            ),
        );

        let run = run(
            root.path(),
            &["operation", "cancel", &id(), "--wait", "--json"],
        );

        assert_eq!(run.code, Some(code), "{state:?}: {run:?}");
        let value = envelope(&run);
        assert_eq!(value["result"]["disposition"], "requested");
        assert_eq!(
            value["result"]["operation"]["state"],
            serde_json::to_value(state).unwrap()
        );
        match error {
            Some(error) => {
                assert_eq!(value["error"]["code"], error);
                if error == "cli.cancel_not_applied" {
                    assert_eq!(
                        detail(&value["error"], "state"),
                        Some(
                            serde_json::to_value(state)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .to_owned()
                        )
                    );
                }
            }
            None => assert_eq!(value["error"], serde_json::Value::Null),
        }
        let cancels = daemon
            .methods()
            .iter()
            .filter(|method| *method == "operation.cancel")
            .count();
        assert_eq!(cancels, 1, "exactly the one cancel the command sends");
    }
    ev.pass();
}

#[test]
fn a_timeout_of_one_second_ends_the_wait_with_exit_8_and_never_cancels() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "--timeout 1s ends the wait in about a second with cli.wait_timeout, exit 8, the operation id and last state, also while the daemon holds the handshake or keeps answering daemon.starting, and never sends operation.cancel",
    );
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::serve(&root, playing(vec![running(2)], None));
    let stalled_root = TestRoot::new().unwrap();
    let stalled = ScriptedDaemon::start(&stalled_root, |_| Handshake::Hang, |_, _| Vec::new());
    let starting_root = TestRoot::new().unwrap();
    let starting = ScriptedDaemon::serve(&starting_root, |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        _ => vec![Step::Fail(
            ErrorBody::new(&daemon::STARTING, "starting").with_detail("subsystem", "operations"),
        )],
    });

    for (root, state) in [
        (&root, "running"),
        (&stalled_root, "unknown"),
        (&starting_root, "unknown"),
    ] {
        let run = run(
            root.path(),
            &[
                "operation",
                "inspect",
                &id(),
                "--wait",
                "--timeout",
                "1s",
                "--json",
            ],
        );

        assert_eq!(run.code, Some(8), "{state}: {run:?}");
        assert!(run.elapsed >= Duration::from_secs(1), "{:?}", run.elapsed);
        assert!(
            run.elapsed < Duration::from_millis(2_500),
            "{state}: {:?}",
            run.elapsed
        );
        let value = envelope(&run);
        assert_eq!(value["error"]["code"], "cli.wait_timeout");
        assert_eq!(value["error"]["operationId"], id());
        assert_eq!(detail(&value["error"], "state").as_deref(), Some(state));
        assert_eq!(
            detail(&value["error"], "timeoutSeconds").as_deref(),
            Some("1")
        );
        if state == "running" {
            assert_eq!(value["result"]["operation"]["state"], "running");
        } else {
            assert_eq!(value["result"], serde_json::Value::Null);
        }
    }
    never_cancelled(&daemon);
    never_cancelled(&stalled);
    never_cancelled(&starting);
    ev.pass();
}

/// Starts `stackly <args>` with stderr piped, waits for the
/// `waiting for operation` line, sends SIGINT, and returns the exit code,
/// stdout, the rest of stderr, and how long the exit took after the signal.
fn interrupt(root: &TestRoot, args: &[&str]) -> (Option<i32>, String, String, Duration) {
    let mut command = stackly(root.path());
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("stackly starts");
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr is piped"));
    let mut first = String::new();
    stderr.read_line(&mut first).expect("stderr can be read");
    assert!(first.starts_with("waiting for operation "), "{first}");
    std::thread::sleep(Duration::from_millis(300));
    let sent = Instant::now();
    let status = std::process::Command::new("/bin/kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stderr, &mut rest).expect("stderr can be read");
    let output = child.wait_with_output().expect("stackly ends");
    (
        output.status.code(),
        String::from_utf8(output.stdout).expect("stdout is utf-8"),
        rest,
        sent.elapsed(),
    )
}

#[test]
fn ctrl_c_stops_waiting_with_exit_130_and_leaves_the_operation_running() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "SIGINT after the waiting line ends the wait at once with cli.interrupted and exit 130, names the operation and how to check it, also while connecting, and never sends operation.cancel",
    );
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::serve(&root, playing(vec![running(2)], None));

    let (code, stdout, stderr, after) =
        interrupt(&root, &["operation", "inspect", &id(), "--wait"]);

    assert_eq!(code, Some(130), "{stderr}");
    assert!(after < Duration::from_secs(1), "{after:?}");
    assert!(
        stdout.starts_with("ID ") && stdout.contains(&id()),
        "{stdout}"
    );
    assert!(
        stderr.contains(&format!(
            "error: stopped waiting; operation {} is still running\n  code: cli.interrupted\n  hint: check it with: stackly operation inspect {}\n",
            id(),
            id()
        )),
        "{stderr}"
    );
    never_cancelled(&daemon);

    let stalled_root = TestRoot::new().unwrap();
    let stalled = ScriptedDaemon::start(&stalled_root, |_| Handshake::Hang, |_, _| Vec::new());
    let (code, stdout, _, after) = interrupt(
        &stalled_root,
        &["operation", "inspect", &id(), "--wait", "--json"],
    );
    assert_eq!(code, Some(130));
    assert!(after < Duration::from_secs(1), "{after:?}");
    let value = envelope_of(&stdout);
    assert_eq!(value["error"]["code"], "cli.interrupted");
    assert_eq!(detail(&value["error"], "state").as_deref(), Some("unknown"));
    never_cancelled(&stalled);
    ev.pass();
}

#[test]
fn a_hundred_progress_events_in_a_second_cause_at_most_five_gets() {
    let root = TestRoot::new().unwrap();
    let started = Arc::new(std::sync::Mutex::new(None::<Instant>));
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        Call::OperationGet(_) => {
            let mut first = started.lock().unwrap();
            let since = *first.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_secs(1) {
                return got(&dto::ended(OperationState::Succeeded, false, 200));
            }
            let mut steps = vec![Step::Reply(Reply::OperationGet(Box::new(dto::detail(
                running(2 + ctx.nth as u64),
            ))))];
            if ctx.nth == 0 {
                for n in 0..100_u64 {
                    let mut progress = running(10 + n);
                    progress.progress = OperationProgress::Counted(CountedProgress {
                        done: Count::new(n).unwrap(),
                        total: Count::new(100).unwrap(),
                    });
                    steps.push(Step::Sleep(Duration::from_millis(5)));
                    steps.push(Step::Event(EventKind::OperationProgress(Box::new(
                        progress,
                    ))));
                }
            }
            steps
        }
        _ => Vec::new(),
    });

    let run = run(
        root.path(),
        &["operation", "inspect", &id(), "--wait", "--json"],
    );

    assert_eq!(run.code, Some(0), "{run:?}");
    let gets = daemon
        .methods()
        .iter()
        .filter(|method| *method == "operation.get")
        .count();
    assert!((2..=5).contains(&gets), "{gets} gets");
}

#[test]
fn a_daemon_that_comes_back_with_a_new_server_instance_lets_the_wait_finish() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054"],
        kind = Kind::Fixture,
        expected = "when the daemon closes the connection while the cli waits and comes back as a new server instance, the cli reconnects, subscribes and gets again, and the wait ends normally",
    );
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::serve(&root, |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        Call::OperationGet(_) if ctx.connection == 0 => {
            vec![
                Step::Reply(Reply::OperationGet(Box::new(dto::detail(running(2))))),
                Step::Sleep(Duration::from_millis(100)),
                Step::Close,
            ]
        }
        Call::OperationGet(_) => got(&dto::ended(OperationState::Succeeded, false, 5)),
        _ => Vec::new(),
    });

    let run = run(
        root.path(),
        &["operation", "inspect", &id(), "--wait", "--json"],
    );

    assert_eq!(run.code, Some(0), "{run:?}");
    assert_eq!(daemon.connections(), 2);
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "events.subscribe",
            "operation.get",
            "daemon.handshake",
            "events.subscribe",
            "operation.get",
        ]
    );
    assert_eq!(envelope(&run)["result"]["operation"]["state"], "succeeded");
    ev.pass();
}

#[test]
fn a_daemon_that_does_not_come_back_ends_the_wait_after_30_seconds_with_exit_6() {
    let root = TestRoot::new().unwrap();
    let mut daemon = ScriptedDaemon::serve(&root, |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        Call::OperationGet(_) => vec![
            Step::Reply(Reply::OperationGet(Box::new(dto::detail(running(2))))),
            Step::Close,
        ],
        _ => Vec::new(),
    });
    let mut command = stackly(root.path());
    command
        .args(["operation", "inspect", &id(), "--wait", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !daemon
        .methods()
        .iter()
        .any(|method| method == "operation.get")
    {
        assert!(Instant::now() < deadline, "the cli never got the operation");
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon.go_away();
    let gone = Instant::now();

    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(
        gone.elapsed() >= Duration::from_secs(29),
        "{:?}",
        gone.elapsed()
    );
    assert!(
        gone.elapsed() < Duration::from_secs(36),
        "{:?}",
        gone.elapsed()
    );
    let value = envelope_of(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(value["error"]["code"], "cli.wait_disconnected");
    assert_eq!(detail(&value["error"], "state").as_deref(), Some("running"));
    assert_eq!(value["error"]["operationId"], id());
    never_cancelled(&daemon);
}

#[test]
fn a_get_without_an_answer_or_with_starting_is_retried_on_the_same_connection() {
    let root = TestRoot::new().unwrap();
    let daemon = ScriptedDaemon::serve(&root, |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        Call::OperationGet(_) => match ctx.nth {
            0 => Vec::new(),
            1 | 2 => vec![Step::Fail(
                ErrorBody::new(&daemon::STARTING, "starting")
                    .with_detail("subsystem", "operations"),
            )],
            _ => got(&dto::ended(OperationState::Succeeded, false, 3)),
        },
        _ => Vec::new(),
    });

    let run = run(
        root.path(),
        &["operation", "inspect", &id(), "--wait", "--json"],
    );

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(
        run.elapsed >= Duration::from_secs(10),
        "the first get timed out"
    );
    assert_eq!(
        daemon.connections(),
        1,
        "no reconnect for a timeout or starting"
    );
    let gets = daemon
        .methods()
        .iter()
        .filter(|method| *method == "operation.get")
        .count();
    assert_eq!(gets, 4);
}

#[test]
fn an_error_that_is_not_a_disconnect_ends_the_wait_with_that_error() {
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        _ => vec![Step::Fail(crate::common::scripted::error(
            "resource.not_found",
            "not_found",
            "no such operation",
        ))],
    });

    let run = run(
        root.path(),
        &["operation", "inspect", &id(), "--wait", "--json"],
    );

    assert_eq!(run.code, Some(10), "{run:?}");
    assert_eq!(envelope(&run)["error"]["code"], "resource.not_found");
}

/// A daemon that plays a few counted progress steps before success, on
/// every connection.
fn progressing(root: &TestRoot) -> ScriptedDaemon {
    let gets = Arc::new(std::sync::Mutex::new(
        std::collections::HashMap::<usize, u64>::new(),
    ));
    ScriptedDaemon::serve(root, move |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        Call::OperationGet(_) => {
            let n = {
                let mut gets = gets.lock().expect("the count lock is never poisoned");
                let count = gets.entry(ctx.connection).or_default();
                *count += 1;
                *count - 1
            };
            if n >= 3 {
                return got(&dto::ended(OperationState::Succeeded, false, 10));
            }
            let mut summary = running(2 + n);
            summary.stage = Some(if n == 0 { "fetch" } else { "unpack" }.to_owned());
            summary.progress = OperationProgress::Counted(CountedProgress {
                done: Count::new(n).expect("small counts are safe"),
                total: Count::new(3).expect("3 is a safe count"),
            });
            got(&summary)
        }
        _ => Vec::new(),
    })
}

#[test]
fn progress_on_a_terminal_is_one_live_line_and_stdout_keeps_only_the_result() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "on a terminal, progress is one live line redrawn with CR and ESC [ K on stderr, also with --color never and NO_COLOR, while stdout holds only the result, and with --json one clean envelope line",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = progressing(&root);
    let args = [
        "operation",
        "inspect",
        dto::OPERATION.to_string().leak(),
        "--wait",
    ];

    for (extra, env) in [
        (&[][..], vec![]),
        (&["--color", "never"][..], vec![]),
        (&[][..], vec![("NO_COLOR", "1")]),
    ] {
        let mut all: Vec<&str> = args.to_vec();
        all.extend_from_slice(extra);
        let run = run_in_pty(
            root.path(),
            &all,
            &PtyOptions {
                env,
                ..PtyOptions::default()
            },
        );
        assert_eq!(run.code, Some(0), "{extra:?}: {run:?}");
        assert!(
            run.raw_stderr.contains("\r\u{1b}[Krunning fetch 0/3 ("),
            "{:?}",
            run.raw_stderr
        );
        assert!(run.raw_stderr.contains("\r\u{1b}[Krunning unpack 2/3 ("));
        assert!(run.stdout.starts_with("ID "), "{}", run.stdout);
        assert!(!run.stdout.contains('\r') && !run.stdout.contains("waiting"));
    }
    let json = run_in_pty(
        root.path(),
        &[
            args[0], args[1], args[2], args[3], "--json", "--color", "always",
        ],
        &PtyOptions::default(),
    );
    assert_eq!(json.code, Some(0));
    assert!(json.raw_stderr.contains("\r\u{1b}[K"));
    envelope_of(&json.stdout);
    ev.pass();
}

#[test]
fn without_a_terminal_or_with_term_dumb_progress_is_one_plain_line_per_change() {
    let root = TestRoot::new().unwrap();
    let _daemon = progressing(&root);
    let id = id();

    let piped = run(root.path(), &["operation", "inspect", &id, "--wait"]);
    let dumb = run_in_pty(
        root.path(),
        &["operation", "inspect", &id, "--wait"],
        &PtyOptions {
            env: vec![("TERM", "dumb")],
            ..PtyOptions::default()
        },
    );
    let quiet = run(root.path(), &["operation", "inspect", &id, "--wait", "-q"]);

    for stderr in [&piped.stderr, &dumb.stderr] {
        assert!(
            !stderr.contains('\r') && !stderr.contains('\u{1b}'),
            "{stderr:?}"
        );
        let lines: Vec<&str> = stderr.lines().collect();
        assert!(lines[0].starts_with("waiting for operation "), "{stderr}");
        let changes: Vec<&str> = lines[1..]
            .iter()
            .map(|line| line.split_once(' ').unwrap().1)
            .collect();
        assert_eq!(
            changes,
            ["running fetch", "running unpack", "succeeded"],
            "{stderr}"
        );
    }
    assert_eq!(quiet.code, Some(0));
    assert_eq!(quiet.stderr, "", "--quiet shows no progress");
}
