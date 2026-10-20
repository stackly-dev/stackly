use std::fmt::Debug;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};

use stackly_platform::{
    FileKind, Filesystem, PlatformError, REMOVE_TREE_MAX_DEPTH, RemoveReport, UserId,
};

use crate::TestRoot;

mod runtime_files;

/// Runs the same behavior checks against any [`Filesystem`], so the fake in
/// this crate and each OS adapter cannot drift apart unnoticed.
///
/// It works in a fresh `conformance` folder inside `root` and checks: mode
/// 0700 for new directories and 0600 for written files, `AlreadyExists` for
/// an existing path and for a symlink, `symlink_metadata` without following,
/// `NotARegularFile` for a symlink, directory, and FIFO, the
/// `read_file_limited` size limit, that no temporary file is left after
/// a failed write, and that `remove_tree` removes a tree with files,
/// symlinks, and a FIFO while every symlink's target stays intact, removes
/// a symlink or file target as itself, reports a missing target, refuses a
/// symlink or file on the way to the target and a symlinked root without
/// removing anything, rejects invalid arguments, and stops past
/// [`REMOVE_TREE_MAX_DEPTH`] levels with `TooDeep`.
///
/// For feature 18 it also checks that `open_read` reads and seeks a
/// regular file and refuses a symlink, directory, and FIFO; that
/// `open_append` creates mode 0600, appends, truncates without growing,
/// and refuses the same kinds; that `rename_no_replace` moves a file, a
/// folder, and a symlink as itself and refuses any existing target; that
/// `volume_space` and `available_space` read the test volume; and that a
/// tree writer creates folders 0700 and files 0600, sets modes without
/// following a symlink, refuses existing entries and every symlink or file
/// on the way, and opens only an empty, non symlinked root with mode 0700.
///
/// # Panics
///
/// Panics with the name of the first check that fails.
pub fn fs_conformance(fs: &dyn Filesystem, root: &TestRoot) {
    let base = root.path().join("conformance");
    let uid = std::fs::metadata(root.path())
        .expect("a test root exists while the test holds it")
        .uid();
    must(
        "create the conformance folder",
        fs.create_dir_private(&base),
    );

    check_directories(fs, &base, uid);
    check_symlinks(fs, &base);
    check_atomic_writes(fs, &base);
    check_limited_reads(fs, &base);
    check_listing(fs, &base);
    check_failed_write_cleanup(fs, &base);
    check_removal(fs, &base);
    runtime_files::check_open_read(fs, &base);
    runtime_files::check_open_append(fs, &base);
    runtime_files::check_rename_no_replace(fs, &base);
    runtime_files::check_volume_space(fs, &base);
    runtime_files::check_tree_writer(fs, &base);
}

