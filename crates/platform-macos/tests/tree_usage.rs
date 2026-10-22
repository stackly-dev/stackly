//! `MacosFilesystem::tree_usage` on the real filesystem: the deadline on a
//! large tree and a disk image attached inside the tree (spec 0021,
//! *Ukuran data*, and spec 0004, *Port platform*). The portable cases live
//! in `fs_conformance`. Every path lives in a `TestRoot`.
#![cfg(target_os = "macos")]

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use stackly_platform::{Filesystem, TreeUsage, UsageLimits};
use stackly_platform_macos::MacosFilesystem;
use stackly_testkit::{DiskImage, TestRoot};

/// Files in the tree of the deadline test.
const MANY_FILES: usize = 50_000;

/// Writes `MANY_FILES` files spread over 100 folders below `root`.
fn write_many_files(root: &Path) {
    for folder in 0..100 {
        let dir = root.join(format!("d{folder:03}"));
        fs::create_dir_all(&dir).expect("a test folder can be created");
        for file in 0..MANY_FILES / 100 {
            fs::write(dir.join(format!("f{file:04}")), b"x").expect("a test file can be written");
        }
    }
}

fn measure(root: &Path, limits: UsageLimits) -> TreeUsage {
    MacosFilesystem
        .tree_usage(root, limits)
        .expect("the tree can be measured")
}

#[test]
fn a_short_deadline_stops_a_large_tree_and_a_long_one_finishes_it() {
    let root = TestRoot::new().unwrap();
    let tree = root.path().join("many");
    write_many_files(&tree);

    let started = Instant::now();
    let cut = measure(&tree, UsageLimits::new(1_000_000, Duration::from_millis(1)));
    let took = started.elapsed();
    let whole = measure(&tree, UsageLimits::new(1_000_000, Duration::from_secs(60)));

    assert!(!cut.complete, "1 ms is not enough for {MANY_FILES} files");
    assert!(
        took < Duration::from_millis(100),
        "a 1 ms deadline took {took:?}"
    );
    assert!(cut.bytes <= whole.bytes, "{cut:?} against {whole:?}");
    assert!(whole.complete);
    assert!(
        whole.bytes >= MANY_FILES as u64 * 512,
        "{} bytes for {MANY_FILES} files",
        whole.bytes
    );
}

#[test]
fn the_entry_limit_stops_a_large_tree_after_the_names_it_may_read() {
    let root = TestRoot::new().unwrap();
    let tree = root.path().join("many");
    write_many_files(&tree);
    // 100 folders and 50 000 files.
    let entries = (MANY_FILES + 100) as u64;

    let exact = measure(&tree, UsageLimits::new(entries, Duration::from_secs(60)));
    let short = measure(
        &tree,
        UsageLimits::new(entries - 1, Duration::from_secs(60)),
    );

    assert!(exact.complete, "{exact:?}");
    assert!(!short.complete, "{short:?}");
    assert!(short.bytes <= exact.bytes);
}

#[test]
fn another_volume_inside_the_tree_is_skipped() {
    let root = TestRoot::new().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("file"), vec![1_u8; 4_096]).unwrap();
    let limits = UsageLimits::new(1_000_000, Duration::from_secs(60));
    let before = measure(&tree, limits);

    let mount = tree.join("mnt");
    let image = match DiskImage::attach(&root, &mount) {
        Ok(image) => image,
        Err(err) => {
            eprintln!("hdiutil cannot attach a disk image on this host: {err}");
            return;
        }
    };
    fs::write(mount.join("big"), vec![2_u8; 256 * 1024]).unwrap();
    let with_image = measure(&tree, limits);
    drop(image);

    assert!(with_image.complete, "{with_image:?}");
    assert_eq!(
        with_image.bytes, before.bytes,
        "the files on the image must not be counted"
    );
}
