//! Spec 0045 AC-16, AC-6, AC-17: `env lock` sends `env.lock` with the
//! refresh its flags name, prints one line per service to stderr and the
//! file line to stdout, and exits 0, 2, 3, or 10; `env plan --locked`
//! sends `locked`, prints the lock and local lines, and refuses a plan
//! from a daemon that does not know the mode with
//! `cli.env_locked_unsupported` (exit 5).

use stackly_protocol::env::{EnvLockResult, EnvPlan};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{detail, envelope, golden, run};
use crate::data;

/// A daemon that knows the fixture environment, answers `env.lock` with
/// `result` (an alias named `ghost` fails `env.service_not_found`), and
/// `env.plan` with `plan`.
fn daemon(result: EnvLockResult, plan: EnvPlan) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let list = data::list();
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::EnvList => vec![Step::Reply(Reply::EnvList(list.clone()))],
        Call::EnvLock(params) => match &params.refresh {
            stackly_protocol::env::LockRefresh::Services(aliases)
                if aliases.iter().any(|alias| alias == "ghost") =>
            {
                vec![Step::Fail(error(
                    "env.service_not_found",
                    "not_found",
                    "the manifest declares no dedicated service of this name",
                ))]
            }
            _ => vec![Step::Reply(Reply::EnvLock(Box::new(result.clone())))],
        },
        Call::EnvPlan(_) => vec![Step::Reply(Reply::EnvPlan(Box::new(plan.clone())))],
        _ => Vec::new(),
    });
    (root, daemon)
}

fn lock_params(daemon: &ScriptedDaemon) -> Vec<serde_json::Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.lock")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

/// AC-16.
#[test]
fn lock_prints_a_line_per_service_then_the_file_and_exits_0() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env lock over lock-written.json writes updated postgres official/postgresql 18.5 -> 18.6 and removed old official/nats 2.15.1 to stderr, Locked 1 services in /Users/dev/erp/stackly.lock. to stdout, exits 0, sends refresh none from ./stackly.yaml, and with --json writes the EnvLockResult with an empty stderr",
    );
    let (root, daemon) = daemon(data::lock_written(), data::applicable());

    let text = run(root.path(), &["env", "lock"]);
    let json = run(root.path(), &["env", "lock", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_lock.txt", &text.stdout);
    golden::check("env_lock_stderr.txt", &text.stderr);
    assert_eq!(
        text.stdout,
        "Locked 1 services in /Users/dev/erp/stackly.lock.\n"
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("env_lock.json", &json.stdout);
    assert_eq!(json.stderr, "");
    let value = envelope(&json);
    assert_eq!(value["command"], "env lock");
    assert_eq!(value["result"]["written"], true);
    let params = lock_params(&daemon);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0]["refresh"], serde_json::json!({ "type": "none" }));
    assert_eq!(params[0]["source"]["type"], "file");
    assert!(
        params[0]["source"]["content"]
            .as_str()
            .is_some_and(|path| path.ends_with("/stackly.yaml")),
        "{params:?}"
    );
    ev.pass();
}

/// AC-16.
#[test]
fn a_lock_that_did_not_change_says_the_file_is_up_to_date() {
    let mut result = data::lock_written();
    result.written = false;
    result.changed = false;
    result.services = vec![stackly_protocol::env::LockedService {
        change: stackly_protocol::env::LockChange::Kept,
        previous_version: None,
        ..result.services[0].clone()
    }];
    let (root, _daemon) = daemon(result, data::applicable());

    let text = run(root.path(), &["env", "lock"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "/Users/dev/erp/stackly.lock is up to date.\n");
    assert_eq!(text.stderr, "kept postgres official/postgresql 18.6\n");
}

/// AC-16: `--update` picks everything again, `--update --service` the
/// named aliases, `--service` alone is refused by clap, an alias the
/// manifest does not declare exits 10, and a name locks the manifest of
/// that registered environment.
#[test]
fn update_and_service_choose_the_refresh_and_an_unknown_alias_exits_10() {
    let (root, daemon) = daemon(data::lock_written(), data::applicable());
    let id = data::detail().summary.id.to_string();

    let all = run(root.path(), &["env", "lock", "--update"]);
    let some = run(
        root.path(),
        &[
            "env",
            "lock",
            "--update",
            "--service",
            "postgres",
            "--service",
            "cache",
        ],
    );
    let alone = run(root.path(), &["env", "lock", "--service", "postgres"]);
    let ghost = run(
        root.path(),
        &["env", "lock", "--update", "--service", "ghost", "--json"],
    );
    let named = run(root.path(), &["env", "lock", "erp-development"]);
    let unknown = run(root.path(), &["env", "lock", "nope"]);

    assert_eq!(all.code, Some(0), "{all:?}");
    assert_eq!(some.code, Some(0), "{some:?}");
    assert_eq!(alone.code, Some(2), "{alone:?}");
    assert_eq!(ghost.code, Some(10), "{ghost:?}");
    assert_eq!(envelope(&ghost)["error"]["code"], "env.service_not_found");
    assert_eq!(named.code, Some(0), "{named:?}");
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    let refreshes: Vec<serde_json::Value> = lock_params(&daemon)
        .into_iter()
        .map(|params| params["refresh"].clone())
        .collect();
    assert_eq!(
        refreshes[..3],
        [
            serde_json::json!({ "type": "all" }),
            serde_json::json!({ "type": "services", "content": ["postgres", "cache"] }),
            serde_json::json!({ "type": "services", "content": ["ghost"] }),
        ]
    );
    assert_eq!(
        lock_params(&daemon)[3]["source"],
        serde_json::json!({ "type": "registered", "content": id })
    );
}

/// AC-16.
#[test]
fn a_blocked_lock_exits_3_with_each_blocker_and_writes_nothing_to_stdout() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env lock over lock-blocked.json writes blocked: env.lock_invalid with its hint to stderr, nothing to stdout, and exits 3 with cli.env_lock_blocked and details.blockers 1, keeping the EnvLockResult as result with --json",
    );
    let (root, _daemon) = daemon(data::lock_blocked(), data::applicable());

    let text = run(root.path(), &["env", "lock"]);
    let json = run(root.path(), &["env", "lock", "--json"]);

    assert_eq!(text.code, Some(3), "{text:?}");
    assert_eq!(text.stdout, "");
    golden::check("env_lock_blocked_stderr.txt", &text.stderr);
    assert_eq!(json.code, Some(3), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.env_lock_blocked");
    assert_eq!(value["error"]["category"], "conflict");
    assert_eq!(detail(&value["error"], "blockers").as_deref(), Some("1"));
    assert_eq!(value["result"]["written"], false);
    ev.pass();
}

