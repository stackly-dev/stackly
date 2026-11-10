//! Spec 0035 AC-6, AC-16 against a real `stacklyd` with the fake runtime:
//! `stackly audit list` finds the rows the daemon wrote for a create and a
//! start that failed because the test held its port, by instance name and
//! by operation, and `stackly settings` reads and changes how long the
//! trail is kept.

use serde_json::Value;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot, allocate_port};
use uuid::Uuid;

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, paths, run};
use crate::stacklyd::seed;

/// The rows of `audit list` with `args`, from its `--json` envelope.
fn rows(root: &TestRoot, args: &[&str]) -> Vec<Value> {
    let mut all = vec!["audit", "list"];
    all.extend_from_slice(args);
    all.push("--json");
    let listed = run(root.path(), &all);
    assert_eq!(listed.code, Some(0), "{args:?}: {listed:?}");
    envelope(&listed)["result"]["items"]
        .as_array()
        .expect("items is an array")
        .clone()
}

/// The `operation.finished` row of the operation of `kind` among `rows`.
fn finished<'a>(rows: &'a [Value], kind: &str) -> &'a Value {
    rows.iter()
        .find(|row| row["action"] == "operation.finished" && row["operationKind"] == kind)
        .unwrap_or_else(|| panic!("a finished {kind} row in {rows:#?}"))
}

/// The value of detail `key` of `row`.
fn detail<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row["details"]
        .as_array()?
        .iter()
        .find(|detail| detail["key"] == key)?["value"]
        .as_str()
}

