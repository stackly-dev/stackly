//! Spec 0033 milestone 2 against a scripted daemon: a failed `start` or
//! `restart` writes the `operation:` line and the next steps in its error
//! block (AC-6), `inspect` and `get` show `Last operation` and `Next
//! steps:` and `lastOperation` in JSON (AC-7), and `--wait` writes the
//! backoff line once per pause (AC-8).
//!
//! The scripted cases make their DTOs here the way spec 0033 describes
//! them. The two `a_real_` tests at the end run the same commands against
//! a real `stacklyd`, which fills `lastFailure.at`, `exitCode`,
//! `suggestedPort`, and the `backoff` stage since milestone 1.

use stackly_protocol::codes::supervisor;
use stackly_protocol::instance::InstanceSummary;
use stackly_protocol::operation::{
    OperationFailureInfo, OperationKind, OperationState, OperationSummary,
};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::scripted::{Ctx, ScriptedDaemon, Step};
use crate::common::{dto, envelope, golden, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";
/// When the last crash of the crash loop was committed.
const FAILED_AT: &str = "2026-11-10T08:01:00Z";
/// A fixture credential a crashing process printed; the daemon redacts it
/// before it leaves, so it must never reach the CLI's output.
const CREDENTIAL: &str = "fixture-password-0033";

/// The static hint of `supervisor.retry_budget_exhausted` (spec 0033,
/// *Hint statis*).
const BUDGET_HINT: &str = "It kept crashing, so Stackly stopped restarting it. Read its logs, fix the cause, then start it again.";
/// The static hint of `supervisor.endpoint_in_use`.
const PORT_HINT: &str = "Stackly never moves a fixed port on its own. Pick another port for this endpoint, or stop the process that holds it.";

fn ts(text: &str) -> stackly_protocol::Timestamp {
    text.parse().expect("a fixed time parses")
}

/// `orders-db` after a crash loop: `failed`, `crash_loop` with its time and
/// exit code, six crashes in a five minute window, and a redacted excerpt.
fn crash_looped() -> InstanceSummary {
    data::edited(|value| {
        value["lifecycle"] = "failed".into();
        value["process"] = serde_json::Value::Null;
        value["readiness"]["state"] = "unknown".into();
        value["readiness"]["reason"] = "not_running".into();
        value["health"]["state"] = "unhealthy".into();
        value["health"]["reason"] = "crash_loop".into();
        value["restart"] = serde_json::json!({
            "attempts": 6,
            "budget": 5,
            "windowSeconds": 300,
            "nextAttemptAt": null,
            "exhausted": true
        });
        value["lastFailure"] = serde_json::json!({
            "code": "crash_loop",
            "at": "2026-11-10T08:01:00.000Z",
            "exitCode": 3,
            "signal": null,
            "excerpt": ["FATAL: password authentication failed for <secret:superuserPassword>"]
        });
    })
}

/// `orders-db` `stopped` after a start that a held port refused before the
/// journal: no `lastFailure`.
fn port_refused() -> InstanceSummary {
    data::edited(|value| {
        value["lifecycle"] = "stopped".into();
        value["desired"] = "stopped".into();
        value["process"] = serde_json::Value::Null;
        value["readiness"]["state"] = "unknown".into();
        value["readiness"]["reason"] = "not_running".into();
        value["health"]["state"] = "unknown".into();
        value["health"]["reason"] = "not_running".into();
        for endpoint in value["endpoints"].as_array_mut().into_iter().flatten() {
            endpoint["bound"] = false.into();
        }
    })
}

/// `orders-db` waiting to respawn: crash `attempts` of 5, next at `next`.
fn recovering(attempts: u32, next: Option<&str>) -> InstanceSummary {
    data::edited(|value| {
        value["lifecycle"] = "recovering".into();
        value["process"] = serde_json::Value::Null;
        value["restart"]["attempts"] = attempts.into();
        value["restart"]["nextAttemptAt"] = next.map_or(serde_json::Value::Null, Into::into);
    })
}

fn budget_error() -> ErrorBody {
    ErrorBody::new(
        &supervisor::RETRY_BUDGET_EXHAUSTED,
        "the process crashed 6 times; the retry budget is spent",
    )
    .with_detail("attempts", "6")
    .with_detail("lastExitCode", "3")
    .with_hint(BUDGET_HINT)
    .with_operation_id(dto::OPERATION)
}

fn port_error(suggested: Option<&str>) -> ErrorBody {
    let body = ErrorBody::new(
        &supervisor::ENDPOINT_IN_USE,
        "endpoint postgres is in use at 127.0.0.1:5432",
    )
    .with_detail("endpoint", "postgres")
    .with_detail("address", "127.0.0.1")
    .with_detail("port", "5432");
    let body = match suggested {
        Some(port) => body.with_detail("suggestedPort", port),
        None => body,
    };
    body.with_hint(PORT_HINT).with_operation_id(dto::OPERATION)
}

/// The operation `kind` in `state` at `revision`, last changed at `at`.
fn op(kind: OperationKind, state: OperationState, revision: u64, at: &str) -> OperationSummary {
    let mut summary = data::operation(kind, state, revision);
    summary.updated_at = ts(at);
    summary
}

/// A `start_instance` that ended `failed` with `error`.
fn failed_start(error: ErrorBody) -> OperationSummary {
    let mut summary = op(
        OperationKind::StartInstance,
        OperationState::Failed,
        3,
        FAILED_AT,
    );
    summary.failure = Some(OperationFailureInfo {
        reason: "step_failed".to_owned(),
        retryable: false,
        error,
    });
    summary
}

fn start_reply() -> Reply {
    Reply::InstanceStart(Box::new(data::accepted(
        op(
            OperationKind::StartInstance,
            OperationState::Queued,
            1,
            FAILED_AT,
        ),
        false,
    )))
}

/// stderr from the error block on, without the wait and progress lines
/// before it, whose durations differ from run to run.
fn error_block(stderr: &str) -> &str {
    let at = stderr.find("error:").expect("stderr holds an error block");
    &stderr[at..]
}

/// A handler whose `instance.start` answers a queued operation that plays
/// `states` and whose `instance.get` answers `instance`.
fn starting(
    states: Vec<OperationSummary>,
    instance: InstanceSummary,
) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    data::scripted(start_reply(), states, Some(instance))
}

