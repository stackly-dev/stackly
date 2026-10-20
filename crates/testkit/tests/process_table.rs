//! `ProcessTable` finds processes by their arguments and reaps them
//! (spec 0014). `bash` processes with a marker argument stand in for the
//! fake server.
#![cfg(unix)]

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use stackly_testkit::{ProcessTable, TestRoot};

/// Replaces the arguments of `sleep` with the marker: argv[0] is the marker.
const SLEEP: &str = r#"exec -a "$0" /bin/sleep 300"#;

/// Starts `bash -c <script> <marker>`, so the marker is in its arguments.
fn marked(script: &str, marker: &std::path::Path) -> Child {
    Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .arg(marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sh starts")
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the process counts and the reaping of the end to end suite).
#[test]
fn a_snapshot_finds_a_process_by_the_path_in_its_arguments_and_leaves_out_the_test() {
    let root = TestRoot::new().unwrap();
    let mut child = marked(SLEEP, root.path());
    let marker = root.path().to_string_lossy().into_owned();

    let rows = ProcessTable::wait_for(&[&marker], Duration::from_secs(5), |rows| !rows.is_empty())
        .unwrap();

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].pid, child.id());
    let table = ProcessTable::snapshot().unwrap();
    assert!(
        table
            .entries()
            .iter()
            .all(|entry| entry.pid != std::process::id())
    );
    assert!(table.containing("no-such-process-argument-b6f2").is_empty());
    assert_eq!(table.containing_all(&[&marker]).len(), 1);
    child.kill().unwrap();
    child.wait().unwrap();
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the process counts and the reaping of the end to end suite).
#[test]
fn wait_for_fails_with_the_matching_rows_when_the_time_runs_out() {
    let root = TestRoot::new().unwrap();
    let marker = root.path().to_string_lossy().into_owned();

    let error = ProcessTable::wait_for(&[&marker], Duration::from_millis(200), |rows| {
        !rows.is_empty()
    })
    .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the process counts and the reaping of the end to end suite).
#[test]
fn reap_under_stops_only_the_processes_that_name_the_path() {
    let root = TestRoot::new().unwrap();
    let other = TestRoot::new().unwrap();
    let mut inside = marked(SLEEP, root.path());
    let mut outside = marked(SLEEP, other.path());

    let reaped = ProcessTable::reap_under(root.path(), Duration::from_secs(5)).unwrap();

    assert_eq!(reaped, 1);
    inside.wait().unwrap();
    assert!(
        outside.try_wait().unwrap().is_none(),
        "the other root's process still runs"
    );
    outside.kill().unwrap();
    outside.wait().unwrap();
    assert_eq!(
        ProcessTable::reap_under(root.path(), Duration::from_secs(1)).unwrap(),
        0
    );
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the process counts and the reaping of the end to end suite).
#[test]
fn reap_under_kills_a_process_that_ignores_sigterm() {
    let root = TestRoot::new().unwrap();
    let mut stubborn = marked("trap '' TERM; while :; do :; done", root.path());
    let marker = root.path().to_string_lossy().into_owned();
    ProcessTable::wait_for(&[&marker], Duration::from_secs(5), |rows| !rows.is_empty()).unwrap();

    let reaped = ProcessTable::reap_under(root.path(), Duration::from_millis(300)).unwrap();

    assert_eq!(reaped, 1);
    let status = stubborn.wait().unwrap();
    assert!(!status.success(), "{status:?}");
}

/// Spec 0014 prerequisite of AC-9, AC-11, AC-12 (the process counts and the reaping of the end to end suite).
#[test]
fn under_matches_a_whole_path_and_never_a_longer_sibling_name() {
    let root = TestRoot::new().unwrap();
    let sibling = std::path::PathBuf::from(format!("{}x", root.path().display()));
    let mut near = marked(SLEEP, &sibling);
    let mut inside = marked(SLEEP, &root.path().join("data/instances/one"));
    let mut flag = Command::new("/bin/bash")
        .arg("-c")
        .arg(r#"exec -a "--config=$0/config/fake.json" /bin/sleep 300"#)
        .arg(root.path())
        .stdin(Stdio::null())
        .spawn()
        .expect("bash starts");
    let marker = root.path().to_string_lossy().into_owned();
    ProcessTable::wait_for(&[&marker], Duration::from_secs(5), |rows| rows.len() == 3).unwrap();

    let table = ProcessTable::snapshot().unwrap();
    let pids: Vec<u32> = table.under(root.path()).iter().map(|row| row.pid).collect();

    assert!(
        pids.contains(&inside.id()) && pids.contains(&flag.id()),
        "{pids:?}"
    );
    assert!(!pids.contains(&near.id()), "{pids:?}");
    for child in [&mut near, &mut inside, &mut flag] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
