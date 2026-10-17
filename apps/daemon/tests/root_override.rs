//! `stacklyd` reads `STACKLY_ROOT` once at start: an accepted override logs
//! the four roots and exits 0, and a rejected one logs the error and exits
//! 1. The daemon creates nothing yet, so the test root stays empty.
#![cfg(target_os = "macos")]

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use stackly_testkit::TestRoot;

const STACKLYD: &str = env!("CARGO_BIN_EXE_stacklyd");

fn run_with_root(value: &OsStr) -> Output {
    Command::new(STACKLYD)
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

#[test]
fn an_accepted_root_override_logs_the_four_roots_and_exits_0() {
    let root = TestRoot::new().unwrap();

    let output = run_with_root(root.path().as_os_str());

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let lines = log_lines(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert_eq!(line["level"], "INFO");
    let path_of = |name| Path::new(field(line, name).as_str().unwrap()).to_path_buf();
    assert_eq!(path_of("data_root"), root.path().join("data"));
    assert_eq!(path_of("logs_root"), root.path().join("logs"));
    assert_eq!(path_of("cache_root"), root.path().join("cache"));
    let runtime = path_of("runtime_root");
    let name = runtime.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("stackly-") && name.len() == "stackly-".len() + 8,
        "{name}"
    );
    assert_eq!(field(line, "root_override"), true);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn without_an_override_the_home_directory_comes_from_the_user_database_not_home() {
    let fake_home = TestRoot::new().unwrap();

    let output = Command::new(STACKLYD)
        .env_remove("STACKLY_ROOT")
        .env("HOME", fake_home.path())
        .output()
        .expect("stacklyd runs");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let lines = log_lines(&output);
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
    let output = run_with_root(OsStr::new(""));

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
        let output = run_with_root(value);

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
