//! Spec 0047 AC-18: `env down` reads `env.preview_down`, writes what stops
//! and what keeps running to stderr, and starts `env.down` with the IDs of
//! that preview, asking nothing. Without `--wait` it prints the operation;
//! with `--wait` one line per instance that ended, then the summary. An
//! incomplete Down exits 7, a set that changed twice 3, and an environment
//! nobody has 10. A file is matched against the manifest locations of
//! `env.list`, so a missing manifest still goes down.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use stackly_protocol::env::{EnvDownPreview, EnvDownReply};
use stackly_protocol::events::{EventCursor, EventKind, Subscribed};
use stackly_protocol::operation::{
    OperationAccepted, OperationFailureInfo, OperationKind, OperationResult, OperationState,
    OperationSummary, PartialItem, PartialOutcome,
};
use stackly_protocol::{Call, ErrorBody, ErrorItem, Reply, ResourceKind, ResourceRef, Sequence};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::scripted::{Ctx, ScriptedDaemon, Step, error, server_instance};
use crate::common::{dto, envelope, golden, run};
use crate::data;

/// The `environment_down` operation the scripted daemon starts.
const DOWN: Uuid = Uuid::from_u128(0x0192_f3a4_0000_7000_8000_0000_0000_00d1);

/// `down-preview.json`: nats and postgres stop, shared-valkey keeps
/// running, traefik is stopped already, leftover can't be confirmed.
fn preview() -> EnvDownPreview {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/env/down-preview.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
        .expect("the preview decodes")
}

/// The preview's instance `n` as a resource.
fn instance(n: u128) -> ResourceRef {
    ResourceRef {
        kind: ResourceKind::Instance,
        id: Uuid::from_u128(0x0192_f3a5_0000_7000_8000_0000_0000_0000 + n).to_string(),
    }
}

/// The Down in `state` at `revision`.
fn down(state: OperationState, revision: u64) -> OperationSummary {
    let mut summary = dto::summary(DOWN, state, revision);
    summary.kind = OperationKind::EnvironmentDown;
    summary.resource = Some(ResourceRef {
        kind: ResourceKind::Environment,
        id: data::detail().summary.id.to_string(),
    });
    summary
}

/// The Down that stopped nats and postgres.
fn succeeded() -> OperationSummary {
    let mut summary = down(OperationState::Succeeded, 9);
    summary.result = Some(OperationResult {
        resources: vec![instance(2), instance(1)],
        partial: Vec::new(),
    });
    summary
}

/// The Down that ended with postgres not stopped and nats left running.
fn incomplete() -> OperationSummary {
    let mut summary = down(OperationState::Failed, 9);
    let item = |n: u128, name: &str, outcome: &str, code: &str| {
        ErrorItem::new("instance")
            .with_field("instanceId", instance(n).id)
            .with_field("name", name)
            .with_field("outcome", outcome)
            .with_field("code", code)
            .with_field("operationId", DOWN.to_string())
    };
    let body = error(
        "env.down_incomplete",
        "failed",
        "some instances of the environment did not stop",
    )
    .with_hint(
        "bring the environment down again, or stop the instances listed; nothing was deleted",
    )
    .with_detail("stopped", "0")
    .with_detail("total", "2");
    let body: ErrorBody = {
        let mut value = serde_json::to_value(body).expect("the body serializes");
        value["items"] = serde_json::to_value(vec![
            item(
                1,
                "erp-development-postgres",
                "failed",
                "supervisor.stop_timeout",
            ),
            item(
                2,
                "erp-development-nats",
                "left_running",
                "env.down_changed",
            ),
        ])
        .expect("the items serialize");
        serde_json::from_value(value).expect("the body decodes")
    };
    summary.failure = Some(OperationFailureInfo {
        reason: "step_failed".to_owned(),
        retryable: false,
        error: body,
    });
    summary
}

/// What the scripted daemon answers.
struct Script {
    previews: Vec<EnvDownPreview>,
    /// One answer per `env.down`, the last one repeated.
    downs: Vec<Result<OperationSummary, ErrorBody>>,
    gets: Vec<OperationSummary>,
}

