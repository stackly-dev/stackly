//! Spec 0046 AC-17: `env up` plans the manifest, writes the plan to stderr,
//! and starts `env.up` with the plan's digest; without `--wait` it prints
//! the operation, with `--wait` one line per phase change and then the
//! services with the ports they really use. A manifest that is not valid
//! exits 2, a blocked plan 3, `--locked` against a daemon without the mode
//! 5, a reference without a value 6 with a `stackly secret set` hint per
//! reference, and an incomplete Up 7.

use std::sync::Arc;
use std::time::Duration;

use stackly_protocol::env::{EnvPlan, EnvUpReply, EnvironmentDetail, PlanBlocker};
use stackly_protocol::events::{EventCursor, EventKind, Subscribed};
use stackly_protocol::operation::{
    OperationAccepted, OperationFailureInfo, OperationKind, OperationState, OperationSummary,
};
use stackly_protocol::{Call, ErrorBody, ErrorItem, Reply, ResourceKind, ResourceRef};
use stackly_protocol::{Count, Sequence};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::scripted::{Ctx, ScriptedDaemon, Step, error, server_instance};
use crate::common::{detail, dto, envelope, golden, run};
use crate::data;

/// The `environment_up` operation of the fixtures.
const UP: Uuid = Uuid::from_u128(0x0192_f3a4_0000_7000_8000_0000_0000_00f1);

/// The applicable plan with the three services of `graph`.
fn graph_plan() -> EnvPlan {
    let mut plan = data::applicable();
    let template = plan.items[0].clone();
    plan.items = ["db", "cache", "api"]
        .into_iter()
        .map(|alias| {
            let mut item = template.clone();
            item.alias = alias.to_owned();
            item.instance.name = format!("graph-{alias}");
            item
        })
        .collect();
    plan.order = vec!["db".to_owned(), "cache".to_owned(), "api".to_owned()];
    plan
}

/// The Up in `state` at `revision`.
fn up(state: OperationState, revision: u64, environment: Uuid) -> OperationSummary {
    let mut summary = dto::summary(UP, state, revision);
    summary.kind = OperationKind::EnvironmentUp;
    summary.resource = Some(ResourceRef {
        kind: ResourceKind::Environment,
        id: environment.to_string(),
    });
    summary
}

/// The Up that ended `failed` with `env.up_incomplete`.
fn incomplete(environment: Uuid) -> OperationSummary {
    let mut summary = up(OperationState::Failed, 9, environment);
    summary.failure = Some(OperationFailureInfo {
        reason: "step_failed".to_owned(),
        retryable: false,
        error: error("env.up_incomplete", "failed", "1 of 3 services are ready")
            .with_detail("ready", "1")
            .with_detail("total", "3")
            .with_hint("set the missing secret, then run stackly env up again"),
    });
    summary
}

/// What the scripted daemon answers.
struct Script {
    plan: EnvPlan,
    up: Result<OperationSummary, ErrorBody>,
    gets: Vec<OperationSummary>,
    details: Vec<EnvironmentDetail>,
}

fn serve(script: Script) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    let script = Arc::new(script);
    move |call, ctx| match call {
        Call::EnvList => vec![Step::Reply(Reply::EnvList(data::list()))],
        Call::EnvPlan(_) => vec![Step::Reply(Reply::EnvPlan(Box::new(script.plan.clone())))],
        Call::EnvUp(_) => match &script.up {
            Ok(operation) => vec![Step::Reply(Reply::EnvUp(Box::new(EnvUpReply {
                environment_id: data::bringing_up().summary.id,
                name: "graph".to_owned(),
                operation: OperationAccepted {
                    operation: operation.clone(),
                    replayed: false,
                },
            })))],
            Err(body) => vec![Step::Fail(body.clone())],
        },
        Call::EventsSubscribe(_) => vec![Step::Reply(Reply::EventsSubscribe(Subscribed {
            subscription_id: 1,
            head: EventCursor {
                server_instance_id: server_instance(ctx.connection),
                sequence: Sequence::ZERO,
            },
        }))],
        Call::OperationGet(_) => {
            let summary = script.gets[ctx.nth.min(script.gets.len() - 1)].clone();
            let mut steps = vec![Step::Reply(Reply::OperationGet(Box::new(dto::detail(
                summary.clone(),
            ))))];
            if !summary.state.is_final() {
                steps.push(Step::Sleep(Duration::from_millis(30)));
                steps.push(Step::Event(EventKind::OperationStateChanged(Box::new(
                    summary,
                ))));
            }
            steps
        }
        Call::EnvGet(_) => {
            let detail = script.details[ctx.nth.min(script.details.len() - 1)].clone();
            vec![Step::Reply(Reply::EnvGet(Box::new(detail)))]
        }
        _ => Vec::new(),
    }
}

