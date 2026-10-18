//! `stacklyd` reads `STACKLY_ROOT` and `STACKLY_LOG` once at start.
//!
//! `--print-roots` logs the four roots, the socket path, and whether an
//! override is set, then exits 0 (1 for a rejected override) without
//! creating anything, so the home directory test never touches the real
//! Application Support folder (spec 0006 AC-17). Without the flag an
//! accepted override starts the daemon: it prepares the layout, takes the
//! lock, logs the start line, opens (here: creates) the registry, and runs
//! until SIGTERM, then exits 0. A registry newer than the app is left
//! untouched and the daemon stays up.
#![cfg(target_os = "macos")]

mod common;

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

use common::{STACKLYD, daemon_paths, field, json_lines, line_with, start_daemon};
use serde_json::Value;
use stackly_testkit::TestRoot;

/// Offset of `user_version` in the SQLite database header.
const USER_VERSION_OFFSET: u64 = 60;

fn print_roots(root: &OsStr) -> Output {
    Command::new(STACKLYD)
        .arg("--print-roots")
        .env("STACKLY_ROOT", root)
        .env_remove("STACKLY_LOG")
        .output()
        .expect("stacklyd runs")
}

fn stderr_lines(output: &Output) -> Vec<Value> {
    json_lines(&String::from_utf8_lossy(&output.stderr))
}

fn registry_db(root: &TestRoot) -> PathBuf {
    root.path().join("data/registry/stackly.sqlite")
}

fn contents(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("the file can be read")
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
    let paths = daemon_paths(root);
    assert_eq!(path_of("data_root"), root.path().join("data"));
    assert_eq!(path_of("logs_root"), root.path().join("logs"));
    assert_eq!(path_of("cache_root"), root.path().join("cache"));
    let runtime = path_of("runtime_root");
    assert_eq!(runtime, paths.runtime);
    let name = runtime
        .file_name()
        .expect("the runtime root has a name")
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("stackly-") && name.len() == "stackly-".len() + 8,
        "{name}"
    );
    assert_eq!(path_of("socket_path"), paths.socket);
    assert_eq!(paths.socket, runtime.join("stacklyd.sock"));
    assert_eq!(field(line, "root_override"), true);
}

#[test]
fn print_roots_with_an_override_logs_the_roots_and_the_socket_and_creates_nothing() {
    let root = TestRoot::new().unwrap();

    let output = print_roots(root.path().as_os_str());

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let lines = stderr_lines(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["level"], "INFO");
    assert_roots(&lines[0], &root);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(!daemon_paths(&root).runtime.exists());
}

