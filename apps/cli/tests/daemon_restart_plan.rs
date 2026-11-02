//! `stackly daemon restart` and `daemon status` with a restart plan against
//! a scripted daemon (spec 0030, AC-15). The scripted daemon plays the old
//! run until it is asked to restart, closes the connection, and plays a new
//! run afterwards. The tests run with `STACKLY_ROOT` set, so the CLI never
//! asks the OS to relaunch anything.
#![cfg(target_os = "macos")]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;
use stackly_protocol::codes::daemon as daemon_codes;
use stackly_protocol::daemon::{LaunchContextKind, ShutdownAccepted, ShutdownMode};
use stackly_protocol::events::{EventCursor, EventKind, Subscribed};
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::restart::{
    PlanCloseReason, PlanDecision, PlanOutcome, PlanReason, PlanState, RestartPlanInstance,
    RestartPlanView, RestartPreview, RestartPreviewInstance,
};
use stackly_protocol::{Call, Reply, Sequence, Timestamp};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use common::pty::{PtyOptions, run_in_pty};
use common::scripted::{Ctx, Handshake, ScriptedDaemon, Step, error, handshake_answer};
use common::{Run, detail, dto, envelope, run};

const PLAN: Uuid = Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d30);
const DIGEST: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";

fn at(millis: i64) -> Timestamp {
    Timestamp::from_jiff(
        jiff::Timestamp::from_millisecond(1_793_000_000_000 + millis).expect("in range"),
    )
}

fn preview(decisions: &[PlanDecision]) -> RestartPreview {
    RestartPreview {
        digest: DIGEST.to_owned(),
        launch_context: LaunchContextKind::Launchd,
        supervisor_available: true,
        instances: decisions
            .iter()
            .enumerate()
            .map(|(n, decision)| RestartPreviewInstance {
                instance_id: Uuid::from_u128(n as u128 + 1),
                name: format!("pg{n}"),
                provider: "postgresql".to_owned(),
                lifecycle: InstanceLifecycle::Running,
                decision: *decision,
                reason: match decision {
                    PlanDecision::Keep => PlanReason::HandoverProven,
                    _ => PlanReason::HandoverUnproven,
                },
            })
            .collect(),
        blocking_operations: Vec::new(),
    }
}

fn member(
    name: &str,
    decision: PlanDecision,
    reason: PlanReason,
    outcome: Option<PlanOutcome>,
    failure_code: Option<&str>,
) -> RestartPlanInstance {
    RestartPlanInstance {
        instance_id: Uuid::now_v7(),
        name: name.to_owned(),
        decision,
        reason,
        stopped_at: None,
        ready_at: None,
        outcome,
        failure_code: failure_code.map(str::to_owned),
    }
}

fn plan(state: PlanState, instances: Vec<RestartPlanInstance>) -> RestartPlanView {
    let done = matches!(state, PlanState::Finished);
    RestartPlanView {
        id: PLAN,
        state,
        created_at: at(0),
        handed_over_at: matches!(
            state,
            PlanState::HandedOver | PlanState::Restoring | PlanState::Finished
        )
        .then(|| at(2_000)),
        reconciled_at: matches!(state, PlanState::Restoring | PlanState::Finished)
            .then(|| at(3_000)),
        finished_at: done.then(|| at(5_500)),
        close_reason: None,
        instances,
    }
}

fn finished() -> RestartPlanView {
    plan(
        PlanState::Finished,
        vec![
            member(
                "pg0",
                PlanDecision::Keep,
                PlanReason::HandoverProven,
                Some(PlanOutcome::Adopted),
                None,
            ),
            member(
                "pg1",
                PlanDecision::StopRestore,
                PlanReason::HandoverUnproven,
                Some(PlanOutcome::Restored),
                None,
            ),
        ],
    )
}

fn subscribed(connection: usize) -> Step {
    Step::Reply(Reply::EventsSubscribe(Subscribed {
        subscription_id: 1,
        head: EventCursor {
            server_instance_id: common::scripted::server_instance(connection),
            sequence: Sequence::ZERO,
        },
    }))
}

/// A scripted pair of daemon runs. `preview` answers the preview; the
/// restart request ends the old run (`old`) after the events it plays; the
/// new run answers `daemon.get_info` with `new_plan`.
struct Scenario {
    preview: Vec<RestartPreview>,
    old_events: Vec<RestartPlanView>,
    keeps_running: bool,
    shutdown_error: Option<(&'static str, &'static str)>,
    new_plan: Option<RestartPlanView>,
}