fn gets(daemon: &ScriptedDaemon) -> usize {
    daemon
        .methods()
        .iter()
        .filter(|method| *method == "instance.get")
        .count()
}

/// Spec 0033 AC-6, AC-15.
#[test]
fn a_crash_loop_under_wait_names_the_operation_and_the_next_steps_in_the_error_block() {
    let ev = evidence!(
        covers = ["AC-068", "NFR-011"],
        kind = Kind::Fixture,
        expected = "instance start --wait whose operation fails supervisor.retry_budget_exhausted exits 7 and writes the message, code, daemon hint, operation <uuid>, then the logs and start commands with the instance name; --json keeps the error as the daemon sent it",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        starting(vec![failed_start(budget_error())], crash_looped()),
    );

    let text = run(root.path(), &["instance", "start", ID, "--wait"]);
    let json = run(root.path(), &["instance", "start", ID, "--wait", "--json"]);

    assert_eq!(text.code, Some(7), "{text:?}");
    golden::check("instance_start_crash_loop.txt", error_block(&text.stderr));
    assert_eq!(json.code, Some(7), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "supervisor.retry_budget_exhausted");
    assert_eq!(value["error"]["operationId"], dto::OPERATION.to_string());
    assert_eq!(value["error"]["hint"], BUDGET_HINT);
    assert!(!json.stdout.contains("read its logs"), "{}", json.stdout);
    assert_eq!(gets(&daemon), 2, "one instance.get per run, after the wait");
    for out in [&text.stdout, &text.stderr, &json.stdout, &json.stderr] {
        assert!(!out.contains(CREDENTIAL), "{out}");
    }
    ev.pass();
}

/// Spec 0033 AC-6.
#[test]
fn a_port_failure_under_wait_suggests_the_free_port_or_asks_for_one() {
    let ev = evidence!(
        covers = ["VAL-020"],
        kind = Kind::Fixture,
        expected = "instance start --wait refused supervisor.endpoint_in_use writes use a free port: stackly instance endpoint set <name> --port postgres=5433 with suggestedPort, pick another port: ... --port postgres=<port> without it, then the logs and start commands, and exits 3",
    );
    for (suggested, name) in [
        (Some("5433"), "instance_start_port_in_use.txt"),
        (None, "instance_start_port_in_use_no_suggestion.txt"),
    ] {
        let root = TestRoot::new().expect("a test root is made");
        let _daemon = ScriptedDaemon::serve(
            &root,
            starting(vec![failed_start(port_error(suggested))], port_refused()),
        );

        let text = run(root.path(), &["instance", "start", ID, "--wait"]);

        assert_eq!(text.code, Some(3), "{text:?}");
        golden::check(name, error_block(&text.stderr));
    }
    ev.pass();
}