#[test]
fn print_roots_without_an_override_takes_the_home_directory_from_the_user_database_not_home() {
    let fake_home = TestRoot::new().unwrap();

    let output = Command::new(STACKLYD)
        .arg("--print-roots")
        .env_remove("STACKLY_ROOT")
        .env_remove("STACKLY_LOG")
        .env("HOME", fake_home.path())
        .output()
        .expect("stacklyd runs");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let lines = stderr_lines(&output);
    assert_eq!(
        lines.len(),
        1,
        "--print-roots stops after the roots: {lines:?}"
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
    let socket = field(&lines[0], "socket_path").as_str().unwrap();
    assert!(socket.ends_with("/stackly/stacklyd.sock"), "{socket}");
    assert_eq!(field(&lines[0], "root_override"), false);
    assert_eq!(std::fs::read_dir(fake_home.path()).unwrap().count(), 0);
}

#[test]
fn an_empty_root_override_is_rejected_with_exit_1() {
    let output = print_roots(OsStr::new(""));

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let lines = stderr_lines(&output);
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
        let output = print_roots(value);

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let lines = stderr_lines(&output);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let error = field(&lines[0], "error").as_str().unwrap().to_owned();
        assert!(error.ends_with(reason), "{error}");
        assert!(
            error.contains(&value.to_string_lossy().into_owned()),
            "{error}"
        );
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn an_invalid_log_filter_is_rejected_with_exit_1_before_the_roots_are_read() {
    let root = TestRoot::new().unwrap();

    let output = Command::new(STACKLYD)
        .arg("--print-roots")
        .env("STACKLY_ROOT", root.path())
        .env("STACKLY_LOG", "stacklyd=loud")
        .output()
        .expect("stacklyd runs");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let lines = stderr_lines(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["level"], "ERROR");
    let error = field(&lines[0], "error").as_str().unwrap();
    assert!(
        error.starts_with(r#"STACKLY_LOG "stacklyd=loud" is not a valid log filter"#),
        "{error}"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn an_unknown_argument_exits_2() {
    let output = Command::new(STACKLYD)
        .arg("--no-such-flag")
        .env_remove("STACKLY_ROOT")
        .output()
        .expect("stacklyd runs");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn an_accepted_override_starts_the_daemon_with_a_registry_and_sigterm_stops_it_with_exit_0() {
    let root = TestRoot::new().unwrap();
    let paths = daemon_paths(&root);
    let daemon = start_daemon(&root);
    let pid = daemon.pid();

    let lines = json_lines(&daemon.stderr());
    let start = line_with(&lines, "server_instance_id");
    assert_eq!(start["level"], "INFO");
    assert_eq!(field(start, "message"), "stacklyd started");
    assert_roots(start, &root);
    assert_eq!(field(start, "pid"), pid);
    assert_eq!(field(start, "version"), env!("CARGO_PKG_VERSION"));
    assert_eq!(field(start, "protocol_major"), 1);
    assert_eq!(
        field(start, "contract_digest"),
        stackly_protocol::CONTRACT_DIGEST
    );
    assert!(field(start, "uid").as_u64().is_some());
    let registry = line_with(&lines, "registry_state");
    assert_eq!(registry["level"], "INFO");
    assert_eq!(field(registry, "registry_state"), "ready");
    assert_eq!(field(registry, "created"), true);
    assert_eq!(field(registry, "pruned_rows"), 0);
    assert_eq!(
        field(registry, "schema_version"),
        stackly_storage::MigrationSet::builtin().latest()
    );
    assert!(
        field(registry, "sqlite_version")
            .as_str()
            .unwrap()
            .starts_with("3.")
    );
    assert!(registry_db(&root).is_file());
    assert!(root.path().join("data/instances").is_dir());

    let lock: Value = serde_json::from_slice(&contents(&paths.layout.daemon_lock())).unwrap();
    assert_eq!(lock["pid"], pid);
    assert_eq!(
        &lock["serverInstanceId"],
        field(start, "server_instance_id")
    );
    assert_eq!(&lock["startedAt"], field(start, "started_at"));
    let log = paths.layout.daemon_log_dir().join("stacklyd.log");
    let mode = std::fs::metadata(&log).unwrap().permissions().mode();
    assert_eq!(mode & 0o7777, 0o600);
    let logged = json_lines(&String::from_utf8_lossy(&contents(&log)));
    assert_eq!(
        line_with(&logged, "server_instance_id"),
        start,
        "the file holds the same start line"
    );

    let status = daemon.stop().unwrap();

    assert_eq!(status.code(), Some(0), "{status:?}");
    let logged = json_lines(&String::from_utf8_lossy(&contents(&log)));
    let messages: Vec<_> = logged.iter().map(|line| field(line, "message")).collect();
    assert!(
        messages.contains(&&Value::from("stacklyd stopped")),
        "{messages:?}"
    );
    assert_eq!(field(line_with(&logged, "signal"), "signal"), "SIGTERM");
    assert!(!paths.runtime.exists(), "the empty runtime root is removed");
}

#[test]
fn a_registry_newer_than_the_app_is_left_untouched_and_the_daemon_stays_up_until_sigterm() {
    let root = TestRoot::new().unwrap();
    let first = start_daemon(&root);
    assert_eq!(first.stop().unwrap().code(), Some(0));
    let db = registry_db(&root);
    let mut file = OpenOptions::new().write(true).open(&db).unwrap();
    file.seek(SeekFrom::Start(USER_VERSION_OFFSET)).unwrap();
    file.write_all(&999_u32.to_be_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let before = contents(&db);

    let mut daemon = start_daemon(&root);

    let lines = json_lines(&daemon.stderr());
    let registry = line_with(&lines, "registry_state");
    assert_eq!(registry["level"], "ERROR");
    assert_eq!(field(registry, "registry_state"), "recovery");
    assert_eq!(field(registry, "reason"), "schema_newer_than_app");
    let report = Path::new(field(registry, "report").as_str().unwrap());
    assert!(report.starts_with(root.path().join("data/registry/recovery")));
    assert!(report.is_file());
    thread::sleep(Duration::from_millis(200));
    assert_eq!(daemon.try_status().unwrap(), None, "the daemon stays up");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    assert_eq!(contents(&db), before, "the registry file changed");
}
