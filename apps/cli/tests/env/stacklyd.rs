//! `stackly env` against a real `stacklyd` (spec 0044 AC-16, AC-17): the
//! daemon serves the catalog `cargo xtask catalog build` writes from
//! `providers/` through `STACKLY_CATALOG_DIR`, reads the manifest itself,
//! and runs `import_environment` and `remove_environment`. The fake
//! provider can't be used here: the daemon binary refuses the `testing`
//! namespace in a catalog and `provider import` refuses its fixture
//! artifact, so the manifest names two official providers. Nothing is
//! downloaded, installed, created, or started: validate and plan are read
//! only and import only registers the file.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, SpawnCatalog, SpawnedDaemon, TestRoot, cargo_bin};

use crate::common::stacklyd::{closed_connections, handshakes};
use crate::common::{detail, envelope, listing, paths, run_command, stackly};

/// Two official services; `queue` waits for `cache`.
const MANIFEST: &str = "apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop

services:
  cache:
    provider: official/valkey
    version: \"9.1\"
  queue:
    provider: official/nats
    version: \"2.15\"
    dependsOn:
      - {service: cache}
";

/// Builds the catalog of `providers/` into a folder of `root`.
fn built_catalog(root: &TestRoot) -> PathBuf {
    let dir = root.path().join("catalog");
    let output = Command::new(cargo_bin("xtask", "xtask"))
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .args(["catalog", "build", "--out"])
        .arg(&dir)
        .output()
        .expect("xtask runs");
    assert!(
        output.status.success(),
        "the catalog build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}

/// Starts `stacklyd` with the catalog folder `dir` and waits until it is
/// ready and quiet.
fn start(root: &TestRoot, dir: PathBuf) -> SpawnedDaemon {
    let paths = paths(root);
    let closed_before = closed_connections(&paths.log);
    let daemon = SpawnedDaemon::start_with_catalog(
        &cargo_bin("stacklyd", "stacklyd"),
        root.path(),
        &paths.socket,
        &paths.runtime,
        SpawnCatalog::Dir(dir),
        |daemon| daemon.stderr().contains("registry_state") && handshakes(daemon.socket()),
    )
    .expect("stacklyd starts");
    let deadline = Instant::now() + Duration::from_secs(5);
    while closed_connections(&paths.log) <= closed_before {
        assert!(
            Instant::now() < deadline,
            "stacklyd did not log the close of the ready probe: {}",
            daemon.stderr()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon
}

/// `stackly <args>` in the folder `cwd`.
fn run_in(root: &TestRoot, cwd: &Path, args: &[&str]) -> crate::common::Run {
    let mut command = stackly(root.path());
    command.current_dir(cwd).args(args);
    run_command(command)
}

/// The `import_environment` and `remove_environment` rows of `audit list`.
fn environment_operations(root: &TestRoot, cwd: &Path) -> Vec<String> {
    let audit = run_in(root, cwd, &["audit", "list", "--json"]);
    assert_eq!(audit.code, Some(0), "{audit:?}");
    let value = envelope(&audit);
    let mut kinds: Vec<String> = value["result"]["items"]
        .as_array()
        .expect("audit items")
        .iter()
        .filter_map(|item| item["operationKind"].as_str())
        .filter(|kind| kind.ends_with("_environment"))
        .map(str::to_owned)
        .collect();
    kinds.sort();
    kinds
}

// covers: AC-16, AC-17 (spec 0044)
#[test]
fn a_real_daemon_validates_plans_imports_lists_and_removes_a_manifest() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-020"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with the catalog of providers/, stackly env validate prints ok shop 2 services; plan prints install, create, and start rows for cache and queue with exit 0 and writes no file into the manifest folder; import registers it, a second import says Already imported. and runs one more import_environment; list shows shop down 0/2; plan by name works; remove without --yes fails cli.confirmation_required and with --yes removes it, leaving the manifest; an invalid manifest exits 2 with the daemon's finding and an unknown name exits 10",
    );
    let root = TestRoot::new().expect("a test root");
    let dir = built_catalog(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start(&root, dir);
    let project = root.path().join("work/shop");
    std::fs::create_dir_all(&project).expect("the project folder can be made");
    std::fs::write(project.join("stackly.yaml"), MANIFEST).expect("the manifest can be written");
    let before = listing(&project);

    let validated = run_in(&root, &project, &["env", "validate"]);
    assert_eq!(validated.code, Some(0), "{validated:?}");
    assert!(
        validated.stdout.starts_with("ok shop 2 services sha256:"),
        "{}",
        validated.stdout
    );

    let planned = run_in(&root, &project, &["env", "plan"]);
    assert_eq!(planned.code, Some(0), "{planned:?}");
    let rows: Vec<Vec<&str>> = planned
        .stdout
        .lines()
        .skip(1)
        .take_while(|line| !line.is_empty())
        .map(|line| line.split_whitespace().take(4).collect())
        .collect();
    assert_eq!(
        rows,
        [
            ["install", "cache", "shop-cache", "official/valkey"],
            ["create", "cache", "shop-cache", "official/valkey"],
            ["start", "cache", "shop-cache", "official/valkey"],
            ["install", "queue", "shop-queue", "official/nats"],
            ["create", "queue", "shop-queue", "official/nats"],
            ["start", "queue", "shop-queue", "official/nats"],
        ]
    );
    assert_eq!(
        planned.stdout.lines().last(),
        Some("Plan: 2 to install, 2 to create, 2 to start, 0 to keep.")
    );
    let json = run_in(&root, &project, &["env", "plan", "--json"]);
    assert_eq!(json.code, Some(0), "{json:?}");
    let plan = envelope(&json);
    assert_eq!(plan["result"]["registration"], "new");
    assert_eq!(
        plan["result"]["order"],
        serde_json::json!(["cache", "queue"])
    );
    assert_eq!(
        listing(&project),
        before,
        "plan writes nothing beside the manifest"
    );

    let imported = run_in(&root, &project, &["env", "import", "--json"]);
    assert_eq!(imported.code, Some(0), "{imported:?}");
    let detail_value = envelope(&imported);
    let id = detail_value["result"]["summary"]["id"]
        .as_str()
        .expect("an environment id")
        .to_owned();
    assert_eq!(detail_value["result"]["summary"]["name"], "shop");
    let again = run_in(&root, &project, &["env", "import"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    assert_eq!(again.stdout.lines().next(), Some("Already imported."));
    assert_eq!(
        environment_operations(&root, &project),
        ["import_environment", "import_environment"]
    );

    let listed = run_in(&root, &project, &["env", "list"]);
    assert_eq!(listed.code, Some(0), "{listed:?}");
    let row: Vec<&str> = listed
        .stdout
        .lines()
        .nth(1)
        .expect("one row")
        .split_whitespace()
        .collect();
    assert_eq!(&row[..3], ["shop", "down", "0/2"]);
    let by_name = run_in(&root, &project, &["env", "plan", "shop", "--json"]);
    assert_eq!(by_name.code, Some(0), "{by_name:?}");
    assert_eq!(envelope(&by_name)["result"]["registration"], "registered");
    let by_id = run_in(&root, &project, &["env", "plan", &id]);
    assert_eq!(by_id.code, Some(0), "{by_id:?}");

    let refused = run_in(&root, &project, &["env", "remove", "shop"]);
    assert_eq!(refused.code, Some(2), "{refused:?}");
    assert!(
        refused.stderr.contains("cli.confirmation_required"),
        "{}",
        refused.stderr
    );
    let removed = run_in(&root, &project, &["env", "remove", "shop", "--yes"]);
    assert_eq!(removed.code, Some(0), "{removed:?}");
    assert_eq!(
        removed.stdout,
        "Removed environment shop. Instances and data are kept.\n"
    );
    let empty = run_in(&root, &project, &["env", "list"]);
    assert_eq!(empty.stdout, "No environments.\n");
    assert_eq!(
        environment_operations(&root, &project),
        [
            "import_environment",
            "import_environment",
            "remove_environment"
        ]
    );
    assert!(project.join("stackly.yaml").exists(), "the manifest stays");

    let gone = run_in(
        &root,
        &project,
        &["env", "remove", "shop", "--yes", "--json"],
    );
    assert_eq!(gone.code, Some(10), "{gone:?}");
    assert_eq!(
        envelope(&gone)["error"]["code"],
        "cli.environment_not_found"
    );
    std::fs::write(
        project.join("broken.yaml"),
        MANIFEST.replace("official/nats", "official/nope"),
    )
    .expect("the broken manifest can be written");
    let invalid = run_in(
        &root,
        &project,
        &["env", "validate", "--file", "broken.yaml", "--json"],
    );
    assert_eq!(invalid.code, Some(2), "{invalid:?}");
    let value = envelope(&invalid);
    assert_eq!(value["error"]["code"], "cli.env_invalid");
    assert_eq!(detail(&value["error"], "errors").as_deref(), Some("1"));
    assert_eq!(value["result"]["findings"][0]["code"], "unknown_provider");
    let missing = run_in(
        &root,
        &project,
        &["env", "plan", "--file", "nope.yaml", "--json"],
    );
    assert_eq!(missing.code, Some(2), "{missing:?}");
    assert_eq!(
        envelope(&missing)["error"]["code"],
        "env.manifest_unreadable"
    );
    ev.pass();
}
