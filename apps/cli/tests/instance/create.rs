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
        [
            "daemon.handshake",
            "instance.get_create_form",
            "instance.create"
        ],
        "only the form is read before the create"
    );
    assert_eq!(daemon.log()[2].idempotency_key.as_deref(), Some(KEY));
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
            && result.contains(
                ",\"idempotencyKey\":\"create-demo-0001\",\"instance\":null,\"start\":null}"
            ),
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
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.get_create_form",
            "instance.create"
        ]
    );
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

    let key = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "instance.create")
        .and_then(|logged| logged.idempotency_key)
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
    let first_run: Vec<&str> = methods.iter().take(6).map(String::as_str).collect();
    assert_eq!(
        first_run,
        [
            "daemon.handshake",
            "instance.get_create_form",
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
    let bad: [&[&str]; 14] = [
        &["--runtime", "not-a-uuid"],
        &["--runtime", "0192f3a5"],
        &["--runtime", "@18"],
        &["--runtime", "postgresql@"],
        &["--settings-file", "/nonexistent/stackly-settings.json"],
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
    // The runtime is the one required flag; the provider and the revision
    // are optional now.
    let refused = run(root.path(), &["instance", "create", "demo"]);
    assert_eq!(refused.code, Some(2), "{refused:?}");
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
        Call::InstanceGetCreateForm(_) => vec![Step::Reply(Reply::InstanceGetCreateForm(
            Box::new(data::create_form()),
        ))],
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

fn forms_only(
    preflight: stackly_protocol::instance::CreatePreflight,
) -> impl Fn(&Call, crate::common::scripted::Ctx) -> Vec<Step> + Send + Sync + 'static {
    move |call, _| match call {
        Call::InstanceGetCreateForm(_) => vec![Step::Reply(Reply::InstanceGetCreateForm(
            Box::new(data::create_form()),
        ))],
        Call::InstancePreflight(_) => vec![Step::Reply(Reply::InstancePreflight(Box::new(
            preflight.clone(),
        )))],
        _ => Vec::new(),
    }
}

fn logged_params(daemon: &ScriptedDaemon, method: &str) -> Vec<serde_json::Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == method)
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

/// Spec 0020 AC-18.
#[test]
fn dry_run_reads_the_form_asks_for_the_preflight_and_creates_nothing() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "create --dry-run sends instance.get_create_form then instance.preflight and nothing else, the secret sources go as plans without a value, the rows print as ok, warn, or fail with the command last, and a ready preflight exits 0",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, forms_only(data::preflight_ready()));
    let mut args = base();
    args.extend([
        "--dry-run",
        "--start",
        "--port",
        "postgres=5432",
        "--secret",
        "superuserPassword=stdin",
        "--secret",
        "auditPassword=copy:local:erp/db/audit",
    ]);

    let text = run(root.path(), &args);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.get_create_form",
            "instance.preflight"
        ]
    );
    let sent = &logged_params(&daemon, "instance.preflight")[0];
    assert_eq!(sent["start"], true);
    assert_eq!(
        sent["secrets"],
        json!([
            { "field": "superuserPassword", "source": { "type": "value" } },
            { "field": "auditPassword", "source": { "type": "copyFrom", "content": "local:erp/db/audit" } },
        ])
    );
    assert!(
        text.stdout
            .contains("ok   Runtime PostgreSQL 18.6: Verified"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("warn Endpoint 127.0.0.1:5432: Also in use on [::1]:5432"),
        "{}",
        text.stdout
    );
    assert!(text.stdout.contains("ready to create\n"));
    golden::check("instance_create_dry_run.txt", &text.stdout);
    assert!(
        text.stdout
            .trim_end()
            .ends_with("--secret superuserPassword=generate"),
        "the command comes last: {}",
        text.stdout
    );

    let mut json_args = base();
    json_args.extend(["--dry-run", "--json"]);
    let json = run(root.path(), &json_args);
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "instance create");
    assert_eq!(value["result"]["preflight"]["ready"], true);
    assert!(value["result"]["operation"].is_null());
    golden::check("instance_create_dry_run.json", &json.stdout);
    ev.pass();
}