fn check_directories(fs: &dyn Filesystem, base: &Path, uid: u32) {
    let dir = base.join("dir");
    must(
        "create_dir_private creates a directory",
        fs.create_dir_private(&dir),
    );
    let info = must(
        "symlink_metadata reads a directory",
        fs.symlink_metadata(&dir),
    )
    .unwrap_or_else(|| fail("symlink_metadata finds a created directory", "None"));
    expect_eq(
        "create_dir_private makes a directory",
        info.kind,
        FileKind::Dir,
    );
    expect_eq("create_dir_private uses mode 0700", info.mode, Some(0o700));
    expect_eq(
        "a new directory belongs to the current user",
        info.owner,
        Some(UserId::Unix(uid)),
    );
    expect_err(
        "create_dir_private refuses an existing directory",
        fs.create_dir_private(&dir),
        |err| matches!(err, PlatformError::AlreadyExists { .. }),
    );
    expect_err(
        "create_dir_private needs the parent to exist",
        fs.create_dir_private(&base.join("missing").join("child")),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
    expect_eq(
        "symlink_metadata returns None for a missing path",
        must(
            "symlink_metadata of a missing path",
            fs.symlink_metadata(&base.join("missing")),
        ),
        None,
    );
}

fn check_symlinks(fs: &dyn Filesystem, base: &Path) {
    let link = base.join("link");
    let dangling = base.join("dangling");
    must_io("create a symlink", symlink(base.join("dir"), &link));
    must_io(
        "create a dangling symlink",
        symlink(base.join("nowhere"), &dangling),
    );

    for (name, path) in [("symlink", &link), ("dangling symlink", &dangling)] {
        let info = must(
            "symlink_metadata reads a symlink",
            fs.symlink_metadata(path),
        )
        .unwrap_or_else(|| fail("symlink_metadata finds a symlink", name));
        expect_eq(
            "symlink_metadata does not follow a symlink",
            info.kind,
            FileKind::Symlink,
        );
        expect_err(
            "create_dir_private refuses a symlink",
            fs.create_dir_private(path),
            |err| matches!(err, PlatformError::AlreadyExists { .. }),
        );
    }
    expect_eq(
        "canonicalize resolves a symlink",
        must("canonicalize a symlink", fs.canonicalize(&link)),
        must(
            "canonicalize a directory",
            fs.canonicalize(&base.join("dir")),
        ),
    );
}

fn check_atomic_writes(fs: &dyn Filesystem, base: &Path) {
    let file = base.join("file.json");
    must(
        "write_file_atomic creates a file",
        fs.write_file_atomic(&file, b"one"),
    );
    must(
        "write_file_atomic replaces a file",
        fs.write_file_atomic(&file, b"two!"),
    );
    let info = must("symlink_metadata reads a file", fs.symlink_metadata(&file))
        .unwrap_or_else(|| fail("symlink_metadata finds a written file", "None"));
    expect_eq(
        "write_file_atomic makes a regular file",
        info.kind,
        FileKind::File,
    );
    expect_eq("write_file_atomic uses mode 0600", info.mode, Some(0o600));
    expect_eq("write_file_atomic writes every byte", info.len, 4);
    expect_err(
        "write_file_atomic needs the parent to exist",
        fs.write_file_atomic(&base.join("missing").join("file.json"), b"x"),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
    expect_no_temp_files("write_file_atomic leaves no temporary file", base);
}

fn check_limited_reads(fs: &dyn Filesystem, base: &Path) {
    let file = base.join("file.json");
    expect_eq(
        "read_file_limited reads a file at the limit",
        must(
            "read_file_limited at the limit",
            fs.read_file_limited(&file, 4),
        ),
        b"two!".to_vec(),
    );
    expect_err(
        "read_file_limited refuses a file over the limit",
        fs.read_file_limited(&file, 3),
        |err| matches!(err, PlatformError::FileTooLarge { max: 3, .. }),
    );
    let file_link = base.join("file-link");
    must_io("create a symlink to a file", symlink(&file, &file_link));
    let fifo = base.join("fifo");
    make_fifo(&fifo);
    for path in [&file_link, &base.join("dir"), &fifo] {
        expect_err(
            "read_file_limited refuses anything but a regular file",
            fs.read_file_limited(path, 1024),
            |err| matches!(err, PlatformError::NotARegularFile { .. }),
        );
    }
    expect_err(
        "read_file_limited reports a missing file",
        fs.read_file_limited(&base.join("missing.json"), 1024),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
}

fn check_listing(fs: &dyn Filesystem, base: &Path) {
    let mut entries = must("read_dir lists a directory", fs.read_dir(base));
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let listed: Vec<(String, FileKind)> = entries
        .iter()
        .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.info.kind))
        .collect();
    let expected: Vec<(String, FileKind)> = [
        ("dangling", FileKind::Symlink),
        ("dir", FileKind::Dir),
        ("fifo", FileKind::Other),
        ("file-link", FileKind::Symlink),
        ("file.json", FileKind::File),
        ("link", FileKind::Symlink),
    ]
    .into_iter()
    .map(|(name, kind)| (name.to_owned(), kind))
    .collect();
    expect_eq(
        "read_dir lists entries without following symlinks",
        listed,
        expected,
    );
}

fn check_failed_write_cleanup(fs: &dyn Filesystem, base: &Path) {
    // Renaming a file over a non empty directory fails on every Unix.
    let target = base.join("occupied");
    must("create the rename target", fs.create_dir_private(&target));
    must(
        "write inside the rename target",
        fs.write_file_atomic(&target.join("keep"), b"k"),
    );
    let result = fs.write_file_atomic(&target, b"never lands");
    if result.is_ok() {
        fail("write_file_atomic fails when the rename fails", "Ok(())");
    }
    let info = must(
        "symlink_metadata after a failed write",
        fs.symlink_metadata(&target),
    );
    expect_eq(
        "a failed write leaves the old target intact",
        info.map(|info| info.kind),
        Some(FileKind::Dir),
    );
    expect_no_temp_files("a failed write leaves no temporary file", base);
}

/// The outcome of [`fs_cross_device_conformance`].
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CrossDevice {
    /// Every check ran and passed.
    Checked,
    /// The disk image could not be attached, so nothing was checked. The
    /// reason names the failed `hdiutil` step and its output.
    Blocked(String),
}

/// Checks that `remove_tree` stops with `CrossesDevice` at a disk image
/// attached inside the tree, before touching anything on it: when the image
/// is in the tree, when it is the target, and when it is on the way to the
/// target. After the image is detached the same tree is removed.
///
/// It works in a fresh `cross-device` folder inside `root` and attaches a
/// [`DiskImage`](crate::DiskImage), which it always detaches before it
/// returns or unwinds.
///
/// # Panics
///
/// Panics with the name of the first check that fails.
#[cfg(target_os = "macos")]
#[must_use = "a blocked check proves nothing; record it as BLOCKED"]
pub fn fs_cross_device_conformance(fs: &dyn Filesystem, root: &TestRoot) -> CrossDevice {
    let tree = root.path().join("cross-device").join("tree");
    must_io("create the tree", std::fs::create_dir_all(&tree));
    must_io(
        "write a file in the tree",
        std::fs::write(tree.join("file"), b"f"),
    );
    let mount = tree.join("mnt");
    let image = match crate::DiskImage::attach(root, &mount) {
        Ok(image) => image,
        Err(err) => return CrossDevice::Blocked(err.to_string()),
    };
    let keep = mount.join("keep");
    must_io("write a file on the image", std::fs::write(&keep, b"image"));

    for relative in [
        "cross-device/tree",
        "cross-device/tree/mnt",
        "cross-device/tree/mnt/keep",
    ] {
        expect_err(
            "remove_tree stops at a mount point in, as, or on the way to the target",
            fs.remove_tree(root.path(), Path::new(relative)),
            |err| matches!(err, PlatformError::CrossesDevice { path } if *path == mount),
        );
    }
    expect_eq(
        "remove_tree touches nothing on another device",
        must_io("read the file on the image", std::fs::read(&keep)),
        b"image".to_vec(),
    );
    drop(image);
    let report = must(
        "remove_tree after the image is detached",
        fs.remove_tree(root.path(), Path::new("cross-device/tree")),
    );
    expect_eq(
        "remove_tree finds the tree after the image is detached",
        report.missing,
        false,
    );
    expect_absent("remove_tree removes the former mount point", &tree);
    CrossDevice::Checked
}

fn check_removal(fs: &dyn Filesystem, base: &Path) {
    let removal = base.join("removal");
    must_io("create the removal folder", std::fs::create_dir(&removal));
    let sentinel = removal.join("sentinel");
    must_io(
        "create the sentinel folder",
        std::fs::create_dir_all(sentinel.join("inner")),
    );
    must_io(
        "write a sentinel file",
        std::fs::write(sentinel.join("keep"), b"sentinel"),
    );
    must_io(
        "write a nested sentinel file",
        std::fs::write(sentinel.join("inner").join("deep"), b"deep"),
    );

    check_removing_a_tree(fs, base, &removal, &sentinel);
    check_removing_single_entries(fs, base, &removal, &sentinel);
    check_refusals(fs, base, &removal, &sentinel);
    check_depth_limit(fs, base, &removal);
}

fn check_removing_a_tree(fs: &dyn Filesystem, base: &Path, removal: &Path, sentinel: &Path) {
    let tree = removal.join("tree");
    must_io(
        "create a nested tree",
        std::fs::create_dir_all(tree.join("sub").join("nested")),
    );
    must_io(
        "create an empty folder in the tree",
        std::fs::create_dir(tree.join("sub").join("empty")),
    );
    must_io(
        "write a file in the tree",
        std::fs::write(tree.join("file"), b"a"),
    );
    must_io(
        "write a nested file in the tree",
        std::fs::write(tree.join("sub").join("nested").join("file"), b"b"),
    );
    must_io(
        "link a folder outside the tree",
        symlink(sentinel, tree.join("link-dir")),
    );
    must_io(
        "link a file outside the tree",
        symlink(sentinel.join("keep"), tree.join("sub").join("link-file")),
    );
    must_io(
        "create a dangling symlink in the tree",
        symlink(removal.join("nowhere"), tree.join("dangling")),
    );
    make_fifo(&tree.join("fifo"));

    expect_eq(
        "remove_tree removes a tree and counts files and folders",
        must(
            "remove_tree on a tree",
            fs.remove_tree(base, Path::new("removal/tree")),
        ),
        RemoveReport::removed(6, 4),
    );
    expect_absent("remove_tree leaves nothing of the tree", &tree);
    expect_sentinel_intact("remove_tree never follows a symlink in the tree", sentinel);
    expect_eq(
        "remove_tree on a removed tree reports it missing",
        must(
            "remove_tree twice",
            fs.remove_tree(base, Path::new("removal/tree")),
        ),
        RemoveReport::missing(),
    );
    expect_eq(
        "remove_tree under a missing folder reports it missing",
        must(
            "remove_tree under a missing folder",
            fs.remove_tree(base, Path::new("removal/absent/child")),
        ),
        RemoveReport::missing(),
    );
}

fn check_removing_single_entries(
    fs: &dyn Filesystem,
    base: &Path,
    removal: &Path,
    sentinel: &Path,
) {
    must_io(
        "link the sentinel folder as a target",
        symlink(sentinel, removal.join("alias")),
    );
    expect_eq(
        "remove_tree removes a symlink target as itself",
        must(
            "remove_tree on a symlink",
            fs.remove_tree(base, Path::new("removal/alias")),
        ),
        RemoveReport::removed(1, 0),
    );
    expect_absent("remove_tree removes the symlink", &removal.join("alias"));
    expect_sentinel_intact("remove_tree never follows a symlink target", sentinel);

    must_io(
        "write a single file",
        std::fs::write(removal.join("single"), b"x"),
    );
    expect_eq(
        "remove_tree removes a file target",
        must(
            "remove_tree on a file",
            fs.remove_tree(base, Path::new("removal/single")),
        ),
        RemoveReport::removed(1, 0),
    );
    expect_absent("remove_tree removes the file", &removal.join("single"));
}

fn check_refusals(fs: &dyn Filesystem, base: &Path, removal: &Path, sentinel: &Path) {
    let via = removal.join("via");
    must_io(
        "link the sentinel folder on the way",
        symlink(sentinel, &via),
    );
    expect_err(
        "remove_tree refuses a symlink on the way to the target",
        fs.remove_tree(base, Path::new("removal/via/keep")),
        |err| matches!(err, PlatformError::SymlinkInPath { path } if *path == via),
    );
    expect_err(
        "remove_tree refuses a symlinked root",
        fs.remove_tree(&via, Path::new("keep")),
        |err| matches!(err, PlatformError::SymlinkInPath { path } if *path == via),
    );
    let plain = removal.join("plain");
    must_io("write a file on the way", std::fs::write(&plain, b"p"));
    expect_err(
        "remove_tree refuses a file on the way to the target",
        fs.remove_tree(base, Path::new("removal/plain/child")),
        |err| matches!(err, PlatformError::NotADirectory { path } if *path == plain),
    );
    for relative in ["", "../conformance", "/removal", "removal/../removal"] {
        expect_err(
            "remove_tree rejects an empty, absolute, or dotted relative path",
            fs.remove_tree(base, Path::new(relative)),
            |err| {
                matches!(
                    err,
                    PlatformError::Io {
                        kind: std::io::ErrorKind::InvalidInput,
                        ..
                    }
                )
            },
        );
    }
    expect_sentinel_intact("a refused removal removes nothing", sentinel);
    expect_eq(
        "a refused removal keeps the file on the way",
        must_io("read the file on the way", std::fs::read(&plain)),
        b"p".to_vec(),
    );
    expect_eq(
        "a rejected argument removes nothing",
        removal.exists(),
        true,
    );
}

fn check_depth_limit(fs: &dyn Filesystem, base: &Path, removal: &Path) {
    let at_limit = nested(&removal.join("deep"), REMOVE_TREE_MAX_DEPTH);
    must_io(
        "create folders nested at the limit",
        std::fs::create_dir_all(&at_limit),
    );
    expect_eq(
        "remove_tree removes folders nested at the limit",
        must(
            "remove_tree at the depth limit",
            fs.remove_tree(base, Path::new("removal/deep")),
        ),
        RemoveReport::removed(0, REMOVE_TREE_MAX_DEPTH as u64),
    );
    let past_limit = nested(&removal.join("deep"), REMOVE_TREE_MAX_DEPTH + 1);
    must_io(
        "create folders nested past the limit",
        std::fs::create_dir_all(&past_limit),
    );
    expect_err(
        "remove_tree stops past the depth limit",
        fs.remove_tree(base, Path::new("removal/deep")),
        |err| {
            matches!(
                err,
                PlatformError::TooDeep { path, max: REMOVE_TREE_MAX_DEPTH } if *path == past_limit
            )
        },
    );
    expect_eq(
        "a removal stopped past the depth limit keeps the deepest folder",
        past_limit.is_dir(),
        true,
    );
}

/// `top` with `levels - 1` more folders named `d` below it.
fn nested(top: &Path, levels: usize) -> PathBuf {
    let mut path = top.to_path_buf();
    for _ in 1..levels {
        path.push("d");
    }
    path
}

fn expect_absent(check: &str, path: &Path) {
    if std::fs::symlink_metadata(path).is_ok() {
        fail(check, &format!("{} still exists", path.display()));
    }
}

fn expect_sentinel_intact(check: &str, sentinel: &Path) {
    let keep =
        std::fs::read(sentinel.join("keep")).unwrap_or_else(|err| fail(check, &err.to_string()));
    let deep = std::fs::read(sentinel.join("inner").join("deep"))
        .unwrap_or_else(|err| fail(check, &err.to_string()));
    expect_eq(
        check,
        (keep, deep),
        (b"sentinel".to_vec(), b"deep".to_vec()),
    );
}

fn expect_no_temp_files(check: &str, dir: &Path) {
    let leftovers: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| fail(check, &err.to_string()))
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect();
    if !leftovers.is_empty() {
        fail(check, &format!("{leftovers:?}"));
    }
}

fn make_fifo(path: &Path) {
    crate::make_fifo(path).unwrap_or_else(|err| fail("run mkfifo", &err.to_string()));
}

fn must<T>(check: &str, result: Result<T, PlatformError>) -> T {
    result.unwrap_or_else(|err| fail(check, &err.to_string()))
}

fn must_io<T>(check: &str, result: std::io::Result<T>) -> T {
    result.unwrap_or_else(|err| fail(check, &err.to_string()))
}

fn expect_eq<T: PartialEq + Debug>(check: &str, actual: T, expected: T) {
    if actual != expected {
        fail(check, &format!("expected {expected:?}, got {actual:?}"));
    }
}

fn expect_err<T: Debug>(
    check: &str,
    result: Result<T, PlatformError>,
    matches: impl FnOnce(&PlatformError) -> bool,
) {
    match result {
        Err(err) if matches(&err) => {}
        other => fail(check, &format!("{other:?}")),
    }
}

fn fail(check: &str, detail: &str) -> ! {
    panic!("fs_conformance check failed: {check}: {detail}")
}
