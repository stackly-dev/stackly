//! `SpawnedDaemon` never lets a process outlive the test and only removes
//! a runtime root that holds nothing but empty folders. System tools stand
//! in for `stacklyd` here; the daemon's own tests run the real binary.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use stackly_testkit::{SpawnedDaemon, TestRoot};

fn paths(root: &TestRoot) -> (PathBuf, PathBuf) {
    let runtime = root.run_dir().join("runtime");
    (runtime.join("stacklyd.sock"), runtime)
}

fn running(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .expect("kill runs")
        .success()
}

#[test]
fn dropping_a_daemon_stops_the_process() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    let daemon = SpawnedDaemon::spawn(
        Path::new("/bin/sleep"),
        root.path(),
        &socket,
        &runtime,
        &["30"],
    )
    .unwrap();
    let pid = daemon.pid();
    assert!(running(pid));

    let started = Instant::now();
    drop(daemon);

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!running(pid));
}

#[test]
fn stop_reports_how_the_process_ended() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    let daemon = SpawnedDaemon::spawn(
        Path::new("/bin/sleep"),
        root.path(),
        &socket,
        &runtime,
        &["30"],
    )
    .unwrap();

    let status = daemon.stop().unwrap();

    assert!(!status.success());
    assert_eq!(status.code(), None, "SIGTERM ends sleep: {status:?}");
}

#[test]
fn kill_ends_the_process_with_sigkill_and_waits_for_it() {
    use std::os::unix::process::ExitStatusExt;

    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    let daemon = SpawnedDaemon::spawn(
        Path::new("/bin/sleep"),
        root.path(),
        &socket,
        &runtime,
        &["30"],
    )
    .unwrap();
    let pid = daemon.pid();

    let status = daemon.kill().unwrap();

    assert_eq!(status.signal(), Some(9), "{status:?}");
    assert!(!running(pid));
}

#[test]
fn start_fails_with_the_stderr_when_the_process_exits_before_it_is_ready() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);

    let error = SpawnedDaemon::start(
        Path::new("/usr/bin/false"),
        root.path(),
        &socket,
        &runtime,
        |_| false,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("stacklyd exited with exit status: 1 before it was ready"),
        "{error}"
    );
}

#[test]
fn wait_ready_returns_once_the_check_passes_and_stderr_lands_in_the_root() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    let script = "echo hello >&2; sleep 30";
    let mut daemon = SpawnedDaemon::spawn(
        Path::new("/bin/sh"),
        root.path(),
        &socket,
        &runtime,
        &["-c", script],
    )
    .unwrap();
    let mut calls = 0;

    daemon
        .wait_ready(|daemon| {
            calls += 1;
            daemon.stderr().contains("hello")
        })
        .unwrap();

    assert!(calls >= 1);
    assert_eq!(daemon.socket(), socket);
    assert!(daemon.stderr_path().starts_with(root.path()));
    assert_eq!(daemon.try_status().unwrap(), None);
}

#[test]
fn wait_exit_returns_the_exit_status() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    let daemon = SpawnedDaemon::spawn(
        Path::new("/bin/sh"),
        root.path(),
        &socket,
        &runtime,
        &["-c", "exit 3"],
    )
    .unwrap();

    let status = daemon.wait_exit(Duration::from_secs(5)).unwrap();

    assert_eq!(status.code(), Some(3));
}

#[test]
fn the_daemon_sees_the_test_root_and_debug_logging() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    let script = r#"printf '%s %s' "$STACKLY_ROOT" "$STACKLY_LOG" >&2"#;
    let daemon = SpawnedDaemon::spawn(
        Path::new("/bin/sh"),
        root.path(),
        &socket,
        &runtime,
        &["-c", script],
    )
    .unwrap();
    let stderr = daemon.stderr_path().to_path_buf();

    daemon.wait_exit(Duration::from_secs(5)).unwrap();

    let text = std::fs::read_to_string(stderr).unwrap();
    assert_eq!(text, format!("{} debug", root.path().display()));
}

#[test]
fn a_runtime_root_with_only_an_empty_runs_folder_is_removed() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    std::fs::create_dir_all(runtime.join("i")).unwrap();
    let daemon = SpawnedDaemon::spawn(
        Path::new("/usr/bin/true"),
        root.path(),
        &socket,
        &runtime,
        &[],
    )
    .unwrap();

    daemon.wait_exit(Duration::from_secs(5)).unwrap();

    assert!(!runtime.exists());
}

#[test]
fn a_runtime_root_with_anything_else_is_kept() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    std::fs::create_dir_all(runtime.join("i")).unwrap();
    std::fs::write(runtime.join("i/run"), b"").unwrap();
    std::fs::write(&socket, b"").unwrap();
    let daemon = SpawnedDaemon::spawn(
        Path::new("/usr/bin/true"),
        root.path(),
        &socket,
        &runtime,
        &[],
    )
    .unwrap();

    daemon.wait_exit(Duration::from_secs(5)).unwrap();

    assert!(runtime.join("i/run").exists());
    assert!(socket.exists());
}

#[test]
fn a_runtime_root_with_a_file_beside_an_empty_runs_folder_is_kept() {
    let root = TestRoot::new().unwrap();
    let (socket, runtime) = paths(&root);
    std::fs::create_dir_all(runtime.join("i")).unwrap();
    std::fs::write(&socket, b"").unwrap();
    let daemon = SpawnedDaemon::spawn(
        Path::new("/usr/bin/true"),
        root.path(),
        &socket,
        &runtime,
        &[],
    )
    .unwrap();

    daemon.wait_exit(Duration::from_secs(5)).unwrap();

    assert!(runtime.join("i").is_dir());
    assert!(socket.exists());
}
