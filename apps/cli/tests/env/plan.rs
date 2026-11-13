//! AC-16: `env plan` calls `env.plan` for a file or a registered
//! environment, prints one row per step and the summary line, writes the
//! blockers, secrets, differences, and detached members to stderr, and
//! exits 0, 2 with `cli.env_invalid`, or 3 with `cli.env_plan_blocked`,
//! with the plan as result each time.

use stackly_protocol::env::{EnvPlan, EnvValidation};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::ScriptedDaemon;
use crate::common::{detail, envelope, golden, run};
use crate::data;

fn daemon(validation: EnvValidation, plan: EnvPlan) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::serving(data::list(), validation, plan, data::detail()),
    );
    (root, daemon)
}

/// AC-16.
#[test]
fn an_applicable_plan_prints_a_row_per_step_and_the_summary_and_exits_0() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env plan over plan-applicable.json prints install, create, and start rows for postgres and a keep row for cache, Plan: 1 to install, 1 to create, 1 to start, 1 to keep., writes secrets, differs, and not in manifest to stderr, exits 0, and writes the EnvPlan as result with --json",
    );
    let (root, daemon) = daemon(data::valid(), data::applicable());

    let text = run(root.path(), &["env", "plan"]);
    let json = run(root.path(), &["env", "plan", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_plan.txt", &text.stdout);
    golden::check("env_plan_stderr.txt", &text.stderr);
    assert_eq!(
        text.stdout.lines().last(),
        Some("Plan: 1 to install, 1 to create, 1 to start, 1 to keep.")
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("env_plan.json", &json.stdout);
    assert_eq!(json.stderr, "");
    let value = envelope(&json);
    assert_eq!(value["command"], "env plan");
    assert_eq!(value["result"]["applicable"], true);
    let sources: Vec<String> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.plan")
        .map(|logged| logged.raw["call"]["content"]["source"]["type"].to_string())
        .collect();
    assert_eq!(sources, ["\"file\"", "\"file\""]);
    ev.pass();
}

/// AC-16.
#[test]
fn a_plan_with_blockers_exits_3_and_lists_each_blocker_on_stderr() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env plan over plan-blocked.json still prints the table, writes one blocked: line per blocker with its hint to stderr, and exits 3 with cli.env_plan_blocked and details.blockers 2, keeping the plan as result with --json",
    );
    let (root, _daemon) = daemon(data::valid(), data::blocked());

    let text = run(root.path(), &["env", "plan"]);
    let json = run(root.path(), &["env", "plan", "--json"]);

    assert_eq!(text.code, Some(3), "{text:?}");
    golden::check("env_plan_blocked_stderr.txt", &text.stderr);
    assert!(
        text.stderr
            .contains("blocked: postgres: supervisor.endpoint_conflict: port 5432 is already used by instance orders-db"),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(3), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.env_plan_blocked");
    assert_eq!(value["error"]["category"], "conflict");
    assert_eq!(detail(&value["error"], "blockers").as_deref(), Some("2"));
    assert_eq!(
        value["result"]["blockers"].as_array().map(Vec::len),
        Some(2)
    );
    ev.pass();
}

/// AC-16; spec 0045 K23: the plan of an invalid manifest prints no
/// `lock:` or `local:` line, even with a valid lock and a local file.
#[test]
fn a_plan_of_an_invalid_manifest_exits_2_with_the_findings() {
    let (root, _daemon) = daemon(data::invalid(), data::invalid_plan());

    let text = run(root.path(), &["env", "plan"]);
    let json = run(root.path(), &["env", "plan", "--json"]);

    assert_eq!(text.code, Some(2), "{text:?}");
    assert_eq!(text.stdout, "");
    assert!(
        text.stderr
            .contains(": services.api.dependsOn: dependency_cycle: "),
        "{}",
        text.stderr
    );
    assert!(text.stderr.contains("2 error(s), 0 warning(s)"));
    golden::check("env_plan_invalid_stderr.txt", &text.stderr);
    assert!(
        !text
            .stderr
            .lines()
            .any(|line| line.starts_with("lock: ") || line.starts_with("local: ")),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(2));
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.env_invalid");
    assert_eq!(value["result"]["validation"]["valid"], false);
}

/// AC-16, AC-17: a registered environment by name plans its manifest
/// location; an unknown name exits 10 before `env.plan` is sent.
#[test]
fn a_registered_environment_is_planned_by_name_or_id_and_an_unknown_name_exits_10() {
    let (root, daemon) = daemon(data::valid(), data::applicable());
    let id = data::detail().summary.id.to_string();

    let by_name = run(root.path(), &["env", "plan", "erp-development"]);
    let by_id = run(root.path(), &["env", "plan", &id.to_uppercase()]);
    let unknown = run(root.path(), &["env", "plan", "nope", "--json"]);
    let both = run(root.path(), &["env", "plan", "erp", "--file", "x.yaml"]);

    assert_eq!(by_name.code, Some(0), "{by_name:?}");
    assert_eq!(by_id.code, Some(0), "{by_id:?}");
    let sources: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.plan")
        .map(|logged| logged.raw["call"]["content"]["source"].clone())
        .collect();
    let registered = serde_json::json!({ "type": "registered", "content": id });
    assert_eq!(sources, [registered.clone(), registered]);
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    let value = envelope(&unknown);
    assert_eq!(value["error"]["code"], "cli.environment_not_found");
    assert_eq!(detail(&value["error"], "name").as_deref(), Some("nope"));
    assert_eq!(both.code, Some(2), "{both:?}");
}
