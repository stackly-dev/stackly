//! AC-16: `runtime install` resolves, starts, waits, and writes the
//! result; `--dry-run`, `--output id`, and the argument rules.

use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{dto, envelope, golden, run};
use crate::data;

const TARGET: &str = "official/postgresql@18";
const KEY: &str = "install-postgresql-0001";

fn count(daemon: &ScriptedDaemon, method: &str) -> usize {
    daemon
        .methods()
        .iter()
        .filter(|name| *name == method)
        .count()
}

/// Spec 0018 AC-16.
#[test]
fn install_resolves_first_then_starts_with_the_digest_it_saw_and_names_the_operation() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "install sends runtime.resolve and then runtime.install with expectedSha256 of the resolved artifact and the key given, and without --wait writes the operation, a hint, and { operation, idempotencyKey, resolved, runtimeId, alreadyInstalled }",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            data::started(data::operation(OperationState::Queued, 1), false),
            Vec::new(),
            data::installing_list(),
        ),
    );

    let text = run(
        root.path(),
        &["runtime", "install", TARGET, "--idempotency-key", KEY],
    );
    let json = run(
        root.path(),
        &[
            "runtime",
            "install",
            TARGET,
            "--idempotency-key",
            KEY,
            "--json",
        ],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        format!(
            "Started installing PostgreSQL 18.6 (operation {})\n",
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
    let value = envelope(&json);
    assert_eq!(value["command"], "runtime install");
    assert_eq!(
        value["result"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [
            "alreadyInstalled",
            "idempotencyKey",
            "operation",
            "resolved",
            "runtimeId"
        ]
    );
    assert_eq!(value["result"]["alreadyInstalled"], false);
    assert_eq!(value["result"]["idempotencyKey"], KEY);
    assert_eq!(value["result"]["resolved"]["sha256"], data::SHA256);
    assert_eq!(value["result"]["runtimeId"], data::RUNTIME.to_string());
    golden::check("runtime_install_started.txt", &text.stdout);
    golden::check("runtime_install_started.json", &json.stdout);
    let log = daemon.log();
    let sent = |method: &str| {
        log.iter()
            .find(|logged| logged.method == method)
            .unwrap_or_else(|| panic!("{method} was sent"))
            .clone()
    };
    let (resolve, install) = (sent("runtime.resolve"), sent("runtime.install"));
    assert_eq!(
        resolve.raw["call"]["content"]["provider"],
        "official/postgresql"
    );
    assert_eq!(resolve.raw["call"]["content"]["selector"], "18");
    assert_eq!(
        resolve.raw["call"]["content"]["expectedSha256"],
        serde_json::Value::Null
    );
    assert_eq!(
        install.raw["call"]["content"]["expectedSha256"],
        data::SHA256
    );
    assert_eq!(install.idempotency_key.as_deref(), Some(KEY));
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn install_with_wait_follows_the_operation_and_names_the_runtime() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "install --wait follows the operation to succeeded, reads the runtime once for its name, and writes Installed <name> <version> (runtime <id>) with the runtime in the result",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            data::started(data::operation(OperationState::Queued, 1), false),
            vec![
                data::operation(OperationState::Running, 2),
                data::operation(OperationState::Succeeded, 3),
            ],
            data::list(),
        ),
    );
    let mut args = vec![
        "runtime",
        "install",
        TARGET,
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
    assert_eq!(
        text.stdout,
        format!("Installed PostgreSQL 18.6 (runtime {})\n", data::RUNTIME)
    );
    golden::check("runtime_install.txt", &text.stdout);
    golden::check("runtime_install.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["result"]["operation"]["state"], "succeeded");
    assert_eq!(value["result"]["runtimeId"], data::RUNTIME.to_string());
    assert_eq!(count(&daemon, "runtime.get"), 2, "one read per run");
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn an_artifact_installed_already_exits_0_with_no_operation_and_names_its_runtime() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "alreadyInstalled exits 0, writes Already installed: <name> <version> (runtime <id>), and the result has a null operation, alreadyInstalled true, and the runtime found by the digest",
    );
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(data::already_installed(), Vec::new(), data::list()),
    );

    let text = run(
        root.path(),
        &["runtime", "install", TARGET, "--idempotency-key", KEY],
    );
    let json = run(
        root.path(),
        &[
            "runtime",
            "install",
            TARGET,
            "--idempotency-key",
            KEY,
            "--json",
        ],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        format!(
            "Already installed: PostgreSQL 18.6 (runtime {})\n",
            data::RUNTIME
        )
    );
    assert_eq!(text.stderr, "", "there is nothing to wait for");
    golden::check("runtime_install_already.txt", &text.stdout);
    golden::check("runtime_install_already.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["result"]["operation"], serde_json::Value::Null);
    assert_eq!(value["result"]["alreadyInstalled"], true);
    assert_eq!(value["result"]["runtimeId"], data::RUNTIME.to_string());
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn dry_run_resolves_without_installing_and_writes_the_resolved_artifact() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "--dry-run sends only runtime.resolve and writes the artifact as text and as the result itself",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(data::already_installed(), Vec::new(), data::list()),
    );

    let text = run(root.path(), &["runtime", "install", TARGET, "--dry-run"]);
    let json = run(
        root.path(),
        &["runtime", "install", TARGET, "--dry-run", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("runtime_install_dry_run.txt", &text.stdout);
    golden::check("runtime_install_dry_run.json", &json.stdout);
    assert_eq!(
        envelope(&json)["result"]["artifactId"],
        "postgresql-18.6-r2-macos-aarch64"
    );
    assert_eq!(count(&daemon, "runtime.install"), 0);
    assert_eq!(count(&daemon, "runtime.resolve"), 2);
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn output_id_waits_and_writes_only_the_runtime_id_also_when_it_was_installed_already() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "--output id implies --wait and writes exactly <runtime-id> and a newline on stdout, for a new install and for alreadyInstalled",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            data::started(data::operation(OperationState::Queued, 1), false),
            vec![data::operation(OperationState::Succeeded, 2)],
            data::list(),
        ),
    );

    let waited = run(
        root.path(),
        &["runtime", "install", TARGET, "--output", "id"],
    );

    assert_eq!(waited.code, Some(0), "{waited:?}");
    assert_eq!(waited.stdout, format!("{}\n", data::RUNTIME));
    assert!(count(&daemon, "operation.get") >= 1, "it waited");
    drop(daemon);

    let _daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(data::already_installed(), Vec::new(), data::list()),
    );
    let already = run(
        root.path(),
        &["runtime", "install", TARGET, "--output", "id"],
    );
    assert_eq!(already.code, Some(0), "{already:?}");
    assert_eq!(already.stdout, format!("{}\n", data::RUNTIME));
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn wrong_arguments_exit_2_before_any_call() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a target without @, a provider without its namespace, --output id with --json or --dry-run all exit 2 and send nothing",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |_, _| Vec::new());

    for args in [
        vec!["runtime", "install", "official/postgresql"],
        vec!["runtime", "install", "postgresql@18"],
        vec!["runtime", "install", "official/postgresql@"],
        vec!["runtime", "install", TARGET, "--output", "id", "--json"],
        vec!["runtime", "install", TARGET, "--output", "id", "--dry-run"],
        vec!["runtime", "install", TARGET, "--output", "name"],
    ] {
        let result = run(root.path(), &args);
        assert_eq!(result.code, Some(2), "{args:?}: {result:?}");
    }
    assert_eq!(daemon.methods(), Vec::<String>::new());
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn the_runtime_rows_of_the_exit_code_matrix() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "selector_unresolved exits 10, catalog.not_installable exits 5, runtime.selector_changed exits 3, and a failed install under --wait exits 7 with the operation's code",
    );
    for (code, category, expected) in [
        ("runtime.selector_unresolved", "not_found", 10),
        ("catalog.not_installable", "unsupported", 5),
        ("runtime.selector_changed", "conflict", 3),
    ] {
        let root = TestRoot::new().expect("a test root is made");
        let _daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
            Call::RuntimeResolve(_) => vec![data::refusal(code, category)],
            _ => Vec::new(),
        });
        let result = run(root.path(), &["runtime", "install", TARGET, "--json"]);
        assert_eq!(result.code, Some(expected), "{code}: {result:?}");
        assert_eq!(envelope(&result)["error"]["code"], code);
    }

    let root = TestRoot::new().expect("a test root is made");
    let mut failed = dto::ended(OperationState::Failed, true, 3);
    failed.kind = stackly_protocol::operation::OperationKind::InstallRuntime;
    failed.resource = data::operation(OperationState::Queued, 1).resource;
    let _daemon = ScriptedDaemon::serve(
        &root,
        data::scripted(
            data::started(data::operation(OperationState::Queued, 1), false),
            vec![failed],
            data::list(),
        ),
    );
    let result = run(
        root.path(),
        &["runtime", "install", TARGET, "--wait", "--json"],
    );
    assert_eq!(result.code, Some(7), "{result:?}");
    assert_eq!(envelope(&result)["result"]["operation"]["state"], "failed");
    let _ = (Reply::EventsUnsubscribe, Step::Close);
    ev.pass();
}
