//! Spec 0045 at the edges of the CLI: `env plan --locked` of a manifest that
//! is not valid prints no lock state (K23), and against a real `stacklyd`
//! an override file with an error lists each override `ignored` and
//! blocks with `env.local_invalid` (K22), an override that names no
//! registered data location blocks with `env.override_location_unknown`,
//! and a lock the folder can't take exits 7 with `env.lock_unwritable` and
//! leaves the lock that was there.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use stackly_protocol::env::LockState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::{ReapGuard, SpawnCatalog, SpawnedDaemon, TestRoot, cargo_bin};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::stacklyd::{closed_connections, handshakes};
use crate::common::{detail, envelope, paths, run, run_command, stackly};
use crate::data;

/// AC-6, AC-12, AC-17 (K23): a plan of a manifest that is not valid, from
/// a daemon that knows `locked` and reads a valid lock and a local file,
/// exits 2 with the findings and prints neither `lock:` nor `local:`.
#[test]
fn plan_locked_of_an_invalid_manifest_exits_2_without_a_lock_or_local_line() {
    let root = TestRoot::new().expect("a test root is made");
    let mut plan = data::invalid_plan();
    plan.lock.file = "stackly.lock".to_owned();
    plan.lock.state = LockState::InSync;
    plan.lock.locked = true;
    plan.local = data::locked_blocked().local;
    assert!(plan.local.is_some(), "the fixture has a local file");
    let validation = data::invalid();
    let _daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::EnvValidate(_) => vec![Step::Reply(Reply::EnvValidate(Box::new(
            validation.clone(),
        )))],
        Call::EnvPlan(_) => vec![Step::Reply(Reply::EnvPlan(Box::new(plan.clone())))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["env", "plan", "--locked"]);
    let json = run(root.path(), &["env", "plan", "--locked", "--json"]);

    assert_eq!(text.code, Some(2), "{text:?}");
    assert_eq!(text.stdout, "");
    assert!(
        !text
            .stderr
            .lines()
            .any(|line| line.starts_with("lock: ") || line.starts_with("local: ")),
        "{}",
        text.stderr
    );
    assert!(text.stderr.contains("2 error(s), 0 warning(s)"));
    assert_eq!(json.code, Some(2), "{json:?}");
    assert_eq!(envelope(&json)["error"]["code"], "cli.env_invalid");
}

const MANIFEST: &str = "apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: edges

services:
  cache:
    provider: official/valkey
    version: \"9.1\"
  queue:
    provider: official/nats
    version: \"2.15\"
";

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

fn run_in(root: &TestRoot, cwd: &Path, args: &[&str]) -> crate::common::Run {
    let mut command = stackly(root.path());
    command.current_dir(cwd).args(args);
    run_command(command)
}

fn local_file(project: &Path, body: &str) {
    std::fs::write(
        project.join(".stackly.local.yaml"),
        format!("apiVersion: stackly/v1alpha1\nkind: EnvironmentLocal\nservices:\n{body}"),
    )
    .expect("the override file writes");
}

