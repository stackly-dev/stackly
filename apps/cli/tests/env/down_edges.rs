//! Edges of `stackly env down` (spec 0047 AC-18) and the `lease:` line of
//! `stackly env status` (AC-22), next to `down` and `status`: a cancelled
//! Down exits 9, `--wait` that runs out of `--timeout` exits 8 without
//! cancelling, every keep reason and the cancelled Up are named, a Down
//! that fails writes one JSON envelope with the error, and the lease line
//! reads one instance, many, none, or nothing from an older daemon.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;
use stackly_protocol::env::{EnvDownPreview, EnvDownReply, EnvPlan, EnvironmentDetail};
use stackly_protocol::events::{EventCursor, EventKind, Subscribed};
use stackly_protocol::operation::{
    OperationAccepted, OperationFailureInfo, OperationKind, OperationState, OperationSummary,
};
use stackly_protocol::stop_all::DemandSourceKind;
use stackly_protocol::{Call, ErrorBody, Reply, ResourceKind, ResourceRef, Sequence};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::scripted::{Ctx, ScriptedDaemon, Step, error, server_instance};
use crate::common::{dto, envelope, run};
use crate::data;

/// The `environment_down` operation the scripted daemon starts.
const DOWN: Uuid = Uuid::from_u128(0x0192_f3a4_0000_7000_8000_0000_0000_00d2);

/// `down-preview.json`, as in `down`.
fn preview() -> EnvDownPreview {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/env/down-preview.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
        .expect("the preview decodes")
}

fn down(state: OperationState, revision: u64) -> OperationSummary {
    let mut summary = dto::summary(DOWN, state, revision);
    summary.kind = OperationKind::EnvironmentDown;
    summary.resource = Some(ResourceRef {
        kind: ResourceKind::Environment,
        id: data::detail().summary.id.to_string(),
    });
    summary
}

struct Script {
    preview: EnvDownPreview,
    down: Result<OperationSummary, ErrorBody>,
    gets: Vec<OperationSummary>,
}