fn serve(script: Script) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    let script = Arc::new(script);
    let sent = Arc::new(AtomicUsize::new(0));
    move |call, ctx| match call {
        Call::EnvList => vec![Step::Reply(Reply::EnvList(data::list()))],
        Call::EnvPreviewDown(_) => {
            let preview = script.previews[ctx.nth.min(script.previews.len() - 1)].clone();
            vec![Step::Reply(Reply::EnvPreviewDown(Box::new(preview)))]
        }
        Call::EnvDown(_) => {
            let nth = sent.fetch_add(1, Ordering::SeqCst);
            match &script.downs[nth.min(script.downs.len() - 1)] {
                Ok(operation) => vec![Step::Reply(Reply::EnvDown(Box::new(EnvDownReply {
                    environment_id: data::detail().summary.id,
                    name: "erp-development".to_owned(),
                    operation: OperationAccepted {
                        operation: operation.clone(),
                        replayed: false,
                    },
                })))],
                Err(body) => vec![Step::Fail(body.clone())],
            }
        }
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
        Call::EnvGet(_) => vec![Step::Reply(Reply::EnvGet(Box::new(data::detail())))],
        _ => Vec::new(),
    }
}

fn daemon(script: Script) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, serve(script));
    (root, daemon)
}

/// The `call.content` of every request with `method`.
fn sent(daemon: &ScriptedDaemon, method: &str) -> Vec<serde_json::Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == method)
        .map(|logged| logged.raw)
        .collect()
}

/// The IDs of the preview's `stop`, as `env.down` should carry them.
fn stop_ids(preview: &EnvDownPreview) -> serde_json::Value {
    serde_json::to_value(
        preview
            .stop
            .iter()
            .map(|instance| instance.instance_id)
            .collect::<Vec<_>>(),
    )
    .expect("the ids serialize")
}