impl Scenario {
    fn new(preview: RestartPreview, new_plan: RestartPlanView) -> Self {
        Self {
            preview: vec![preview],
            old_events: vec![
                plan(PlanState::Stopping, Vec::new()),
                plan(PlanState::HandedOver, Vec::new()),
            ],
            keeps_running: false,
            shutdown_error: None,
            new_plan: Some(new_plan),
        }
    }

    fn start(self, root: &TestRoot) -> ScriptedDaemon {
        let restarted = Arc::new(AtomicBool::new(false));
        let for_handshake = Arc::clone(&restarted);
        let scenario = Arc::new(self);
        let handler_scenario = Arc::clone(&scenario);
        ScriptedDaemon::start(
            root,
            move |index| {
                let mut answer = handshake_answer(index, false);
                let run = if for_handshake.load(Ordering::SeqCst) {
                    2
                } else {
                    1
                };
                answer.server_instance_id = common::scripted::server_instance(run);
                answer.head.server_instance_id = answer.server_instance_id;
                Handshake::Accept(Box::new(answer))
            },
            move |call: &Call, ctx: Ctx| match call {
                Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
                Call::DaemonRestartPreview => {
                    let index = ctx.nth.min(handler_scenario.preview.len() - 1);
                    vec![Step::Reply(Reply::DaemonRestartPreview(
                        handler_scenario.preview[index].clone(),
                    ))]
                }
                Call::DaemonShutdown(params) if params.mode == ShutdownMode::Restart => {
                    if let Some((code, category)) = handler_scenario.shutdown_error {
                        return vec![Step::Fail(error(code, category, "scripted"))];
                    }
                    if !handler_scenario.keeps_running {
                        restarted.store(true, Ordering::SeqCst);
                    }
                    let mut steps = vec![Step::Reply(Reply::DaemonShutdown(ShutdownAccepted {
                        server_instance_id: common::scripted::server_instance(1),
                        mode: ShutdownMode::Restart,
                        plan_id: Some(PLAN),
                    }))];
                    for view in &handler_scenario.old_events {
                        steps.push(Step::Sleep(Duration::from_millis(30)));
                        steps.push(Step::Event(EventKind::DaemonRestartPlan(Box::new(
                            view.clone(),
                        ))));
                    }
                    if !handler_scenario.keeps_running {
                        steps.push(Step::Sleep(Duration::from_millis(30)));
                        steps.push(Step::Close);
                    }
                    steps
                }
                Call::DaemonGetInfo => {
                    let mut info = dto::info(true);
                    info.server_instance_id = common::scripted::server_instance(2);
                    info.restart = handler_scenario.new_plan.clone().map(Box::new);
                    vec![Step::Reply(Reply::DaemonGetInfo(info))]
                }
                _ => vec![Step::Fail(error(
                    "test.unexpected",
                    "failed",
                    "unexpected call",
                ))],
            },
        )
    }
}

fn json_run(root: &TestRoot, extra: &[&str]) -> (Run, Value) {
    let mut args = vec!["daemon", "restart", "--json"];
    args.extend_from_slice(extra);
    let result = run(root.path(), &args);
    let value = envelope(&result);
    (result, value)
}

fn shutdown_requests(daemon: &ScriptedDaemon) -> Vec<common::scripted::Logged> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "daemon.shutdown")
        .collect()
}

#[test]
fn a_confirmed_plan_runs_follows_the_new_daemon_and_reports_each_instance_and_the_durations() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "daemon restart --yes --json sends the digest of the previewed plan with an idempotency key, follows the plan through the old and the new daemon, exits 0, and prints the finished plan with the three durations",
    );
    let root = TestRoot::new().unwrap();
    let daemon = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        finished(),
    )
    .start(&root);

    let (result, value) = json_run(&root, &["--yes"]);

    assert_eq!(result.code, Some(0), "{result:?}");
    assert_eq!(value["result"]["plan"]["state"], "finished");
    assert_eq!(
        value["result"]["plan"]["instances"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        value["result"]["durations"],
        serde_json::json!({ "stopping": 2000, "daemonGap": 1000, "restoring": 2500 })
    );
    assert_eq!(
        value["result"]["previousServerInstanceId"],
        common::scripted::server_instance(1).to_string()
    );
    assert_eq!(
        value["result"]["info"]["serverInstanceId"],
        common::scripted::server_instance(2).to_string()
    );
    let requests = shutdown_requests(&daemon);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].raw["call"]["content"]["mode"], "restart");
    assert_eq!(
        requests[0].raw["call"]["content"]["expectedPlanDigest"],
        DIGEST
    );
    assert!(
        requests[0].idempotency_key.is_some(),
        "a restart carries a key"
    );
    ev.pass();
}

