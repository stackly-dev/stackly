//! End to end test of `cargo xtask docs check` (spec 0038, AC-9): the real
//! binary checks the public docs of this repo, reads the `stackly` command
//! table through `stackly manpage --json`, and finds nothing wrong.

use std::path::Path;
use std::process::Command;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
}

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
