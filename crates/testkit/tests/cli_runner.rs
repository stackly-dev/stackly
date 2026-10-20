//! `CliRunner` runs a binary with the environment and flags of the
//! end to end suite (spec 0014). A shell script stands in for `stackly`.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use stackly_testkit::{CliRunner, TestRoot};

fn script(root: &TestRoot, body: &str) -> PathBuf {
    let path = root.path().join("fake-stackly");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("the script is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .expect("the script is executable");
    path
}

const ECHO: &str = r#"printf '{"args":"%s","root":"%s","noInput":"%s","noColor":"%s"}' "$*" "$STACKLY_ROOT" "$STACKLY_NO_INPUT" "$NO_COLOR""#;

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the CLI runner rules of the end to end suite).
#[test]
fn a_run_sets_the_environment_and_adds_json() {
    let root = TestRoot::new().unwrap();
    let runner = CliRunner::new(&script(&root, ECHO), root.path());

    let output = runner.run(&["instance", "get", "abc"]).unwrap();

    assert_eq!(output.code(), Some(0));
    let json = output.json().unwrap();
    assert_eq!(json["args"], "instance get abc --json");
    assert_eq!(json["root"], root.path().to_string_lossy().as_ref());
    assert_eq!(json["noInput"], "1");
    assert_eq!(json["noColor"], "1");
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the CLI runner rules of the end to end suite).
#[test]
fn a_wait_without_a_timeout_gets_sixty_seconds_and_an_own_timeout_stays() {
    let root = TestRoot::new().unwrap();
    let runner = CliRunner::new(&script(&root, ECHO), root.path());

    let plain = runner.run(&["instance", "start", "x", "--wait"]).unwrap();
    let own = runner
        .run(&[
            "instance",
            "start",
            "x",
            "--wait",
            "--timeout",
            "5s",
            "--json",
        ])
        .unwrap();

    assert_eq!(
        plain.json().unwrap()["args"],
        "instance start x --wait --json --timeout 60s"
    );
    assert_eq!(
        own.json().unwrap()["args"],
        "instance start x --wait --timeout 5s --json"
    );
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the CLI runner rules of the end to end suite).
#[test]
fn stdin_is_null_unless_the_test_passes_bytes() {
    let root = TestRoot::new().unwrap();
    let runner = CliRunner::new(&script(&root, "cat"), root.path());

    let none = runner.run(&[]).unwrap();
    let some = runner.run_with_stdin(&[], b"fixture bytes\n").unwrap();

    assert_eq!(none.stdout, "");
    assert_eq!(some.stdout, "fixture bytes\n");
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the CLI runner rules of the end to end suite).
#[test]
fn a_failing_run_keeps_its_code_and_streams_and_non_json_stdout_is_an_error() {
    let root = TestRoot::new().unwrap();
    let runner = CliRunner::new(
        &script(&root, "echo oops >&2; echo plain; exit 7"),
        root.path(),
    );

    let output = runner.run(&[]).unwrap();

    assert_eq!(output.code(), Some(7));
    assert_eq!(output.stderr, "oops\n");
    assert!(output.json().unwrap_err().to_string().contains("plain"));
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the CLI runner rules of the end to end suite).
#[test]
fn a_run_that_outlives_the_limit_is_killed_and_reported() {
    let root = TestRoot::new().unwrap();
    let runner = CliRunner::new(&script(&root, "exec sleep 30"), root.path());

    let error = runner
        .run_with_timeout(&[], None, Duration::from_millis(300))
        .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
}