/// AC-16.
#[test]
fn a_lock_of_an_invalid_manifest_exits_2_with_the_findings() {
    let mut result = data::lock_blocked();
    result.validation = data::invalid();
    result.blockers.clear();
    result.services.clear();
    let (root, _daemon) = daemon(result, data::applicable());

    let text = run(root.path(), &["env", "lock"]);
    let json = run(root.path(), &["env", "lock", "--json"]);

    assert_eq!(text.code, Some(2), "{text:?}");
    assert_eq!(text.stdout, "");
    assert!(
        text.stderr.contains("2 error(s), 0 warning(s)"),
        "{}",
        text.stderr
    );
    assert_eq!(envelope(&json)["error"]["code"], "cli.env_invalid");
}

/// AC-6, AC-17.
#[test]
fn plan_locked_sends_locked_and_prints_the_lock_and_local_lines() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-023"],
        kind = Kind::Fixture,
        expected = "env plan --locked sends locked true; over plan-locked-blocked.json it prints the table with the SOURCE column to stdout, then lock: stackly.lock (stale, macos-aarch64), the stale alias, local: .stackly.local.yaml with its findings, and one blocked: line per blocker to stderr, and exits 3 with cli.env_plan_blocked; without --locked it sends locked false",
    );
    let (root, daemon) = daemon(data::lock_written(), data::locked_blocked());

    let locked = run(root.path(), &["env", "plan", "--locked"]);
    let plain = run(root.path(), &["env", "plan"]);

    assert_eq!(locked.code, Some(3), "{locked:?}");
    golden::check("env_plan_locked.txt", &locked.stdout);
    golden::check("env_plan_locked_stderr.txt", &locked.stderr);
    assert!(locked.stdout.starts_with("ACTION "), "{}", locked.stdout);
    assert!(
        locked
            .stdout
            .lines()
            .next()
            .is_some_and(|header| header.contains("SOURCE"))
    );
    assert_eq!(plain.code, Some(3), "{plain:?}");
    let flags: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.plan")
        .map(|logged| logged.raw["call"]["content"]["locked"].clone())
        .collect();
    assert_eq!(flags, [serde_json::json!(true), serde_json::json!(false)]);
    ev.pass();
}

/// AC-6: a daemon that does not know `locked` sends a plan whose
/// `lock.locked` is false; the CLI refuses it rather than trust a plan
/// that may have resolved again.
#[test]
fn plan_locked_against_a_daemon_without_the_mode_exits_5() {
    let ev = evidence!(
        covers = ["AC-023"],
        kind = Kind::Fixture,
        expected = "env plan --locked against a reply whose lock.locked is false (a daemon that does not know the field) exits 5 with cli.env_locked_unsupported, category unsupported, no result and no table, while env plan without --locked over the same reply exits 0",
    );
    let mut old = data::applicable();
    old.lock = stackly_protocol::env::PlanLock::default();
    let (root, _daemon) = daemon(data::lock_written(), old);

    let text = run(root.path(), &["env", "plan", "--locked"]);
    let json = run(root.path(), &["env", "plan", "--locked", "--json"]);
    let plain = run(root.path(), &["env", "plan"]);

    assert_eq!(text.code, Some(5), "{text:?}");
    assert_eq!(text.stdout, "");
    assert!(
        text.stderr.contains("cli.env_locked_unsupported"),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(5), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.env_locked_unsupported");
    assert_eq!(value["error"]["category"], "unsupported");
    assert!(value["result"].is_null(), "{value}");
    assert_eq!(plain.code, Some(0), "{plain:?}");
    assert!(!plain.stderr.contains("lock:"), "{}", plain.stderr);
    ev.pass();
}
