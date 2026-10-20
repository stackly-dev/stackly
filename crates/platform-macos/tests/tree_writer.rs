//! The feature 18 additions to `MacosFilesystem` on the real filesystem:
//! the race in which another thread keeps swapping folders of a tree being
//! written for symlinks to a sentinel tree, letter case collisions on
//! APFS, and `rename_no_replace` and `volume_space` across a disk image.
//! Every path lives in a `TestRoot`.
#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use stackly_platform::{EntryPath, Filesystem, PlatformError};
use stackly_platform_macos::MacosFilesystem;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{DiskImage, TestRoot};

/// How long the race test keeps swapping folders.
const RACE_DURATION: Duration = Duration::from_secs(2);

/// Creates an empty tree root with mode 0700 at `path`.
fn tree_root(path: &Path) {
    fs::create_dir(path).expect("the tree root can be created");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("the tree root mode can be set");
}

fn entry(path: &str) -> EntryPath {
    EntryPath::new(path).expect("the test uses valid entry paths")
}

/// Every entry below `dir` with its kind, inode, and contents.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (bool, u64, Vec<u8>)> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for listed in fs::read_dir(&current).expect("the tree can be listed") {
            let path = listed.expect("a listed entry can be read").path();
            let info = fs::symlink_metadata(&path).expect("a listed entry exists");
            let bytes = if info.is_file() {
                fs::read(&path).expect("a listed file can be read")
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
        let _ = fs::remove_file(real);
    }
    let _ = fs::rename(away, real);
}

#[derive(Debug, Default)]
struct Tally {
    created: u64,
    symlink_in_path: u64,
    not_found: u64,
    other: BTreeMap<String, u64>,
}

