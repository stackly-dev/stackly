//! The `skeleton_seed` example of `stacklyd` (spec 0014 AC-1 and AC-2): it
//! seeds a development root with the fake runtime and the fake provider
//! package, prints a create command that works against a real daemon, does
//! nothing the second time, and refuses a bad root without writing.
#![cfg(target_os = "macos")]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::{daemon_paths, start_daemon};
use serde_json::Value;
use stackly_core::{daemon_info, prepare_layout};
use stackly_domain::Registry;
use stackly_platform_macos::MacosPlatform;
use stackly_storage::{RegistryOpen, open_registry};
use stackly_testkit::{ReapGuard, TestRoot, cargo_bin, cargo_example, fake_server};
use uuid::Uuid;

/// How long the printed command gets before the test kills it and fails.
const CLI_LIMIT: Duration = Duration::from_secs(90);

fn seed_with(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(cargo_example("stacklyd", "skeleton_seed"))
        .args(args)
        .output()
        .expect("skeleton_seed runs")
}

fn seed(root: &TestRoot) -> Output {
    let fake = fake_server::binary();
    seed_with(&[
        "--root".as_ref(),
        root.path().as_os_str(),
        "--fake-server".as_ref(),
        fake.as_os_str(),
        "--json".as_ref(),
    ])
}

fn seed_json(root: &TestRoot) -> Value {
    let output = seed(root);
    assert!(
        output.status.success(),
        "the seed failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("the seed prints one JSON object")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The names of every file and folder under the runtime and provider
/// folders of the root.
fn seeded_paths(root: &TestRoot) -> BTreeSet<PathBuf> {
    fn walk(dir: &Path, into: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            into.insert(entry.path());
            walk(&entry.path(), into);
        }
    }
    let layout = daemon_paths(root).layout;
    let mut paths = BTreeSet::new();
    walk(&layout.runtimes_dir(), &mut paths);
    walk(&layout.providers_dir(), &mut paths);
    paths
}

/// Opens the registry of `root` in process, which makes the layout and an
/// empty registry when the root has none, as a daemon start would. The
/// daemon must not run on the root.
fn open(root: &TestRoot) -> stackly_storage::SqliteRegistry {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
    prepare_layout(&platform).expect("the layout can be prepared");
    let layout = daemon_paths(root).layout;
    let RegistryOpen::Ready { registry, .. } =
        open_registry(&platform, &layout, daemon_info().version).expect("the registry opens")
    else {
        panic!("the registry opens ready");
    };
    registry
}

/// How many runtime rows and fake provider rows the registry of the root
/// holds. The provider row is looked up by the digest folder on disk.
fn rows(root: &TestRoot) -> (usize, usize) {
    let registry = open(root);
    let runtimes = registry.runtimes().expect("the registry reads").value.len();
    let providers = pinned_key(root).map_or(0, |key| {
        usize::from(
            registry
                .provider_revision(&key)
                .expect("the registry reads")
                .value
                .is_some(),
        )
    });
    (runtimes, providers)
}

/// The key of the fake provider folder on disk, if there is one: the only
/// digest folder of its only revision.
fn pinned_key(root: &TestRoot) -> Option<stackly_domain::ProviderRevisionKey> {
    let layout = daemon_paths(root).layout;
    let revision = only_child(&layout.providers_dir().join("testing/fake"))?;
    let digest = only_child(&revision)?;
    let name_of = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .expect("a folder is named")
            .to_owned()
    };
    Some(stackly_domain::ProviderRevisionKey::new(
        "testing/fake".parse().expect("the fake provider name"),
        name_of(&revision)
            .parse()
            .expect("the folder is a revision label"),
        name_of(&digest)
            .parse()
            .expect("the folder is a manifest digest"),
    ))
}

fn only_child(dir: &Path) -> Option<PathBuf> {
    let mut children = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path());
    let child = children.next()?;
    assert!(children.next().is_none(), "{} has one child", dir.display());
    Some(child)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the target folder can be made");
    for entry in std::fs::read_dir(from).expect("the folder reads").flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the type reads").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a file copies");
        }
    }
}

#[test]
fn seeding_an_empty_root_prints_ids_and_a_command_the_daemon_accepts() {
    let root = TestRoot::new().expect("a test root");

    let seeded = seed_json(&root);

    let canonical = root
        .path()
        .canonicalize()
        .expect("the test root canonicalizes");
    assert_eq!(seeded["root"], canonical.display().to_string());
    assert_eq!(seeded["provider"]["name"], "testing/fake");
    let runtime_id: Uuid = seeded["runtimeId"]
        .as_str()
        .expect("a runtime ID")
        .parse()
        .expect("the runtime ID is a UUID");
    let revision = seeded["provider"]["revision"].as_str().expect("a revision");
    assert_eq!(rows(&root), (1, 1));
    let command = seeded["createCommand"].as_str().expect("a command");
    assert!(
        command.starts_with(&format!(
            "STACKLY_ROOT={} stackly instance create demo",
            canonical.display()
        )),
        "the command names the root: {command}"
    );
    assert!(command.contains(&format!(
        "--runtime {runtime_id} --provider testing/fake --revision {revision}"
    )));

    let _reap = ReapGuard::new(root.path(), &daemon_paths(&root).runtime);
    let daemon = start_daemon(&root);
    let created = run_printed_command(&root, command);
    assert_eq!(
        created["result"]["operation"]["state"], "succeeded",
        "{created}"
    );
    daemon.stop().expect("the daemon stops");
}