/// Spec 0020 AC-18.
#[test]
fn a_dry_run_that_is_not_ready_exits_by_the_category_of_its_first_error_row() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "a preflight that is not ready exits with the exit code of the category of its first error row, prints that row as fail, and in json carries both the preflight and the error",
    );
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, forms_only(data::preflight_blocked()));
    let mut args = base();
    args.push("--dry-run");

    let text = run(root.path(), &args);

    assert_eq!(text.code, Some(3), "conflict exits 3: {text:?}");
    assert!(
        text.stdout
            .contains("fail Endpoint 127.0.0.1:5432: In use by orders-db-2"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("endpoints[0].port: "),
        "{}",
        text.stdout
    );
    assert!(text.stdout.contains("not ready to create\n"));
    golden::check("instance_create_dry_run_blocked.txt", &text.stdout);

    let mut json_args = args.clone();
    json_args.push("--json");
    let json = run(root.path(), &json_args);
    assert_eq!(json.code, Some(3), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "supervisor.endpoint_conflict");
    assert_eq!(value["result"]["preflight"]["ready"], false);
    ev.pass();
}

/// Spec 0020 AC-18.
#[test]
fn a_runtime_selector_picks_the_exact_version_first_then_the_line() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "--runtime software@x picks the installed runtime whose version is x, else the one whose line is x; none is cli.runtime_not_found with exit 10, more than one is cli.runtime_ambiguous with exit 3, and neither calls the form",
    );
    let root = TestRoot::new().expect("a test root is made");
    let mut page = data::runtimes();
    let mut other = page.items[0].clone();
    other.id = uuid::Uuid::from_u128(0x0192_f3a6_0000_7000_8000_0000_0000_00a3);
    other.version = "18.5".to_owned();
    page.items.push(other);
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::RuntimeList(_) => vec![Step::Reply(Reply::RuntimeList(Box::new(page.clone())))],
        other => forms_only(data::preflight_ready())(other, ctx),
    });
    let dry = |runtime: &str| {
        run(
            root.path(),
            &[
                "instance",
                "create",
                "demo",
                "--runtime",
                runtime,
                "--dry-run",
                "--json",
            ],
        )
    };

    let exact = dry("postgresql@18.6");
    assert_eq!(exact.code, Some(0), "{exact:?}");
    let by_line = dry("nats@2.15");
    assert_eq!(by_line.code, Some(0), "{by_line:?}");
    let forms = logged_params(&daemon, "instance.get_create_form");
    assert_eq!(
        forms[0]["runtimeId"],
        "0192f3a6-0000-7000-8000-0000000000a1"
    );
    assert_eq!(
        forms[1]["runtimeId"],
        "0192f3a6-0000-7000-8000-0000000000a2"
    );

    let none = dry("mysql@8");
    assert_eq!(none.code, Some(10), "{none:?}");
    assert_eq!(envelope(&none)["error"]["code"], "cli.runtime_not_found");
    assert_eq!(envelope(&none)["error"]["details"][0]["value"], "mysql@8");

    let ambiguous = dry("postgresql@18");
    assert_eq!(ambiguous.code, Some(3), "{ambiguous:?}");
    let error = &envelope(&ambiguous)["error"];
    assert_eq!(error["code"], "cli.runtime_ambiguous");
    let candidates = error["details"]
        .as_array()
        .and_then(|details| details.iter().find(|detail| detail["key"] == "candidates"))
        .map(|detail| detail["value"].as_str().unwrap_or_default().to_owned())
        .unwrap_or_default();
    assert!(
        candidates.contains("0192f3a6-0000-7000-8000-0000000000a1")
            && candidates.contains("0192f3a6-0000-7000-8000-0000000000a3"),
        "{candidates}"
    );
    assert_eq!(
        logged_params(&daemon, "instance.get_create_form").len(),
        2,
        "a selector that failed never reached the form"
    );
    ev.pass();
}

