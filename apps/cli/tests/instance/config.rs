//! `stackly instance config get|validate|apply|rollback|keep-revision`,
//! `rename`, and `endpoint set` against a scripted daemon (spec 0029, AC-17):
//! the flags map to the params, `--dry-run` prints the verdict the daemon
//! gave, an invalid verdict exits with the code and the field errors of
//! apply, and a command that gives something up asks before anything is
//! sent.

use serde_json::{Value, json};
use stackly_protocol::instance::InstanceActionAccepted;
use stackly_protocol::instance::config::ConfigApplyAccepted;
use stackly_protocol::operation::{OperationAccepted, OperationKind, OperationState};
use stackly_protocol::{Call, ConfigRevisionNumber, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{Ctx, ScriptedDaemon, Step};
use crate::common::{detail, envelope, golden, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";
const KEY: &str = "config-demo-0001";

fn from_fixture<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_value(data::fixture(name)).expect("the fixture decodes")
}

fn view() -> Reply {
    Reply::InstanceConfigGet(Box::new(from_fixture(
        "frame-response-instance-config-get-fake.json",
    )))
}

fn validation(name: &str) -> Reply {
    Reply::InstanceConfigValidate(Box::new(from_fixture(name)))
}

fn operation(kind: OperationKind) -> OperationAccepted {
    OperationAccepted {
        operation: data::operation(kind, OperationState::Queued, 1),
        replayed: false,
    }
}

fn apply_reply() -> Reply {
    Reply::InstanceConfigApply(Box::new(ConfigApplyAccepted {
        accepted: Some(operation(OperationKind::ApplyConfig)),
        unchanged: false,
        revision: ConfigRevisionNumber::new(3).expect("a revision"),
        impact: stackly_protocol::instance::config::ConfigImpact::Restart,
    }))
}

/// A daemon that answers the config methods with `validate` and the
/// fixtures, and the other methods with what `other` says.
fn daemon(root: &TestRoot, validate: &'static str) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, move |call: &Call, _: Ctx| match call {
        Call::InstanceConfigGet(_) => vec![Step::Reply(view())],
        Call::InstanceConfigValidate(_) => vec![Step::Reply(validation(validate))],
        Call::InstanceConfigApply(_) => vec![Step::Reply(apply_reply())],
        Call::InstanceConfigKeepRevision(_) => {
            vec![Step::Reply(Reply::InstanceConfigKeepRevision(Box::new(
                InstanceActionAccepted {
                    accepted: Some(operation(OperationKind::ResolveConfigDrift)),
                    already_in_state: false,
                },
            )))]
        }
        Call::InstanceRename(_) => vec![Step::Reply(Reply::InstanceRename(Box::new(
            InstanceActionAccepted {
                accepted: Some(operation(OperationKind::RenameInstance)),
                already_in_state: false,
            },
        )))],
        Call::InstanceChangeEndpoints(params) => {
            let reply: Value = if params.dry_run {
                data::fixture("frame-response-instance-change-endpoints-dry-run.json")
            } else {
                data::fixture("frame-response-instance-change-endpoints-applied.json")
            };
            vec![Step::Reply(Reply::InstanceChangeEndpoints(Box::new(
                serde_json::from_value(reply).expect("the fixture decodes"),
            )))]
        }
        Call::InstanceGet(_) => vec![Step::Fail(crate::common::scripted::error(
            "resource.not_found",
            "not_found",
            "no instance has this id",
        ))],
        _ => Vec::new(),
    })
}

fn sent(daemon: &ScriptedDaemon, method: &str) -> Vec<crate::common::scripted::Logged> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == method)
        .collect()
}

fn content(logged: &crate::common::scripted::Logged) -> Value {
    logged.raw["call"]["content"].clone()
}

