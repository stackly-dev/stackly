//! AC-11 (spec 0024): `instance duplicate` resolves its source, reads the
//! duplicate form, puts the flags over the draft, and then sends exactly
//! the `instance.preflight` or `instance.create` that `instance create`
//! sends, with the output, exit codes, `--start`, and keys of create.

use serde_json::{Value, json};
use stackly_protocol::instance::{InstancePage, InstanceSummary};
use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run};
use crate::data::{self, INSTANCE};

const KEY: &str = "duplicate-demo-0001";
/// The source of the duplicate form fixture, `pg17-dev`.
const SOURCE: &str = "0192f3a6-0000-7000-8000-0000000000b2";
const REFERENCE: &str = "local:instance/0192f3a6-0000-7000-8000-0000000000b2/superuser-password";
const LINE: &str = "Duplicating settings of pg17-dev; no data is copied.\n";

fn accepted(state: OperationState, revision: u64) -> Reply {
    Reply::InstanceCreate(Box::new(data::accepted(
        data::operation(OperationKind::CreateInstance, state, revision),
        false,
    )))
}

fn stopped() -> InstanceSummary {
    data::edited(|value| {
        value["name"] = "pg17-test".into();
        value["lifecycle"] = "stopped".into();
        value["desired"] = "stopped".into();
        value["process"] = Value::Null;
    })
}

fn params(daemon: &ScriptedDaemon, method: &str) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == method)
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

fn derived_start_key(key: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(format!("{key}:start").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn start(args: &[&'static str]) -> Vec<&'static str> {
    let mut all = vec!["instance", "duplicate"];
    all.extend(args);
    all
}

/// The default scripted daemon: create accepted at `state`, the duplicate
/// form of the fixture, and the operation playing `states`.
fn serve(
    root: &TestRoot,
    states: Vec<stackly_protocol::operation::OperationSummary>,
) -> ScriptedDaemon {
    ScriptedDaemon::serve(
        root,
        data::scripted(accepted(OperationState::Queued, 1), states, Some(stopped())),
    )
}

// covers: AC-11
#[test]
fn the_flags_go_over_the_draft_and_copy_takes_the_reference_the_source_uses() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-028"],
        kind = Kind::Fixture,
        expected = "duplicate reads the form of the source, then sends one instance.create whose params are the draft with the name, the port, the setting, and the copy secret the flags give; the runtime and the provider revision are the ones of the source, the other secrets are generated, and stderr says no data is copied",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = serve(&root, Vec::new());
    let file = root.path().join("settings.json");
    std::fs::write(&file, r#"{"logStatement":"all","sharedBuffersMb":256}"#)
        .expect("the settings file is written");

    let json = run(
        root.path(),
        &[
            "instance",
            "duplicate",
            SOURCE,
            "pg17-test",
            "--display-name",
            "Test",
            "--port",
            "client=5555",
            "--settings-file",
            file.to_str().expect("a utf 8 path"),
            "--set",
            "maxConnections=300",
            "--secret",
            "superuserPassword=copy",
            "--idempotency-key",
            KEY,
            "--json",
        ],
    );

    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(json.stderr, LINE, "{json:?}");
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.get_duplicate_form",
            "instance.create"
        ],
        "an ID needs no lookup"
    );
    assert_eq!(
        params(&daemon, "instance.get_duplicate_form"),
        [json!({ "instanceId": SOURCE })]
    );
    assert_eq!(daemon.log()[2].idempotency_key.as_deref(), Some(KEY));
    assert_eq!(
        params(&daemon, "instance.create")[0],
        json!({
            "name": "pg17-test",
            "displayName": "Test",
            "provider": { "name": "official/postgresql", "revision": "1.0.0" },
            "runtimeId": "0192f3a6-0000-7000-8000-0000000000a1",
            "endpoints": [{ "name": "client", "port": { "type": "fixed", "content": 5555 } }],
            "settings": {
                "logMinDurationStatementMs": -1,
                "logStatement": "all",
                "maxConnections": 300,
                "sharedBuffersMb": 256
            },
            "secrets": [{
                "field": "superuserPassword",
                "input": { "type": "copyFrom", "content": REFERENCE }
            }]
        })
    );
    let value = envelope(&json);
    assert_eq!(
        value["result"]["source"],
        json!({ "id": SOURCE, "name": "pg17-dev" })
    );
    assert_eq!(value["result"]["idempotencyKey"], KEY);
    ev.pass();
}

// covers: AC-11
#[test]
fn without_a_name_the_draft_name_is_used_and_the_secrets_are_generated() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = serve(&root, Vec::new());

    let text = run(root.path(), &start(&[SOURCE]));

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(text.stderr.starts_with(LINE), "{}", text.stderr);
    let create = &params(&daemon, "instance.create")[0];
    assert_eq!(create["name"], "pg17-dev-copy");
    assert_eq!(
        create["endpoints"],
        json!([{ "name": "client", "port": { "type": "auto" } }]),
        "the Fixed port of the source is never copied"
    );
    assert_eq!(
        create["secrets"],
        json!([{ "field": "superuserPassword", "input": { "type": "generate" } }])
    );
    assert_eq!(
        create["settings"],
        json!({
            "logMinDurationStatementMs": -1,
            "logStatement": "ddl",
            "maxConnections": 200,
            "sharedBuffersMb": 128
        })
    );
}

// covers: AC-11
#[test]
fn a_name_that_is_not_an_id_is_looked_up_in_the_instance_list() {
    let root = TestRoot::new().expect("a test root is made");
    let page = data::list_mixed();
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: page.items.clone(),
            next: None,
        })))],
        other => data::scripted(accepted(OperationState::Queued, 1), Vec::new(), None)(other, ctx),
    });

    let found = run(root.path(), &start(&["analytics-db", "--json"]));
    let missing = run(root.path(), &start(&["gamma", "--json"]));

    assert_eq!(found.code, Some(0), "{found:?}");
    assert_eq!(
        params(&daemon, "instance.get_duplicate_form"),
        [json!({ "instanceId": "0192f3a5-0000-7000-8000-000000000002" })]
    );
    assert_eq!(missing.code, Some(10), "{missing:?}");
    let value = envelope(&missing);
    assert_eq!(value["error"]["code"], "cli.instance_not_found");
    assert!(
        missing.stderr.is_empty(),
        "no line before the source is known: {missing:?}"
    );
    assert_eq!(
        params(&daemon, "instance.get_duplicate_form").len(),
        1,
        "an unknown name never asks for a form"
    );
}

