//! `fake_server::binary`: builds `stackly-fake-server` once per test
//! process and returns a binary that runs.
#![cfg(unix)]

use std::process::Command;

use stackly_testkit::fake_server;

#[test]
fn binary_is_built_once_and_runs_the_version_command() {
    let first = fake_server::binary();
    let second = fake_server::binary();

    assert_eq!(first, second);
    assert!(first.is_absolute(), "{}", first.display());
    assert!(
        first.ends_with("debug/stackly-fake-server"),
        "{}",
        first.display()
    );
    let output = Command::new(&first).arg("version").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("stackly-fake-server {}\n", env!("CARGO_PKG_VERSION"))
    );
}