/// Spec 0033 AC-6.
#[test]
fn a_refused_restart_reads_the_name_and_writes_the_next_steps_without_an_operation_line() {
    let root = TestRoot::new().expect("a test root is made");
    let instance = crash_looped();
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceRestart(_) => vec![Step::Fail(
            ErrorBody::new(
                &supervisor::INSTANCE_NOT_RUNNING,
                "the instance is not running",
            )
            .with_detail("lifecycle", "failed"),
        )],
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["instance", "restart", ID]);
    let json = run(root.path(), &["instance", "restart", ID, "--json"]);

    assert_eq!(text.code, Some(3), "{text:?}");
    assert_eq!(
        text.stderr,
        "error: the instance is not running\n  code: supervisor.instance_not_running\n\
         \x20 hint: read its logs: stackly instance logs orders-db\n\
         \x20 hint: check this Mac: stackly doctor --instance orders-db\n\
         \x20 hint: start it again: stackly instance start orders-db\n"
    );
    assert_eq!(json.code, Some(3), "{json:?}");
    assert_eq!(envelope(&json)["result"], serde_json::Value::Null);
    assert_eq!(gets(&daemon), 1, "only the text run reads the name");
}

/// Spec 0033 AC-6: a wait that stopped early, `--quiet`, and an instance
/// that is gone get no next steps.
#[test]
fn a_wait_that_ran_out_quiet_and_a_missing_instance_get_no_next_steps() {
    let root = TestRoot::new().expect("a test root is made");
    let running = op(
        OperationKind::StartInstance,
        OperationState::Running,
        2,
        FAILED_AT,
    );
    let _daemon = ScriptedDaemon::serve(&root, starting(vec![running], crash_looped()));

    let late = run(
        root.path(),
        &["instance", "start", ID, "--wait", "--timeout", "1s"],
    );

    assert_eq!(late.code, Some(8), "{late:?}");
    assert!(
        late.stderr
            .contains(&format!("  operation: {}\n", dto::OPERATION)),
        "{}",
        late.stderr
    );
    assert!(!late.stderr.contains("read its logs"), "{}", late.stderr);
    drop(_daemon);

    let _daemon = ScriptedDaemon::serve(
        &root,
        starting(vec![failed_start(budget_error())], crash_looped()),
    );
    let quiet = run(root.path(), &["instance", "start", ID, "--wait", "--quiet"]);
    assert_eq!(quiet.code, Some(7), "{quiet:?}");
    assert!(!quiet.stderr.contains("read its logs"), "{}", quiet.stderr);
    assert!(
        quiet
            .stderr
            .contains(&format!("  operation: {}\n", dto::OPERATION)),
        "{}",
        quiet.stderr
    );
    drop(_daemon);

    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceStart(_) => vec![Step::Fail(crate::common::scripted::error(
            "resource.not_found",
            "not_found",
            "no instance has this id",
        ))],
        _ => Vec::new(),
    });
    let gone = run(root.path(), &["instance", "start", ID]);
    assert_eq!(gone.code, Some(10), "{gone:?}");
    assert!(!gone.stderr.contains("read its logs"), "{}", gone.stderr);
}

/// A daemon that answers `instance.get`, `get_detail`, and
/// `get_data_usage` for `instance` and `operation.list` with `operations`,
/// or with no answer when `operations` is `None`.
fn inspecting(
    instance: InstanceSummary,
    operations: Option<Vec<OperationSummary>>,
) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        Call::OperationList(_) => match &operations {
            Some(items) => vec![data::operations(items.clone())],
            None => vec![Step::Fail(crate::common::scripted::error(
                "daemon.recovery_mode",
                "unavailable",
                "the daemon runs in recovery mode",
            ))],
        },
        other => data::detail_steps(other, &instance).unwrap_or_default(),
    });
    (root, daemon)
}