#[test]
fn the_text_result_lists_every_instance_and_how_long_each_part_took() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "daemon restart --yes prints the plan to stderr first and then the finished plan with one row per instance and the three durations on stdout",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        finished(),
    )
    .start(&root);

    let result = run(root.path(), &["daemon", "restart", "--yes"]);

    assert_eq!(result.code, Some(0), "{result:?}");
    assert!(
        result
            .stderr
            .contains("keeps 1 instance running and stops 1 for a moment"),
        "{}",
        result.stderr
    );
    for expected in [
        "finished",
        "pg0",
        "adopted",
        "pg1",
        "restored",
        "stopping 2.0s",
    ] {
        assert!(
            result.stdout.contains(expected),
            "{expected}: {}",
            result.stdout
        );
    }
    assert!(
        result.stdout.contains("daemon gap 1.0s"),
        "{}",
        result.stdout
    );
    assert!(
        result.stdout.contains("restoring 2.5s"),
        "{}",
        result.stdout
    );
    ev.pass();
}

#[test]
fn a_plan_that_stops_instances_needs_yes_without_a_terminal_and_sends_nothing() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "daemon restart without a terminal and without --yes, in text and in --json, exits 2 cli.confirmation_required before any shutdown request reaches the daemon",
    );
    let root = TestRoot::new().unwrap();
    let daemon = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        finished(),
    )
    .start(&root);

    let (json, value) = json_run(&root, &[]);
    let text = run(root.path(), &["daemon", "restart"]);

    assert_eq!(json.code, Some(2), "{json:?}");
    assert_eq!(value["error"]["code"], "cli.confirmation_required");
    assert_eq!(detail(&value["error"], "flag").as_deref(), Some("--yes"));
    assert_eq!(text.code, Some(2), "{text:?}");
    assert!(shutdown_requests(&daemon).is_empty());
    ev.pass();
}

#[test]
fn a_plan_that_stops_nothing_runs_without_yes() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "daemon restart with a plan that has only keep instances asks nothing and exits 0 without --yes",
    );
    let root = TestRoot::new().unwrap();
    let keeps = plan(
        PlanState::Finished,
        vec![member(
            "pg0",
            PlanDecision::Keep,
            PlanReason::HandoverProven,
            Some(PlanOutcome::Adopted),
            None,
        )],
    );
    let _daemon = Scenario::new(preview(&[PlanDecision::Keep]), keeps).start(&root);

    let (result, value) = json_run(&root, &[]);

    assert_eq!(result.code, Some(0), "{result:?}");
    assert_eq!(value["result"]["plan"]["state"], "finished");
    ev.pass();
}

#[test]
fn the_prompt_on_a_terminal_declines_with_9_and_continues_with_a_yes() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "on a terminal, daemon restart shows the plan and asks y/N; n exits 9 cli.confirmation_declined with no shutdown request, y restarts and exits 0",
    );
    let root = TestRoot::new().unwrap();
    let daemon = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        finished(),
    )
    .start(&root);
    let options = |input| PtyOptions {
        stdin_tty: true,
        input: Some(input),
        ..PtyOptions::default()
    };

    let declined = run_in_pty(root.path(), &["daemon", "restart"], &options("n\n"));

    assert_eq!(declined.code, Some(9), "{declined:?}");
    assert!(declined.stderr.contains("[y/N]"), "{}", declined.stderr);
    assert!(
        declined
            .stderr
            .contains("pg1 (postgresql): stops, then starts again"),
        "{}",
        declined.stderr
    );
    assert!(shutdown_requests(&daemon).is_empty());

    let accepted = run_in_pty(root.path(), &["daemon", "restart"], &options("y\n"));

    assert_eq!(accepted.code, Some(0), "{accepted:?}");
    assert_eq!(shutdown_requests(&daemon).len(), 1);
    ev.pass();
}

#[test]
fn a_changed_plan_exits_3_without_a_terminal() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "daemon restart --yes --json against a daemon that answers daemon.restart_plan_changed exits 3 with that code and asks the daemon once",
    );
    let root = TestRoot::new().unwrap();
    let mut scenario = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        finished(),
    );
    scenario.shutdown_error = Some((daemon_codes::RESTART_PLAN_CHANGED.code, "conflict"));
    let daemon = scenario.start(&root);

    let (result, value) = json_run(&root, &["--yes"]);

    assert_eq!(result.code, Some(3), "{result:?}");
    assert_eq!(value["error"]["code"], "daemon.restart_plan_changed");
    assert_eq!(shutdown_requests(&daemon).len(), 1);
    ev.pass();
}

