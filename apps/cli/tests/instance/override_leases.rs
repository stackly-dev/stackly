//! Spec 0047 AC-19: `instance stop`, `stop --all`, and `endpoint set` send
//! `overrideLeases` true only with `--override-leases`. Without it a
//! refusal exits 3 with one `<name> is used by <environment> (<alias>)`
//! line per environment and hints that name the flag and `stackly env
//! down`. A stop with the override names each environment that is no
//! longer complete.

use serde_json::{Value, json};
use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_protocol::stop_all::{StopAllPreview, StopAllResult};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";
const ERP: &str = "0192f3a7-0000-7000-8000-0000000000e1";

/// The refusal `code` names `erp` (alias `db`) in.
fn refusal(code: &str, message: &str) -> ErrorBody {
    let mut value =
        serde_json::to_value(error(code, "conflict", message)).expect("the body serializes");
    value["items"] = json!([{
        "kind": "environment",
        "fields": [
            { "key": "environmentId", "value": ERP },
            { "key": "name", "value": "erp" },
            { "key": "alias", "value": "db" },
            { "key": "ownership", "value": "shared" },
        ],
    }]);
    serde_json::from_value(value).expect("the body decodes")
}

/// The stopped fixture instance, still leased by `erp`.
fn stopped_leased() -> stackly_protocol::instance::InstanceSummary {
    data::edited(|value| {
        value["lifecycle"] = "stopped".into();
        value["desired"] = "stopped".into();
        value["process"] = Value::Null;
        value["readiness"]["state"] = "unknown".into();
        value["environments"] = json!([{
            "environmentId": ERP,
            "name": "erp",
            "alias": "db",
            "ownership": "shared",
            "leaseActive": true,
        }]);
    })
}

fn content(daemon: &ScriptedDaemon, method: &str) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == method)
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

/// AC-19.
#[test]
fn a_refused_stop_exits_3_naming_the_environment_and_the_way_around_it() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance stop <id> refused with supervisor.stop_requires_override writes <id> is used by erp (db), then the error block with a hint naming stackly instance stop <id> --override-leases and stackly env down erp, exits 3, and sent overrideLeases false",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceStop(_) => vec![Step::Fail(refusal(
            "supervisor.stop_requires_override",
            "an active environment lease needs an override to stop the instance",
        ))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["instance", "stop", ID]);
    let json = run(root.path(), &["instance", "stop", ID, "--json"]);

    assert_eq!(text.code, Some(3), "{text:?}");
    assert!(
        text.stderr
            .starts_with(&format!("{ID} is used by erp (db)\n")),
        "{}",
        text.stderr
    );
    assert!(
        text.stderr.contains(&format!(
            "hint: to go ahead anyway, run: stackly instance stop {ID} --override-leases"
        )),
        "{}",
        text.stderr
    );
    assert!(
        text.stderr
            .contains("hint: or bring the environment down first: stackly env down erp"),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(3), "{json:?}");
    assert_eq!(
        envelope(&json)["error"]["code"],
        "supervisor.stop_requires_override"
    );
    assert!(!json.stderr.contains("is used by"), "{}", json.stderr);
    assert_eq!(
        content(&daemon, "instance.stop")[0]["overrideLeases"],
        false
    );
    ev.pass();
}

/// AC-19.
#[test]
fn a_stop_with_the_override_sends_it_and_names_the_environment_left_incomplete() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance stop <id> --override-leases --wait sends instance.stop with overrideLeases true, follows it to succeeded, and writes orders-db stopped, readiness unknown then orders-db stopped. erp is no longer complete.",
    );
    let root = TestRoot::new().expect("a test root is made");
    let stopping = data::scripted(
        Reply::InstanceStop(Box::new(data::accepted(
            data::operation(OperationKind::StopInstance, OperationState::Queued, 1),
            false,
        ))),
        vec![
            data::operation(OperationKind::StopInstance, OperationState::Running, 2),
            data::operation(OperationKind::StopInstance, OperationState::Succeeded, 3),
        ],
        Some(stopped_leased()),
    );
    let daemon = ScriptedDaemon::serve(&root, stopping);

    let text = run(
        root.path(),
        &["instance", "stop", ID, "--override-leases", "--wait"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        "orders-db stopped, readiness unknown\norders-db stopped. erp is no longer complete.\n"
    );
    assert_eq!(content(&daemon, "instance.stop")[0]["overrideLeases"], true);
    ev.pass();
}

/// AC-19: `--override-leases` and `--force` do not go together.
#[test]
fn the_override_cannot_be_given_with_force() {
    let root = TestRoot::new().expect("a test root is made");
    let text = run(
        root.path(),
        &["instance", "stop", ID, "--force", "--override-leases"],
    );
    assert_eq!(text.code, Some(2), "{text:?}");
}

fn preview(value: Value) -> StopAllPreview {
    serde_json::from_value(value).expect("a preview decodes")
}

