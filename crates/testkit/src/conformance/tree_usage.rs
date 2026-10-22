//! Conformance checks for `Filesystem::tree_usage` (spec 0021, *Ukuran
//! data*, and spec 0004, *Port platform*): the same trees are measured by
//! the adapter and by a plain recursive count, and the limits are checked
//! at their edges.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::time::Duration;

use stackly_platform::{Filesystem, PlatformError, TREE_USAGE_MAX_DEPTH, TreeUsage, UsageLimits};

use super::{expect_eq, expect_err, fail, must, must_io};

/// A limit no test tree reaches.
const ROOMY: UsageLimits = UsageLimits::new(1_000_000, Duration::from_secs(60));

pub(super) fn check_tree_usage(fs: &dyn Filesystem, base: &Path) {
    let top = base.join("tree-usage");
    must_io("create the tree_usage folder", fs::create_dir(&top));

    check_sizes_match_a_plain_count(fs, &top);
    check_symlinks_and_hard_links(fs, &top);
    check_entry_limit(fs, &top);
    check_depth(fs, &top);
    check_zero_deadline(fs, &top);
    check_root_errors(fs, &top);
}

/// The bytes of `path` and everything below it, counted with `lstat`: every
/// entry's `st_blocks * 512`, a hard link once, no symlink followed. The
/// reference the adapters are held to.
fn plain_count(path: &Path) -> u64 {
    let mut linked = HashSet::new();
    plain_count_in(path, true, &mut linked)
}

fn plain_count_in(path: &Path, is_root: bool, linked: &mut HashSet<(u64, u64)>) -> u64 {
    let info = must_io("lstat an entry", fs::symlink_metadata(path));
    let mut bytes = 0;
    if is_root || info.is_dir() || info.nlink() <= 1 || linked.insert((info.dev(), info.ino())) {
        bytes += info.blocks() * 512;
    }
    if info.is_dir() {
        for entry in must_io("list a folder", fs::read_dir(path)) {
            let entry = must_io("read a folder entry", entry);
            bytes += plain_count_in(&entry.path(), false, linked);
        }
    }
    bytes
}

fn usage(fs: &dyn Filesystem, root: &Path, limits: UsageLimits) -> TreeUsage {
    must("tree_usage measures a tree", fs.tree_usage(root, limits))
}

fn write_distinct(path: &Path, len: usize, seed: u8) {
    let bytes: Vec<u8> = (0..len)
        .map(|at| seed.wrapping_add(u8::try_from(at % 251).unwrap_or(0)))
        .collect();
    must_io("write a file", fs::write(path, bytes));
}

fn check_sizes_match_a_plain_count(fs: &dyn Filesystem, top: &Path) {
    let root = top.join("sizes");
    must_io(
        "create a tree",
        fs::create_dir_all(root.join("a").join("b")),
    );
    write_distinct(&root.join("one"), 100, 1);
    write_distinct(&root.join("a").join("two"), 70_000, 2);
    write_distinct(&root.join("a").join("b").join("three"), 5_000, 3);

    let measured = usage(fs, &root, ROOMY);
    expect_eq(
        "tree_usage of a complete tree is complete",
        measured.complete,
        true,
    );
    expect_eq(
        "tree_usage adds st_blocks * 512 of every entry, root and folders included",
        measured.bytes,
        plain_count(&root),
    );
    if measured.bytes < 70_000 {
        fail(
            "tree_usage counts the allocated blocks of the files",
            &format!("{} bytes for a tree with a 70000 byte file", measured.bytes),
        );
    }
}

fn check_symlinks_and_hard_links(fs: &dyn Filesystem, top: &Path) {
    let outside = top.join("outside");
    must_io("create the outside folder", fs::create_dir(&outside));
    write_distinct(&outside.join("big"), 1 << 20, 4);

    let linked = top.join("linked");
    must_io("create the linked tree", fs::create_dir(&linked));
    write_distinct(&linked.join("file"), 65_536, 5);
    must_io(
        "hard link a file",
        fs::hard_link(linked.join("file"), linked.join("again")),
    );
    must_io(
        "symlink to a folder outside",
        symlink(&outside, linked.join("out-dir")),
    );
    must_io(
        "symlink to a file outside",
        symlink(outside.join("big"), linked.join("out-file")),
    );
    must_io(
        "dangling symlink",
        symlink(top.join("nowhere"), linked.join("dangling")),
    );

    let copied = top.join("copied");
    must_io("create the copied tree", fs::create_dir(&copied));
    write_distinct(&copied.join("file"), 65_536, 5);
    write_distinct(&copied.join("again"), 65_536, 6);

    let measured = usage(fs, &linked, ROOMY);
    expect_eq(
        "tree_usage counts a hard link once and a symlink as itself",
        measured.bytes,
        plain_count(&linked),
    );
    if measured.bytes >= (1 << 20) {
        fail(
            "tree_usage never follows a symlink out of the root",
            &format!(
                "{} bytes with a 1 MiB file behind a symlink",
                measured.bytes
            ),
        );
    }
    let twice = usage(fs, &copied, ROOMY);
    if measured.bytes >= twice.bytes {
        fail(
            "a hard linked file counts once, a copy counts twice",
            &format!(
                "linked {} bytes, copied {} bytes",
                measured.bytes, twice.bytes
            ),
        );
    }
}