#[test]
fn an_aborted_plan_exits_7_and_the_old_daemon_keeps_answering() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "when the daemon publishes an aborted plan and stays up, daemon restart --yes --json exits 7 cli.restart_aborted with the plan and its close reason in the result",
    );
    let root = TestRoot::new().unwrap();
    let mut aborted = plan(PlanState::Aborted, Vec::new());
    aborted.close_reason = Some(PlanCloseReason::StopTimeout);
    let mut scenario = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        finished(),
    );
    scenario.old_events = vec![plan(PlanState::Stopping, Vec::new()), aborted];
    scenario.keeps_running = true;
    let _daemon = scenario.start(&root);

    let (result, value) = json_run(&root, &["--yes"]);

    assert_eq!(result.code, Some(7), "{result:?}");
    assert_eq!(value["error"]["code"], "cli.restart_aborted");
    assert_eq!(
        detail(&value["error"], "closeReason").as_deref(),
        Some("stop_timeout")
    );
    assert_eq!(value["result"]["plan"]["state"], "aborted");
    ev.pass();
}

#[test]
fn an_aborted_plan_lists_who_starts_again_and_who_still_runs_and_has_no_durations() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "an aborted plan prints one list of the instances that stopped and start again and one of those that did not stop and still run, taken from stoppedAt and not from the skipped outcome, and its durations are null in --json",
    );
    let root = TestRoot::new().unwrap();
    let skipped = |name: &str, decision, stopped: bool| {
        let mut entry = member(
            name,
            decision,
            PlanReason::HandoverUnproven,
            Some(PlanOutcome::Skipped),
            None,
        );
        entry.stopped_at = stopped.then(|| at(1_000));
        entry
    };
    let mut aborted = plan(
        PlanState::Aborted,
        vec![
            skipped("pg0", PlanDecision::StopRestore, true),
            skipped("pg1", PlanDecision::StopRestore, false),
        ],
    );
    aborted.close_reason = Some(PlanCloseReason::StopTimeout);
    let mut scenario = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        finished(),
    );
    scenario.old_events = vec![plan(PlanState::Stopping, Vec::new()), aborted];
    scenario.keeps_running = true;
    let _daemon = scenario.start(&root);

    let text = run(root.path(), &["daemon", "restart", "--yes"]);

    assert_eq!(text.code, Some(7), "{text:?}");
    let shown = format!("{}{}", text.stdout, text.stderr);
    assert!(
        shown.contains("pg0") && shown.contains("stopped, starting again"),
        "{shown}"
    );
    assert!(
        shown.contains("pg1") && shown.contains("did not stop, still running"),
        "{shown}"
    );
    assert!(!shown.contains("skipped"), "{shown}");

    let (_, value) = json_run(&root, &["--yes"]);
    assert!(value["result"]["durations"].is_null(), "{value}");
    ev.pass();
}

#[test]
fn an_instance_that_did_not_come_back_exits_7_with_the_whole_plan() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "a finished plan with a failed restore makes daemon restart exit 7 cli.restart_restore_failed with every instance and the durations in --json",
    );
    let root = TestRoot::new().unwrap();
    let failed = plan(
        PlanState::Finished,
        vec![
            member(
                "pg0",
                PlanDecision::Keep,
                PlanReason::HandoverProven,
                Some(PlanOutcome::Adopted),
                None,
            ),
            member(
                "pg1",
                PlanDecision::StopRestore,
                PlanReason::HandoverUnproven,
                Some(PlanOutcome::Failed),
                Some("endpoint_in_use"),
            ),
        ],
    );
    let _daemon = Scenario::new(
        preview(&[PlanDecision::Keep, PlanDecision::StopRestore]),
        failed,
    )
    .start(&root);

    let (result, value) = json_run(&root, &["--yes"]);

    assert_eq!(result.code, Some(7), "{result:?}");
    assert_eq!(value["error"]["code"], "cli.restart_restore_failed");
    let instances = value["result"]["plan"]["instances"]
        .as_array()
        .expect("instances");
    assert_eq!(instances.len(), 2);
    assert_eq!(instances[1]["failureCode"], "endpoint_in_use");
    assert!(value["result"]["durations"]["restoring"].is_number());
    ev.pass();
}

#[test]
fn an_instance_that_was_and_stays_unknown_is_a_warning_and_exit_0() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "an instance that is unknown before and after the restart gives the cli.restart_instance_unknown warning and exit 0",
    );
    let root = TestRoot::new().unwrap();
    let unknown = plan(
        PlanState::Finished,
        vec![member(
            "odd",
            PlanDecision::Keep,
            PlanReason::IdentityUnproven,
            Some(PlanOutcome::Failed),
            Some("identity_unproven"),
        )],
    );
    let _daemon = Scenario::new(preview(&[PlanDecision::Keep]), unknown).start(&root);

    let (result, value) = json_run(&root, &[]);

    assert_eq!(result.code, Some(0), "{result:?}");
    assert_eq!(value["warnings"][0]["code"], "cli.restart_instance_unknown");
    ev.pass();
}