// covers: AC-11
#[test]
fn copy_for_a_field_the_source_has_no_reference_for_is_a_usage_error_before_any_create() {
    let root = TestRoot::new().expect("a test root is made");
    let mut form = data::duplicate_form();
    form.secrets[0].source_reference = None;
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceGetDuplicateForm(_) => vec![Step::Reply(Reply::InstanceGetDuplicateForm(
            Box::new(form.clone()),
        ))],
        other => data::scripted(accepted(OperationState::Queued, 1), Vec::new(), None)(other, ctx),
    });

    let refused = run(
        root.path(),
        &start(&[SOURCE, "--secret", "superuserPassword=copy", "--json"]),
    );

    assert_eq!(refused.code, Some(2), "{refused:?}");
    assert_eq!(envelope(&refused)["error"]["code"], "cli.usage");
    assert_eq!(
        daemon.methods(),
        ["daemon.handshake", "instance.get_duplicate_form"],
        "nothing is created"
    );
}

// covers: AC-11
#[test]
fn no_flag_picks_the_runtime_or_the_provider_and_clap_refuses_each_before_connecting() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = serve(&root, Vec::new());

    for flag in [
        ["--runtime", "postgresql@17"],
        ["--provider", "official/postgresql"],
        ["--revision", "1.0.0"],
    ] {
        let mut args = start(&[SOURCE]);
        args.extend(flag);
        let refused = run(root.path(), &args);
        assert_eq!(refused.code, Some(2), "{flag:?}: {refused:?}");
        assert!(refused.stdout.is_empty());
    }
    assert!(daemon.methods().is_empty(), "the daemon was never reached");
}