// covers: AC-16 (spec 0035)
#[test]
fn the_trail_of_a_real_create_and_failed_start_reads_by_name_and_by_operation() {
    let ev = evidence!(
        covers = ["REQ-045", "AC-045"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, after instance create and an instance start that fails with supervisor.endpoint_in_use because the test holds the port, audit list --instance <name> has the succeeded create row with configRevision 1 and the failed start row whose operationId is the start's operation and whose errorCode is supervisor.endpoint_in_use, the text names both, --operation <id> has only that start row, a removed or unknown UUID gives an empty page, and an unknown name exits 10 with cli.instance_not_found",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let port = allocate_port().expect("a free port");
    let request = format!("http={port}");

    let created = run(
        root.path(),
        &[
            "instance",
            "create",
            "audited",
            "--runtime",
            &runtime,
            "--provider",
            "testing/fake",
            "--revision",
            "0.4.0",
            "--port",
            &request,
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(created.code, Some(0), "{created:?}");
    let created = envelope(&created);
    let create_operation = created["result"]["operation"]["id"]
        .as_str()
        .expect("the create operation")
        .to_owned();
    let instance = created["result"]["instance"]["id"]
        .as_str()
        .expect("the instance ID")
        .to_owned();
    // Create checks the port too, so the test takes it only afterwards.
    let held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .expect("the test holds the port");
    let started = run(
        root.path(),
        &[
            "instance",
            "start",
            "audited",
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(started.code, Some(3), "{started:?}");
    let started = envelope(&started);
    assert_eq!(started["error"]["code"], "supervisor.endpoint_in_use");
    let start_operation = started["error"]["operationId"]
        .as_str()
        .expect("the failed start names its operation")
        .to_owned();
    drop(held);

    let by_name = rows(&root, &["--instance", "audited"]);
    let by_id = rows(&root, &["--instance", &instance]);
    let by_operation = rows(&root, &["--operation", &start_operation]);
    let text = run(root.path(), &["audit", "list", "--instance", "audited"]);
    let unknown_id = rows(&root, &["--instance", &Uuid::now_v7().to_string()]);
    let unknown_name = run(
        root.path(),
        &["audit", "list", "--instance", "nope", "--json"],
    );

    assert_eq!(by_name, by_id, "a name finds the same rows as its ID");
    let create = finished(&by_name, "create_instance");
    assert_eq!(create["operationId"], create_operation.as_str());
    assert_eq!(create["result"], "succeeded");
    assert_eq!(create["configRevision"], 1);
    assert_eq!(create["actor"], "user");
    assert_eq!(create["client"], "cli");
    assert_eq!(create["instanceId"], instance.as_str());
    let start = finished(&by_name, "start_instance");
    assert_eq!(start["operationId"], start_operation.as_str());
    assert_eq!(start["result"], "failed");
    assert_eq!(
        detail(start, "errorCode"),
        Some("supervisor.endpoint_in_use")
    );
    assert!(
        by_name
            .iter()
            .position(|row| row == start)
            .expect("the start row")
            < by_name
                .iter()
                .position(|row| row == create)
                .expect("the create row"),
        "newest first"
    );
    assert_eq!(by_operation, vec![start.clone()], "{by_operation:#?}");

    assert_eq!(text.code, Some(0), "{text:?}");
    let line = text
        .stdout
        .lines()
        .find(|line| line.contains(&start_operation))
        .unwrap_or_else(|| panic!("the start line in {}", text.stdout));
    assert!(line.contains("user (cli)"), "{line}");
    assert!(
        line.contains("operation.finished (start_instance)"),
        "{line}"
    );
    assert!(line.contains("failed"), "{line}");
    assert!(
        line.contains("errorCode=supervisor.endpoint_in_use"),
        "{line}"
    );
    let create_line = text
        .stdout
        .lines()
        .find(|line| line.contains(&create_operation))
        .unwrap_or_else(|| panic!("the create line in {}", text.stdout));
    assert!(create_line.contains("configRevision=1"), "{create_line}");

    assert!(unknown_id.is_empty(), "{unknown_id:#?}");
    assert_eq!(unknown_name.code, Some(10), "{unknown_name:?}");
    assert_eq!(
        envelope(&unknown_name)["error"]["code"],
        "cli.instance_not_found"
    );
    assert_eq!(daemon.stop().expect("the daemon stops").code(), Some(0));
    ev.pass();
}

// covers: AC-6 (spec 0035)
#[test]
fn the_audit_retention_reads_and_changes_through_settings_against_a_real_daemon() {
    let ev = evidence!(
        covers = ["REQ-045"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, settings get shows audit retention: 30 days, up to 100000 entries; settings set audit-retention-days 7 and then 90 change it and the JSON carries auditRetention; 14 exits 2 with cli.usage and changes nothing",
    );
    let root = TestRoot::new().expect("a test root");
    let daemon = start_daemon(&root);

    let fresh = run(root.path(), &["settings", "get"]);
    let seven = run(
        root.path(),
        &["settings", "set", "audit-retention-days", "7"],
    );
    let ninety = run(
        root.path(),
        &["settings", "set", "audit-retention-days", "90", "--json"],
    );
    let refused = run(
        root.path(),
        &["settings", "set", "audit-retention-days", "14", "--json"],
    );
    let after = run(root.path(), &["settings", "get", "--json"]);

    assert_eq!(fresh.code, Some(0), "{fresh:?}");
    assert!(
        fresh
            .stdout
            .contains("audit retention    30 days, up to 100000 entries\n"),
        "{}",
        fresh.stdout
    );
    assert_eq!(seven.code, Some(0), "{seven:?}");
    assert!(
        seven
            .stdout
            .contains("audit retention    7 days, up to 100000 entries\n"),
        "{}",
        seven.stdout
    );
    assert_eq!(ninety.code, Some(0), "{ninety:?}");
    assert_eq!(
        envelope(&ninety)["result"]["auditRetention"],
        serde_json::json!({ "days": 90, "maxRows": 100_000 })
    );
    assert_eq!(refused.code, Some(2), "{refused:?}");
    assert_eq!(envelope(&refused)["error"]["code"], "cli.usage");
    assert_eq!(envelope(&after)["result"]["auditRetention"]["days"], 90);
    assert_eq!(daemon.stop().expect("the daemon stops").code(), Some(0));
    ev.pass();
}