#[test]
fn no_wait_instances_returns_when_the_new_daemon_answers() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "daemon restart --no-wait-instances exits 0 once the new daemon answers, with the plan the new daemon reports, still open, and null durations",
    );
    let root = TestRoot::new().unwrap();
    let open = plan(PlanState::Restoring, Vec::new());
    let _daemon = Scenario::new(preview(&[PlanDecision::Keep]), open).start(&root);

    let (result, value) = json_run(&root, &["--no-wait-instances"]);

    assert_eq!(result.code, Some(0), "{result:?}");
    assert_eq!(value["result"]["plan"]["state"], "restoring");
    assert_eq!(value["result"]["durations"], Value::Null);
    ev.pass();
}

#[test]
fn daemon_status_shows_the_last_restart() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Fixture,
        expected = "daemon status prints a Last restart row and lastRestart in --json from daemon.get_info restart, and null without one",
    );
    let root = TestRoot::new().unwrap();
    let with_plan = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::DaemonGetInfo => {
            let mut info = dto::info(true);
            info.restart = Some(Box::new(finished()));
            vec![Step::Reply(Reply::DaemonGetInfo(info))]
        }
        Call::DaemonGetHealth => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            stackly_protocol::daemon::DaemonHealthStatus::Ready,
        )))],
        _ => vec![Step::Fail(error(
            "test.unexpected",
            "failed",
            "unexpected call",
        ))],
    });

    let json = run(root.path(), &["daemon", "status", "--json"]);
    let text = run(root.path(), &["daemon", "status"]);

    let value = envelope(&json);
    assert_eq!(value["result"]["lastRestart"]["state"], "finished");
    let row = text
        .stdout
        .lines()
        .find(|line| line.starts_with("Last restart"))
        .unwrap_or_else(|| panic!("no Last restart row in {}", text.stdout));
    assert!(
        row.contains("finished") && row.contains("1 adopted, 1 restored"),
        "{row}"
    );
    drop(with_plan);

    let without = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::DaemonGetInfo => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(true)))],
        Call::DaemonGetHealth => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            stackly_protocol::daemon::DaemonHealthStatus::Ready,
        )))],
        _ => vec![Step::Fail(error(
            "test.unexpected",
            "failed",
            "unexpected call",
        ))],
    });
    let json = run(root.path(), &["daemon", "status", "--json"]);
    assert_eq!(envelope(&json)["result"]["lastRestart"], Value::Null);
    let text = run(root.path(), &["daemon", "status"]);
    assert!(!text.stdout.contains("Last restart"), "{}", text.stdout);
    drop(without);
    ev.pass();
}

#[test]
fn daemon_status_shows_the_login_session() {
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::DaemonGetInfo => {
            let mut info = dto::info(true);
            info.session = Some(stackly_protocol::daemon::DaemonSessionInfo {
                id: "6f9619ff-8b86-4011-b42d-00c04fc964ff/100014".to_owned(),
                started_at: "2026-11-02T07:58:00Z".parse().unwrap(),
                rotation: stackly_protocol::daemon::SessionRotationKind::New,
                previous_end: Some(stackly_protocol::daemon::SessionEndKind::Clean),
            });
            vec![Step::Reply(Reply::DaemonGetInfo(info))]
        }
        Call::DaemonGetHealth => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            stackly_protocol::daemon::DaemonHealthStatus::Ready,
        )))],
        _ => vec![Step::Fail(error(
            "test.unexpected",
            "failed",
            "unexpected call",
        ))],
    });

    let text = run(root.path(), &["daemon", "status"]);
    let json = run(root.path(), &["daemon", "status", "--json"]);

    let row = text
        .stdout
        .lines()
        .find(|line| line.starts_with("Session"))
        .unwrap_or_else(|| panic!("no Session row in {}", text.stdout));
    assert!(
        row.contains("6f9619ff-8b86-4011-b42d-00c04fc964ff/100014")
            && row.contains("rotation new")
            && row.contains("previous ended clean"),
        "{row}"
    );
    let value = envelope(&json);
    assert_eq!(value["result"]["info"]["session"]["rotation"], "new");
    assert_eq!(value["result"]["info"]["session"]["previousEnd"], "clean");
}