fn serve(script: Script) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    let script = Arc::new(script);
    let gets = Arc::new(AtomicUsize::new(0));
    move |call, ctx| match call {
        Call::EnvList => vec![Step::Reply(Reply::EnvList(data::list()))],
        Call::EnvPreviewDown(_) => vec![Step::Reply(Reply::EnvPreviewDown(Box::new(
            script.preview.clone(),
        )))],
        Call::EnvDown(_) => match &script.down {
            Ok(operation) => vec![Step::Reply(Reply::EnvDown(Box::new(EnvDownReply {
                environment_id: data::detail().summary.id,
                name: "erp-development".to_owned(),
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
            let nth = gets.fetch_add(1, Ordering::SeqCst);
            let summary = script.gets[nth.min(script.gets.len() - 1)].clone();
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

// covers: AC-9, AC-18 (spec 0047): a cancelled Down exits 9 and never
// prints the summary of a Down that finished
#[test]
fn a_cancelled_down_exits_9_without_the_down_summary() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env down --wait over a Down that ends cancelled at stage stop exits 9, prints no Down. summary, and with --json writes one envelope with the cancelled operation",
    );
    let mut cancelled = down(OperationState::Cancelled, 5);
    cancelled.stage = Some("stop".to_owned());
    let script = || Script {
        preview: preview(),
        down: Ok(down(OperationState::Running, 2)),
        gets: vec![down(OperationState::Running, 3), cancelled.clone()],
    };
    let (root, _daemon) = daemon(script());

    let text = run(root.path(), &["env", "down", "erp-development", "--wait"]);

    assert_eq!(text.code, Some(9), "{text:?}");
    assert!(!text.stdout.contains("Down."), "{}", text.stdout);
    assert!(
        text.stderr.contains("stop erp-development-nats\n"),
        "{}",
        text.stderr
    );

    let (root, _daemon) = daemon(script());
    let json = run(
        root.path(),
        &["env", "down", "erp-development", "--wait", "--json"],
    );
    assert_eq!(json.code, Some(9), "{json:?}");
    let value = envelope(&json);
    assert_eq!(json.stdout.trim().lines().count(), 1, "one envelope");
    assert!(
        value.to_string().contains("cancelled"),
        "the cancelled state is in the envelope: {value}"
    );
    ev.pass();
}

// covers: AC-18 (spec 0047): `--wait` that runs out of `--timeout` exits 8
// and leaves the Down running
#[test]
fn a_wait_that_runs_out_of_its_timeout_exits_8_and_never_cancels_the_down() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env down --wait --timeout 1s against a Down that keeps running exits 8 with cli.wait_timeout and never sends operation.cancel",
    );
    let (root, scripted) = daemon(Script {
        preview: preview(),
        down: Ok(down(OperationState::Running, 2)),
        gets: vec![down(OperationState::Running, 3)],
    });

    let json = run(
        root.path(),
        &[
            "env",
            "down",
            "erp-development",
            "--wait",
            "--timeout",
            "1s",
            "--json",
        ],
    );

    assert_eq!(json.code, Some(8), "{json:?}");
    assert_eq!(envelope(&json)["error"]["code"], "cli.wait_timeout");
    assert!(
        !scripted
            .methods()
            .iter()
            .any(|method| method == "operation.cancel"),
        "the Down keeps running"
    );
    ev.pass();
}

// covers: AC-1, AC-18 (spec 0047): every reason a preview keeps an
// instance, several environments, and the Up it cancels
#[test]
fn the_preview_names_every_keep_reason_and_the_up_it_cancels() {
    let mut shown = preview();
    let mut login = shown.keep[0].clone();
    login.instance.name = "login-cache".to_owned();
    login.instance.instance_id = Uuid::now_v7();
    login.demand = vec![DemandSourceKind::Startup];
    login.environments.clear();
    let mut two = shown.keep[0].clone();
    two.instance.name = "two-db".to_owned();
    two.instance.instance_id = Uuid::now_v7();
    two.demand = vec![DemandSourceKind::Environment];
    let mut wiki = two.environments[0].clone();
    wiki.name = "wiki".to_owned();
    wiki.environment_id = Uuid::now_v7();
    two.environments.push(wiki);
    shown.keep.push(login);
    shown.keep.push(two);
    shown.cancels_up = true;
    let (root, _daemon) = daemon(Script {
        preview: shown,
        down: Ok(down(OperationState::Queued, 1)),
        gets: Vec::new(),
    });

    let text = run(root.path(), &["env", "down", "erp-development"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    for line in [
        "keep shared-valkey: started by you; used by reporting\n",
        "keep login-cache: starts at login\n",
        "keep two-db: used by reporting, wiki\n",
        "skip leftover: Stackly can’t confirm this process\n",
        "cancels the running up\n",
    ] {
        assert!(text.stderr.contains(line), "{line:?} in {}", text.stderr);
    }
    assert!(!text.stderr.contains("Nothing to stop."), "{}", text.stderr);
}

/// A Down that ended `env.down_incomplete` with one instance left running.
fn left_running() -> OperationSummary {
    let mut summary = down(OperationState::Failed, 9);
    let body = error(
        "env.down_incomplete",
        "failed",
        "some instances of the environment did not stop",
    )
    .with_hint(
        "bring the environment down again, or stop the instances listed; nothing was deleted",
    )
    .with_detail("stopped", "1")
    .with_detail("total", "2");
    let mut value = serde_json::to_value(body).expect("the body serializes");
    value["items"] = serde_json::json!([{
        "kind": "instance",
        "fields": [
            { "key": "instanceId", "value": "0192f3a5-0000-7000-8000-000000000002" },
            { "key": "name", "value": "erp-development-nats" },
            { "key": "outcome", "value": "left_running" },
            { "key": "code", "value": "env.down_changed" },
        ],
    }]);
    summary.failure = Some(OperationFailureInfo {
        reason: "step_failed".to_owned(),
        retryable: false,
        error: serde_json::from_value(value).expect("the body decodes"),
    });
    summary
}

// covers: AC-8, AC-18 (spec 0047): `--json` over a Down that failed, and
// over a set that changed twice
#[test]
fn a_down_that_fails_writes_one_json_envelope_with_its_error_and_exit_code() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env down --wait --json over env.down_incomplete with erp-development-nats left_running exits 7 with one envelope whose error code is env.down_incomplete and whose items name the instance; env down --json whose env.down answers env.down_changed twice exits 3 with one envelope whose error code is env.down_changed",
    );
    let (root, _daemon) = daemon(Script {
        preview: preview(),
        down: Ok(down(OperationState::Running, 2)),
        gets: vec![left_running()],
    });
    let incomplete = run(
        root.path(),
        &["env", "down", "erp-development", "--wait", "--json"],
    );
    assert_eq!(incomplete.code, Some(7), "{incomplete:?}");
    assert_eq!(incomplete.stdout.trim().lines().count(), 1);
    let value = envelope(&incomplete);
    assert_eq!(value["error"]["code"], "env.down_incomplete");
    assert!(
        value["error"].to_string().contains("erp-development-nats"),
        "{value}"
    );

    let (root, scripted) = daemon(Script {
        preview: preview(),
        down: Err(error(
            "env.down_changed",
            "conflict",
            "what down would stop changed",
        )),
        gets: Vec::new(),
    });
    let changed = run(root.path(), &["env", "down", "erp-development", "--json"]);
    assert_eq!(changed.code, Some(3), "{changed:?}");
    assert_eq!(envelope(&changed)["error"]["code"], "env.down_changed");
    assert_eq!(
        scripted
            .methods()
            .iter()
            .filter(|method| *method == "env.down")
            .count(),
        2,
        "one retry, then the refusal"
    );
    ev.pass();
}

/// `env status` against a daemon whose `env.get` answers `detail`.
fn status_of(detail: EnvironmentDetail) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let plan: EnvPlan = data::applicable();
    let serving = data::serving(data::list(), plan.validation.clone(), plan, data::detail());
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::EnvGet(_) => vec![Step::Reply(Reply::EnvGet(Box::new(detail.clone())))],
        _ => serving(call, ctx),
    });
    (root, daemon)
}

