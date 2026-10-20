//! AC-6, AC-8: `instance start` and `stop` send one call, answer
//! `alreadyInState` with exit 0, follow the operation with `--wait`, and
//! fill `instance` with one `instance.get` once nothing is left to wait
//! for.

use std::time::Duration;

use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{detail, dto, envelope, golden, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";
const KEY: &str = "start-demo-0001";

fn op(
    kind: OperationKind,
    state: OperationState,
    revision: u64,
) -> stackly_protocol::operation::OperationSummary {
    data::operation(kind, state, revision)
}

fn start_reply(state: OperationState, replayed: bool) -> Reply {
    Reply::InstanceStart(Box::new(data::accepted(
        op(OperationKind::StartInstance, state, 1),
        replayed,
    )))
}

fn stop_reply(state: OperationState) -> Reply {
    Reply::InstanceStop(Box::new(data::accepted(
        op(OperationKind::StopInstance, state, 1),
        false,
    )))
}

fn gets(daemon: &ScriptedDaemon) -> usize {
    daemon
        .methods()
        .iter()
        .filter(|method| *method == "instance.get")
        .count()
}

fn unused() -> Option<stackly_protocol::instance::InstanceSummary> {
    None
}

fn stopped() -> stackly_protocol::instance::InstanceSummary {
    data::edited(|value| {
        value["lifecycle"] = "stopped".into();
        value["desired"] = "stopped".into();
        value["process"] = serde_json::Value::Null;
        value["readiness"]["state"] = "unknown".into();
    })
}

/// Spec 0014 AC-6.
#[test]
fn start_and_stop_send_the_instance_id_in_lower_case_and_never_override_leases() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "start sends instance.start { instanceId } and stop sends instance.stop { instanceId, overrideLeases: false } with an upper case id written in lower case, each with the key given",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceStart(_) => vec![Step::Reply(start_reply(OperationState::Queued, false))],
        Call::InstanceStop(_) => vec![Step::Reply(stop_reply(OperationState::Queued))],
        _ => Vec::new(),
    });
    let upper = ID.to_uppercase();

    let start = run(
        root.path(),
        &[
            "instance",
            "start",
            &upper,
            "--idempotency-key",
            KEY,
            "--json",
        ],
    );
    let stop = run(root.path(), &["instance", "stop", &upper, "--json"]);

    assert_eq!(start.code, Some(0), "{start:?}");
    assert_eq!(stop.code, Some(0), "{stop:?}");
    let log = daemon.log();
    let sent = |method: &str| {
        log.iter()
            .find(|logged| logged.method == method)
            .unwrap_or_else(|| panic!("{method} was sent"))
            .clone()
    };
    let (started, stopped) = (sent("instance.start"), sent("instance.stop"));
    assert_eq!(started.idempotency_key.as_deref(), Some(KEY));
    assert_eq!(started.raw["call"]["content"]["instanceId"], ID);
    assert_eq!(stopped.raw["call"]["content"]["instanceId"], ID);
    assert_eq!(stopped.raw["call"]["content"]["overrideLeases"], false);
    assert_eq!(envelope(&start)["result"]["idempotencyKey"], KEY);
    assert_eq!(
        envelope(&stop)["result"]["idempotencyKey"],
        stopped.idempotency_key.as_deref().unwrap()
    );
    ev.pass();
}

/// Spec 0014 AC-6, AC-8.
#[test]
fn start_without_wait_prints_the_operation_and_a_hint_and_reads_no_instance() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            start_reply(OperationState::Queued, false),
            Vec::new(),
            unused(),
        ),
    );

    let text = run(root.path(), &["instance", "start", ID]);
    let json = run(root.path(), &["instance", "start", ID, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        format!("operation {} queued\n", dto::OPERATION)
    );
    assert_eq!(
        text.stderr,
        format!(
            "hint: wait with: stackly operation inspect {} --wait\n",
            dto::OPERATION
        )
    );
    assert_eq!(
        envelope(&json)["result"]["instance"],
        serde_json::Value::Null
    );
    assert_eq!(gets(&daemon), 0);
}

