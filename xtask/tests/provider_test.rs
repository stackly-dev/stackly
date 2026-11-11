//! End to end test of `cargo xtask provider new`, `check`, and `test` on a
//! package outside the repository (spec 0036, AC-14): the real binary
//! scaffolds `testing/mytool` into a test root, checks it, runs the
//! conformance harness on it, and the repository is the same afterwards.
#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::{Command, Output};

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
}

/// `cargo xtask provider <args>`, from the repo root.
fn provider(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .arg("provider")
        .args(args)
        .output()
        .expect("the xtask binary runs")
}

/// `git status --porcelain` of the repository.
fn git_status() -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["status", "--porcelain"])
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git status fails");
    String::from_utf8(output.stdout).expect("git status is UTF-8")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// covers: AC-14, AC-13 (spec 0036)
#[test]
fn a_scaffold_outside_the_repo_passes_check_and_test_without_changing_the_repo() {
    let ev = evidence!(
        covers = ["AC-029", "AC-034"],
        kind = Kind::Fixture,
        expected = "cargo xtask provider new mytool --out <test root>, then provider check and provider test on the result, exit 0 with an ok line and a pass report, the last stderr line names the report, and git status of the repository is the same before and after",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let out = root.path().display().to_string();
    let package = root.path().join("mytool");
    let shown = package.display().to_string();
    let report = root.path().join("report.json");
    let before = git_status();

    let made = provider(&["new", "mytool", "--out", &out]);
    let checked = provider(&["check", &shown]);
    let tested = provider(&["test", &shown, "--report", &report.display().to_string()]);

    assert!(made.status.success(), "{}", text(&made.stderr));
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(text(&checked.stdout).starts_with("ok testing/mytool 0.1.0 "));
    assert_eq!(tested.status.code(), Some(0), "{}", text(&tested.stderr));
    let stderr = text(&tested.stderr);
    assert_eq!(
        stderr.lines().last(),
        Some(format!("provider test report: {}", report.display()).as_str()),
        "{stderr}"
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).expect("the report was written"))
            .expect("the report is JSON");
    assert_eq!(value["schema"], "stackly.provider-conformance/v1");
    assert_eq!(value["verdict"], "pass", "{value:#}");
    assert_eq!(value["package"]["provider"], "testing/mytool");
    assert_eq!(git_status(), before, "the repository did not change");
    assert!(package.is_dir(), "the package folder the user gave stays");
    ev.pass();
}

// covers: AC-12 (spec 0036)
#[test]
fn cleanup_runs_alone_and_exits_0() {
    let output = provider(&["test", "--cleanup"]);

    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        text(&output.stderr)
            .lines()
            .last()
            .is_some_and(|line| line.starts_with("provider test --cleanup: removed ")),
        "{}",
        text(&output.stderr)
    );
}