// covers: AC-1, AC-17
#[test]
fn get_prints_the_view_of_the_daemon_and_one_file_with_file() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "config get --json is the view the daemon sent, with no other request than instance.config.get, and --file prints one config file as text",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(
        &root,
        "frame-response-instance-config-validate-restart.json",
    );

    let json = run(root.path(), &["instance", "config", "get", ID, "--json"]);
    let file = run(
        root.path(),
        &["instance", "config", "get", ID, "--file", "fake.conf"],
    );
    let missing = run(
        root.path(),
        &["instance", "config", "get", ID, "--file", "nope.conf"],
    );

    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(
        envelope(&json)["result"],
        data::fixture("frame-response-instance-config-get-fake.json")
    );
    assert_eq!(file.code, Some(0), "{file:?}");
    assert_eq!(file.stdout, "greeting = hello\nworkers = 4\n");
    assert_ne!(missing.code, Some(0), "{missing:?}");
    let others: Vec<String> = daemon
        .methods()
        .into_iter()
        .filter(|method| method != "instance.config.get" && method != "daemon.handshake")
        .collect();
    assert!(others.is_empty(), "{others:?}");
    ev.pass();
}

// covers: AC-2, AC-17
#[test]
fn apply_dry_run_sends_the_merged_settings_and_prints_the_verdict_of_the_daemon() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "apply --dry-run reads the config once, sends instance.config.validate with the active settings and the --set typed by the form, the base revision just read, and prints exactly the verdict fixture, without any apply",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(
        &root,
        "frame-response-instance-config-validate-restart.json",
    );

    let dry = run(
        root.path(),
        &[
            "instance",
            "config",
            "apply",
            ID,
            "--set",
            "workers=4",
            "--dry-run",
            "--json",
        ],
    );
    let validate = run(
        root.path(),
        &[
            "instance",
            "config",
            "validate",
            ID,
            "--set",
            "workers=4",
            "--json",
        ],
    );

    assert_eq!(dry.code, Some(0), "{dry:?}");
    assert_eq!(
        envelope(&dry)["result"],
        data::fixture("frame-response-instance-config-validate-restart.json")
    );
    assert_eq!(envelope(&validate)["result"], envelope(&dry)["result"]);
    let params = content(&sent(&daemon, "instance.config.validate")[0]);
    assert_eq!(params["baseRevision"], 2);
    assert_eq!(params["change"]["type"], "settings");
    assert_eq!(params["change"]["content"]["workers"], 4);
    assert_eq!(params["change"]["content"]["greeting"], "hello");
    assert!(sent(&daemon, "instance.config.apply").is_empty());
    ev.pass();
}

// covers: AC-2, AC-17
#[test]
fn an_invalid_verdict_exits_with_the_code_and_the_field_errors_of_apply() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = daemon(
        &root,
        "frame-response-instance-config-validate-invalid.json",
    );

    let run = run(
        root.path(),
        &[
            "instance",
            "config",
            "validate",
            ID,
            "--set",
            "workers=0",
            "--json",
        ],
    );

    assert_ne!(run.code, Some(0), "{run:?}");
    let value = envelope(&run);
    assert_eq!(value["error"]["code"], "instance.settings_invalid");
    let codes: Vec<&str> = value["error"]["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .filter_map(|field| field["code"].as_str())
        .collect();
    assert_eq!(
        codes,
        ["secret_read_only", "immutable_after_init", "out_of_range"]
    );
    assert_eq!(value["result"]["valid"], false);
}

