//! AC-5, AC-8: `instance create` sends one `instance.create` whose params
//! the flags decide, refuses a malformed argument before it connects, and
//! follows the operation with `--wait`.

use serde_json::json;
use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{dto, envelope, golden, run};
use crate::data::{self, RUNTIME};

const KEY: &str = "create-demo-0001";

fn accepted(state: OperationState, revision: u64, replayed: bool) -> Reply {
    Reply::InstanceCreate(Box::new(data::accepted(
        data::operation(OperationKind::CreateInstance, state, revision),
        replayed,
    )))
}

fn base() -> Vec<&'static str> {
    vec![
        "instance",
        "create",
        "demo",
        "--runtime",
        RUNTIME,
        "--provider",
        "fake",
        "--revision",
        "0.3.0",
    ]
}

fn stopped() -> stackly_protocol::instance::InstanceSummary {
    data::edited(|value| {
        value["name"] = "demo".into();
        value["lifecycle"] = "stopped".into();
        value["desired"] = "stopped".into();
        value["process"] = serde_json::Value::Null;
    })
}

fn failed_instance() -> stackly_protocol::instance::InstanceSummary {
    data::instance("failed")
}

fn params(daemon: &ScriptedDaemon) -> Vec<serde_json::Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "instance.create")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

/// Spec 0014 AC-5.
#[test]
fn create_sends_one_call_with_the_params_the_flags_map_to() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance create sends exactly one instance.create before anything else, the params follow the flag table (ports in order with duplicates, the last --set winning, --set-json winning over --set), the key is the one given, and the result has operation, idempotencyKey, and a null instance",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(accepted(OperationState::Queued, 1, false), Vec::new(), None),
    );
    let mut args = base();
    args.extend([
        "--display-name",
        "Demo 数据库",
        "--port",
        "http=auto",
        "--port",
        "admin=8080",
        "--port",
        "http=9",
        "--set",
        "mode=a",
        "--set",
        "mode=b",
        "--set",
        "kept=text",
        "--set-json",
        "readyDelayMs=1500",
        "--set-json",
        "kept={\"x\":1}",
        "--idempotency-key",
        KEY,
        "--json",
    ]);

    let json = run(root.path(), &args);

    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(
        daemon.methods(),
        ["daemon.handshake", "instance.create"],
        "no call comes before the create"
    );
    assert_eq!(daemon.log()[1].idempotency_key.as_deref(), Some(KEY));
    assert_eq!(
        params(&daemon)[0],
        json!({
            "name": "demo",
            "displayName": "Demo 数据库",
            "provider": { "name": "fake", "revision": "0.3.0" },
            "runtimeId": RUNTIME,
            "endpoints": [
                { "name": "http", "port": { "type": "auto" } },
                { "name": "admin", "port": { "type": "fixed", "content": 8080 } },
                { "name": "http", "port": { "type": "fixed", "content": 9 } },
            ],
            "settings": { "mode": "b", "kept": { "x": 1 }, "readyDelayMs": 1500 },
            "secrets": [],
        })
    );
    let value = envelope(&json);
    assert_eq!(value["command"], "instance create");
    assert_eq!(value["result"]["idempotencyKey"], KEY);
    assert_eq!(value["result"]["operation"]["state"], "queued");
    assert_eq!(value["result"]["instance"], serde_json::Value::Null);
    let line = json.stdout.trim_end();
    let result = &line[line.find("\"result\":").unwrap() + 9..];
    assert!(
        result.starts_with("{\"operation\":")
            && result.contains(",\"idempotencyKey\":\"create-demo-0001\",\"instance\":null}"),
        "the result keys come in the order of the spec: {result}"
    );
    ev.pass();
}

/// Spec 0014 AC-5, AC-8.
#[test]
fn create_without_wait_prints_the_instance_and_the_operation_with_a_hint() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(accepted(OperationState::Queued, 1, false), Vec::new(), None),
    );

    let text = run(root.path(), &base());

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        format!(
            "instance {}\noperation {} queued\n",
            data::INSTANCE,
            dto::OPERATION
        )
    );
    assert_eq!(
        text.stderr,
        format!(
            "hint: wait with: stackly operation inspect {} --wait\n",
            dto::OPERATION
        )
    );
    assert_eq!(daemon.methods(), ["daemon.handshake", "instance.create"]);
}

