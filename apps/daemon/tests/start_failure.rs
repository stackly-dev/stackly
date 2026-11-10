//! The record of a failed start (spec 0034 AC-11), through the real
//! `stacklyd` binary: a `STACKLY_ROOT` that others can write (refused by
//! spec 0004 AC-7) with a private `data/` inside makes the daemon exit 1 and
//! write `data/daemon-start-failure.json`, mode 0600, at most 4 KiB, and no
//! folder; a data folder others can write, a data folder that is a
//! symlink, or no data folder at all gets no record; exits 2 and 3 never
//! write one; and the next start that binds its socket removes it.
#![cfg(target_os = "macos")]

mod common;

use std::fs::Permissions;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{STACKLYD, daemon_paths, start_daemon};
use serde_json::Value;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{SpawnedDaemon, TestRoot};

/// Runs the daemon on `root` until it exits, as a shell would start it.
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(STACKLYD)
        .args(args)
        .env("STACKLY_ROOT", root)
        .env_remove("STACKLY_LOG")
        .env_remove("STACKLY_CATALOG_DIR")
        .env_remove("XPC_SERVICE_NAME")
        .output()
        .expect("stacklyd runs")
}

fn record_path(root: &Path) -> PathBuf {
    root.join("data/daemon-start-failure.json")
}

fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, Permissions::from_mode(mode)).expect("the mode can be set");
}

/// The names in `folder`, sorted.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .expect("the folder lists")
        .map(|entry| {
            entry
                .expect("an entry reads")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

// covers: AC-11 (spec 0034)
#[test]
fn a_refused_root_with_a_private_data_folder_exits_1_with_a_record_the_next_start_removes() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "stacklyd on a STACKLY_ROOT with mode 0777 and a private data folder exits 1 and writes data/daemon-start-failure.json with mode 0600, at most 4 KiB, schema 1, exitCode 1, code daemon.start_failed, the refusal, the version, and launch manual, without creating any folder; a later start on the fixed root removes it once its socket is bound",
    );
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    chmod(&data, 0o700);
    chmod(root.path(), 0o777);

    let output = run(root.path(), &[]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let path = record_path(root.path());
    let meta = std::fs::symlink_metadata(&path).expect("the record is written");
    assert!(meta.file_type().is_file());
    assert_eq!(meta.mode() & 0o7777, 0o600);
    assert_eq!(meta.uid(), common::uid());
    assert!(meta.len() <= 4096, "{}", meta.len());
    let record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(record["schema"], 1);
    assert_eq!(record["exitCode"], 1);
    assert_eq!(record["code"], "daemon.start_failed");
    let message = record["message"].as_str().expect("a message");
    assert!(message.contains("writable by group or others"), "{message}");
    assert!(message.chars().count() <= 512);
    assert_eq!(record["version"], stackly_core::daemon_info().version);
    assert_eq!(record["launch"], "manual");
    assert_eq!(record["bundlePath"], Value::Null);
    record["at"]
        .as_str()
        .expect("a time")
        .parse::<jiff::Timestamp>()
        .expect("an RFC 3339 time");
    assert_eq!(names(root.path()), ["data"], "no folder besides data");
    assert_eq!(names(&data), ["daemon-start-failure.json"]);

    chmod(root.path(), 0o700);
    let daemon = start_daemon(&root);
    assert!(
        !path.exists(),
        "the start that bound its socket removed the record"
    );
    let stderr = daemon.stderr();
    assert!(
        stderr.contains("removed the record of an earlier failed start"),
        "{stderr}"
    );
    daemon.stop().unwrap();
    ev.pass();
}

// covers: AC-11 (spec 0034)
#[test]
fn a_data_folder_others_can_write_a_symlinked_one_or_none_gets_no_record() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "a refused STACKLY_ROOT whose data folder is mode 0777, a symlink to a private folder, or missing exits 1 and writes no record anywhere, and creates no data folder",
    );
    let open = TestRoot::new().unwrap();
    let data = open.path().join("data");
    std::fs::create_dir(&data).unwrap();
    chmod(&data, 0o777);
    chmod(open.path(), 0o777);
    assert_eq!(run(open.path(), &[]).status.code(), Some(1));
    assert!(names(&data).is_empty(), "{:?}", names(&data));

    let linked = TestRoot::new().unwrap();
    let target = linked.path().join("elsewhere");
    std::fs::create_dir(&target).unwrap();
    chmod(&target, 0o700);
    std::os::unix::fs::symlink(&target, linked.path().join("data")).unwrap();
    chmod(linked.path(), 0o777);
    assert_eq!(run(linked.path(), &[]).status.code(), Some(1));
    assert!(
        names(&target).is_empty(),
        "nothing is written through a symlink"
    );

    let bare = TestRoot::new().unwrap();
    chmod(bare.path(), 0o777);
    assert_eq!(run(bare.path(), &[]).status.code(), Some(1));
    assert!(names(bare.path()).is_empty(), "no folder is created");
    ev.pass();
}

// covers: AC-11 (spec 0034)
#[test]
fn exits_2_and_3_write_no_record() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "stacklyd with an unknown flag exits 2 and a second stacklyd for a root whose lock is held exits 3, and neither writes daemon-start-failure.json",
    );
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    chmod(&data, 0o700);
    assert_eq!(run(root.path(), &["--no-such-flag"]).status.code(), Some(2));
    assert!(!record_path(root.path()).exists());

    let first = start_daemon(&root);
    let paths = daemon_paths(&root);
    let second = SpawnedDaemon::spawn(
        Path::new(STACKLYD),
        root.path(),
        &paths.socket,
        &paths.runtime,
        &[],
    )
    .unwrap();
    let status = second
        .wait_exit(std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(status.code(), Some(3));
    assert!(!record_path(root.path()).exists());
    first.stop().unwrap();
    ev.pass();
}
