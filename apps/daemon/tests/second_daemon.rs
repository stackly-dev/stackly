//! Only one daemon runs per data root (spec 0006 AC-4). A second `stacklyd`
//! for the same `STACKLY_ROOT` fails to take the lock, names the daemon
//! that holds it on stderr, and exits 3, without changing the lock file or
//! writing a single line to the first daemon's log file. Once the first
//! daemon stopped, the next one takes the lock at once.
#![cfg(target_os = "macos")]

mod common;

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Duration;

use common::{STACKLYD, daemon_paths, field, json_lines, line_with, start_daemon};
use serde_json::Value;
use stackly_testkit::{SpawnedDaemon, TestRoot};

#[test]
fn a_second_daemon_for_the_same_root_exits_3_names_the_owner_and_changes_nothing() {
    let root = TestRoot::new().unwrap();
    let paths = daemon_paths(&root);
    let mut first = start_daemon(&root);
    let lines = json_lines(&first.stderr());
    let start = line_with(&lines, "server_instance_id").clone();
    let lock = paths.layout.daemon_lock();
    let log = paths.layout.daemon_log_dir().join("stacklyd.log");
    let lock_before = std::fs::read(&lock).unwrap();
    let log_before = std::fs::read(&log).unwrap();
    let log_inode = std::fs::metadata(&log).unwrap().ino();

    let second = SpawnedDaemon::spawn(
        Path::new(STACKLYD),
        root.path(),
        &paths.socket,
        &paths.runtime,
        &[],
    )
    .unwrap();
    let second_pid = second.pid();
    let second_stderr = second.stderr_path().to_path_buf();
    let status = second.wait_exit(Duration::from_secs(10)).unwrap();

    assert_eq!(status.code(), Some(3), "{status:?}");
    let text = std::fs::read_to_string(&second_stderr).unwrap();
    let lines = json_lines(&text);
    let held = line_with(&lines, "code");
    assert_eq!(held["level"], "ERROR");
    assert_eq!(field(held, "code"), "daemon.lock_held");
    assert_eq!(field(held, "owner_pid"), first.pid().to_string().as_str());
    assert_eq!(
        field(held, "owner_server_instance_id"),
        field(&start, "server_instance_id")
    );
    assert_eq!(field(held, "owner_started_at"), field(&start, "started_at"));
    assert_eq!(field(held, "owner_version"), env!("CARGO_PKG_VERSION"));
    assert_eq!(field(held, "lock"), lock.display().to_string().as_str());
    let message = field(held, "message").as_str().unwrap();
    assert!(
        message.starts_with(&format!(
            "another stacklyd holds the lock (pid {}",
            first.pid()
        )),
        "{message}"
    );
    assert!(
        lines
            .iter()
            .all(|line| field(line, "message") != &Value::from("stacklyd started")),
        "{text}"
    );

    assert_eq!(std::fs::read(&lock).unwrap(), lock_before);
    assert_eq!(std::fs::metadata(&log).unwrap().ino(), log_inode);
    let log_after = std::fs::read(&log).unwrap();
    assert_eq!(
        log_after, log_before,
        "the second daemon wrote to the log file"
    );
    let log_text = String::from_utf8_lossy(&log_after);
    assert!(!log_text.contains("daemon.lock_held"));
    assert!(!log_text.contains(&format!("\"pid\":{second_pid}")));
    assert_eq!(
        first.try_status().unwrap(),
        None,
        "the first daemon still runs"
    );

    assert_eq!(first.stop().unwrap().code(), Some(0));
    let next = start_daemon(&root);
    let owner: Value = serde_json::from_slice(&std::fs::read(&lock).unwrap()).unwrap();
    assert_eq!(owner["pid"], next.pid());
    assert_eq!(next.stop().unwrap().code(), Some(0));
}