fn daemon(script: Script) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, serve(script));
    (root, daemon)
}

fn environment() -> Uuid {
    data::bringing_up().summary.id
}

/// AC-17.
#[test]
fn up_without_wait_sends_the_plan_digest_and_prints_the_operation_it_started() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env up over plan-applicable.json writes the plan table to stderr, sends env.up with the file source, the plan's planDigest, and locked false, prints Started bringing up graph (operation <id>) with a wait hint, exits 0, and with --json writes { operation, idempotencyKey, environment: null }",
    );
    let (root, daemon) = daemon(Script {
        plan: graph_plan(),
        up: Ok(up(OperationState::Queued, 1, environment())),
        gets: Vec::new(),
        details: Vec::new(),
    });

    let text = run(root.path(), &["env", "up"]);
    let json = run(
        root.path(),
        &["env", "up", "--json", "--idempotency-key", "up-1"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_up.txt", &text.stdout);
    golden::check("env_up_stderr.txt", &text.stderr);
    assert!(
        text.stderr
            .contains("Plan: 3 to install, 3 to create, 3 to start, 0 to keep."),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("env_up.json", &json.stdout);
    assert_eq!(json.stderr, "");
    let value = envelope(&json);
    assert_eq!(value["command"], "env up");
    assert_eq!(value["result"]["operation"]["id"], UP.to_string());
    assert_eq!(value["result"]["idempotencyKey"], "up-1");
    assert!(value["result"]["environment"].is_null());
    let ups: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.up")
        .map(|logged| logged.raw)
        .collect();
    assert_eq!(ups.len(), 2);
    let params = &ups[1]["call"]["content"];
    assert_eq!(
        params["expectedPlanDigest"],
        graph_plan().plan_digest.expect("a digest")
    );
    assert_eq!(params["locked"], false);
    assert_eq!(params["source"]["type"], "file");
    assert_eq!(ups[1]["idempotencyKey"], "up-1");
    ev.pass();
}

/// AC-17.
#[test]
fn up_with_wait_writes_a_line_per_phase_change_then_the_bound_ports() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env up --wait writes ready graph-db 127.0.0.1:49152, wait api: waiting for cache, start graph-cache, then ready lines for cache and api to stderr, prints the SERVICE INSTANCE STATUS ENDPOINTS table with the auto ports 49152 to 49154 and 3 of 3 ready., exits 0, and with --json fills environment with the detail env.get answered",
    );
    let script = || Script {
        plan: graph_plan(),
        up: Ok(up(OperationState::Running, 2, environment())),
        gets: vec![
            up(OperationState::Running, 3, environment()),
            up(OperationState::Succeeded, 9, environment()),
        ],
        details: vec![data::bringing_up(), data::brought_up()],
    };
    let (root, _daemon) = daemon(script());

    let text = run(root.path(), &["env", "up", "--wait"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_up_wait.txt", &text.stdout);
    let progress: Vec<&str> = text
        .stderr
        .lines()
        .skip_while(|line| !line.starts_with("waiting for operation "))
        .filter(|line| {
            ["ready ", "wait ", "start ", "create ", "install ", "skip "]
                .iter()
                .any(|word| line.starts_with(word))
        })
        .collect();
    assert_eq!(
        progress,
        [
            "ready graph-db 127.0.0.1:49152",
            "wait api: waiting for cache",
            "start graph-cache",
            "ready graph-api 127.0.0.1:49153",
            "ready graph-cache 127.0.0.1:49154",
        ],
        "{}",
        text.stderr
    );
    assert_eq!(text.stdout.lines().last(), Some("3 of 3 ready."));

    // --json reads no progress, so env.get answers only once, at the end.
    let mut quiet = script();
    quiet.details = vec![data::brought_up()];
    let (root, scripted) = daemon(quiet);
    let json = run(root.path(), &["env", "up", "--wait", "--json"]);
    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(
        scripted
            .methods()
            .iter()
            .filter(|method| *method == "env.get")
            .count(),
        1
    );
    let value = envelope(&json);
    assert_eq!(value["result"]["operation"]["state"], "succeeded");
    assert_eq!(
        value["result"]["environment"],
        serde_json::to_value(data::brought_up()).expect("the detail serializes")
    );
    ev.pass();
}

/// AC-17; scope: the operation shows the stage that failed and how to
/// recover.
#[test]
fn an_incomplete_up_exits_7_with_the_table_the_reasons_and_the_daemon_hint() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env up --wait that ends with env.up_incomplete prints the table with 1 of 3 ready., writes not ready: with the reference to set for cache and the failed dependency of api, the daemon's hint, and exits 7; --json keeps { operation, idempotencyKey, environment } next to the error",
    );
    let script = || Script {
        plan: graph_plan(),
        up: Ok(up(OperationState::Running, 2, environment())),
        gets: vec![incomplete(environment())],
        details: vec![data::up_incomplete()],
    };
    let (root, _daemon) = daemon(script());

    let text = run(root.path(), &["env", "up", "--wait"]);

    assert_eq!(text.code, Some(7), "{text:?}");
    golden::check("env_up_incomplete.txt", &text.stdout);
    assert!(
        text.stderr.contains(
            "  cache: needs secret local:graph/cache-token (missing); set it with stackly secret set local:graph/cache-token"
        ),
        "{}",
        text.stderr
    );
    assert!(
        text.stderr
            .contains("  api: not started: dependency cache failed"),
        "{}",
        text.stderr
    );
    assert!(text.stderr.contains("  code: env.up_incomplete"));
    assert!(
        text.stderr
            .contains("  hint: set the missing secret, then run stackly env up again")
    );

    let (root, _daemon) = daemon(script());
    let json = run(root.path(), &["env", "up", "--wait", "--json"]);
    assert_eq!(json.code, Some(7), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "env.up_incomplete");
    assert_eq!(value["result"]["environment"]["summary"]["readyCount"], 1);
    ev.pass();
}