// covers: AC-11
#[test]
fn dry_run_sends_only_the_preflight_of_the_draft_and_prints_it_like_create() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-028"],
        kind = Kind::Fixture,
        expected = "duplicate --dry-run sends one instance.preflight with the draft and the plans of the secrets, creates nothing, and prints the rows of create with the source in the JSON",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = serve(&root, Vec::new());

    let text = run(
        root.path(),
        &start(&[SOURCE, "pg17-test", "--port", "client=auto", "--dry-run"]),
    );
    let json = run(
        root.path(),
        &start(&[
            SOURCE,
            "pg17-test",
            "--secret",
            "superuserPassword=copy",
            "--start",
            "--dry-run",
            "--json",
        ]),
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("instance_duplicate_dry_run.txt", &text.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("instance_duplicate_dry_run.json", &json.stdout);
    assert_eq!(text.stderr, LINE);
    assert_eq!(json.stderr, LINE, "the line is printed with --json too");
    let sent = params(&daemon, "instance.preflight");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1]["start"], true);
    assert_eq!(
        sent[1]["secrets"],
        json!([{ "field": "superuserPassword", "source": { "type": "copyFrom", "content": REFERENCE } }])
    );
    assert!(params(&daemon, "instance.create").is_empty());
    let value = envelope(&json);
    assert_eq!(
        value["result"]["source"],
        json!({ "id": SOURCE, "name": "pg17-dev" })
    );
    assert!(value["result"]["preflight"]["ready"].as_bool().is_some());
    ev.pass();
}

// covers: AC-11
#[test]
fn a_blocked_preflight_exits_by_its_first_error_like_create() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstancePreflight(_) => vec![Step::Reply(Reply::InstancePreflight(Box::new(
            data::preflight_blocked(),
        )))],
        other => data::scripted(accepted(OperationState::Queued, 1), Vec::new(), None)(other, ctx),
    });

    let blocked = run(root.path(), &start(&[SOURCE, "--dry-run", "--json"]));

    assert_eq!(blocked.code, Some(3), "{blocked:?}");
    let value = envelope(&blocked);
    assert_eq!(value["error"]["code"], "supervisor.endpoint_conflict");
    assert!(value["result"]["preflight"].is_object());
    assert!(value["result"]["source"].is_object());
}

// covers: AC-11
#[test]
fn start_waits_for_the_create_then_starts_with_the_key_derived_from_the_create_key() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-028"],
        kind = Kind::Fixture,
        expected = "duplicate --start --wait follows the create to succeeded, reads the instance, sends instance.start with the SHA-256 of <key>:start as its key, and the JSON carries operation, idempotencyKey, instance, start, and source",
    );
    let root = TestRoot::new().expect("a test root is made");
    let script = data::scripted(
        accepted(OperationState::Queued, 1),
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

    let json = run(
        root.path(),
        &start(&[
            SOURCE,
            "pg17-test",
            "--start",
            "--wait",
            "--timeout",
            "60s",
            "--idempotency-key",
            KEY,
            "--json",
        ]),
    );

    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("instance_duplicate.json", &json.stdout);
    let log = daemon.log();
    let start_call = log
        .iter()
        .find(|logged| logged.method == "instance.start")
        .expect("the start is sent");
    assert_eq!(
        start_call.idempotency_key.as_deref(),
        Some(derived_start_key(KEY).as_str())
    );
    assert_eq!(
        start_call.raw["call"]["content"]["instanceId"],
        INSTANCE.to_string()
    );
    let value = envelope(&json);
    for key in ["operation", "idempotencyKey", "instance", "start", "source"] {
        assert!(value["result"].get(key).is_some(), "{key}: {value}");
    }
    ev.pass();
}

// covers: AC-11
#[test]
fn a_form_the_daemon_refuses_exits_by_its_error_without_a_create() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceGetDuplicateForm(_) => vec![Step::Fail(error(
            "supervisor.instance_retained",
            "conflict",
            "the instance is retained",
        ))],
        other => data::scripted(accepted(OperationState::Queued, 1), Vec::new(), None)(other, ctx),
    });

    let refused = run(root.path(), &start(&[SOURCE, "--json"]));

    assert_eq!(refused.code, Some(3), "{refused:?}");
    assert_eq!(
        envelope(&refused)["error"]["code"],
        "supervisor.instance_retained"
    );
    assert_eq!(
        daemon.methods(),
        ["daemon.handshake", "instance.get_duplicate_form"]
    );
    assert!(
        refused.stderr.is_empty(),
        "the name of the source is known only once the form is: {refused:?}"
    );
}