/// Runs the command the seed printed with the real `stackly` binary, noninteractive,
/// with `--timeout` and `--json` added, and returns the envelope.
fn run_printed_command(root: &TestRoot, command: &str) -> Value {
    let words: Vec<&str> = command.split(' ').collect();
    assert!(
        words[0].starts_with("STACKLY_ROOT=") && words[1] == "stackly",
        "{command}"
    );
    assert!(!command.contains('\''), "no quoting expected: {command}");
    let mut child = Command::new(cargo_bin("stackly-cli", "stackly"))
        .args(&words[2..])
        .args(["--timeout", "60s", "--json"])
        .env("STACKLY_ROOT", root.path())
        .env("STACKLY_NO_INPUT", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stackly runs");
    let deadline = Instant::now() + CLI_LIMIT;
    while child.try_wait().expect("stackly can be polled").is_none() {
        if Instant::now() > deadline {
            child.kill().expect("stackly can be killed");
            panic!("{command} did not finish in {CLI_LIMIT:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("stackly output reads");
    assert!(
        output.status.success(),
        "{command} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stackly prints JSON")
}

#[test]
fn seeding_again_writes_nothing_and_prints_the_same_ids() {
    let root = TestRoot::new().expect("a test root");
    let first = seed_json(&root);
    let paths = seeded_paths(&root);

    let second = seed_json(&root);

    assert_eq!(second, first);
    assert_eq!(seeded_paths(&root), paths);
    assert_eq!(rows(&root), (1, 1));
}

#[test]
fn the_text_output_has_four_labelled_lines() {
    let root = TestRoot::new().expect("a test root");
    let fake = fake_server::binary();

    let output = seed_with(&[
        "--root".as_ref(),
        root.path().as_os_str(),
        "--fake-server".as_ref(),
        fake.as_os_str(),
    ]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = String::from_utf8_lossy(&output.stdout);
    let labels: Vec<&str> = text
        .lines()
        .map(|line| line.split_whitespace().next().unwrap_or_default())
        .collect();
    assert_eq!(labels, ["root", "runtime", "provider", "next"], "{text}");
}

#[test]
fn a_missing_root_argument_is_a_usage_error() {
    let output = seed_with(&[]);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_relative_or_missing_root_is_refused_with_one_error_line() {
    let root = TestRoot::new().expect("a test root");
    let missing = root.path().join("nowhere");

    for bad in [Path::new("relative/root"), missing.as_path()] {
        let output = seed_with(&["--root".as_ref(), bad.as_os_str()]);

        assert_eq!(output.status.code(), Some(1), "{bad:?}");
        let stderr = stderr_of(&output);
        assert!(stderr.starts_with("error: "), "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
    assert!(!missing.exists(), "the refusal created nothing");
}

#[test]
fn a_root_with_a_running_daemon_is_refused_and_unchanged() {
    let root = TestRoot::new().expect("a test root");
    seed_json(&root);
    let paths = seeded_paths(&root);
    let _reap = ReapGuard::new(root.path(), &daemon_paths(&root).runtime);
    let daemon = start_daemon(&root);

    let output = seed(&root);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(stderr.contains("stop the daemon"), "{stderr}");
    daemon.stop().expect("the daemon stops");
    assert_eq!(seeded_paths(&root), paths);
    assert_eq!(rows(&root), (1, 1));
}

#[test]
fn a_missing_fake_server_binary_names_the_build_command() {
    let root = TestRoot::new().expect("a test root");
    let missing = root.path().join("no-such-server");

    let output = seed_with(&[
        "--root".as_ref(),
        root.path().as_os_str(),
        "--fake-server".as_ref(),
        missing.as_os_str(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("cargo build -p stackly-fake-server"),
        "{stderr}"
    );
    assert!(seeded_paths(&root).is_empty());
}

#[test]
fn folders_without_registry_rows_are_a_leftover_and_nothing_changes() {
    let seeded = TestRoot::new().expect("a test root");
    seed_json(&seeded);
    // A root whose registry is empty but whose folders hold the seed.
    let root = TestRoot::new().expect("a test root");
    assert_eq!(rows(&root), (0, 0));
    let layout = daemon_paths(&root).layout;
    let from = daemon_paths(&seeded).layout;
    copy_tree(&from.runtimes_dir(), &layout.runtimes_dir());
    copy_tree(&from.providers_dir(), &layout.providers_dir());
    let paths = seeded_paths(&root);

    let output = seed(&root);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(
        stderr.contains("exists without its registry row"),
        "{stderr}"
    );
    assert_eq!(seeded_paths(&root), paths);
    assert_eq!(rows(&root), (0, 0));
}

#[test]
fn another_fake_server_binary_for_the_same_version_is_a_conflict() {
    let root = TestRoot::new().expect("a test root");
    seed_json(&root);
    let paths = seeded_paths(&root);
    let other = root.path().join("other-server");
    std::fs::write(&other, b"not the fake server").expect("another binary can be written");

    let output = seed_with(&[
        "--root".as_ref(),
        root.path().as_os_str(),
        "--fake-server".as_ref(),
        other.as_os_str(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("seed a new root"), "{stderr}");
    assert_eq!(seeded_paths(&root), paths);
    assert_eq!(rows(&root), (1, 1));
}
