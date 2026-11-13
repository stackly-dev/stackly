//! `stackly env` against a real `stacklyd` (spec 0044 AC-16, AC-17): the
//! daemon serves the catalog `cargo xtask catalog build` writes from
//! `providers/` through `STACKLY_CATALOG_DIR`, reads the manifest itself,
//! and runs `import_environment` and `remove_environment`. The fake
//! provider can't be used here: the daemon binary refuses the `testing`
//! namespace in a catalog and `provider import` refuses its fixture
//! artifact, so the manifest names two official providers. Nothing is
//! downloaded, installed, created, or started: validate and plan are read
//! only, import only registers the file, and the one Up is refused before
//! it starts (spec 0046 AC-17, AC-18).

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

// covers: AC-6, AC-16, AC-17 (spec 0045)
#[test]
fn a_real_daemon_locks_plans_locked_and_exports_a_manifest() {
    let ev = evidence!(
        covers = ["REQ-023", "REQ-028", "AC-023"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with the catalog of providers/, env plan --locked without a lock exits 3 with env.lock_missing; env lock writes only stackly.lock beside the manifest, prints added cache and added queue to stderr and Locked 2 services in <file>. to stdout; a second env lock prints <file> is up to date. with kept lines and leaves the bytes; env plan --locked then exits 0 with SOURCE lock on every runtime row and lock: stackly.lock (in_sync, ...) on stderr; --update --service ghost exits 10 with env.service_not_found; export of the file before import exits 10 with env.not_registered, and after import exits 0 with an empty stdout and one skipped: <service>: not_created line per service",
    );
    let root = TestRoot::new().expect("a test root");
    let dir = built_catalog(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start(&root, dir);
    let project = root.path().join("work/shop");
    std::fs::create_dir_all(&project).expect("the project folder can be made");
    std::fs::write(project.join("stackly.yaml"), MANIFEST).expect("the manifest can be written");
    let before = listing(&project);

    let missing = run_in(&root, &project, &["env", "plan", "--locked"]);
    assert_eq!(missing.code, Some(3), "{missing:?}");
    assert!(
        missing.stderr.contains("lock: stackly.lock (absent)\n")
            && missing.stderr.contains("blocked: env.lock_missing: "),
        "{}",
        missing.stderr
    );

    let locked = run_in(&root, &project, &["env", "lock"]);
    assert_eq!(locked.code, Some(0), "{locked:?}");
    assert!(
        locked.stdout.starts_with("Locked 2 services in ")
            && locked.stdout.ends_with("stackly.lock.\n"),
        "{}",
        locked.stdout
    );
    let lines: Vec<&str> = locked.stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{}", locked.stderr);
    assert!(
        lines[0].starts_with("added cache official/valkey 9.1."),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("added queue official/nats 2.15."),
        "{lines:?}"
    );
    let mut expected = before.clone();
    expected.insert(PathBuf::from("stackly.lock"));
    assert_eq!(listing(&project), expected, "lock writes only stackly.lock");
    let bytes = std::fs::read(project.join("stackly.lock")).expect("the lock reads");

    let again = run_in(&root, &project, &["env", "lock"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    assert!(
        again.stdout.ends_with("stackly.lock is up to date.\n"),
        "{}",
        again.stdout
    );
    assert!(
        again.stderr.lines().all(|line| line.starts_with("kept ")),
        "{}",
        again.stderr
    );
    assert_eq!(
        std::fs::read(project.join("stackly.lock")).expect("the lock reads"),
        bytes
    );

    let planned = run_in(&root, &project, &["env", "plan", "--locked"]);
    assert_eq!(planned.code, Some(0), "{planned:?}");
    let sources: Vec<&str> = planned
        .stdout
        .lines()
        .skip(1)
        .take_while(|line| !line.is_empty())
        .map(|line| line.split_whitespace().nth(5).unwrap_or_default())
        .collect();
    assert_eq!(sources.len(), 6, "{}", planned.stdout);
    assert!(
        sources.iter().all(|source| *source == "lock"),
        "{sources:?}"
    );
    assert!(
        planned.stderr.starts_with("lock: stackly.lock (in_sync, "),
        "{}",
        planned.stderr
    );

    let ghost = run_in(
        &root,
        &project,
        &["env", "lock", "--update", "--service", "ghost", "--json"],
    );
    assert_eq!(ghost.code, Some(10), "{ghost:?}");
    assert_eq!(envelope(&ghost)["error"]["code"], "env.service_not_found");

    let unregistered = run_in(&root, &project, &["env", "export", "--json"]);
    assert_eq!(unregistered.code, Some(10), "{unregistered:?}");
    assert_eq!(
        envelope(&unregistered)["error"]["code"],
        "env.not_registered"
    );
    let imported = run_in(&root, &project, &["env", "import"]);
    assert_eq!(imported.code, Some(0), "{imported:?}");
    let exported = run_in(&root, &project, &["env", "export"]);
    assert_eq!(exported.code, Some(0), "{exported:?}");
    assert_eq!(exported.stdout, "");
    assert_eq!(
        exported.stderr,
        "skipped: cache: not_created\nskipped: queue: not_created\n"
    );
    let by_name = run_in(
        &root,
        &project,
        &["env", "export", "shop", "--format", "shell"],
    );
    assert_eq!(by_name.code, Some(0), "{by_name:?}");
    ev.pass();
}

/// [`MANIFEST`] with a reference for the Valkey password that has no value.
const SECRET_MANIFEST: &str = "apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop

services:
  cache:
    provider: official/valkey
    version: \"9.1\"
    secretRefs:
      password: \"local:shop/valkey\"
  queue:
    provider: official/nats
    version: \"2.15\"
    dependsOn:
      - {service: cache}
";

// covers: AC-17, AC-18 (spec 0046)
#[test]
fn a_real_daemon_refuses_an_up_that_needs_a_secret_and_shows_the_status() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with the catalog of providers/, env status <folder> of a file no environment is registered at exits 10 with Run stackly env up or stackly env import.; env up <folder> for a Valkey password reference without a value exits 6 with secret.needs_secret, the reference, and stackly secret set local:shop/valkey, and creates no instance, environment, or operation; after env import, env status <folder> prints shop  Down  0 of 2 ready and both services not_created with exit 0, and its --json result, the env.get reply, equals the one of env status shop",
    );
    let root = TestRoot::new().expect("a test root");
    let dir = built_catalog(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start(&root, dir);
    let work = root.path().join("work");
    let project = work.join("shop");
    std::fs::create_dir_all(&project).expect("the project folder can be made");
    std::fs::write(project.join("stackly.yaml"), SECRET_MANIFEST)
        .expect("the manifest can be written");

    let unknown = run_in(&root, &work, &["env", "status", "./shop"]);
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    assert!(
        unknown
            .stderr
            .contains("  hint: Run stackly env up or stackly env import.\n"),
        "{}",
        unknown.stderr
    );

    let refused = run_in(&root, &work, &["env", "up", "./shop", "--wait"]);
    assert_eq!(refused.code, Some(6), "{refused:?}");
    assert_eq!(refused.stdout, "");
    assert!(
        refused.stderr.contains("  code: secret.needs_secret\n")
            && refused
                .stderr
                .contains("  hint: stackly secret set local:shop/valkey\n")
            && refused
                .stderr
                .contains("  reference: reference=local:shop/valkey "),
        "{}",
        refused.stderr
    );
    let json = run_in(&root, &work, &["env", "up", "./shop", "--json"]);
    assert_eq!(json.code, Some(6), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "secret.needs_secret");
    assert_eq!(
        value["error"]["items"][0]["fields"][0]["value"], "local:shop/valkey",
        "{value}"
    );
    let instances = run_in(&root, &work, &["instance", "list", "--json"]);
    assert_eq!(
        envelope(&instances)["result"]["items"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    let environments = run_in(&root, &work, &["env", "list", "--json"]);
    assert_eq!(
        envelope(&environments)["result"]["items"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    let audit = run_in(&root, &work, &["audit", "list", "--json"]);
    assert_eq!(audit.code, Some(0), "{audit:?}");
    assert!(
        !envelope(&audit)["result"]["items"]
            .as_array()
            .expect("audit items")
            .iter()
            .any(|item| item["operationKind"]
                .as_str()
                .is_some_and(|kind| kind.contains("environment"))),
        "{}",
        audit.stdout
    );

    let imported = run_in(
        &root,
        &work,
        &["env", "import", "--file", "shop/stackly.yaml"],
    );
    assert_eq!(imported.code, Some(0), "{imported:?}");
    let status = run_in(&root, &work, &["env", "status", "./shop"]);
    assert_eq!(status.code, Some(0), "{status:?}");
    let mut lines = status.stdout.lines();
    assert!(
        lines
            .next()
            .is_some_and(|line| line.starts_with("shop  Down  0 of 2 ready  ")),
        "{}",
        status.stdout
    );
    assert!(
        status
            .stdout
            .contains("\nnot ready:\n  cache: not_created\n  queue: not_created\n"),
        "{}",
        status.stdout
    );
    let by_path = envelope(&run_in(
        &root,
        &work,
        &["env", "status", "shop/stackly.yaml", "--json"],
    ));
    let by_name = envelope(&run_in(&root, &work, &["env", "status", "shop", "--json"]));
    assert_eq!(by_path["result"], by_name["result"]);
    assert_eq!(by_name["result"]["summary"]["name"], "shop");
    assert_eq!(
        by_name["result"]["services"]
            .as_array()
            .map(|services| services
                .iter()
                .map(|service| service["notReady"]["reason"].clone())
                .collect::<Vec<_>>()),
        Some(vec![
            serde_json::json!("not_created"),
            serde_json::json!("not_created")
        ])
    );
    let planned = run_in(&root, &work, &["env", "plan", "./shop", "--json"]);
    assert_eq!(envelope(&planned)["result"]["registration"], "registered");
    ev.pass();
}

// covers: AC-18, AC-22 (spec 0047)
#[test]
fn a_real_daemon_brings_down_an_environment_with_nothing_running_even_without_its_manifest() {
    let ev = evidence!(
        covers = ["REQ-022", "REQ-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, after env import of a manifest with two services that were never created, env status prints lease: none under the head line; env down ./shop --wait writes Nothing to stop. to stderr, prints Down. 0 stopped, 0 kept running., and exits 0 with one environment_down operation that succeeded; with the manifest deleted, env down ./shop --json still exits 0 for the same environment and sends no env.plan, and no instance exists afterwards",
    );
    let root = TestRoot::new().expect("a test root");
    let dir = built_catalog(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start(&root, dir);
    let work = root.path().join("work");
    let project = work.join("shop");
    std::fs::create_dir_all(&project).expect("the project folder can be made");
    std::fs::write(project.join("stackly.yaml"), MANIFEST).expect("the manifest can be written");
    let imported = run_in(
        &root,
        &work,
        &["env", "import", "--file", "shop/stackly.yaml"],
    );
    assert_eq!(imported.code, Some(0), "{imported:?}");

    let status = run_in(&root, &work, &["env", "status", "shop"]);
    assert_eq!(status.code, Some(0), "{status:?}");
    assert_eq!(
        status.stdout.lines().nth(1),
        Some("lease: none"),
        "{}",
        status.stdout
    );

    let down = run_in(&root, &work, &["env", "down", "./shop", "--wait"]);
    assert_eq!(down.code, Some(0), "{down:?}");
    assert!(
        down.stderr.contains("Nothing to stop.\n"),
        "{}",
        down.stderr
    );
    assert_eq!(down.stdout, "Down. 0 stopped, 0 kept running.\n");

    std::fs::remove_file(project.join("stackly.yaml")).expect("the manifest can be removed");
    let again = run_in(&root, &work, &["env", "down", "./shop", "--json"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    let value = envelope(&again);
    assert_eq!(value["result"]["preview"]["environment"]["name"], "shop");
    assert_eq!(value["result"]["operation"]["kind"], "environment_down");

    let audit = run_in(&root, &work, &["audit", "list", "--json"]);
    let downs = envelope(&audit)["result"]["items"]
        .as_array()
        .expect("audit items")
        .iter()
        .filter(|item| item["operationKind"] == "environment_down")
        .count();
    assert!(downs >= 1, "{}", audit.stdout);
    let instances = run_in(&root, &work, &["instance", "list", "--json"]);
    assert_eq!(
        envelope(&instances)["result"]["items"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    ev.pass();
}