/// AC-17, AC-2.
#[test]
fn a_reference_without_a_value_exits_6_with_a_secret_set_hint_per_reference() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a plan blocked only by an instance that needs a secret still goes to env.up with a digest of zeros; the daemon's secret.needs_secret exits 6 with one reference line and one hint: stackly secret set <reference> per reference",
    );
    let mut plan = graph_plan();
    plan.applicable = false;
    plan.plan_digest = None;
    plan.needs_secrets = Count::saturating(2);
    plan.blockers = vec![PlanBlocker {
        service: Some("cache".to_owned()),
        error: error(
            "env.instance_blocked",
            "conflict",
            "the instance needs a secret",
        )
        .with_detail("reason", "needs_secret"),
    }];
    let needs = error(
        "secret.needs_secret",
        "unavailable",
        "a resource needs references that cannot be read",
    )
    .with_detail("count", "2")
    .with_items(vec![
        ErrorItem::new("reference")
            .with_field("reference", "local:graph/cache-token")
            .with_field("reason", "missing")
            .with_field("generateAllowed", "true"),
        ErrorItem::new("reference")
            .with_field("reference", "local:graph/db-password")
            .with_field("reason", "unregistered")
            .with_field("generateAllowed", "false"),
    ]);
    let (root, daemon) = daemon(Script {
        plan,
        up: Err(needs),
        gets: Vec::new(),
        details: Vec::new(),
    });

    let text = run(root.path(), &["env", "up", "--wait"]);
    let json = run(root.path(), &["env", "up", "--json"]);

    assert_eq!(text.code, Some(6), "{text:?}");
    assert_eq!(text.stdout, "");
    golden::check("env_up_needs_secret_stderr.txt", &text.stderr);
    for reference in ["local:graph/cache-token", "local:graph/db-password"] {
        assert!(
            text.stderr
                .contains(&format!("  hint: stackly secret set {reference}\n")),
            "{}",
            text.stderr
        );
    }
    assert_eq!(json.code, Some(6), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "secret.needs_secret");
    assert!(value["result"].is_null());
    let digests: Vec<String> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.up")
        .map(|logged| logged.raw["call"]["content"]["expectedPlanDigest"].to_string())
        .collect();
    let zeros = format!("\"{}\"", "0".repeat(64));
    assert_eq!(digests, [zeros.clone(), zeros]);
    ev.pass();
}