#[test]
fn tree_writer_never_writes_through_a_folder_swapped_for_a_symlink() {
    let ev = evidence!(
        covers = ["VAL-011", "AC-057"],
        kind = Kind::Fixture,
        expected = "for 2 seconds one thread keeps swapping two nested folders of a tree for symlinks to a sentinel tree outside the tree root while MacosTreeWriter keeps creating files and folders inside them; every sentinel entry stays identical and every entry created lies inside the tree root (VAL-011, AC-057)",
    );
    let root = TestRoot::new().unwrap();
    let stage = root.path().join("stage");
    tree_root(&stage);
    let sentinel = root.path().join("sentinel");
    fs::create_dir_all(sentinel.join("inner")).unwrap();
    fs::write(sentinel.join("keep"), b"sentinel").unwrap();
    fs::write(sentinel.join("inner").join("deep"), b"deep").unwrap();
    let before = snapshot(&sentinel);

    let mut writer = MacosFilesystem.open_tree_writer(&stage).unwrap();
    writer.create_dir(&entry("a")).unwrap();
    writer.create_dir(&entry("a/b")).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let attacker = {
        let stop = Arc::clone(&stop);
        let (outer, sentinel) = (stage.join("a"), sentinel.clone());
        thread::spawn(move || {
            let inner = outer.join("b");
            let mut swaps = 0_u64;
            while !stop.load(Ordering::SeqCst) {
                swap_and_restore(&outer, &outer.with_file_name("a.away"), &sentinel);
                swap_and_restore(
                    &inner,
                    &inner.with_file_name("b.away"),
                    &sentinel.join("inner"),
                );
                swaps += 1;
                thread::yield_now();
            }
            swaps
        })
    };

    let mut tally = Tally::default();
    let start = Instant::now();
    let mut index = 0_u64;
    while start.elapsed() < RACE_DURATION {
        index += 1;
        let path = if index.is_multiple_of(2) {
            entry(&format!("a/b/f{index}"))
        } else {
            entry(&format!("a/d{index}"))
        };
        let result = if index.is_multiple_of(2) {
            writer.create_file(&path).and_then(|mut file| {
                file.write_all(b"payload")
                    .map_err(|err| PlatformError::from_io("write", path.as_path(), &err))?;
                file.finish().map(drop)
            })
        } else {
            writer.create_dir(&path)
        };
        match result {
            Ok(()) => tally.created += 1,
            Err(PlatformError::SymlinkInPath { .. }) => tally.symlink_in_path += 1,
            Err(PlatformError::NotFound { .. }) => tally.not_found += 1,
            Err(err @ PlatformError::AlreadyExists { .. }) => {
                panic!("a fresh name cannot exist: {err}")
            }
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
    writer.sync().unwrap();
    drop(writer);

    assert_eq!(snapshot(&sentinel), before, "the sentinel tree changed");
    let inside = snapshot(&stage);
    assert!(
        inside
            .values()
            .all(|(_, _, bytes)| bytes.is_empty() || bytes == b"payload"),
        "an unexpected file appeared in the tree"
    );
    // The two folders made before the race plus every entry created in it.
    assert_eq!(inside.len() as u64, tally.created + 2, "{tally:?}");
    let calls =
        tally.created + tally.symlink_in_path + tally.not_found + tally.other.values().sum::<u64>();
    assert!(calls > 0 && swaps > 0, "the race never ran: {tally:?}");
    assert!(tally.created > 0, "nothing was ever created: {tally:?}");
    ev.pass_with(&format!(
        "{calls} create calls against {swaps} swap cycles in {} ms: {} created inside the root, {} SymlinkInPath, {} NotFound, other {:?}; all {} sentinel entries identical",
        start.elapsed().as_millis(),
        tally.created,
        tally.symlink_in_path,
        tally.not_found,
        tally.other,
        before.len()
    ));
}

#[test]
fn tree_writer_reports_a_letter_case_collision_as_an_existing_entry() {
    let ev = evidence!(
        covers = ["VAL-011", "AC-057"],
        kind = Kind::Fixture,
        expected = "on a volume that ignores letter case, MacosTreeWriter refuses `foo` after `Foo` with AlreadyExists for a file, a folder, and a symlink, and the first entry keeps its bytes (VAL-011, AC-057)",
    );
    let root = TestRoot::new().unwrap();
    let probe = root.path().join("CaseProbe");
    fs::write(&probe, b"x").unwrap();
    if !root.path().join("caseprobe").exists() {
        ev.blocked(
            "the test root lies on a volume that tells letter case apart",
            "feature 18",
        );
        return;
    }
    let stage = root.path().join("stage");
    tree_root(&stage);
    let mut writer = MacosFilesystem.open_tree_writer(&stage).unwrap();
    let mut file = writer.create_file(&entry("Foo")).unwrap();
    file.write_all(b"first").unwrap();
    file.finish().unwrap();

    let as_file = writer.create_file(&entry("foo")).map(drop);
    let as_dir = writer.create_dir(&entry("FOO"));
    let as_symlink = writer.create_symlink(&entry("fOo"), "Foo");

    for result in [as_file, as_dir, as_symlink] {
        assert!(
            matches!(result, Err(PlatformError::AlreadyExists { .. })),
            "{result:?}"
        );
    }
    assert_eq!(fs::read(stage.join("Foo")).unwrap(), b"first");
    ev.pass();
}

#[test]
fn rename_no_replace_and_volume_space_see_another_device() {
    let ev = evidence!(
        covers = ["VAL-011"],
        kind = Kind::Fixture,
        expected = "MacosFilesystem::rename_no_replace refuses a move onto a disk image attached inside the test root with CrossesDevice and leaves the source in place, and volume_space reads the image's own size, not the test root's",
    );
    let root = TestRoot::new().unwrap();
    let image = match DiskImage::attach(&root, &root.path().join("image")) {
        Ok(image) => image,
        Err(err) => {
            ev.blocked(
                &format!("hdiutil cannot attach a disk image on this host: {err}"),
                "feature 18",
            );
            return;
        }
    };
    let source = root.path().join("artifact");
    fs::write(&source, b"bytes").unwrap();

    let moved = MacosFilesystem.rename_no_replace(&source, &image.mount_point().join("artifact"));
    let on_image = MacosFilesystem.volume_space(image.mount_point()).unwrap();
    let on_root = MacosFilesystem.volume_space(root.path()).unwrap();

    assert!(
        matches!(moved, Err(PlatformError::CrossesDevice { .. })),
        "{moved:?}"
    );
    assert_eq!(fs::read(&source).unwrap(), b"bytes");
    assert!(
        on_image.total <= 8 << 20 && on_image.available <= on_image.total,
        "{on_image:?}"
    );
    assert!(
        on_root.total > on_image.total,
        "{on_root:?} vs {on_image:?}"
    );
    drop(image);
    ev.pass();
}
