//! `cargo_example`: builds an example of a workspace package once per test
//! process and returns a binary that runs (spec 0014).
#![cfg(unix)]

use std::process::Command;

use stackly_testkit::cargo_example;

/// Spec 0014 prerequisite of AC-1 and AC-2 (the seed binary of the suite).
#[test]
fn an_example_is_built_once_and_the_binary_runs() {
    let first = cargo_example("stacklyd", "skeleton_seed");
    let second = cargo_example("stacklyd", "skeleton_seed");

    assert_eq!(first, second);
    assert!(first.is_absolute(), "{}", first.display());
    assert!(
        first.ends_with("debug/examples/skeleton_seed"),
        "{}",
        first.display()
    );
    // Without `--root` the seed ends in the parser with exit 2.
    let output = Command::new(&first).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--root"),
        "{output:?}"
    );
}

/// Spec 0014 prerequisite of AC-1 and AC-2.
#[test]
#[should_panic(expected = "failed with")]
fn an_example_that_does_not_exist_fails_the_test_with_the_cargo_status() {
    let _ = cargo_example("stacklyd", "no_such_example");
}