// covers: AC-9, AC-11, AC-12, AC-16, AC-17 (spec 0045, K22, K23)
#[test]
fn a_real_daemon_ignores_an_override_file_with_an_error_and_refuses_a_lock_it_cannot_write() {
    let root = TestRoot::new().expect("a test root");
    let dir = built_catalog(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start(&root, dir);
    let project = root.path().join("work/edges");
    std::fs::create_dir_all(&project).expect("the project folder can be made");
    std::fs::write(project.join("stackly.yaml"), MANIFEST).expect("the manifest writes");
    let locked = run_in(&root, &project, &["env", "lock"]);
    assert_eq!(locked.code, Some(0), "{locked:?}");
    let lock_bytes = std::fs::read(project.join("stackly.lock")).expect("the lock reads");

    // K22: a good port beside a refused data location applies neither.
    local_file(
        &project,
        "  cache:\n    endpoints:\n      client: {port: 16399}\n  queue:\n    storage:\n      dataLocation: relative/path\n",
    );
    let invalid = run_in(&root, &project, &["env", "plan"]);
    assert_eq!(invalid.code, Some(3), "{invalid:?}");
    assert!(
        invalid
            .stderr
            .contains("cache.endpoints.client.port: 16399 (ignored)"),
        "{}",
        invalid.stderr
    );
    assert!(!invalid.stderr.contains("(applied)"), "{}", invalid.stderr);
    assert!(
        invalid.stderr.contains("blocked: env.local_invalid: "),
        "{}",
        invalid.stderr
    );
    assert!(!invalid.stdout.contains("16399"), "{}", invalid.stdout);
    let json = run_in(&root, &project, &["env", "plan", "--json"]);
    let value = envelope(&json);
    assert_eq!(value["result"]["local"]["state"], "invalid");
    assert!(
        value["result"]["local"]["overrides"]
            .as_array()
            .expect("overrides")
            .iter()
            .all(|o| o["state"] == "ignored"),
        "{value}"
    );

    // A path that is no registered data location.
    let elsewhere = root.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a folder");
    local_file(
        &project,
        &format!(
            "  queue:\n    storage:\n      dataLocation: {}\n",
            elsewhere.display()
        ),
    );
    let unknown = run_in(&root, &project, &["env", "plan", "--json"]);
    assert_eq!(unknown.code, Some(3), "{unknown:?}");
    let value = envelope(&unknown);
    let codes: Vec<&str> = value["result"]["blockers"]
        .as_array()
        .expect("blockers")
        .iter()
        .filter_map(|b| b["error"]["code"].as_str())
        .collect();
    assert_eq!(codes, ["env.override_location_unknown"], "{value}");

    // K23: the same lock and override next to a manifest that is not
    // valid: only the findings.
    std::fs::write(
        project.join("stackly.yaml"),
        MANIFEST.replace("kind: Environment", "kind: Environmnt"),
    )
    .expect("writes");
    let broken = run_in(&root, &project, &["env", "plan", "--locked"]);
    assert_eq!(broken.code, Some(2), "{broken:?}");
    assert!(
        !broken
            .stderr
            .lines()
            .any(|line| line.starts_with("lock: ") || line.starts_with("local: ")),
        "{}",
        broken.stderr
    );
    std::fs::write(project.join("stackly.yaml"), MANIFEST).expect("writes");
    std::fs::remove_file(project.join(".stackly.local.yaml")).expect("removes");

    // AC-16: a selector without an artifact blocks, writes nothing to
    // stdout, and leaves the lock.
    std::fs::write(
        project.join("stackly.yaml"),
        MANIFEST.replace("version: \"9.1\"", "version: \"8\""),
    )
    .expect("writes");
    let blocked = run_in(&root, &project, &["env", "lock", "--update"]);
    assert_eq!(blocked.code, Some(3), "{blocked:?}");
    assert_eq!(blocked.stdout, "");
    assert!(
        blocked
            .stderr
            .contains("blocked: cache: runtime.selector_unresolved: "),
        "{}",
        blocked.stderr
    );
    let json = run_in(&root, &project, &["env", "lock", "--update", "--json"]);
    assert_eq!(json.code, Some(3), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.env_lock_blocked");
    assert_eq!(value["result"]["written"], false);
    assert_eq!(
        std::fs::read(project.join("stackly.lock")).expect("reads"),
        lock_bytes
    );

    // A folder without write access takes no new lock.
    std::fs::write(
        project.join("stackly.yaml"),
        MANIFEST.replace("version: \"9.1\"", "version: stable"),
    )
    .expect("writes");
    std::fs::set_permissions(&project, std::fs::Permissions::from_mode(0o500)).expect("read only");
    let refused = run_in(&root, &project, &["env", "lock", "--json"]);
    std::fs::set_permissions(&project, std::fs::Permissions::from_mode(0o700)).expect("writable");
    assert_eq!(refused.code, Some(7), "{refused:?}");
    let value = envelope(&refused);
    assert_eq!(value["error"]["code"], "env.lock_unwritable");
    assert_eq!(
        detail(&value["error"], "reason").as_deref(),
        Some("permission_denied")
    );
    assert_eq!(
        std::fs::read(project.join("stackly.lock")).expect("reads"),
        lock_bytes,
        "the lock that was there stays"
    );
}