/// Spec 0014 AC-8.
#[test]
fn a_key_made_by_the_cli_is_a_lowercase_uuid_v7_sent_and_returned() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(accepted(OperationState::Queued, 1, false), Vec::new(), None),
    );
    let mut args = base();
    args.push("--json");

    let json = run(root.path(), &args);

    let key = daemon.log()[1]
        .idempotency_key
        .clone()
        .expect("a key is sent");
    assert_eq!(envelope(&json)["result"]["idempotencyKey"], key.as_str());
    assert_eq!(key, key.to_lowercase());
    let uuid = uuid::Uuid::try_parse(&key).expect("the key is a uuid");
    assert_eq!(uuid.get_version_num(), 7);
}

/// Spec 0014 AC-8.
#[test]
fn a_replayed_create_returns_the_same_operation_and_says_so_on_stderr() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(accepted(OperationState::Running, 3, true), Vec::new(), None),
    );
    let mut args = base();
    args.extend(["--idempotency-key", KEY]);

    let text = run(root.path(), &args);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stdout
            .ends_with(&format!("operation {} running\n", dto::OPERATION))
    );
    assert!(
        text.stderr.contains(
            "hint: the idempotency key matched an earlier request, so this is its operation\n"
        ),
        "{}",
        text.stderr
    );
}

/// Spec 0014 AC-5, AC-6.
#[test]
fn create_with_wait_follows_the_operation_then_reads_the_instance_once() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "create --wait follows the operation to succeeded, reads the instance with one instance.get, and writes created <name> (<id>) with the instance in the result",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            accepted(OperationState::Queued, 1, false),
            vec![
                data::operation(OperationKind::CreateInstance, OperationState::Running, 2),
                data::operation(OperationKind::CreateInstance, OperationState::Succeeded, 3),
            ],
            Some(stopped()),
        ),
    );
    let mut args = base();
    args.extend(["--wait", "--timeout", "60s", "--idempotency-key", KEY]);
    let mut json_args = args.clone();
    json_args.push("--json");

    let json = run(root.path(), &json_args);
    let text = run(root.path(), &args);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, format!("created demo ({})\n", data::INSTANCE));
    golden::check("instance_create.txt", &text.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("instance_create.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["result"]["operation"]["state"], "succeeded");
    assert_eq!(value["result"]["instance"]["name"], "demo");
    let methods = daemon.methods();
    let first_run: Vec<&str> = methods.iter().take(5).map(String::as_str).collect();
    assert_eq!(
        first_run,
        [
            "daemon.handshake",
            "instance.create",
            "events.subscribe",
            "operation.get",
            "operation.get"
        ],
        "{methods:?}"
    );
    assert_eq!(
        methods
            .iter()
            .filter(|method| *method == "instance.get")
            .count(),
        2,
        "one instance.get per run: {methods:?}"
    );
    ev.pass();
}

/// Spec 0014 AC-6, AC-8.
#[test]
fn a_create_that_ends_needs_recovery_exits_by_its_error_and_still_shows_the_instance() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a create that ends needs_recovery under --wait exits by the category of its error, reads the instance once, and has the failed instance in the result next to the error",
    );
    let root = TestRoot::new().expect("a test root is made");
    let mut ended = data::operation(
        OperationKind::CreateInstance,
        OperationState::NeedsRecovery,
        3,
    );
    ended.recovery = Some(
        dto::ended(OperationState::NeedsRecovery, true, 3)
            .recovery
            .unwrap(),
    );
    ended.recovery.as_mut().unwrap().error = Some(
        error(
            "supervisor.config_invalid",
            "failed",
            "the config does not render",
        )
        .with_hint("check the settings"),
    );
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            accepted(OperationState::Queued, 1, false),
            vec![ended],
            Some(failed_instance()),
        ),
    );
    let mut args = base();
    args.extend(["--wait", "--json"]);

    let json = run(root.path(), &args);

    assert_eq!(json.code, Some(7), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "supervisor.config_invalid");
    assert_eq!(value["result"]["operation"]["state"], "needs_recovery");
    assert_eq!(value["result"]["instance"]["lifecycle"], "failed");
    let reads = daemon
        .methods()
        .iter()
        .filter(|method| *method == "instance.get")
        .count();
    assert_eq!(reads, 1);
    ev.pass();
}