/// Spec 0014 AC-6.
#[test]
fn already_in_state_exits_0_with_a_null_operation_and_reads_the_instance_once() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "an alreadyInState answer exits 0 with no operation, a key in the result, already <lifecycle> in text, and one instance.get, with and without --wait and also when that read fails",
    );
    for (wait, read, expected) in [
        (false, true, "already running\n"),
        (true, true, "already running\n"),
        (true, false, "already running\n"),
    ] {
        let root = TestRoot::new().expect("a test root is made");
        let instance = if read { Some(data::instance("")) } else { None };
        let daemon = ScriptedDaemon::serve(
            &root,
            data::scripted(
                Reply::InstanceStart(Box::new(data::already())),
                Vec::new(),
                instance,
            ),
        );
        let mut args = vec!["instance", "start", ID];
        if wait {
            args.push("--wait");
        }
        let mut json_args = args.clone();
        json_args.push("--json");

        let text = run(root.path(), &args);
        let json = run(root.path(), &json_args);

        assert_eq!(text.code, Some(0), "{wait} {read}: {text:?}");
        assert_eq!(text.stdout, expected);
        assert_eq!(text.stderr, "", "no hint: there is nothing to wait for");
        assert_eq!(json.code, Some(0), "{wait} {read}: {json:?}");
        let value = envelope(&json);
        assert_eq!(value["result"]["operation"], serde_json::Value::Null);
        let key = value["result"]["idempotencyKey"].as_str().unwrap();
        assert_eq!(uuid::Uuid::try_parse(key).unwrap().get_version_num(), 7);
        assert_eq!(
            value["result"]["instance"].is_null(),
            !read,
            "{wait} {read}"
        );
        assert_eq!(gets(&daemon), 2, "one instance.get per run");
        assert!(!daemon.methods().contains(&"events.subscribe".to_owned()));
    }
    ev.pass();
}

/// Spec 0014 AC-6.
#[test]
fn start_and_stop_with_wait_write_one_line_and_golden_json() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "start --wait and stop --wait follow the operation to succeeded, read the instance once, and write <name> <lifecycle>, readiness <state> with the instance in the result",
    );
    let root = TestRoot::new().expect("a test root is made");
    let started = data::scripted(
        start_reply(OperationState::Queued, false),
        vec![
            op(OperationKind::StartInstance, OperationState::Running, 2),
            op(OperationKind::StartInstance, OperationState::Succeeded, 3),
        ],
        Some(data::instance("")),
    );
    let daemon = ScriptedDaemon::serve(&root, started);
    let mut args = vec![
        "instance",
        "start",
        ID,
        "--wait",
        "--timeout",
        "60s",
        "--idempotency-key",
        KEY,
    ];
    let text = run(root.path(), &args);
    args.push("--json");
    let json = run(root.path(), &args);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "orders-db running, readiness ready\n");
    golden::check("instance_start.txt", &text.stdout);
    golden::check("instance_start.json", &json.stdout);
    assert_eq!(envelope(&json)["result"]["operation"]["state"], "succeeded");
    assert_eq!(gets(&daemon), 2);
    drop(daemon);

    let stopping = data::scripted(
        stop_reply(OperationState::Queued),
        vec![
            op(OperationKind::StopInstance, OperationState::Running, 2),
            op(OperationKind::StopInstance, OperationState::Succeeded, 3),
        ],
        Some(stopped()),
    );
    let daemon = ScriptedDaemon::serve(&root, stopping);
    let mut args = vec![
        "instance",
        "stop",
        ID,
        "--wait",
        "--timeout",
        "60s",
        "--idempotency-key",
        "stop-demo-0001",
    ];
    let text = run(root.path(), &args);
    args.push("--json");
    let json = run(root.path(), &args);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "orders-db stopped, readiness unknown\n");
    golden::check("instance_stop.txt", &text.stdout);
    golden::check("instance_stop.json", &json.stdout);
    assert_eq!(gets(&daemon), 2);
    ev.pass();
}

/// Spec 0014 AC-6.
#[test]
fn a_start_that_fails_under_wait_keeps_the_failed_instance_in_the_result() {
    let root = TestRoot::new().expect("a test root is made");
    let mut ended = dto::ended(OperationState::Failed, true, 3);
    ended.resource = Some(stackly_protocol::ResourceRef {
        kind: stackly_protocol::ResourceKind::Instance,
        id: ID.to_owned(),
    });
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            start_reply(OperationState::Queued, false),
            vec![ended],
            Some(data::instance("failed")),
        ),
    );

    let json = run(root.path(), &["instance", "start", ID, "--wait", "--json"]);
    let text = run(root.path(), &["instance", "start", ID, "--wait"]);

    assert_eq!(json.code, Some(7), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "instance.start_failed");
    assert_eq!(value["result"]["instance"]["lifecycle"], "failed");
    assert_eq!(
        text.stdout,
        format!("operation {} failed\n", dto::OPERATION)
    );
    assert!(
        text.stderr.contains("code: instance.start_failed"),
        "{}",
        text.stderr
    );
    assert_eq!(gets(&daemon), 2);
}