/// The detail fixture with its `lease` replaced by `lease`.
fn with_lease(lease: Value) -> EnvironmentDetail {
    let mut value = serde_json::to_value(data::detail()).expect("the detail serializes");
    value["lease"] = lease;
    serde_json::from_value(value).expect("the detail decodes")
}

// covers: AC-12, AC-22 (spec 0047)
#[test]
fn the_lease_line_reads_many_instances_none_and_nothing_from_an_older_daemon() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env status prints lease: held on 3 instances since <time> as its second line for three active leases, lease: none for none, and no lease line at all when the detail has no lease field, as from a daemon before 4.5",
    );
    let since = "2026-11-13T09:00:00.000Z";
    let (root, _daemon) = status_of(with_lease(
        serde_json::json!({ "activeCount": 3, "heldSince": since }),
    ));
    let many = run(root.path(), &["env", "status", "erp-development"]);
    assert_eq!(many.code, Some(0), "{many:?}");
    assert_eq!(
        many.stdout.lines().nth(1),
        Some(format!("lease: held on 3 instances since {since}").as_str()),
        "{}",
        many.stdout
    );

    let (root, _daemon) = status_of(with_lease(serde_json::json!({ "activeCount": 0 })));
    let none = run(root.path(), &["env", "status", "erp-development"]);
    assert_eq!(
        none.stdout.lines().nth(1),
        Some("lease: none"),
        "{}",
        none.stdout
    );

    let (root, _daemon) = status_of(with_lease(Value::Null));
    let old = run(root.path(), &["env", "status", "erp-development"]);
    assert_eq!(old.code, Some(0), "{old:?}");
    assert!(!old.stdout.contains("lease:"), "{}", old.stdout);
    ev.pass();
}