/// Spec 0014 AC-6.
#[test]
fn a_failed_read_after_the_wait_leaves_instance_null_and_the_operation_error_in_charge() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "when the one instance.get after a wait fails, instance is null and the exit code and error stay the operation's, and the read is not retried",
    );
    let root = TestRoot::new().expect("a test root is made");
    let mut ended = dto::ended(OperationState::Failed, true, 3);
    ended.kind = OperationKind::CreateInstance;
    ended.resource = Some(stackly_protocol::ResourceRef {
        kind: stackly_protocol::ResourceKind::Instance,
        id: data::INSTANCE.to_string(),
    });
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            accepted(OperationState::Queued, 1, false),
            vec![ended],
            None,
        ),
    );
    let mut args = base();
    args.extend(["--wait", "--json"]);

    let json = run(root.path(), &args);

    assert_eq!(json.code, Some(7), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "instance.start_failed");
    assert_eq!(value["result"]["instance"], serde_json::Value::Null);
    assert_eq!(value["result"]["operation"]["state"], "failed");
    assert_eq!(
        daemon
            .methods()
            .iter()
            .filter(|method| *method == "instance.get")
            .count(),
        1
    );
    ev.pass();
}

/// Spec 0014 AC-5.
#[test]
fn arguments_with_a_wrong_shape_exit_2_without_connecting() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a runtime that is not a uuid, a port or setting without =, a port outside 0 to 65535 or not a number, JSON that is not JSON, and a key outside the grammar exit 2 as cli.usage and never reach the daemon",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(accepted(OperationState::Queued, 1, false), Vec::new(), None),
    );
    let bad: [&[&str]; 11] = [
        &["--runtime", "not-a-uuid"],
        &["--runtime", "0192f3a5"],
        &["--port", "http"],
        &["--port", "http=65536"],
        &["--port", "http=eighty"],
        &["--port", "http=-1"],
        &["--set", "mode"],
        &["--set-json", "mode"],
        &["--set-json", "mode={nope"],
        &["--idempotency-key", "has space"],
        &["--idempotency-key", ""],
    ];
    for extra in bad {
        let mut args = vec![
            "instance",
            "create",
            "demo",
            "--provider",
            "fake",
            "--revision",
            "0.3.0",
        ];
        if !extra.contains(&"--runtime") {
            args.extend(["--runtime", RUNTIME]);
        }
        args.extend_from_slice(extra);
        args.push("--json");
        let refused = run(root.path(), &args);
        assert_eq!(refused.code, Some(2), "{extra:?}: {refused:?}");
        assert_eq!(
            envelope(&refused)["error"]["code"],
            "cli.usage",
            "{extra:?}"
        );
    }
    for missing in ["--runtime", "--provider", "--revision"] {
        let mut args = vec!["instance", "create", "demo"];
        for (flag, value) in [
            ("--runtime", RUNTIME),
            ("--provider", "fake"),
            ("--revision", "0.3.0"),
        ] {
            if flag != missing {
                args.extend([flag, value]);
            }
        }
        let refused = run(root.path(), &args);
        assert_eq!(refused.code, Some(2), "{missing}: {refused:?}");
    }
    assert_eq!(daemon.connections(), 0, "{:?}", daemon.methods());
    ev.pass();
}

/// Spec 0014 AC-5.
#[test]
fn port_zero_is_sent_for_the_daemon_to_refuse() {
    let root = TestRoot::new().expect("a test root is made");
    let refusal: ErrorBody = error(
        "request.invalid_params",
        "invalid_input",
        "endpoints[0].port is out of range",
    );
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceCreate(_) => vec![Step::Fail(refusal.clone())],
        _ => Vec::new(),
    });
    let mut args = base();
    args.extend(["--port", "http=0", "--json"]);

    let json = run(root.path(), &args);

    assert_eq!(json.code, Some(2), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "request.invalid_params");
    assert_eq!(
        params(&daemon)[0]["endpoints"],
        json!([{ "name": "http", "port": { "type": "fixed", "content": 0 } }])
    );
    assert_eq!(value["result"], serde_json::Value::Null);
}