/// Spec 0033 AC-7, AC-15.
#[test]
fn inspect_of_a_crash_loop_shows_the_last_operation_and_the_next_steps() {
    let ev = evidence!(
        covers = ["AC-068", "NFR-011"],
        kind = Kind::Fixture,
        expected = "instance inspect of a failed crash_loop instance reads operation.list { resource, limit 10 }, skips a newer rename, writes Last operation start_instance failed, supervisor.retry_budget_exhausted, <uuid>, the failure time and exit code, Next steps after Failure output, and lastOperation in JSON; get writes the same",
    );
    let rename = op(
        OperationKind::RenameInstance,
        OperationState::Succeeded,
        2,
        "2026-11-10T08:05:00Z",
    );
    let mut rename = rename;
    rename.id = Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d0f);
    let start = failed_start(budget_error());
    let (root, daemon) = inspecting(crash_looped(), Some(vec![rename, start]));

    let text = run(root.path(), &["instance", "inspect", ID]);
    let json = run(root.path(), &["instance", "inspect", ID, "--json"]);
    let get = run(root.path(), &["instance", "get", ID]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("instance_inspect_crash_loop.txt", &text.stdout);
    golden::check("instance_inspect_crash_loop.json", &json.stdout);
    assert_eq!(get.stdout, text.stdout);
    let value = envelope(&json);
    assert_eq!(
        value["result"]["lastOperation"]["id"],
        dto::OPERATION.to_string()
    );
    let listed = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "operation.list")
        .expect("operation.list was sent");
    assert_eq!(
        listed.raw["call"]["content"],
        serde_json::json!({
            "activeOnly": false,
            "resource": { "kind": "instance", "id": ID },
            "limit": 10,
            "before": null
        })
    );
    for out in [&text.stdout, &json.stdout] {
        assert!(!out.contains(CREDENTIAL), "{out}");
    }
    ev.pass();
}

/// Spec 0033 AC-7.
#[test]
fn a_stopped_instance_shows_the_block_only_after_a_failed_start() {
    let refused = failed_start(port_error(Some("5433")));
    let (root, _daemon) = inspecting(port_refused(), Some(vec![refused.clone()]));
    let text = run(root.path(), &["instance", "inspect", ID]);
    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("instance_inspect_port_refused.txt", &text.stdout);
    drop(_daemon);

    let stop = op(
        OperationKind::StopInstance,
        OperationState::Succeeded,
        4,
        "2026-11-10T08:09:00Z",
    );
    for operations in [Some(vec![stop, refused]), Some(Vec::new()), None] {
        let (root, _daemon) = inspecting(port_refused(), operations.clone());
        let text = run(root.path(), &["instance", "inspect", ID]);
        assert_eq!(text.code, Some(0), "{operations:?}: {text:?}");
        assert!(!text.stdout.contains("Last operation"), "{}", text.stdout);
        assert!(!text.stdout.contains("Next steps:"), "{}", text.stdout);
    }
}

/// Spec 0033 AC-7: a read that fails is `unavailable` and never fails the
/// command; no related operation is `-`.
#[test]
fn a_failed_instance_without_its_operations_shows_unavailable_or_a_dash() {
    for (operations, expected) in [(None, "unavailable"), (Some(Vec::new()), "-")] {
        let (root, _daemon) = inspecting(crash_looped(), operations);
        let text = run(root.path(), &["instance", "inspect", ID]);
        let json = run(root.path(), &["instance", "inspect", ID, "--json"]);

        assert_eq!(text.code, Some(0), "{text:?}");
        let line = text
            .stdout
            .lines()
            .find(|line| line.starts_with("Last operation"))
            .unwrap_or_else(|| panic!("a Last operation row in {}", text.stdout));
        assert_eq!(
            line.split_whitespace().skip(2).collect::<Vec<_>>(),
            [expected]
        );
        assert!(text.stdout.contains("\nNext steps:\n  read its logs: stackly instance logs orders-db\n  check this Mac: stackly doctor --instance orders-db\n  start it again: stackly instance start orders-db\n"), "{}", text.stdout);
        assert_eq!(json.code, Some(0), "{json:?}");
        assert_eq!(
            envelope(&json)["result"]["lastOperation"],
            serde_json::Value::Null
        );
    }
}

/// Spec 0033 AC-7: a running instance shows neither row nor block, and
/// its related operation is still in JSON.
#[test]
fn a_running_instance_shows_no_failure_block_but_keeps_last_operation_in_json() {
    let started = op(
        OperationKind::StartInstance,
        OperationState::Succeeded,
        3,
        FAILED_AT,
    );
    let (root, _daemon) = inspecting(data::instance(""), Some(vec![started]));

    let text = run(root.path(), &["instance", "inspect", ID]);
    let json = run(root.path(), &["instance", "inspect", ID, "--json"]);

    assert!(!text.stdout.contains("Last operation"), "{}", text.stdout);
    assert!(!text.stdout.contains("Next steps:"), "{}", text.stdout);
    assert_eq!(
        envelope(&json)["result"]["lastOperation"]["state"],
        "succeeded"
    );
}