/// Spec 0014 AC-6, AC-8.
#[test]
fn a_wait_that_ran_out_of_time_reads_no_instance() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            start_reply(OperationState::Queued, false),
            vec![op(OperationKind::StartInstance, OperationState::Running, 2)],
            Some(data::instance("")),
        ),
    );

    let json = run(
        root.path(),
        &[
            "instance",
            "start",
            ID,
            "--wait",
            "--timeout",
            "1s",
            "--json",
        ],
    );

    assert_eq!(json.code, Some(8), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.wait_timeout");
    assert_eq!(value["result"]["instance"], serde_json::Value::Null);
    assert_eq!(value["result"]["operation"]["state"], "running");
    assert_eq!(gets(&daemon), 0);
}

/// Spec 0014 AC-6.
#[test]
fn a_synchronous_error_exits_by_its_category_and_reads_no_instance() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceStart(_) => vec![Step::Fail(error(
            "resource.not_found",
            "not_found",
            "no instance has this id",
        ))],
        Call::InstanceStop(_) => vec![Step::Fail(error(
            "supervisor.instance_retained",
            "conflict",
            "the instance is retained",
        ))],
        _ => Vec::new(),
    });

    let start = run(root.path(), &["instance", "start", ID, "--json"]);
    let stop = run(root.path(), &["instance", "stop", ID, "--json"]);

    assert_eq!(start.code, Some(10), "{start:?}");
    assert_eq!(stop.code, Some(3), "{stop:?}");
    assert_eq!(envelope(&start)["error"]["code"], "resource.not_found");
    assert_eq!(envelope(&stop)["result"], serde_json::Value::Null);
    assert_eq!(gets(&daemon), 0);
}

/// Spec 0014 AC-8.
#[test]
fn a_request_that_ran_out_of_time_names_the_key_to_retry_with() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "an instance.start the daemon does not answer in time fails ipc.call_timeout with the key in its details and the text hint retry safely with: --idempotency-key <key>",
    );
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceStart(_) => vec![Step::Sleep(Duration::from_secs(12))],
        _ => Vec::new(),
    });

    let json = run(
        root.path(),
        &["instance", "start", ID, "--idempotency-key", KEY, "--json"],
    );
    let text = run(
        root.path(),
        &["instance", "start", ID, "--idempotency-key", KEY],
    );

    assert_eq!(json.code, Some(8), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "ipc.call_timeout");
    assert_eq!(
        detail(&value["error"], "idempotencyKey").as_deref(),
        Some(KEY)
    );
    assert!(
        text.stderr.contains(&format!(
            "hint: retry safely with: --idempotency-key {KEY}\n"
        )),
        "{}",
        text.stderr
    );
    ev.pass();
}

/// Spec 0014 AC-6, AC-8.
#[test]
fn a_bad_instance_id_or_key_exits_2_without_connecting() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            start_reply(OperationState::Queued, false),
            Vec::new(),
            unused(),
        ),
    );
    for args in [
        &["instance", "start", "orders-db"][..],
        &["instance", "start", "0192f3a5"],
        &["instance", "stop", "0192f3a500007000800000000000000001"],
        &["instance", "start", ID, "--idempotency-key", "a b"],
        &["instance", "start", ID, "--timeout", "5s"],
    ] {
        let mut all = args.to_vec();
        all.push("--json");
        let refused = run(root.path(), &all);
        assert_eq!(refused.code, Some(2), "{args:?}: {refused:?}");
        assert_eq!(envelope(&refused)["error"]["code"], "cli.usage");
    }
    assert_eq!(daemon.connections(), 0);
}

/// Spec 0014 AC-6.
#[test]
fn already_in_state_with_a_succeeded_no_op_operation_waits_for_nothing() {
    let root = TestRoot::new().expect("a test root is made");
    let mut reply = data::accepted(
        op(OperationKind::StartInstance, OperationState::Succeeded, 2),
        false,
    );
    reply.already_in_state = true;
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            Reply::InstanceStart(Box::new(reply)),
            Vec::new(),
            Some(data::instance("")),
        ),
    );

    let text = run(root.path(), &["instance", "start", ID, "--wait"]);
    let json = run(root.path(), &["instance", "start", ID, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "already running\n");
    assert_eq!(text.stderr, "");
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["result"]["operation"]["state"], "succeeded");
    assert_eq!(value["result"]["instance"]["lifecycle"], "running");
    assert_eq!(gets(&daemon), 2, "one instance.get per run");
    assert!(!daemon.methods().contains(&"events.subscribe".to_owned()));
    assert!(!daemon.methods().contains(&"operation.get".to_owned()));
}