/// Spec 0020 AC-18.
#[test]
fn a_set_value_reads_as_the_kind_the_form_gives_its_field_and_the_file_comes_first() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "--set reads a number for an integer field, true or false for a boolean, and text otherwise; a value that does not read as its kind goes as text for the daemon to refuse; --settings-file comes first and --set and --set-json win over it",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, forms_only(data::preflight_ready()));
    let file = root.path().join("settings.json");
    std::fs::write(
        &file,
        r#"{"maxConnections": 10, "fromFile": true, "sharedBuffersMb": 8}"#,
    )
    .expect("the settings file is written");
    let file = file.to_str().expect("a UTF-8 path").to_owned();

    let mut args = base();
    args.extend([
        "--dry-run",
        "--settings-file",
        &file,
        "--set",
        "maxConnections=50",
        "--set",
        "logStatement=ddl",
        "--set",
        "sharedBuffersMb=lots",
        "--set-json",
        "extra=[1]",
    ]);
    let result = run(root.path(), &args);

    assert_eq!(result.code, Some(0), "{result:?}");
    assert_eq!(
        logged_params(&daemon, "instance.preflight")[0]["settings"],
        json!({
            "maxConnections": 50,
            "fromFile": true,
            "sharedBuffersMb": "lots",
            "logStatement": "ddl",
            "extra": [1],
        })
    );
    ev.pass();
}

/// Spec 0020 AC-18.
#[test]
fn a_settings_file_that_is_not_a_json_object_exits_2_without_connecting() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, forms_only(data::preflight_ready()));
    for (name, content) in [("array.json", "[1,2]"), ("text.json", "not json")] {
        let file = root.path().join(name);
        std::fs::write(&file, content).expect("the file is written");
        let mut args = base();
        args.extend([
            "--settings-file",
            file.to_str().expect("a UTF-8 path"),
            "--json",
        ]);
        let refused = run(root.path(), &args);
        assert_eq!(refused.code, Some(2), "{name}: {refused:?}");
        assert_eq!(envelope(&refused)["error"]["code"], "cli.usage", "{name}");
    }
    assert_eq!(daemon.connections(), 0);
}

/// Spec 0020 AC-18.
#[test]
fn start_waits_for_the_create_then_starts_with_a_key_made_from_the_create_key() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "create --start waits for the create without --wait and then sends instance.start with the lowercase hex SHA-256 of the create key and :start as its key, and the result carries the start next to the create",
    );
    let root = TestRoot::new().expect("a test root is made");
    let script = data::scripted(
        accepted(OperationState::Queued, 1, false),
        vec![
            data::operation(OperationKind::CreateInstance, OperationState::Running, 2),
            data::operation(OperationKind::CreateInstance, OperationState::Succeeded, 3),
        ],
        Some(stopped()),
    );
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceStart(_) => {
            vec![Step::Reply(Reply::InstanceStart(Box::new(data::accepted(
                data::operation(OperationKind::StartInstance, OperationState::Queued, 1),
                false,
            ))))]
        }
        other => script(other, ctx),
    });
    let mut args = base();
    args.extend(["--start", "--idempotency-key", KEY, "--json"]);

    let json = run(root.path(), &args);

    assert_eq!(json.code, Some(0), "{json:?}");
    let methods = daemon.methods();
    let create = methods
        .iter()
        .position(|method| method == "instance.create")
        .expect("the create was sent");
    let start = methods
        .iter()
        .position(|method| method == "instance.start")
        .expect("the start was sent");
    assert!(create < start, "{methods:?}");
    assert!(
        methods[create..start]
            .iter()
            .any(|method| method == "operation.get"),
        "the create was waited for without --wait: {methods:?}"
    );
    let expected = {
        use sha2::{Digest, Sha256};
        Sha256::digest(format!("{KEY}:start").as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let started = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "instance.start")
        .expect("the start was logged");
    assert_eq!(started.idempotency_key.as_deref(), Some(expected.as_str()));
    let value = envelope(&json);
    assert_eq!(
        value["result"]["start"]["idempotencyKey"],
        expected.as_str()
    );
    assert!(value["result"]["start"]["operation"].is_object());
    assert_eq!(value["result"]["operation"]["state"], "succeeded");
    ev.pass();
}