/// AC-19.
#[test]
fn stop_all_with_the_override_sends_it_to_the_preview_and_the_stop_and_names_the_environment() {
    let ev = evidence!(
        covers = ["REQ-027", "REQ-028"],
        kind = Kind::Fixture,
        expected = "stop --all --override-leases --yes sends instance.preview_stop_all and instance.stop_all with overrideLeases true, writes orders-db · Used by erp and affects erp (1 instance) to stderr; without the flag both carry overrideLeases false and a skipped leased instance adds a hint naming stop --all --override-leases",
    );
    let leased = preview(json!({
        "items": [{
            "instanceId": ID,
            "name": "orders-db",
            "lifecycle": "running",
            "demand": ["environment"],
            "autostart": { "policy": "manual", "session": "none" },
            "environments": [{
                "environmentId": ERP, "name": "erp", "alias": "db",
                "ownership": "shared", "leaseActive": true,
            }],
        }],
        "alreadyStopping": [],
        "skipped": [],
        "atLoginCount": 0,
        "affectedEnvironments": [
            { "environmentId": ERP, "name": "erp", "instanceCount": 1 }
        ],
    }));
    let held = preview(json!({
        "items": [],
        "alreadyStopping": [],
        "skipped": [{
            "instanceId": ID,
            "name": "orders-db",
            "code": "supervisor.stop_requires_override",
            "environments": [{
                "environmentId": ERP, "name": "erp", "alias": "db",
                "ownership": "shared", "leaseActive": true,
            }],
        }],
        "atLoginCount": 0,
    }));
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstancePreviewStopAll(params) => {
            let shown = if params.override_leases {
                leased.clone()
            } else {
                held.clone()
            };
            vec![Step::Reply(Reply::InstancePreviewStopAll(Box::new(shown)))]
        }
        Call::InstanceStopAll(_) => {
            let result: StopAllResult = serde_json::from_value(json!({
                "stopped": [ID], "failed": [], "skipped": []
            }))
            .expect("a result decodes");
            vec![Step::Reply(Reply::InstanceStopAll(Box::new(result)))]
        }
        _ => Vec::new(),
    });

    let overriding = run(
        root.path(),
        &["instance", "stop", "--all", "--override-leases", "--yes"],
    );
    let plain = run(root.path(), &["instance", "stop", "--all", "--yes"]);

    assert_eq!(overriding.code, Some(0), "{overriding:?}");
    assert!(
        overriding.stderr.contains("  orders-db · Used by erp\n")
            && overriding.stderr.contains("affects erp (1 instance)\n"),
        "{}",
        overriding.stderr
    );
    assert_eq!(overriding.stdout, "Stopped 1 service. Data is kept.\n");
    let previews = content(&daemon, "instance.preview_stop_all");
    assert_eq!(previews[0]["overrideLeases"], true);
    assert_eq!(
        content(&daemon, "instance.stop_all")[0]["overrideLeases"],
        true
    );
    assert_eq!(plain.code, Some(0), "{plain:?}");
    assert_eq!(previews.len(), 2);
    assert_eq!(previews[1]["overrideLeases"], false);
    assert!(
        plain
            .stderr
            .contains("stackly instance stop --all --override-leases"),
        "{}",
        plain.stderr
    );
    assert_eq!(content(&daemon, "instance.stop_all").len(), 1);
    ev.pass();
}

/// AC-19.
#[test]
fn endpoint_set_sends_the_override_and_a_refusal_names_the_flag() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "endpoint set <id> --port main=5556 refused with instance.endpoints_require_override writes <id> is used by erp (db), exits 3 with a hint naming stackly instance endpoint set <id> --port main=5556 --override-leases and stackly env down erp, and sent overrideLeases false; with --override-leases it sends overrideLeases true",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceConfigGet(_) => vec![Step::Reply(Reply::InstanceConfigGet(Box::new(
            serde_json::from_value(data::fixture(
                "frame-response-instance-config-get-fake.json",
            ))
            .expect("the fixture decodes"),
        )))],
        Call::InstanceChangeEndpoints(params) if !params.override_leases => {
            vec![Step::Fail(refusal(
                "instance.endpoints_require_override",
                "an active environment lease needs an override to change the ports of the instance",
            ))]
        }
        Call::InstanceChangeEndpoints(_) => {
            vec![Step::Reply(Reply::InstanceChangeEndpoints(Box::new(
                serde_json::from_value(data::fixture(
                    "frame-response-instance-change-endpoints-applied.json",
                ))
                .expect("the fixture decodes"),
            )))]
        }
        _ => Vec::new(),
    });

    let refused = run(
        root.path(),
        &["instance", "endpoint", "set", ID, "--port", "main=5556"],
    );
    let overridden = run(
        root.path(),
        &[
            "instance",
            "endpoint",
            "set",
            ID,
            "--port",
            "main=5556",
            "--override-leases",
            "--json",
        ],
    );

    assert_eq!(refused.code, Some(3), "{refused:?}");
    assert!(
        refused
            .stderr
            .starts_with(&format!("{ID} is used by erp (db)\n")),
        "{}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains(&format!(
            "stackly instance endpoint set {ID} --port main=5556 --override-leases"
        )),
        "{}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains("stackly env down erp"),
        "{}",
        refused.stderr
    );
    assert_eq!(overridden.code, Some(0), "{overridden:?}");
    let calls = content(&daemon, "instance.change_endpoints");
    assert_eq!(calls[0]["overrideLeases"], false);
    assert_eq!(calls[1]["overrideLeases"], true);
    ev.pass();
}
