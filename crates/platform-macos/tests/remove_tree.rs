//! `MacosFilesystem::remove_tree` on the real filesystem: a disk image
//! attached inside the tree, a socket, and the race in which another thread
//! keeps swapping folders for symlinks to a sentinel tree while trees are
//! removed (VAL-007). Every path lives in a `TestRoot`.
#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use stackly_platform::{Filesystem, PlatformError, RemoveReport};
use stackly_platform_macos::MacosFilesystem;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{CrossDevice, TestRoot, fs_cross_device_conformance};

/// How long the race test keeps swapping folders.
const RACE_DURATION: Duration = Duration::from_secs(2);

#[test]
fn macos_remove_tree_stops_at_another_device() {
    let ev = evidence!(
        covers = ["VAL-007"],
        kind = Kind::Fixture,
        expected = "MacosFilesystem::remove_tree stops with CrossesDevice at a disk image attached inside, as, or on the way to the target, and the file on the image stays intact (VAL-007)",
    );
    let root = TestRoot::new().unwrap();

    match fs_cross_device_conformance(&MacosFilesystem, &root) {
        CrossDevice::Checked => ev.pass(),
        CrossDevice::Blocked(reason) => ev.blocked(
            &format!("hdiutil cannot attach a disk image on this host: {reason}"),
            "feature 7",
        ),
        other => panic!("unexpected cross device outcome {other:?}"),
    }
}

#[test]
fn remove_tree_removes_a_socket_as_an_entry() {
    let root = TestRoot::new().unwrap();
    let dir = root.run_dir().join("sockets");
    fs::create_dir(&dir).unwrap();
    let _listener = UnixListener::bind(dir.join("s.sock")).unwrap();

    let report = MacosFilesystem
        .remove_tree(root.run_dir(), Path::new("sockets"))
        .unwrap();

    assert_eq!(report, RemoveReport::removed(1, 1));
    assert!(!dir.exists());
}

/// The files of one tree: `tree/f1`, `tree/mid/f2`, `tree/mid/leaf/f3`.
const TREE_FILES: [(&str, &[u8]); 3] = [
    ("tree/f1", b"one"),
    ("tree/mid/f2", b"two"),
    ("tree/mid/leaf/f3", b"three"),
];

fn build_tree(top: &Path) {
    fs::create_dir_all(top.join("tree").join("mid").join("leaf"))
        .expect("the tree folders can be created while no swap is active");
    for (name, bytes) in TREE_FILES {
        fs::write(top.join(name), bytes)
            .expect("the tree files can be written while no swap is active");
    }
}

/// Every entry below `dir` with its kind, inode, and contents.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (bool, u64, Vec<u8>)> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).expect("the sentinel tree can be listed") {
            let path = entry.expect("a sentinel entry can be read").path();
            let info = fs::symlink_metadata(&path).expect("a listed sentinel entry exists");
            let bytes = if info.is_file() {
                fs::read(&path).expect("a sentinel file can be read")
            } else {
                Vec::new()
            };
            if info.is_dir() {
                pending.push(path.clone());
            }
            entries.insert(
                path.strip_prefix(dir)
                    .expect("a listed entry lies below the listed folder")
                    .to_path_buf(),
                (info.is_dir(), info.ino(), bytes),
            );
        }
    }
    entries
}

/// Renames `real` away, puts a symlink to `target` in its place, yields,
/// then puts `real` back. Leaves no symlink behind.
fn swap_and_restore(real: &Path, away: &Path, target: &Path) {
    if fs::rename(real, away).is_err() {
        return;
    }
    if symlink(target, real).is_ok() {
        thread::yield_now();
        // The removal may have unlinked the symlink already.
        let _ = fs::remove_file(real);
    }
    // The removal may have removed the real folder while it was away.
    let _ = fs::rename(away, real);
}

#[derive(Debug, Default)]
struct Tally {
    removed: u64,
    missing: u64,
    symlink_in_path: u64,
    other: BTreeMap<String, u64>,
}

#[test]
fn remove_tree_never_follows_a_folder_swapped_for_a_symlink() {
    let ev = evidence!(
        covers = ["VAL-007"],
        kind = Kind::Fixture,
        expected = "for 2 seconds one thread keeps swapping the folder on the way to the target and a folder inside the tree for symlinks to a sentinel tree outside the removal root while MacosFilesystem::remove_tree keeps removing freshly built trees; every sentinel entry stays identical (VAL-007)",
    );
    let root = TestRoot::new().unwrap();
    let base = root.path().join("owned");
    let gate = base.join("gate");
    let sentinel = root.path().join("sentinel");
    fs::create_dir(&base).unwrap();
    build_tree(&sentinel);
    let before = snapshot(&sentinel);

    let setup = Arc::new(Mutex::new(()));
    let stop = Arc::new(AtomicBool::new(false));
    let attacker = {
        let (setup, stop) = (Arc::clone(&setup), Arc::clone(&stop));
        let (gate, sentinel) = (gate.clone(), sentinel.clone());
        thread::spawn(move || {
            let mid = gate.join("tree").join("mid");
            let mut swaps = 0_u64;
            while !stop.load(Ordering::SeqCst) {
                let held = setup.lock().unwrap_or_else(PoisonError::into_inner);
                swap_and_restore(&gate, &gate.with_file_name("gate.away"), &sentinel);
                swap_and_restore(
                    &mid,
                    &mid.with_file_name("mid.away"),
                    &sentinel.join("tree").join("mid"),
                );
                swaps += 1;
                drop(held);
                // Let the removal thread take the lock for its next setup.
                thread::yield_now();
            }
            swaps
        })
    };

    let mut tally = Tally::default();
    let start = Instant::now();
    while start.elapsed() < RACE_DURATION {
        {
            let _held = setup.lock().unwrap_or_else(PoisonError::into_inner);
            assert!(!fs::symlink_metadata(&gate).is_ok_and(|info| info.is_symlink()));
            build_tree(&gate);
        }
        match MacosFilesystem.remove_tree(&base, Path::new("gate/tree")) {
            Ok(report) if report.missing => tally.missing += 1,
            Ok(_) => tally.removed += 1,
            Err(PlatformError::SymlinkInPath { .. }) => tally.symlink_in_path += 1,
            Err(
                err @ (PlatformError::CrossesDevice { .. }
                | PlatformError::TooDeep { .. }
                | PlatformError::OutsideTestRoots { .. }),
            ) => panic!("remove_tree failed in a way the race cannot cause: {err}"),
            Err(err) => {
                let kind = match &err {
                    PlatformError::Io { kind, .. } => format!("io {kind:?}"),
                    other => format!("{other:?}")
                        .split([' ', '{'])
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                };
                *tally.other.entry(kind).or_default() += 1;
            }
        }
    }
    stop.store(true, Ordering::SeqCst);
    let swaps = attacker.join().unwrap();

    assert_eq!(snapshot(&sentinel), before, "the sentinel tree changed");
    let calls =
        tally.removed + tally.missing + tally.symlink_in_path + tally.other.values().sum::<u64>();
    assert!(calls > 0 && swaps > 0, "the race never ran: {tally:?}");
    ev.pass_with(&format!(
        "{calls} remove_tree calls against {swaps} swap cycles in {} ms: {} removed, {} missing, {} SymlinkInPath, other {:?}; all {} sentinel entries identical",
        start.elapsed().as_millis(),
        tally.removed,
        tally.missing,
        tally.symlink_in_path,
        tally.other,
        before.len()
    ));
}
