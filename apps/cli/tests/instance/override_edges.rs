//! Edges of `--override-leases` (spec 0047 AC-19), next to
//! `override_leases`: a refusal that names two environments writes one
//! line for each, `--json` writes the refusal as one envelope with its
//! items and no text lines, and a stop with the override names no
//! environment whose lease is no longer active.

use serde_json::{Value, json};
use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";

/// The refusal `code` naming `erp` (alias `db`) and `shop` (alias
/// `cache`).
fn two_holders(code: &str, message: &str) -> ErrorBody {
    let mut value =
        serde_json::to_value(error(code, "conflict", message)).expect("the body serializes");
    let item = |id: &str, name: &str, alias: &str, ownership: &str| {
        json!({
            "kind": "environment",
            "fields": [
                { "key": "environmentId", "value": id },
                { "key": "name", "value": name },
                { "key": "alias", "value": alias },
                { "key": "ownership", "value": ownership },
            ],
        })
    };
    value["items"] = json!([
        item(
            "0192f3a7-0000-7000-8000-0000000000e1",
            "erp",
            "db",
            "shared"
        ),
        item(
            "0192f3a7-0000-7000-8000-0000000000e2",
            "shop",
            "cache",
            "dedicated"
        ),
    ]);
    serde_json::from_value(value).expect("the body decodes")
}

fn content(daemon: &ScriptedDaemon, method: &str) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == method)
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

// covers: AC-13, AC-19 (spec 0047)
#[test]
fn a_refused_stop_names_each_environment_on_its_own_line_and_json_carries_the_items() {
    let ev = evidence!(
        covers = ["REQ-028", "BR-015"],
        kind = Kind::Fixture,
        expected = "instance stop <id> refused with supervisor.stop_requires_override naming erp and shop writes <id> is used by erp (db) and <id> is used by shop (cache), exits 3, and hints the override flag; with --json it exits 3 with one envelope whose error items carry name, alias, and ownership of both and writes no is used by line",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceStop(_) => vec![Step::Fail(two_holders(
            "supervisor.stop_requires_override",
            "an active environment lease needs an override to stop the instance",
        ))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["instance", "stop", ID]);
    let json = run(root.path(), &["instance", "stop", ID, "--json"]);

    assert_eq!(text.code, Some(3), "{text:?}");
    assert!(
        text.stderr.starts_with(&format!(
            "{ID} is used by erp (db)\n{ID} is used by shop (cache)\n"
        )),
        "{}",
        text.stderr
    );
    assert!(
        text.stderr
            .contains(&format!("stackly instance stop {ID} --override-leases")),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(3), "{json:?}");
    assert_eq!(json.stdout.trim().lines().count(), 1, "one envelope");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "supervisor.stop_requires_override");
    let items = value["error"]["items"].to_string();
    for needle in ["erp", "shop", "cache", "dedicated", "shared"] {
        assert!(items.contains(needle), "{needle} in {items}");
    }
    assert!(!json.stderr.contains("is used by"), "{}", json.stderr);
    for sent in content(&daemon, "instance.stop") {
        assert_eq!(sent["overrideLeases"], false);
    }
    ev.pass();
}

// covers: AC-15, AC-19 (spec 0047)
#[test]
fn a_refused_port_change_with_json_is_one_envelope_and_exit_3() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceConfigGet(_) => vec![Step::Reply(Reply::InstanceConfigGet(Box::new(
            serde_json::from_value(data::fixture(
                "frame-response-instance-config-get-fake.json",
            ))
            .expect("the fixture decodes"),
        )))],
        Call::InstanceChangeEndpoints(_) => vec![Step::Fail(two_holders(
            "instance.endpoints_require_override",
            "an active environment lease needs an override to change the ports of the instance",
        ))],
        _ => Vec::new(),
    });

    let json = run(
        root.path(),
        &[
            "instance",
            "endpoint",
            "set",
            ID,
            "--port",
            "main=5556",
            "--json",
        ],
    );

    assert_eq!(json.code, Some(3), "{json:?}");
    assert_eq!(
        envelope(&json)["error"]["code"],
        "instance.endpoints_require_override"
    );
    assert!(!json.stderr.contains("is used by"), "{}", json.stderr);
}

// covers: AC-19 (spec 0047): the closing line names only environments whose
// lease is still active
#[test]
fn a_stop_with_the_override_names_no_environment_whose_lease_is_gone() {
    let root = TestRoot::new().expect("a test root is made");
    let stopped = data::edited(|value| {
        value["lifecycle"] = "stopped".into();
        value["desired"] = "stopped".into();
        value["process"] = Value::Null;
        value["readiness"]["state"] = "unknown".into();
        value["environments"] = json!([
            {
                "environmentId": "0192f3a7-0000-7000-8000-0000000000e1",
                "name": "erp", "alias": "db", "ownership": "shared",
                "leaseActive": true,
            },
            {
                "environmentId": "0192f3a7-0000-7000-8000-0000000000e2",
                "name": "shop", "alias": "cache", "ownership": "dedicated",
                "leaseActive": false,
            },
        ]);
    });
    let stopping = data::scripted(
        Reply::InstanceStop(Box::new(data::accepted(
            data::operation(OperationKind::StopInstance, OperationState::Queued, 1),
            false,
        ))),
        vec![data::operation(
            OperationKind::StopInstance,
            OperationState::Succeeded,
            2,
        )],
        Some(stopped),
    );
    let _daemon = ScriptedDaemon::serve(&root, stopping);

    let text = run(
        root.path(),
        &["instance", "stop", ID, "--override-leases", "--wait"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stdout.contains("erp is no longer complete."),
        "{}",
        text.stdout
    );
    assert!(!text.stdout.contains("shop"), "{}", text.stdout);
}
