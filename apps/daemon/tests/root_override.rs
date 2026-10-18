//! `stacklyd` reads `STACKLY_ROOT` once at start. An accepted override logs
//! the four roots, prepares the layout, opens (here: creates) the registry,
//! logs `registry_state=ready`, and exits 0; a rejected one logs the error
//! and exits 1. A registry newer than the app is left untouched, logged as
//! `registry_state=recovery`, and exits 3 (provisional until feature 6).
//!
//! The home directory test runs without `STACKLY_ROOT`, so it passes the
//! hidden `--roots-only` flag: the daemon stops after resolving the roots
//! and never touches the real Application Support folder.
#![cfg(target_os = "macos")]

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use stackly_testkit::TestRoot;

const STACKLYD: &str = env!("CARGO_BIN_EXE_stacklyd");
/// Offset of `user_version` in the SQLite database header.
const USER_VERSION_OFFSET: u64 = 60;

fn run_with_root(value: &OsStr, args: &[&str]) -> Output {
    Command::new(STACKLYD)
        .args(args)
        .env("STACKLY_ROOT", value)
        .output()
        .expect("stacklyd runs")
}

/// Every JSON log line the daemon wrote to stderr.
fn log_lines(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each log line is JSON"))
        .collect()
}

fn field<'a>(line: &'a Value, name: &str) -> &'a Value {
    &line["fields"][name]
}

/// The one line that carries the field `name`.
fn line_with<'a>(lines: &'a [Value], name: &str) -> &'a Value {
    let matching: Vec<_> = lines
        .iter()
        .filter(|line| !field(line, name).is_null())
        .collect();
    assert_eq!(matching.len(), 1, "lines with {name}: {lines:?}");
    matching[0]
}

fn registry_db(root: &TestRoot) -> PathBuf {
    root.path().join("data/registry/stackly.sqlite")
}

fn contents(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("the registry file can be read")
}

/// Removes the runtime root a daemon run prepared under the real user
/// temporary directory (`STACKLY_ROOT` places only the runtime root
/// there). Only empty folders are removed, never their contents.
struct RuntimeRootCleanup(Option<PathBuf>);

impl RuntimeRootCleanup {
    fn from_lines(lines: &[Value]) -> Self {
        let runtime = lines
            .iter()
            .find_map(|line| field(line, "runtime_root").as_str())
            .map(PathBuf::from);
        Self(runtime)
    }
}

impl Drop for RuntimeRootCleanup {
    fn drop(&mut self) {
        if let Some(runtime) = &self.0 {
            let _ = std::fs::remove_dir(runtime.join("i"));
            let _ = std::fs::remove_dir(runtime);
        }
    }
}

fn assert_roots(line: &Value, root: &TestRoot) {
    let path_of = |name| {
        Path::new(
            field(line, name)
                .as_str()
                .expect("a root is logged as text"),
        )
        .to_path_buf()
    };
    assert_eq!(path_of("data_root"), root.path().join("data"));
    assert_eq!(path_of("logs_root"), root.path().join("logs"));
    assert_eq!(path_of("cache_root"), root.path().join("cache"));
    let runtime = path_of("runtime_root");
    let name = runtime
        .file_name()
        .expect("the runtime root has a name")
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("stackly-") && name.len() == "stackly-".len() + 8,
        "{name}"
    );
    assert_eq!(field(line, "root_override"), true);
}

#[test]
fn an_accepted_root_override_prepares_the_layout_creates_the_registry_and_exits_0() {
    let root = TestRoot::new().unwrap();

    let output = run_with_root(root.path().as_os_str(), &[]);

    let lines = log_lines(&output);
    let _cleanup = RuntimeRootCleanup::from_lines(&lines);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_roots(line_with(&lines, "data_root"), &root);
    let registry = line_with(&lines, "registry_state");
    assert_eq!(registry["level"], "INFO");
    assert_eq!(field(registry, "registry_state"), "ready");
    assert_eq!(field(registry, "created"), true);
    assert_eq!(field(registry, "schema_version"), 1);
    assert!(
        field(registry, "sqlite_version")
            .as_str()
            .unwrap()
            .starts_with("3.")
    );
    assert!(registry_db(&root).is_file());
    assert!(root.path().join("data/instances").is_dir());
}

#[test]
fn a_registry_newer_than_the_app_is_left_untouched_and_the_daemon_exits_3() {
    let root = TestRoot::new().unwrap();
    let first = run_with_root(root.path().as_os_str(), &[]);
    let _cleanup = RuntimeRootCleanup::from_lines(&log_lines(&first));
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    let db = registry_db(&root);
    let mut file = OpenOptions::new().write(true).open(&db).unwrap();
    file.seek(SeekFrom::Start(USER_VERSION_OFFSET)).unwrap();
    file.write_all(&999_u32.to_be_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let before = contents(&db);

    let output = run_with_root(root.path().as_os_str(), &[]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let lines = log_lines(&output);
    let registry = line_with(&lines, "registry_state");
    assert_eq!(registry["level"], "ERROR");
    assert_eq!(field(registry, "registry_state"), "recovery");
    assert_eq!(field(registry, "reason"), "schema_newer_than_app");
    let report = Path::new(field(registry, "report").as_str().unwrap());
    assert!(report.starts_with(root.path().join("data/registry/recovery")));
    assert!(report.is_file());
    assert_eq!(contents(&db), before, "the registry file changed");
}

#[test]
fn roots_only_with_an_override_logs_the_roots_and_creates_nothing() {
    let root = TestRoot::new().unwrap();

    let output = run_with_root(root.path().as_os_str(), &["--roots-only"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let lines = log_lines(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_roots(&lines[0], &root);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn without_an_override_the_home_directory_comes_from_the_user_database_not_home() {
    let fake_home = TestRoot::new().unwrap();

    let output = Command::new(STACKLYD)
        .arg("--roots-only")
        .env_remove("STACKLY_ROOT")
        .env("HOME", fake_home.path())
        .output()
        .expect("stacklyd runs");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let lines = log_lines(&output);
    assert_eq!(
        lines.len(),
        1,
        "--roots-only stops after the roots: {lines:?}"
    );
    let data_root = field(&lines[0], "data_root").as_str().unwrap();
    assert!(
        data_root.ends_with("/Library/Application Support/Stackly"),
        "{data_root}"
    );
    assert!(
        !Path::new(data_root).starts_with(fake_home.path()),
        "{data_root}"
    );
    assert_eq!(field(&lines[0], "root_override"), false);
    assert_eq!(std::fs::read_dir(fake_home.path()).unwrap().count(), 0);
}

#[test]
fn an_empty_root_override_is_rejected_with_exit_1() {
    let output = run_with_root(OsStr::new(""), &[]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let lines = log_lines(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["level"], "ERROR");
    assert_eq!(
        field(&lines[0], "error"),
        r#"root override "" rejected: empty"#
    );
    assert_eq!(field(&lines[0], "root_override"), true);
}

#[test]
fn a_relative_or_missing_root_override_is_rejected_with_exit_1() {
    let root = TestRoot::new().unwrap();
    let missing = root.path().join("missing");

    for (value, reason) in [
        (OsStr::new("relative/root"), "not absolute"),
        (missing.as_os_str(), "does not exist"),
    ] {
        let output = run_with_root(value, &[]);

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let error = log_lines(&output)[0]["fields"]["error"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(error.ends_with(reason), "{error}");
        assert!(
            error.contains(&value.to_string_lossy().into_owned()),
            "{error}"
        );
    }
}