/// AC-17.
#[test]
fn a_blocked_or_invalid_plan_or_an_unchecked_lock_never_reaches_env_up() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env up over plan-blocked.json exits 3 with cli.env_plan_blocked and the blocker lines, over an invalid manifest exits 2 with cli.env_invalid, and --locked over a plan whose lock.locked is false exits 5 with cli.env_locked_unsupported; none of them sends env.up",
    );
    let cases: [(EnvPlan, &[&str], i32, &str); 3] = [
        (
            data::blocked(),
            &["env", "up", "--json"],
            3,
            "cli.env_plan_blocked",
        ),
        (
            data::invalid_plan(),
            &["env", "up", "--json"],
            2,
            "cli.env_invalid",
        ),
        (
            {
                let mut plan = graph_plan();
                plan.lock.locked = false;
                plan
            },
            &["env", "up", "--locked", "--json"],
            5,
            "cli.env_locked_unsupported",
        ),
    ];
    for (plan, args, code, error_code) in cases {
        let (root, daemon) = daemon(Script {
            plan,
            up: Err(error(
                "env.plan_changed",
                "conflict",
                "env.up is not expected",
            )),
            gets: Vec::new(),
            details: Vec::new(),
        });
        let json = run(root.path(), args);
        assert_eq!(json.code, Some(code), "{json:?}");
        assert_eq!(envelope(&json)["error"]["code"], error_code);
        assert!(!daemon.methods().iter().any(|method| method == "env.up"));
    }

    let (root, _daemon) = daemon(Script {
        plan: data::blocked(),
        up: Err(error(
            "env.plan_changed",
            "conflict",
            "env.up is not expected",
        )),
        gets: Vec::new(),
        details: Vec::new(),
    });
    let text = run(root.path(), &["env", "up"]);
    assert_eq!(text.code, Some(3), "{text:?}");
    assert!(
        text.stderr
            .contains("blocked: postgres: supervisor.endpoint_conflict"),
        "{}",
        text.stderr
    );
    let blocked = run(root.path(), &["env", "up", "--json"]);
    assert_eq!(
        detail(&envelope(&blocked)["error"], "blockers").as_deref(),
        Some("2")
    );
    ev.pass();
}

/// AC-17: a positional folder stands for its `stackly.yaml`, a name for a
/// registered environment.
#[test]
fn a_folder_argument_plans_its_stackly_yaml_and_a_name_its_environment() {
    let (root, daemon) = daemon(Script {
        plan: graph_plan(),
        up: Ok(up(OperationState::Queued, 1, environment())),
        gets: Vec::new(),
        details: Vec::new(),
    });
    let project = root.path().join("work/graph");
    std::fs::create_dir_all(&project).expect("the project folder can be made");

    let folder = run(
        root.path(),
        &["env", "up", project.to_str().expect("a utf-8 path")],
    );
    let named = run(root.path(), &["env", "up", "erp-development"]);

    assert_eq!(folder.code, Some(0), "{folder:?}");
    assert_eq!(named.code, Some(0), "{named:?}");
    let sources: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.up")
        .map(|logged| logged.raw["call"]["content"]["source"].clone())
        .collect();
    assert_eq!(sources[0]["type"], "file");
    assert_eq!(
        sources[0]["content"],
        project.join("stackly.yaml").display().to_string()
    );
    assert_eq!(sources[1]["type"], "registered");
    assert_eq!(sources[1]["content"], data::detail().summary.id.to_string());
}
