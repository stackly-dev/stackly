//! `ReapGuard` leaves no process under the test root and removes the
//! runtime root of that root (spec 0014).
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::Duration;

use stackly_testkit::{ProcessTable, ReapGuard, TestRoot};

/// Replaces the arguments of `sleep` with the marker: argv[0] is the marker.
const SLEEP: &str = r#"exec -a "$0" /bin/sleep 300"#;

#[test]
fn dropping_the_guard_stops_the_processes_and_removes_a_runtime_root() {
    let root = TestRoot::new().unwrap();
    let runtime = root
        .run_dir()
        .parent()
        .unwrap()
        .join(format!("stackly-guard-test-{}", std::process::id()));
    std::fs::create_dir_all(runtime.join("i/x")).unwrap();
    std::fs::write(runtime.join("i/x/spool"), b"x").unwrap();
    let mut child = Command::new("/bin/bash")
        .args(["-c", SLEEP])
        .arg(root.path())
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let marker = root.path().to_string_lossy().into_owned();
    ProcessTable::wait_for(&[&marker], Duration::from_secs(5), |rows| !rows.is_empty()).unwrap();

    drop(ReapGuard::new(root.path(), &runtime));

    child.wait().unwrap();
    assert!(!runtime.exists());
}

#[test]
fn a_folder_that_is_not_named_like_a_runtime_root_is_never_removed() {
    let root = TestRoot::new().unwrap();
    let keep = root
        .run_dir()
        .parent()
        .unwrap()
        .join(format!("keep-me-{}", std::process::id()));
    std::fs::create_dir_all(&keep).unwrap();

    drop(ReapGuard::new(root.path(), &keep));

    assert!(keep.exists());
    std::fs::remove_dir(&keep).unwrap();
}
