//! Spec 0046 AC-17: `env up --wait` that runs out of `--timeout` exits 8
//! with `cli.wait_timeout`, and Ctrl+C during the wait exits 130 with
//! `cli.interrupted`; neither cancels the Up, which keeps running in the
//! daemon.

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Duration, Instant};

use stackly_protocol::env::{EnvPlan, EnvUpReply};
use stackly_protocol::events::{EventCursor, EventKind, Subscribed};
use stackly_protocol::operation::{
    OperationAccepted, OperationKind, OperationState, OperationSummary,
};
use stackly_protocol::{Call, Reply, ResourceKind, ResourceRef, Sequence};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::scripted::{Ctx, ScriptedDaemon, Step, server_instance};
use crate::common::{detail, dto, envelope, envelope_of, run, stackly};
use crate::data;

/// The `environment_up` operation of the fixtures.
const UP: Uuid = Uuid::from_u128(0x0192_f3a4_0000_7000_8000_0000_0000_00f1);

fn plan() -> EnvPlan {
    data::applicable()
}

/// The Up, still running.
fn running() -> OperationSummary {
    let mut summary = dto::summary(UP, OperationState::Running, 2);
    summary.kind = OperationKind::EnvironmentUp;
    summary.resource = Some(ResourceRef {
        kind: ResourceKind::Environment,
        id: data::bringing_up().summary.id.to_string(),
    });
    summary
}

/// A daemon whose Up never ends.
fn serve(call: &Call, ctx: Ctx) -> Vec<Step> {
    match call {
        Call::EnvList => vec![Step::Reply(Reply::EnvList(data::list()))],
        Call::EnvPlan(_) => vec![Step::Reply(Reply::EnvPlan(Box::new(plan())))],
        Call::EnvUp(_) => vec![Step::Reply(Reply::EnvUp(Box::new(EnvUpReply {
            environment_id: data::bringing_up().summary.id,
            name: "graph".to_owned(),
            operation: OperationAccepted {
                operation: running(),
                replayed: false,
            },
        })))],
        Call::EventsSubscribe(_) => vec![Step::Reply(Reply::EventsSubscribe(Subscribed {
            subscription_id: 1,
            head: EventCursor {
                server_instance_id: server_instance(ctx.connection),
                sequence: Sequence::ZERO,
            },
        }))],
        Call::OperationGet(_) => vec![
            Step::Reply(Reply::OperationGet(Box::new(dto::detail(running())))),
            Step::Sleep(Duration::from_millis(200)),
            Step::Event(EventKind::OperationStateChanged(Box::new(running()))),
        ],
        Call::EnvGet(_) => vec![Step::Reply(Reply::EnvGet(Box::new(data::bringing_up())))],
        _ => Vec::new(),
    }
}

fn daemon() -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, serve);
    (root, daemon)
}

/// No request the daemon read was `operation.cancel`.
fn never_cancelled(daemon: &ScriptedDaemon) {
    let methods = daemon.methods();
    assert!(
        methods.iter().any(|method| method == "env.up"),
        "{methods:?}"
    );
    assert!(
        !methods.iter().any(|method| method == "operation.cancel"),
        "{methods:?}"
    );
}

// covers: AC-17 (spec 0046), exit 8 on `--timeout`
#[test]
fn up_wait_that_runs_out_of_its_timeout_exits_8_and_never_cancels_the_up() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env up --wait --timeout 1s against an Up that keeps running ends in about a second with cli.wait_timeout, exit 8, the Up's operation id and state running, and never sends operation.cancel",
    );
    let (root, daemon) = daemon();

    let text = run(root.path(), &["env", "up", "--wait", "--timeout", "1s"]);
    let json = run(
        root.path(),
        &["env", "up", "--wait", "--timeout", "1s", "--json"],
    );

    assert_eq!(text.code, Some(8), "{text:?}");
    assert!(
        text.stderr.contains("code: cli.wait_timeout"),
        "{}",
        text.stderr
    );
    assert!(text.elapsed >= Duration::from_secs(1), "{:?}", text.elapsed);
    assert!(text.elapsed < Duration::from_secs(5), "{:?}", text.elapsed);
    assert_eq!(json.code, Some(8), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "env up");
    assert_eq!(value["error"]["code"], "cli.wait_timeout");
    assert_eq!(value["error"]["operationId"], UP.to_string());
    assert_eq!(detail(&value["error"], "state").as_deref(), Some("running"));
    assert_eq!(
        detail(&value["error"], "timeoutSeconds").as_deref(),
        Some("1")
    );
    never_cancelled(&daemon);
    ev.pass();
}

/// Starts `stackly <args>`; with `marker`, waits until a stderr line
/// starts with it, else waits a second and a half; then sends SIGINT and
/// returns the exit code, stdout, all of stderr, and how long the exit
/// took after the signal.
fn interrupt(
    root: &TestRoot,
    args: &[&str],
    marker: Option<&str>,
) -> (Option<i32>, String, String, Duration) {
    let mut command = stackly(root.path());
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("stackly starts");
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr is piped"));
    let mut seen = String::new();
    if let Some(marker) = marker {
        loop {
            let mut line = String::new();
            let read = stderr.read_line(&mut line).expect("stderr can be read");
            assert!(read > 0, "stderr ended before {marker}: {seen}");
            seen.push_str(&line);
            if line.starts_with(marker) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    } else {
        std::thread::sleep(Duration::from_millis(1_500));
    }
    let sent = Instant::now();
    let status = std::process::Command::new("/bin/kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stderr, &mut rest).expect("stderr can be read");
    let output = child.wait_with_output().expect("stackly ends");
    seen.push_str(&rest);
    (
        output.status.code(),
        String::from_utf8(output.stdout).expect("stdout is utf-8"),
        seen,
        sent.elapsed(),
    )
}

// covers: AC-17 (spec 0046), Ctrl+C during `--wait`
#[test]
fn ctrl_c_during_up_wait_exits_130_and_leaves_the_up_running() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "SIGINT while env up --wait waits ends it at once with cli.interrupted and exit 130, says the Up's operation is still running, and never sends operation.cancel",
    );
    let (root, daemon) = daemon();

    let (code, _, stderr, after) = interrupt(
        &root,
        &["env", "up", "--wait"],
        Some("waiting for operation "),
    );
    let (json_code, stdout, _, _) = interrupt(&root, &["env", "up", "--wait", "--json"], None);

    assert_eq!(code, Some(130), "{stderr}");
    assert!(after < Duration::from_secs(1), "{after:?}");
    assert!(
        stderr.contains(&format!(
            "error: stopped waiting; operation {UP} is still running\n  code: cli.interrupted\n"
        )),
        "{stderr}"
    );
    assert_eq!(json_code, Some(130), "{stdout}");
    let value = envelope_of(&stdout);
    assert_eq!(value["error"]["code"], "cli.interrupted");
    assert_eq!(value["error"]["operationId"], UP.to_string());
    never_cancelled(&daemon);
    ev.pass();
}