fn check_entry_limit(fs: &dyn Filesystem, top: &Path) {
    let root = top.join("entries");
    must_io("create a tree", fs::create_dir_all(root.join("d")));
    for n in 0..5_u8 {
        write_distinct(&root.join(format!("f{n}")), 10_000, n);
    }
    for n in 0..4_u8 {
        write_distinct(&root.join("d").join(format!("g{n}")), 10_000, n);
    }
    // Five files, the folder `d`, and four files in it: ten entries.
    let full = usage(fs, &root, UsageLimits::new(10, Duration::from_secs(60)));
    expect_eq(
        "a tree with exactly max_entries entries is complete",
        full.complete,
        true,
    );
    expect_eq(
        "the complete result of the edge case holds every byte",
        full.bytes,
        plain_count(&root),
    );

    let cut = usage(fs, &root, UsageLimits::new(9, Duration::from_secs(60)));
    expect_eq(
        "one entry over max_entries is incomplete",
        cut.complete,
        false,
    );
    if cut.bytes > full.bytes {
        fail(
            "an incomplete result never exceeds the complete one",
            &format!("{} against {}", cut.bytes, full.bytes),
        );
    }

    let empty = top.join("empty");
    must_io("create an empty folder", fs::create_dir(&empty));
    let none = usage(fs, &empty, UsageLimits::new(0, Duration::from_secs(60)));
    expect_eq(
        "an empty root is complete with max_entries 0",
        none.complete,
        true,
    );
    expect_eq(
        "an empty root holds its own block only",
        none.bytes,
        plain_count(&empty),
    );
    let one = usage(fs, &root, UsageLimits::new(0, Duration::from_secs(60)));
    expect_eq(
        "a root with an entry is incomplete with max_entries 0",
        one.complete,
        false,
    );
}

/// Builds `levels` nested folders named `d` below `root`, and a file in the
/// deepest one.
fn nest(root: &Path, levels: usize) {
    let mut path = root.to_path_buf();
    for _ in 0..levels {
        path.push("d");
    }
    must_io("create nested folders", fs::create_dir_all(&path));
    write_distinct(&path.join("leaf"), 5_000, 7);
}

fn check_depth(fs: &dyn Filesystem, top: &Path) {
    let fits = top.join("depth-fits");
    let too_deep = top.join("depth-over");
    // The root is level 1, so 31 folders below it reach level 32.
    nest(&fits, TREE_USAGE_MAX_DEPTH - 1);
    nest(&too_deep, TREE_USAGE_MAX_DEPTH);

    let deep = usage(fs, &fits, ROOMY);
    expect_eq("a tree 32 levels deep is complete", deep.complete, true);
    expect_eq(
        "a tree 32 levels deep is counted in full",
        deep.bytes,
        plain_count(&fits),
    );
    let over = usage(fs, &too_deep, ROOMY);
    expect_eq(
        "a folder at level 33 makes the result incomplete",
        over.complete,
        false,
    );
    let whole = plain_count(&too_deep);
    if over.bytes >= whole {
        fail(
            "the content of the folder at level 33 is not counted",
            &format!("{} bytes against {whole} with the content", over.bytes),
        );
    }
}

fn check_zero_deadline(fs: &dyn Filesystem, top: &Path) {
    let empty = top.join("deadline-empty");
    must_io("create an empty folder", fs::create_dir(&empty));
    let none = usage(fs, &empty, UsageLimits::new(1_000, Duration::ZERO));
    expect_eq(
        "a zero deadline finishes an empty root",
        none.complete,
        true,
    );

    let full = top.join("deadline-full");
    must_io("create a folder", fs::create_dir(&full));
    write_distinct(&full.join("file"), 70_000, 8);
    let cut = usage(fs, &full, UsageLimits::new(1_000, Duration::ZERO));
    expect_eq(
        "a zero deadline stops a root with an entry",
        cut.complete,
        false,
    );
    expect_eq(
        "a zero deadline counts the block of the root only",
        cut.bytes,
        fs::symlink_metadata(&full).map_or(u64::MAX, |info| info.blocks() * 512),
    );
}

fn check_root_errors(fs: &dyn Filesystem, top: &Path) {
    let dir = top.join("errors");
    must_io("create a folder", fs::create_dir(&dir));
    let file = top.join("a-file");
    must_io("write a file", fs::write(&file, b"x"));
    let link = top.join("a-link");
    must_io("create a symlink to a folder", symlink(&dir, &link));

    let invalid = |err: &PlatformError| {
        matches!(
            err,
            PlatformError::Io { op: "tree_usage", kind, .. }
                if *kind == std::io::ErrorKind::InvalidInput
        )
    };
    let mut trailing = OsString::from(dir.as_os_str());
    trailing.push("/");
    let dotted: PathBuf = dir.join("..").join("errors");
    for (check, root) in [
        (
            "tree_usage refuses a relative root",
            PathBuf::from("relative/path"),
        ),
        (
            "tree_usage refuses a root with a slash at the end",
            PathBuf::from(trailing),
        ),
        ("tree_usage refuses a root with a .. component", dotted),
    ] {
        expect_err(check, fs.tree_usage(&root, ROOMY), invalid);
    }
    expect_err(
        "tree_usage refuses a root that is a symlink",
        fs.tree_usage(&link, ROOMY),
        |err| matches!(err, PlatformError::SymlinkInPath { path } if *path == link),
    );
    expect_err(
        "tree_usage refuses a root that is a file",
        fs.tree_usage(&file, ROOMY),
        |err| matches!(err, PlatformError::NotADirectory { path } if *path == file),
    );
    expect_err(
        "tree_usage reports a missing root",
        fs.tree_usage(&top.join("missing"), ROOMY),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
}