/// A start that waits through two pauses: `lock`, `backoff` twice, `lock`,
/// `backoff`, then `succeeded`.
/// A running start in `stage` at `revision`.
fn staged(stage: &str, revision: u64) -> OperationSummary {
    let mut summary = op(
        OperationKind::StartInstance,
        OperationState::Running,
        revision,
        FAILED_AT,
    );
    summary.stage = Some(stage.to_owned());
    summary
}

/// A start with two pauses, the first read twice. A pause commits only its
/// stage, so both reads of it have the same revision.
fn pausing() -> Vec<OperationSummary> {
    vec![
        staged("lock", 2),
        staged("backoff", 3),
        staged("backoff", 3),
        staged("lock", 5),
        staged("backoff", 6),
        op(
            OperationKind::StartInstance,
            OperationState::Succeeded,
            7,
            FAILED_AT,
        ),
    ]
}

/// Spec 0033 AC-8.
#[test]
fn wait_writes_the_backoff_line_once_per_pause_from_one_instance_get() {
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "instance start --wait writes crashed, restart <attempts> of <budget> at <local time> once each time the operation enters the backoff stage, from one instance.get per pause, and crashed, restarting when nextAttemptAt is null",
    );
    let root = TestRoot::new().expect("a test root is made");
    let play = data::scripted(start_reply(), pausing(), Some(data::instance("")));
    let _daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceGet(_) if ctx.nth == 0 => vec![Step::Reply(Reply::InstanceGet(Box::new(
            recovering(2, Some("2026-11-10T08:00:10Z")),
        )))],
        Call::InstanceGet(_) if ctx.nth == 1 => {
            vec![Step::Reply(Reply::InstanceGet(Box::new(recovering(
                3, None,
            ))))]
        }
        other => play(other, ctx),
    });

    let text = run(root.path(), &["instance", "start", ID, "--wait"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    let progress: Vec<&str> = text
        .stderr
        .lines()
        .skip(1)
        .map(|line| line.split_once(' ').map_or(line, |(_, rest)| rest))
        .collect();
    assert_eq!(
        progress,
        [
            "running lock",
            "crashed, restart 2 of 5 at 2026-11-10 08:00:10 +00:00",
            "running lock",
            "crashed, restarting",
            "succeeded",
        ],
        "{}",
        text.stderr
    );
    assert_eq!(text.stdout, "orders-db running, readiness ready\n");
    ev.pass();
}

/// Spec 0033 AC-8: the stages between two pauses can pass between two
/// reads, so the wait sees `backoff` and then `backoff` again; the newer
/// revision is the next pause, which gets its own read and line.
#[test]
fn a_pause_right_after_a_pause_writes_its_own_backoff_line() {
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "instance start --wait that reads the backoff stage at revision 3, again at revision 3, and then at revision 6 without the lock between them writes crashed, restart 2 of 5 and then crashed, restart 3 of 5, from one instance.get per pause",
    );
    let root = TestRoot::new().expect("a test root is made");
    let states = vec![
        staged("lock", 2),
        staged("backoff", 3),
        staged("backoff", 3),
        staged("backoff", 6),
        op(
            OperationKind::StartInstance,
            OperationState::Succeeded,
            7,
            FAILED_AT,
        ),
    ];
    let play = data::scripted(start_reply(), states, Some(data::instance("")));
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceGet(_) if ctx.nth == 0 => vec![Step::Reply(Reply::InstanceGet(Box::new(
            recovering(2, Some("2026-11-10T08:00:10Z")),
        )))],
        Call::InstanceGet(_) if ctx.nth == 1 => vec![Step::Reply(Reply::InstanceGet(Box::new(
            recovering(3, Some("2026-11-10T08:00:14Z")),
        )))],
        other => play(other, ctx),
    });

    let text = run(root.path(), &["instance", "start", ID, "--wait"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    let progress: Vec<&str> = text
        .stderr
        .lines()
        .skip(1)
        .map(|line| line.split_once(' ').map_or(line, |(_, rest)| rest))
        .collect();
    assert_eq!(
        progress,
        [
            "running lock",
            "crashed, restart 2 of 5 at 2026-11-10 08:00:10 +00:00",
            "crashed, restart 3 of 5 at 2026-11-10 08:00:14 +00:00",
            "succeeded",
        ],
        "{}",
        text.stderr
    );
    assert_eq!(
        gets(&daemon),
        3,
        "one instance.get per pause, and one after the wait"
    );
    ev.pass();
}

/// Spec 0033 AC-8: the line stays when the read fails.
#[test]
fn a_backoff_read_that_fails_writes_crashed_restarting() {
    let root = TestRoot::new().expect("a test root is made");
    let play = data::scripted(start_reply(), pausing(), Some(data::instance("")));
    let _daemon =
        ScriptedDaemon::serve(&root, move |call, ctx| match call {
            Call::InstanceGet(_) if ctx.nth < 2 => vec![Step::Fail(
                crate::common::scripted::error("test.failed", "internal", "the read failed"),
            )],
            other => play(other, ctx),
        });

    let text = run(root.path(), &["instance", "start", ID, "--wait"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stderr.matches("crashed, restarting\n").count(),
        2,
        "{}",
        text.stderr
    );
}

/// Spec 0033 AC-1, AC-3, AC-7 against a real `stacklyd`, on the other
/// path of a crash loop: the fake becomes ready and crashes after, so the
/// start you waited for succeeds, the restart policy respawns it as system
/// starts, and the sixth crash ends it `failed` without a failed operation.
/// `inspect` then names the last respawn, not an error, and still writes
/// the next steps.
#[test]
fn a_real_crash_after_ready_fails_the_instance_after_the_start_succeeded_and_inspect_names_the_last_respawn()
 {
    // covers: spec 0033 AC-1, AC-3, AC-7 (crash after ready, real stacklyd)
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with the restart constants of spec 0010, instance start --wait of a fake that becomes ready and crashes 700 ms after each spawn exits 0; the restart policy respawns it as background start_instance operations until the sixth crash leaves it failed with crash_loop, exit code 3, and an exhausted budget; inspect writes Last failure crash_loop with its exit code, Last operation as the last respawn that succeeded (not the start you waited for, and no error code), and Next steps, and its JSON names that respawn as lastOperation",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = crate::stacklyd::seed(&root);
    let _reap = stackly_testkit::ReapGuard::new(root.path(), &crate::common::paths(&root).runtime);
    let _daemon = crate::common::stacklyd::start_daemon(&root);
    create_real(
        &root,
        &runtime,
        "flaky",
        &[
            "--set",
            "crashAfterMs=700",
            "--set",
            "crashExitCode=3",
            "--set",
            "readyDelayMs=0",
        ],
    );

    let started = run(
        root.path(),
        &[
            "instance",
            "start",
            "flaky",
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(started.code, Some(0), "{started:?}");
    let started = envelope(&started);
    assert_eq!(started["result"]["operation"]["state"], "succeeded");
    let waited_for = started["result"]["operation"]["id"]
        .as_str()
        .expect("the start names its operation")
        .to_owned();

    // The real restart constants pause 0.5 to 31 seconds in all, after six
    // runs of 700 ms.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let inspected = loop {
        let json = run(root.path(), &["instance", "inspect", "flaky", "--json"]);
        assert_eq!(json.code, Some(0), "{json:?}");
        let value = envelope(&json);
        if value["result"]["instance"]["lifecycle"] == "failed" {
            break value;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the crash loop did not end within 120 s: {value}"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
    };
    let text = run(root.path(), &["instance", "inspect", "flaky"]);

    let instance = &inspected["result"]["instance"];
    assert_eq!(instance["lastFailure"]["code"], "crash_loop");
    assert_eq!(instance["lastFailure"]["exitCode"], 3);
    assert!(instance["lastFailure"]["at"].is_string(), "{instance}");
    assert_eq!(instance["restart"]["exhausted"], true, "{instance}");
    let last = &inspected["result"]["lastOperation"];
    assert_eq!(last["kind"], "start_instance", "{last}");
    assert_eq!(last["state"], "succeeded", "{last}");
    assert_eq!(last["trigger"]["origin"], "background", "{last}");
    let respawn = last["id"].as_str().expect("the respawn has an id");
    assert_ne!(
        respawn, waited_for,
        "the last respawn, not the start you waited for"
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    let failure = line_after(&text.stdout, "Last failure").expect("the Last failure line");
    assert!(
        failure.trim_start().starts_with("crash_loop at ") && failure.ends_with(", exit code 3"),
        "{failure}"
    );
    let line = line_after(&text.stdout, "Last operation").expect("the Last operation line");
    assert_eq!(
        line.trim_start(),
        format!("start_instance succeeded, {respawn}")
    );
    assert!(
        !text.stdout.contains("supervisor.retry_budget_exhausted"),
        "no operation failed, so no error code: {}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("Next steps:\n  read its logs: stackly instance logs flaky\n"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("  start it again: stackly instance start flaky\n"),
        "{}",
        text.stdout
    );
    ev.pass();
}

/// The output of one real CLI run: its exit code, stdout, and stderr.
fn all_output(run: &crate::common::Run) -> String {
    format!("{}\n{}", run.stdout, run.stderr)
}

/// Creates a fake instance named `name` through the real CLI and waits for
/// it; `extra` are more `instance create` arguments.
fn create_real(root: &TestRoot, runtime: &str, name: &str, extra: &[&str]) {
    let mut args = vec![
        "instance",
        "create",
        name,
        "--runtime",
        runtime,
        "--provider",
        "testing/fake",
        "--revision",
        "0.4.0",
        "--wait",
        "--timeout",
        "60s",
        "--json",
    ];
    args.extend(extra);
    let created = run(root.path(), &args);
    assert_eq!(created.code, Some(0), "{created:?}");
}

/// The value after `prefix` on the line of `text` that starts with it.
fn line_after<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix(prefix))
}

/// Spec 0033 AC-6, AC-7, AC-8, AC-15 against a real `stacklyd`.
#[test]
fn a_real_crash_loop_ends_the_start_with_its_operation_and_inspect_shows_the_same() {
    let ev = evidence!(
        covers = ["AC-068", "NFR-011"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, instance start --wait of a fake that prints its fixture credential and crashes before ready writes the backoff line of each of the five pauses in order, then exits 7 with code supervisor.retry_budget_exhausted, the static hint, operation <uuid>, and the logs and start commands; inspect writes Last failure crash_loop with its exit code, Last operation with the same uuid, and Next steps, its JSON carries lastOperation and the failure time, and no output holds the credential",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = crate::stacklyd::seed(&root);
    let _reap = stackly_testkit::ReapGuard::new(root.path(), &crate::common::paths(&root).runtime);
    let _daemon = crate::common::stacklyd::start_daemon(&root);
    let credential = stackly_testkit::evidence::FixtureCredential::new();
    let token = root.path().join("token.txt");
    std::fs::write(&token, credential.value()).expect("the token file is written");
    let secret = format!("authToken=file:{}", token.display());
    create_real(
        &root,
        &runtime,
        "leaky",
        &[
            "--set",
            "crashAfterMs=50",
            "--set",
            "crashExitCode=3",
            "--set",
            "readyDelayMs=60000",
            "--set",
            "leakToken=true",
            "--secret",
            &secret,
        ],
    );

    // The real restart constants pause 0.5 to 31 seconds in all.
    let started = run(
        root.path(),
        &["instance", "start", "leaky", "--wait", "--timeout", "120s"],
    );
    let text = run(root.path(), &["instance", "inspect", "leaky"]);
    let json = run(root.path(), &["instance", "inspect", "leaky", "--json"]);

    assert_eq!(started.code, Some(7), "{started:?}");
    let block = error_block(&started.stderr);
    assert!(
        block.contains("  code: supervisor.retry_budget_exhausted\n"),
        "{block}"
    );
    assert!(
        block.contains(&format!("  hint: {BUDGET_HINT}\n")),
        "{block}"
    );
    let operation = line_after(block, "operation: ").expect("the operation line");
    Uuid::parse_str(operation).expect("the operation line holds a uuid");
    assert!(
        block.contains("  hint: read its logs: stackly instance logs leaky\n"),
        "{block}"
    );
    assert!(
        block.ends_with("  hint: start it again: stackly instance start leaky\n"),
        "{block}"
    );
    // AC-8: one line per pause, in order, even when a respawn lives too
    // short for the wait to see its stages.
    let pauses: Vec<&str> = started
        .stderr
        .lines()
        .filter_map(|line| line.split_once("crashed, restart ").map(|(_, rest)| rest))
        .map(|rest| rest.split_once(" at ").map_or(rest, |(count, _)| count))
        .collect();
    assert_eq!(
        pauses,
        ["1 of 5", "2 of 5", "3 of 5", "4 of 5", "5 of 5"],
        "{}",
        started.stderr
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    let failure = line_after(&text.stdout, "Last failure").expect("the Last failure line");
    assert!(
        failure.trim_start().starts_with("crash_loop at ") && failure.ends_with(", exit code 3"),
        "{failure}"
    );
    let last = line_after(&text.stdout, "Last operation").expect("the Last operation line");
    assert_eq!(
        last.trim_start(),
        format!("start_instance failed, supervisor.retry_budget_exhausted, {operation}")
    );
    assert!(
        text.stdout
            .contains("Next steps:\n  read its logs: stackly instance logs leaky\n"),
        "{}",
        text.stdout
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["result"]["lastOperation"]["id"], operation);
    let failure = &value["result"]["instance"]["lastFailure"];
    assert_eq!(failure["code"], "crash_loop");
    assert_eq!(failure["exitCode"], 3);
    assert!(failure["at"].is_string(), "{failure}");

    for (out, what) in [
        (all_output(&started), "instance start"),
        (all_output(&text), "instance inspect"),
        (all_output(&json), "instance inspect --json"),
    ] {
        credential.assert_absent(&out, what);
    }
    ev.pass();
}

/// Spec 0033 AC-4, AC-6, AC-7, AC-14 against a real `stacklyd`.
#[test]
fn a_real_port_refusal_suggests_a_free_port_that_a_dry_run_accepts_without_a_change() {
    let ev = evidence!(
        covers = ["AC-010", "AC-055", "NFR-011"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, instance start --wait of a fake whose fixed port the test holds exits 3 with supervisor.endpoint_in_use, the static hint, its operation, and a use a free port command whose port lies after the held one; inspect writes the same operation and command, the suggested command with --dry-run passes, and the stored port stays the held one",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = crate::stacklyd::seed(&root);
    let _reap = stackly_testkit::ReapGuard::new(root.path(), &crate::common::paths(&root).runtime);
    let _daemon = crate::common::stacklyd::start_daemon(&root);
    let port = stackly_testkit::allocate_port().expect("a free port");
    let request = format!("http={port}");
    create_real(&root, &runtime, "held", &["--port", &request]);
    // Create checks the port too, so the test takes it only afterwards.
    let held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .expect("the test holds the port");

    let started = run(
        root.path(),
        &["instance", "start", "held", "--wait", "--timeout", "60s"],
    );
    let text = run(root.path(), &["instance", "inspect", "held"]);

    assert_eq!(started.code, Some(3), "{started:?}");
    let block = error_block(&started.stderr);
    assert!(
        block.contains("  code: supervisor.endpoint_in_use\n"),
        "{block}"
    );
    assert!(block.contains(&format!("  hint: {PORT_HINT}\n")), "{block}");
    let operation = line_after(block, "operation: ").expect("the operation line");
    let command = line_after(block, "hint: use a free port: ").expect("the port step");
    let suggested: u16 = command
        .strip_prefix("stackly instance endpoint set held --port http=")
        .unwrap_or_else(|| panic!("the port step names the endpoint: {command}"))
        .parse()
        .expect("a suggested port");
    assert!(suggested > port, "{suggested} after {port}");

    assert_eq!(text.code, Some(0), "{text:?}");
    let last = line_after(&text.stdout, "Last operation").expect("the Last operation line");
    assert_eq!(
        last.trim_start(),
        format!("start_instance failed, supervisor.endpoint_in_use, {operation}")
    );
    assert!(
        text.stdout
            .contains(&format!("Next steps:\n  use a free port: {command}\n")),
        "{}",
        text.stdout
    );

    let suggested_arg = format!("http={suggested}");
    let dry = run(
        root.path(),
        &[
            "instance",
            "endpoint",
            "set",
            "held",
            "--port",
            &suggested_arg,
            "--dry-run",
        ],
    );
    assert_eq!(
        dry.code,
        Some(0),
        "the suggested port passes the verdict: {dry:?}"
    );
    let after = envelope(&run(
        root.path(),
        &["instance", "inspect", "held", "--json"],
    ));
    assert_eq!(after["result"]["instance"]["lifecycle"], "stopped");
    assert_eq!(
        after["result"]["instance"]["endpoints"][0]["port"], port,
        "the port stays"
    );
    drop(held);
    ev.pass();
}