/// AC-18.
#[test]
fn down_without_wait_sends_the_preview_ids_and_prints_the_operation_it_started() {
    let ev = evidence!(
        covers = ["REQ-022", "REQ-028"],
        kind = Kind::Fixture,
        expected = "env down erp-development over down-preview.json writes stop erp-development-nats, stop erp-development-postgres, keep shared-valkey: started by you; used by reporting, and skip leftover: Stackly can’t confirm this process to stderr, asks no question, sends env.down with the two stop IDs of the preview, prints Started bringing down erp-development (operation <id>) with a wait hint, exits 0, and with --json writes { operation, idempotencyKey, preview, environment: null }",
    );
    let (root, scripted) = daemon(Script {
        previews: vec![preview()],
        downs: vec![Ok(down(OperationState::Queued, 1))],
        gets: Vec::new(),
    });

    let text = run(root.path(), &["env", "down", "erp-development"]);
    let json = run(
        root.path(),
        &[
            "env",
            "down",
            "erp-development",
            "--json",
            "--idempotency-key",
            "down-1",
        ],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_down.txt", &text.stdout);
    golden::check("env_down_stderr.txt", &text.stderr);
    assert!(
        text.stdout.starts_with(&format!(
            "Started bringing down erp-development (operation {DOWN})"
        )),
        "{}",
        text.stdout
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("env_down.json", &json.stdout);
    assert_eq!(json.stderr, "");
    let value = envelope(&json);
    assert_eq!(value["command"], "env down");
    assert_eq!(value["result"]["operation"]["id"], DOWN.to_string());
    assert_eq!(value["result"]["idempotencyKey"], "down-1");
    assert_eq!(
        value["result"]["preview"],
        serde_json::to_value(preview()).expect("the preview serializes")
    );
    assert!(value["result"]["environment"].is_null());
    let downs = sent(&scripted, "env.down");
    assert_eq!(downs.len(), 2);
    let params = &downs[1]["call"]["content"];
    assert_eq!(params["id"], data::detail().summary.id.to_string());
    assert_eq!(params["expectedStopInstanceIds"], stop_ids(&preview()));
    assert_eq!(downs[1]["idempotencyKey"], "down-1");
    ev.pass();
}

/// AC-18.
#[test]
fn down_with_wait_writes_a_line_per_instance_then_the_summary() {
    let ev = evidence!(
        covers = ["REQ-022", "REQ-028"],
        kind = Kind::Fixture,
        expected = "env down --wait writes stopped erp-development-nats and stopped erp-development-postgres to stderr after the preview, prints Down. 2 stopped, 1 kept running., exits 0, and with --json fills environment with the detail env.get answered",
    );
    let script = || Script {
        previews: vec![preview()],
        downs: vec![Ok(down(OperationState::Running, 2))],
        gets: vec![down(OperationState::Running, 3), succeeded()],
    };
    let (root, _daemon) = daemon(script());

    let text = run(root.path(), &["env", "down", "erp-development", "--wait"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_down_wait.txt", &text.stdout);
    assert_eq!(text.stdout, "Down. 2 stopped, 1 kept running.\n");
    let stopped = text
        .stderr
        .find("stopped erp-development-nats\nstopped erp-development-postgres\n")
        .expect("one line per stopped instance");
    let previewed = text
        .stderr
        .find("stop erp-development-nats\n")
        .expect("the preview");
    assert!(previewed < stopped, "{}", text.stderr);

    let (root, _daemon) = daemon(script());
    let json = run(
        root.path(),
        &["env", "down", "erp-development", "--wait", "--json"],
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["result"]["operation"]["state"], "succeeded");
    assert_eq!(
        value["result"]["environment"],
        serde_json::to_value(data::detail()).expect("the detail serializes")
    );
    ev.pass();
}

/// AC-18: an instance another demand kept since the preview is named.
#[test]
fn an_instance_kept_after_the_preview_is_named_and_counted() {
    let mut done = succeeded();
    if let Some(result) = done.result.as_mut() {
        result.resources = vec![instance(2)];
        result.partial = vec![PartialItem {
            item: "erp-development-postgres".to_owned(),
            resource: Some(instance(1)),
            outcome: PartialOutcome::Skipped,
            code: Some("env.instance_in_use".to_owned()),
        }];
    }
    let (root, _daemon) = daemon(Script {
        previews: vec![preview()],
        downs: vec![Ok(down(OperationState::Running, 2))],
        gets: vec![done],
    });

    let text = run(root.path(), &["env", "down", "erp-development", "--wait"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stderr.contains("stopped erp-development-nats\n")
            && text.stderr.contains("kept erp-development-postgres\n"),
        "{}",
        text.stderr
    );
    assert_eq!(text.stdout, "Down. 1 stopped, 2 kept running.\n");
}

/// AC-18: `env.down_incomplete` exits 7 with a line per instance.
#[test]
fn an_incomplete_down_exits_7_and_names_each_instance_that_did_not_stop() {
    let ev = evidence!(
        covers = ["REQ-022", "REQ-028"],
        kind = Kind::Fixture,
        expected = "env down --wait over a Down that failed with env.down_incomplete writes failed erp-development-postgres: supervisor.stop_timeout and left running erp-development-nats to stderr, then the error block with code env.down_incomplete and the hint that nothing was deleted, and exits 7",
    );
    let (root, _daemon) = daemon(Script {
        previews: vec![preview()],
        downs: vec![Ok(down(OperationState::Running, 2))],
        gets: vec![incomplete()],
    });

    let text = run(root.path(), &["env", "down", "erp-development", "--wait"]);

    assert_eq!(text.code, Some(7), "{text:?}");
    golden::check("env_down_incomplete_stderr.txt", &text.stderr);
    assert!(
        text.stderr.contains(
            "failed erp-development-postgres: supervisor.stop_timeout\nleft running erp-development-nats\n"
        ),
        "{}",
        text.stderr
    );
    assert!(text.stderr.contains("code: env.down_incomplete"));
    assert!(text.stderr.contains("nothing was deleted"));
    ev.pass();
}

/// AC-18: `env.down_changed` is retried once with a fresh preview, then
/// exits 3.
#[test]
fn a_changed_set_is_sent_again_once_with_the_new_preview_then_exits_3() {
    let ev = evidence!(
        covers = ["REQ-022", "REQ-028"],
        kind = Kind::Fixture,
        expected = "env down whose first env.down answers env.down_changed reads env.preview_down again and sends env.down once more with the stop IDs of the new preview and the same idempotency key, then exits 0; when the second also answers env.down_changed it exits 3 after exactly two env.down requests",
    );
    let changed: Result<OperationSummary, ErrorBody> = Err(error(
        "env.down_changed",
        "conflict",
        "what down would stop changed",
    ));
    let mut fewer = preview();
    fewer.stop.truncate(1);

    let (root, daemon_once) = daemon(Script {
        previews: vec![preview(), fewer.clone()],
        downs: vec![changed.clone(), Ok(down(OperationState::Queued, 1))],
        gets: Vec::new(),
    });
    let once = run(root.path(), &["env", "down", "erp-development"]);
    assert_eq!(once.code, Some(0), "{once:?}");
    assert!(
        once.stderr.contains("What Down will stop changed."),
        "{}",
        once.stderr
    );
    let downs = sent(&daemon_once, "env.down");
    assert_eq!(downs.len(), 2);
    assert_eq!(
        downs[0]["call"]["content"]["expectedStopInstanceIds"],
        stop_ids(&preview())
    );
    assert_eq!(
        downs[1]["call"]["content"]["expectedStopInstanceIds"],
        stop_ids(&fewer)
    );
    assert_eq!(downs[0]["idempotencyKey"], downs[1]["idempotencyKey"]);

    let (root, daemon_twice) = daemon(Script {
        previews: vec![preview(), fewer],
        downs: vec![changed],
        gets: Vec::new(),
    });
    let twice = run(root.path(), &["env", "down", "erp-development"]);
    assert_eq!(twice.code, Some(3), "{twice:?}");
    assert!(twice.stderr.contains("code: env.down_changed"));
    assert_eq!(sent(&daemon_twice, "env.down").len(), 2);
    ev.pass();
}

/// AC-18: an empty preview still goes to `env.down`.
#[test]
fn an_empty_preview_says_nothing_to_stop_and_still_sends_env_down() {
    let mut empty = preview();
    empty.stop.clear();
    empty.keep.clear();
    empty.blocked.clear();
    empty.lease_count = stackly_protocol::Count::saturating(0);
    let (root, scripted) = daemon(Script {
        previews: vec![empty],
        downs: vec![Ok(down(OperationState::Queued, 1))],
        gets: Vec::new(),
    });

    let text = run(root.path(), &["env", "down", "erp-development"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stderr.contains("Nothing to stop.\n"),
        "{}",
        text.stderr
    );
    let downs = sent(&scripted, "env.down");
    assert_eq!(downs.len(), 1);
    assert_eq!(
        downs[0]["call"]["content"]["expectedStopInstanceIds"],
        serde_json::json!([])
    );
}

/// AC-18: a file is matched against `env.list` without planning it, so a
/// missing manifest goes down; a name nobody has exits 10.
#[test]
fn a_registered_file_goes_down_without_a_plan_and_an_unknown_name_exits_10() {
    let ev = evidence!(
        covers = ["REQ-022", "REQ-028"],
        kind = Kind::Fixture,
        expected = "env down --file /Users/dev/erp/stackly.yaml, a manifest that does not exist on this Mac, finds erp-development by its manifest location in env.list, never sends env.plan, and sends env.down for its id; env down nobody exits 10 with cli.environment_not_found and sends neither env.preview_down nor env.down",
    );
    let (root, scripted) = daemon(Script {
        previews: vec![preview()],
        downs: vec![Ok(down(OperationState::Queued, 1))],
        gets: Vec::new(),
    });

    let file = run(
        root.path(),
        &["env", "down", "--file", "/Users/dev/erp/stackly.yaml"],
    );
    assert_eq!(file.code, Some(0), "{file:?}");
    assert!(scripted.methods().iter().all(|method| method != "env.plan"));
    let downs = sent(&scripted, "env.down");
    assert_eq!(
        downs[0]["call"]["content"]["id"],
        data::detail().summary.id.to_string()
    );

    let (root, nobody_daemon) = daemon(Script {
        previews: vec![preview()],
        downs: vec![Ok(down(OperationState::Queued, 1))],
        gets: Vec::new(),
    });
    let unknown = run(root.path(), &["env", "down", "nobody", "--json"]);
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    assert_eq!(
        envelope(&unknown)["error"]["code"],
        "cli.environment_not_found"
    );
    assert!(
        nobody_daemon
            .methods()
            .iter()
            .all(|method| method != "env.preview_down" && method != "env.down")
    );
    ev.pass();
}
