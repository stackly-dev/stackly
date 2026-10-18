use std::fmt::Debug;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;

use stackly_platform::{FileKind, Filesystem, PlatformError, UserId};

use crate::TestRoot;

/// Runs the same behavior checks against any [`Filesystem`], so the fake in
/// this crate and each OS adapter cannot drift apart unnoticed.
///
/// It works in a fresh `conformance` folder inside `root` and checks: mode
/// 0700 for new directories and 0600 for written files, `AlreadyExists` for
/// an existing path and for a symlink, `symlink_metadata` without following,
/// `NotARegularFile` for a symlink, directory, and FIFO, the
/// `read_file_limited` size limit, and that no temporary file is left after
/// a failed write.
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
