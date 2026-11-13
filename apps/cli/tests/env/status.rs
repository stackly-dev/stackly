//! Spec 0046 AC-18: `env status` reads `env.get` with `checkManifest`
//! true and prints the head line, the services with the ports they really
//! use, and why each service that is not ready is not; it exits 0 whatever
//! the status is and 10 for an environment that is not known. A path is
//! planned first, for the environment registered at it.

use stackly_protocol::Reply;
use stackly_protocol::env::{EnvPlan, EnvironmentDetail, PlanRegistration};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{envelope, golden, run};
use crate::data;

/// A daemon whose `env.list` holds `erp-development` and whose `env.get`
/// answers `detail`, planning every file as `plan`.
fn daemon(plan: EnvPlan, detail: EnvironmentDetail) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let serving = data::serving(data::list(), plan.validation.clone(), plan, data::detail());
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        stackly_protocol::Call::EnvGet(_) => {
            vec![Step::Reply(Reply::EnvGet(Box::new(detail.clone())))]
        }
        _ => serving(call, ctx),
    });
    (root, daemon)
}

/// AC-18.
#[test]
fn status_prints_the_head_the_services_and_each_reason_and_exits_0() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env status graph-like detail-up-incomplete.json prints graph  Partially ready  1 of 3 ready  <file>, the SERVICE INSTANCE VERSION OWNERSHIP STATUS ENDPOINTS table with http 127.0.0.1:49152 for db and - for the others, then not ready: with the reference to set and the failed dependency, exits 0 although not every service is ready, asks env.get with checkManifest true, and with --json writes the EnvironmentDetail exactly as the daemon sent it",
    );
    let (root, daemon) = daemon(data::applicable(), data::up_incomplete());

    let text = run(root.path(), &["env", "status", "erp-development"]);
    let json = run(root.path(), &["env", "status", "erp-development", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_status.txt", &text.stdout);
    assert_eq!(
        text.stdout.lines().next(),
        Some("graph  Partially ready  1 of 3 ready  /Users/dev/erp/stackly.yaml")
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("env_status.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(
        value["result"],
        serde_json::to_value(data::up_incomplete()).expect("the detail serializes")
    );
    let gets: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.get")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect();
    assert_eq!(gets[0]["checkManifest"], true);
    assert_eq!(gets[0]["id"], data::detail().summary.id.to_string());
    ev.pass();
}

/// AC-18: the manifest line, and the reasons of an Up that still runs.
#[test]
fn a_changed_manifest_and_a_running_up_are_shown() {
    let (root, _daemon) = daemon(data::applicable(), data::detail());
    let changed = run(root.path(), &["env", "status", "erp-development"]);
    assert_eq!(changed.code, Some(0), "{changed:?}");
    assert_eq!(changed.stdout.lines().last(), Some("manifest: changed"));

    let (root, _daemon) = daemon(data::applicable(), data::bringing_up());
    let running = run(root.path(), &["env", "status", "erp-development"]);
    assert_eq!(running.code, Some(0), "{running:?}");
    assert!(
        running
            .stdout
            .contains("  api: bringing up (waiting), waiting for cache\n"),
        "{}",
        running.stdout
    );
    assert!(
        running.stdout.contains("  cache: bringing up (starting)\n"),
        "{}",
        running.stdout
    );
}

/// AC-18: a path is planned for the ID registered at it; a file no
/// environment is registered at, or a name nobody has, exits 10.
#[test]
fn a_path_resolves_through_the_plan_and_an_unknown_target_exits_10() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env status <folder> plans <folder>/stackly.yaml and reads env.get for the registered environment of the plan; a file whose plan says new exits 10 with cli.environment_not_found and Run stackly env up or stackly env import.; an unknown name exits 10; an invalid manifest exits 2 with a hint to name the environment",
    );
    let (root, scripted) = daemon(data::applicable(), data::up_incomplete());
    let project = root.path().join("work/erp");
    std::fs::create_dir_all(&project).expect("the project folder can be made");
    let folder = project.to_str().expect("a utf-8 path");

    let registered = run(root.path(), &["env", "status", folder]);
    assert_eq!(registered.code, Some(0), "{registered:?}");
    let logged = scripted.log();
    let planned = logged
        .iter()
        .find(|logged| logged.method == "env.plan")
        .expect("the folder is planned");
    assert_eq!(
        planned.raw["call"]["content"]["source"]["content"],
        project.join("stackly.yaml").display().to_string()
    );
    assert!(!logged.iter().any(|logged| logged.method == "env.list"));

    let unknown = run(root.path(), &["env", "status", "nobody", "--json"]);
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    assert_eq!(
        envelope(&unknown)["error"]["code"],
        "cli.environment_not_found"
    );

    let mut new = data::applicable();
    new.registration = Some(PlanRegistration::New);
    new.environment = None;
    let (root, _daemon) = daemon(new, data::up_incomplete());
    let not_imported = run(root.path(), &["env", "status"]);
    assert_eq!(not_imported.code, Some(10), "{not_imported:?}");
    golden::check("env_status_not_imported_stderr.txt", &not_imported.stderr);
    assert!(
        not_imported
            .stderr
            .contains("  hint: Run stackly env up or stackly env import.\n"),
        "{}",
        not_imported.stderr
    );

    let (root, _daemon) = daemon(data::invalid_plan(), data::up_incomplete());
    let invalid = run(root.path(), &["env", "status", "./stackly.yaml"]);
    assert_eq!(invalid.code, Some(2), "{invalid:?}");
    assert!(
        invalid
            .stderr
            .contains("hint: Name the environment instead: stackly env status <environment>."),
        "{}",
        invalid.stderr
    );
    ev.pass();
}
