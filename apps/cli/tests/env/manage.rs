//! AC-17: `env import` binds `env.import` to the digest `env.plan` showed
//! and relinks only with `--relink`, `env list` prints the table, and
//! `env remove` asks with the `ConfirmArgs` rule before `env.remove`.

use stackly_protocol::env::{EnvPlan, EnvironmentList, PlanRegistration};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::ScriptedDaemon;
use crate::common::{detail, envelope, golden, run};
use crate::data;

fn serve(
    list: EnvironmentList,
    plan: EnvPlan,
    imported: stackly_protocol::env::EnvironmentDetail,
) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, data::serving(list, data::valid(), plan, imported));
    (root, daemon)
}

fn import_params(daemon: &ScriptedDaemon) -> Vec<serde_json::Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.import")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

/// AC-17.
#[test]
fn import_sends_the_planned_digest_and_says_whether_anything_changed() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env import sends env.plan then env.import with expectedManifestSha256 from the plan and an idempotency key, prints Imported environment <name>. with its rows, prints Already imported. when the revision did not move, and writes the EnvironmentDetail as result with --json",
    );
    let (root, daemon) = serve(
        data::list(),
        data::applicable(),
        data::at_revision(data::detail(), 4),
    );

    let text = run(root.path(), &["env", "import"]);
    let json = run(root.path(), &["env", "import", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_import.txt", &text.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "env import");
    assert_eq!(value["result"]["summary"]["revision"], 4);
    let params = import_params(&daemon);
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[0]["expectedManifestSha256"],
        data::applicable().validation.manifest_sha256.unwrap()
    );
    assert_eq!(params[0]["relinkEnvironmentId"], serde_json::Value::Null);
    let keys: Vec<Option<String>> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.import")
        .map(|logged| logged.idempotency_key)
        .collect();
    assert!(keys.iter().all(Option::is_some), "{keys:?}");
    assert_ne!(keys[0], keys[1]);

    let (root, _daemon) = serve(data::list(), data::applicable(), data::detail());
    let same = run(root.path(), &["env", "import"]);
    assert_eq!(same.code, Some(0), "{same:?}");
    assert_eq!(same.stdout.lines().next(), Some("Already imported."));
    ev.pass();
}

/// AC-17.
#[test]
fn relink_sends_the_environment_id_only_when_the_name_is_taken() {
    let mut taken = data::applicable();
    taken.registration = Some(PlanRegistration::NameTaken);
    let (root, daemon) = serve(
        data::list(),
        taken.clone(),
        data::at_revision(data::detail(), 4),
    );

    assert_eq!(run(root.path(), &["env", "import"]).code, Some(0));
    assert_eq!(
        run(root.path(), &["env", "import", "--relink"]).code,
        Some(0)
    );

    let params = import_params(&daemon);
    assert_eq!(params[0]["relinkEnvironmentId"], serde_json::Value::Null);
    assert_eq!(
        params[1]["relinkEnvironmentId"],
        taken.environment.unwrap().id.to_string()
    );

    let (root, daemon) = serve(data::list(), data::applicable(), data::detail());
    assert_eq!(
        run(root.path(), &["env", "import", "--relink"]).code,
        Some(0)
    );
    assert_eq!(
        import_params(&daemon)[0]["relinkEnvironmentId"],
        serde_json::Value::Null
    );
}

/// AC-17: an invalid manifest is never sent to `env.import`.
#[test]
fn import_of_an_invalid_manifest_exits_2_without_importing() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::serving(
            data::list(),
            data::invalid(),
            data::invalid_plan(),
            data::detail(),
        ),
    );

    let text = run(root.path(), &["env", "import"]);

    assert_eq!(text.code, Some(2), "{text:?}");
    assert!(text.stderr.contains("cli.env_invalid"), "{}", text.stderr);
    assert!(import_params(&daemon).is_empty());
}

/// AC-17.
#[test]
fn list_prints_the_table_or_no_environments() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env list prints NAME STATUS READY FILE with one row per environment, No environments. for none, and { items } with --json",
    );
    let (root, _daemon) = serve(data::list(), data::applicable(), data::detail());
    let text = run(root.path(), &["env", "list"]);
    let json = run(root.path(), &["env", "list", "--json"]);
    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("env_list.txt", &text.stdout);
    assert_eq!(json.code, Some(0));
    golden::check("env_list.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(
        value["result"]
            .as_object()
            .expect("an object")
            .keys()
            .collect::<Vec<_>>(),
        ["items"]
    );

    let (root, _daemon) = serve(
        EnvironmentList { items: Vec::new() },
        data::applicable(),
        data::detail(),
    );
    let empty = run(root.path(), &["env", "list"]);
    assert_eq!(empty.code, Some(0));
    assert_eq!(empty.stdout, "No environments.\n");
    ev.pass();
}

/// AC-17.
#[test]
fn remove_needs_yes_when_noninteractive_and_sends_the_listed_revision() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env remove without --yes and without a terminal fails cli.confirmation_required before env.remove; with --yes it sends env.remove with the id and revision from env.list and prints Removed environment <name>. Instances and data are kept.; an unknown name exits 10 with cli.environment_not_found",
    );
    let (root, daemon) = serve(data::list(), data::applicable(), data::detail());

    let refused = run(root.path(), &["env", "remove", "erp-development", "--json"]);
    assert!(!daemon.methods().contains(&"env.remove".to_owned()));
    let removed = run(root.path(), &["env", "remove", "erp-development", "--yes"]);
    let unknown = run(root.path(), &["env", "remove", "nope", "--yes"]);

    let value = envelope(&refused);
    assert_eq!(value["error"]["code"], "cli.confirmation_required");
    assert_eq!(detail(&value["error"], "flag").as_deref(), Some("--yes"));
    assert_eq!(removed.code, Some(0), "{removed:?}");
    assert_eq!(
        removed.stdout,
        "Removed environment erp-development. Instances and data are kept.\n"
    );
    let summary = data::detail().summary;
    let params: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.remove")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect();
    assert_eq!(
        params,
        [serde_json::json!({ "id": summary.id.to_string(), "expectedRevision": 3 })]
    );
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    assert!(
        unknown.stderr.contains("cli.environment_not_found"),
        "{}",
        unknown.stderr
    );
    ev.pass();
}
