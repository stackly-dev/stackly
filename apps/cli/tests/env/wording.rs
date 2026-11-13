//! Spec 0044 AC-11, AC-16, AC-17, the low findings of the F44 verify:
//! `env.name_taken` names where the name is registered and points at
//! `--relink` in text, `env remove --yes` writes only its result, and
//! `env plan` writes its table before the sections on stderr.

use std::process::Stdio;

use stackly_protocol::Call;
use stackly_protocol::ErrorBody;
use stackly_protocol::env::{EnvPlan, PlanBlocker, PlanRegistration};
use stackly_testkit::TestRoot;

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{envelope, golden, run, run_command, stackly};
use crate::data;

/// `env.name_taken` as the daemon sends it: the name, and the location
/// it is registered at with home as `~`.
fn name_taken() -> ErrorBody {
    serde_json::from_value(serde_json::json!({
        "code": "env.name_taken",
        "category": "conflict",
        "message": "the environment name is registered at another manifest location",
        "retryable": false,
        "hint": "rename metadata.name, or use this file for the registered environment",
        "details": [
            { "key": "name", "value": "erp-development" },
            { "key": "location", "value": "~/old/erp/stackly.yaml" },
        ],
    }))
    .expect("the error decodes")
}

/// The applicable plan of a file whose name is registered elsewhere.
fn taken_plan() -> EnvPlan {
    let mut plan = data::applicable();
    plan.registration = Some(PlanRegistration::NameTaken);
    plan.applicable = false;
    plan.plan_digest = None;
    plan.blockers = vec![PlanBlocker {
        service: None,
        error: name_taken(),
    }];
    plan
}

/// A daemon whose plan is `plan`, whose import fails `env.name_taken`,
/// and that removes the fixture environment.
fn daemon(plan: EnvPlan) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let serving = data::serving(data::list(), data::valid(), plan, data::detail());
    let daemon = ScriptedDaemon::serve(&root, move |call, cx| match call {
        Call::EnvImport(_) => vec![Step::Fail(name_taken())],
        _ => serving(call, cx),
    });
    (root, daemon)
}

/// AC-11, AC-17.
#[test]
fn a_taken_name_says_where_it_is_registered_and_how_to_relink_in_text_only() {
    let (root, _daemon) = daemon(taken_plan());

    let import = run(root.path(), &["env", "import"]);
    let import_json = run(root.path(), &["env", "import", "--json"]);
    let plan = run(root.path(), &["env", "plan"]);

    assert_eq!(import.code, Some(3), "{import:?}");
    golden::check("env_import_name_taken_stderr.txt", &import.stderr);
    assert!(
        import
            .stderr
            .contains("error: environment erp-development is registered at ~/old/erp/stackly.yaml"),
        "{}",
        import.stderr
    );
    assert!(
        import
            .stderr
            .contains("hint: to use this file for it, run stackly env import --relink"),
        "{}",
        import.stderr
    );
    let value = envelope(&import_json);
    assert_eq!(value["error"]["code"], "env.name_taken");
    assert_eq!(
        value["error"]["message"],
        "the environment name is registered at another manifest location"
    );
    assert_eq!(plan.code, Some(3), "{plan:?}");
    assert!(
        plan.stderr.contains(
            "blocked: env.name_taken: environment erp-development is registered at ~/old/erp/stackly.yaml\n"
        ),
        "{}",
        plan.stderr
    );
    assert!(
        plan.stderr
            .contains("  hint: to use this file for it, run stackly env import --relink\n"),
        "{}",
        plan.stderr
    );
}

/// AC-17.
#[test]
fn remove_with_yes_writes_only_its_result() {
    let (root, _daemon) = daemon(data::applicable());

    let removed = run(root.path(), &["env", "remove", "erp-development", "--yes"]);

    assert_eq!(removed.code, Some(0), "{removed:?}");
    assert_eq!(
        removed.stdout,
        "Removed environment erp-development. Instances and data are kept.\n"
    );
    assert_eq!(removed.stderr, "");
}

/// AC-16: with stdout and stderr on one pipe, the table and the summary
/// come before the first `blocked:` line.
#[test]
fn the_plan_table_comes_before_the_sections_on_stderr() {
    let (root, _daemon) = daemon(data::blocked());
    let both = root.path().join("both.txt");
    let file = std::fs::File::create(&both).expect("the output file is made");
    let mut command = stackly(root.path());
    command
        .args(["env", "plan"])
        .stdout(Stdio::from(file.try_clone().expect("the file clones")))
        .stderr(Stdio::from(file));

    let ran = run_command(command);
    let text = std::fs::read_to_string(&both).expect("the output reads");

    assert_eq!(ran.code, Some(3), "{ran:?}");
    let summary = text.find("Plan: ").expect("the summary is written");
    let blocked = text.find("blocked: ").expect("a blocker is written");
    let table = text.find("ACTION ").expect("the table is written");
    assert!(table < summary && summary < blocked, "{text}");
}