// covers: AC-17
#[test]
fn apply_sends_the_base_revision_the_expected_one_and_the_key() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "config apply sends instance.config.apply with the active revision as baseRevision, --expect-revision instead when given, the key given, and the result has the operation and the key",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(
        &root,
        "frame-response-instance-config-validate-restart.json",
    );

    let first = run(
        root.path(),
        &[
            "instance",
            "config",
            "apply",
            ID,
            "--set",
            "greeting=hola",
            "--idempotency-key",
            KEY,
            "--json",
        ],
    );
    let expected = run(
        root.path(),
        &[
            "instance",
            "config",
            "apply",
            ID,
            "--set",
            "greeting=hola",
            "--expect-revision",
            "1",
            "--json",
        ],
    );

    assert_eq!(first.code, Some(0), "{first:?}");
    assert_eq!(expected.code, Some(0), "{expected:?}");
    let calls = sent(&daemon, "instance.config.apply");
    assert_eq!(calls[0].idempotency_key.as_deref(), Some(KEY));
    assert_eq!(content(&calls[0])["baseRevision"], 2);
    assert_eq!(content(&calls[0])["change"]["content"]["greeting"], "hola");
    assert_eq!(content(&calls[1])["baseRevision"], 1);
    let value = envelope(&first);
    assert_eq!(value["result"]["idempotencyKey"], KEY);
    assert_eq!(value["result"]["operation"]["kind"], "apply_config");
    ev.pass();
}

// covers: AC-17
#[test]
fn from_revision_restores_alone_and_starts_the_settings_when_edited() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(
        &root,
        "frame-response-instance-config-validate-restart.json",
    );

    let alone = run(
        root.path(),
        &[
            "instance",
            "config",
            "apply",
            ID,
            "--from-revision",
            "1",
            "--json",
        ],
    );
    let edited = run(
        root.path(),
        &[
            "instance",
            "config",
            "apply",
            ID,
            "--from-revision",
            "1",
            "--set",
            "greeting=hola",
            "--unset",
            "dataSeed",
            "--json",
        ],
    );
    let unknown = run(
        root.path(),
        &[
            "instance",
            "config",
            "apply",
            ID,
            "--from-revision",
            "9",
            "--set",
            "greeting=hola",
            "--json",
        ],
    );

    assert_eq!(alone.code, Some(0), "{alone:?}");
    assert_eq!(edited.code, Some(0), "{edited:?}");
    let calls = sent(&daemon, "instance.config.apply");
    assert_eq!(content(&calls[0])["change"]["type"], "restore");
    assert_eq!(content(&calls[0])["change"]["content"], 1);
    let settings = &content(&calls[1])["change"]["content"];
    assert_eq!(content(&calls[1])["change"]["type"], "settings");
    assert_eq!(settings["workers"], 2, "the values of revision 1");
    assert_eq!(settings["greeting"], "hola");
    assert!(settings.get("dataSeed").is_none());
    assert_ne!(unknown.code, Some(0), "{unknown:?}");
    assert_eq!(calls.len(), 2, "an unknown revision sends nothing");
}

// covers: AC-17
#[test]
fn rollback_and_keep_revision_need_yes_without_a_terminal_and_send_nothing_without_it() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "rollback and keep-revision with --json and no --yes fail cli.confirmation_required before any request reaches the daemon; with --yes they send instance.config.apply with the rollback change and instance.config.keep_revision",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(
        &root,
        "frame-response-instance-config-validate-restart.json",
    );

    let rollback = run(
        root.path(),
        &["instance", "config", "rollback", ID, "--json"],
    );
    let keep = run(
        root.path(),
        &["instance", "config", "keep-revision", ID, "--json"],
    );

    for run in [&rollback, &keep] {
        assert_ne!(run.code, Some(0), "{run:?}");
        assert_eq!(envelope(run)["error"]["code"], "cli.confirmation_required");
        assert_eq!(
            detail(&envelope(run)["error"], "flag").as_deref(),
            Some("--yes")
        );
    }
    let calls: Vec<String> = daemon
        .methods()
        .into_iter()
        .filter(|method| method != "daemon.handshake")
        .collect();
    assert!(calls.is_empty(), "{calls:?}");

    let rollback = run(
        root.path(),
        &["instance", "config", "rollback", ID, "--yes", "--json"],
    );
    let keep = run(
        root.path(),
        &["instance", "config", "keep-revision", ID, "--yes", "--json"],
    );

    assert_eq!(rollback.code, Some(0), "{rollback:?}");
    assert_eq!(keep.code, Some(0), "{keep:?}");
    let apply = &sent(&daemon, "instance.config.apply")[0];
    assert_eq!(content(apply)["change"], json!({ "type": "rollback" }));
    assert_eq!(content(apply)["baseRevision"], 2);
    assert_eq!(sent(&daemon, "instance.config.keep_revision").len(), 1);
    ev.pass();
}

