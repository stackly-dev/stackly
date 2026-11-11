//! End to end test of `cargo xtask docs check` (spec 0038, AC-9): the real
//! binary checks the public docs of this repo, reads the `stackly` command
//! table through `stackly manpage --json`, and finds nothing wrong.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
}

// covers: AC-2, AC-7, AC-9, AC-10, AC-12, AC-13, AC-14 (spec 0038)
#[test]
fn the_public_docs_of_this_repo_pass_the_docs_check() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .args(["docs", "check"])
        .output()
        .expect("the xtask binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "docs check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.starts_with("ok docs "), "{stdout}");
}

#[test]
fn an_unknown_docs_task_is_refused() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .args(["docs", "nope"])
        .output()
        .expect("the xtask binary runs");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown docs task `nope`"));
}

// covers: AC-8 (spec 0038)
#[test]
fn the_walkthrough_refuses_an_unknown_argument_before_it_clones() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .args(["docs", "walkthrough", "--nope"])
        .output()
        .expect("the xtask binary runs");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("usage: cargo xtask docs walkthrough [--keep]")
    );
}

// covers: AC-12 (spec 0038)
#[test]
fn the_trying_locally_page_quotes_the_approval_statement_of_the_fixture() {
    let read = |rel: &str| {
        std::fs::read_to_string(repo_root().join(rel)).expect("the file is in the repo")
    };
    let words = |text: &str| {
        text.lines()
            .map(|line| line.trim_start().trim_start_matches('>'))
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let page = words(&read("docs/providers/trying-locally.md"));
    let statement = words(&read("protocols/fixtures/provider/approval-statement.txt"));
    assert!(
        page.contains(&statement),
        "the page does not quote: {statement}"
    );
    // The page says plainly that a scaffolded package cannot be imported,
    // and that a pull request is never imported before its review.
    assert!(page.contains("cannot be imported"));
    assert!(page.contains("before the review is done"));
}

/// Runs `cargo xtask docs check` with `CARGO` set to `cargo`, which reads
/// the `stackly` command table.
fn docs_check_with_cargo(cargo: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .env("CARGO", cargo)
        .args(["docs", "check"])
        .output()
        .expect("the xtask binary runs")
}

/// A fake `cargo` that ignores its arguments and runs `script`.
fn fake_cargo(dir: &Path, script: &str) -> std::path::PathBuf {
    let path = dir.join("cargo");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("the fake cargo is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the fake cargo is executable");
    path
}

// covers: AC-9 (spec 0038)
#[test]
fn an_unreadable_stackly_command_table_exits_2_with_no_problem_lines() {
    let temp = tempfile::tempdir().expect("a temp dir");
    let cargo = fake_cargo(temp.path(), "echo broken >&2; exit 3");
    let output = docs_check_with_cargo(&cargo);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot read the stackly command table"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// covers: AC-9 (spec 0038)
#[test]
fn a_command_table_with_no_pages_exits_2() {
    let temp = tempfile::tempdir().expect("a temp dir");
    let cargo = fake_cargo(temp.path(), r#"echo '{"result":{"pages":[]}}'"#);
    let output = docs_check_with_cargo(&cargo);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("lists no pages"));
}

// covers: AC-2, AC-9 (spec 0038)
#[test]
fn a_command_the_table_lacks_exits_1_with_one_sorted_path_line_problem_each() {
    // Only `stackly` and `stackly daemon status` exist, so every other
    // `stackly` command in a bash block of the public docs is unknown.
    let temp = tempfile::tempdir().expect("a temp dir");
    let cargo = fake_cargo(
        temp.path(),
        r#"echo '{"result":{"pages":[{"file":"stackly.1"},{"file":"stackly-daemon.1"},{"file":"stackly-daemon-status.1"}]}}'"#,
    );
    let output = docs_check_with_cargo(&cargo);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    let line = regex::Regex::new(r"^([^:]+):([0-9]+): unknown stackly command `stackly [a-z -]+`$")
        .expect("the pattern compiles");
    let mut keys = Vec::new();
    for problem in stdout.lines() {
        let caps = line
            .captures(problem)
            .unwrap_or_else(|| panic!("not a path:line problem: {problem}"));
        let number: usize = caps[2].parse().expect("a line number");
        keys.push((caps[1].to_owned(), number));
    }
    assert!(
        keys.iter()
            .any(|(path, _)| path == "docs/providers/trying-locally.md"),
        "{stdout}"
    );
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "problems are sorted by path then line");
}

// covers: AC-9 (spec 0038)
#[test]
fn the_docs_check_writes_nothing_in_the_repo() {
    let status = || {
        let output = Command::new("git")
            .current_dir(repo_root())
            .args(["status", "--porcelain", "--untracked-files=all"])
            .output()
            .expect("git runs");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let before = status();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .args(["docs", "check"])
        .output()
        .expect("the xtask binary runs");
    assert!(output.status.success());
    assert_eq!(status(), before);
}