// covers: AC-17
#[test]
fn rename_sends_the_new_name_and_endpoint_set_sends_ports_and_the_dry_run_flag() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "rename sends instance.rename { name }, endpoint set --dry-run sends dryRun true with the ports in order and exits with the code of the first problem, and without --dry-run sends dryRun false with the base revision read",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(
        &root,
        "frame-response-instance-config-validate-restart.json",
    );

    let renamed = run(
        root.path(),
        &["instance", "rename", ID, "orders-primary", "--json"],
    );
    let dry = run(
        root.path(),
        &[
            "instance",
            "endpoint",
            "set",
            ID,
            "--port",
            "main=5556",
            "--dry-run",
            "--json",
        ],
    );
    let set = run(
        root.path(),
        &[
            "instance",
            "endpoint",
            "set",
            ID,
            "--port",
            "main=auto",
            "--json",
        ],
    );

    assert_eq!(renamed.code, Some(0), "{renamed:?}");
    assert_eq!(
        content(&sent(&daemon, "instance.rename")[0])["name"],
        "orders-primary"
    );
    assert_ne!(dry.code, Some(0), "{dry:?}");
    assert_eq!(
        envelope(&dry)["error"]["code"],
        "supervisor.endpoint_conflict"
    );
    assert_eq!(
        detail(&envelope(&dry)["error"], "suggestedPort").as_deref(),
        Some("5434")
    );
    assert_eq!(set.code, Some(0), "{set:?}");
    let calls = sent(&daemon, "instance.change_endpoints");
    assert_eq!(content(&calls[0])["dryRun"], true);
    assert_eq!(
        content(&calls[0])["endpoints"],
        json!([{ "name": "main", "port": { "type": "fixed", "content": 5556 } }])
    );
    assert_eq!(content(&calls[1])["dryRun"], false);
    assert_eq!(content(&calls[1])["baseRevision"], 2);
    ev.pass();
}

// covers: AC-17
#[test]
fn the_text_of_every_config_verb_is_golden() {
    let root = TestRoot::new().expect("a test root is made");
    let _valid = daemon(
        &root,
        "frame-response-instance-config-validate-restart.json",
    );
    let get = run(root.path(), &["instance", "config", "get", ID]);
    let validate = run(
        root.path(),
        &["instance", "config", "validate", ID, "--set", "workers=4"],
    );
    let apply = run(
        root.path(),
        &["instance", "config", "apply", ID, "--set", "workers=4"],
    );
    let dry = run(
        root.path(),
        &[
            "instance",
            "endpoint",
            "set",
            ID,
            "--port",
            "main=5556",
            "--dry-run",
        ],
    );
    drop(_valid);
    let root = TestRoot::new().expect("a test root is made");
    let _invalid = daemon(
        &root,
        "frame-response-instance-config-validate-invalid.json",
    );
    let invalid = run(
        root.path(),
        &["instance", "config", "validate", ID, "--set", "workers=0"],
    );

    golden::check("instance_config_get.txt", &get.stdout);
    golden::check("instance_config_validate.txt", &validate.stdout);
    golden::check("instance_config_validate_invalid.txt", &invalid.stdout);
    golden::check(
        "instance_config_validate_invalid_stderr.txt",
        &invalid.stderr,
    );
    golden::check("instance_config_apply.txt", &apply.stdout);
    golden::check("instance_endpoint_set_dry_run.txt", &dry.stdout);
}
