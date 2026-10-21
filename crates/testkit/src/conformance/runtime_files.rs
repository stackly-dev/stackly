//! Conformance checks for the `Filesystem` methods feature 18 adds:
//! `open_read`, `open_append`, `rename_no_replace`, `volume_space` with
//! `available_space`, and `open_tree_writer` (spec 0018).

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use stackly_platform::{EntryPath, FileKind, FileMode, Filesystem, PlatformError, TreeWriter};

use super::{expect_absent, expect_eq, expect_err, fail, make_fifo, must, must_io};

pub(super) fn check_open_read(fs: &dyn Filesystem, base: &Path) {
    let dir = base.join("open-read");
    must("create the open_read folder", fs.create_dir_private(&dir));
    let file = dir.join("artifact");
    must(
        "write a file to open",
        fs.write_file_atomic(&file, b"0123456789"),
    );

    let mut opened = must("open_read opens a regular file", fs.open_read(&file));
    expect_eq("open_read reports the size", opened.len(), 10);
    let mut all = Vec::new();
    must_io("read the opened file", opened.read_to_end(&mut all));
    expect_eq(
        "open_read reads the whole file",
        all,
        b"0123456789".to_vec(),
    );
    must_io("seek in the opened file", opened.seek(SeekFrom::Start(4)));
    let mut part = [0_u8; 3];
    must_io("read after a seek", opened.read_exact(&mut part));
    expect_eq("open_read seeks", &part, b"456");

    let link = dir.join("link");
    must_io("create a symlink to the file", symlink(&file, &link));
    let fifo = dir.join("fifo");
    make_fifo(&fifo);
    for (check, path) in [
        ("open_read refuses a symlink", &link),
        ("open_read refuses a directory", &dir),
        ("open_read refuses a FIFO", &fifo),
    ] {
        expect_err(check, fs.open_read(path).map(drop), |err| {
            matches!(err, PlatformError::NotARegularFile { .. })
        });
    }
    expect_err(
        "open_read reports a missing file",
        fs.open_read(&dir.join("missing")).map(drop),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
}

pub(super) fn check_open_append(fs: &dyn Filesystem, base: &Path) {
    let dir = base.join("open-append");
    must("create the open_append folder", fs.create_dir_private(&dir));
    let part = dir.join("artifact.part");

    let mut file = must("open_append creates a file", fs.open_append(&part));
    expect_eq("a new append file is empty", file.len(), 0);
    must_io("append to a new file", file.write_all(b"hello"));
    expect_eq("open_append counts written bytes", file.len(), 5);
    must("sync an append file", file.sync());
    drop(file);
    let info = must("stat the append file", fs.symlink_metadata(&part))
        .unwrap_or_else(|| fail("open_append leaves the file", "None"));
    expect_eq("open_append creates mode 0600", info.mode, Some(0o600));

    let mut file = must("open_append reopens a file", fs.open_append(&part));
    expect_eq("a reopened append file reports its size", file.len(), 5);
    must_io("append to an existing file", file.write_all(b" world"));
    must("truncate an append file", file.truncate(5));
    expect_eq("truncate sets the size", file.len(), 5);
    must_io("append after a truncate", file.write_all(b"!"));
    expect_err(
        "truncate never grows a file",
        file.truncate(7),
        |err| matches!(err, PlatformError::Io { kind, .. } if *kind == std::io::ErrorKind::InvalidInput),
    );
    must("sync after a truncate", file.sync());
    drop(file);
    expect_eq(
        "writes after a truncate continue from the cut",
        must_io("read the append file", std::fs::read(&part)),
        b"hello!".to_vec(),
    );

    let target = dir.join("target");
    must(
        "write a symlink target",
        fs.write_file_atomic(&target, b"keep"),
    );
    let link = dir.join("link");
    must_io("create a symlink to a file", symlink(&target, &link));
    let fifo = dir.join("fifo");
    make_fifo(&fifo);
    for (check, path) in [
        ("open_append refuses a symlink", &link),
        ("open_append refuses a directory", &dir),
        ("open_append refuses a FIFO", &fifo),
    ] {
        expect_err(check, fs.open_append(path).map(drop), |err| {
            matches!(err, PlatformError::NotARegularFile { .. })
        });
    }
    expect_eq(
        "open_append leaves a symlink's target intact",
        must_io("read the symlink target", std::fs::read(&target)),
        b"keep".to_vec(),
    );
    expect_err(
        "open_append needs the parent to exist",
        fs.open_append(&dir.join("missing").join("file")).map(drop),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
}

pub(super) fn check_rename_no_replace(fs: &dyn Filesystem, base: &Path) {
    let dir = base.join("rename");
    must("create the rename folder", fs.create_dir_private(&dir));
    let read = |path: &Path| must_io("read a renamed file", std::fs::read(path));

    let (a, b) = (dir.join("a"), dir.join("b"));
    must("write a file to rename", fs.write_file_atomic(&a, b"A"));
    must(
        "rename_no_replace moves a file",
        fs.rename_no_replace(&a, &b),
    );
    expect_absent("rename_no_replace leaves no source", &a);
    expect_eq("rename_no_replace keeps the bytes", read(&b), b"A".to_vec());

    let (tree, moved) = (dir.join("tree"), dir.join("moved"));
    must("create a folder to rename", fs.create_dir_private(&tree));
    must(
        "write a file in the folder",
        fs.write_file_atomic(&tree.join("inner"), b"I"),
    );
    must(
        "rename_no_replace moves a folder",
        fs.rename_no_replace(&tree, &moved),
    );
    expect_eq(
        "rename_no_replace moves a folder with its contents",
        read(&moved.join("inner")),
        b"I".to_vec(),
    );

    let c = dir.join("c");
    must("write an existing target", fs.write_file_atomic(&c, b"C"));
    let dangling = dir.join("dangling");
    must_io(
        "create a dangling symlink",
        symlink(dir.join("nowhere"), &dangling),
    );
    for (check, target) in [
        ("rename_no_replace refuses an existing file", &c),
        ("rename_no_replace refuses an existing folder", &moved),
        ("rename_no_replace refuses a dangling symlink", &dangling),
    ] {
        expect_err(check, fs.rename_no_replace(&b, target), |err| {
            matches!(err, PlatformError::AlreadyExists { .. })
        });
    }
    expect_eq("a refused rename keeps the source", read(&b), b"A".to_vec());
    expect_eq("a refused rename keeps the target", read(&c), b"C".to_vec());
    let dangling_kind =
        must("stat the dangling symlink", fs.symlink_metadata(&dangling)).map(|info| info.kind);
    expect_eq(
        "a refused rename keeps a symlink target as a symlink",
        dangling_kind,
        Some(FileKind::Symlink),
    );

    let link = dir.join("link");
    must_io("create a symlink to move", symlink(&b, &link));
    let link_moved = dir.join("link-moved");
    must(
        "rename_no_replace moves a symlink",
        fs.rename_no_replace(&link, &link_moved),
    );
    let moved_kind =
        must("stat the moved symlink", fs.symlink_metadata(&link_moved)).map(|info| info.kind);
    expect_eq(
        "rename_no_replace moves a symlink as itself",
        moved_kind,
        Some(FileKind::Symlink),
    );
    expect_eq("moving a symlink keeps its target", read(&b), b"A".to_vec());

    expect_err(
        "rename_no_replace reports a missing source",
        fs.rename_no_replace(&dir.join("missing"), &dir.join("new")),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
    expect_err(
        "rename_no_replace reports a missing target parent",
        fs.rename_no_replace(&b, &dir.join("missing").join("new")),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
}

pub(super) fn check_volume_space(fs: &dyn Filesystem, base: &Path) {
    let space = must("volume_space reads the test volume", fs.volume_space(base));
    if space.total == 0 || space.available > space.total {
        fail("volume_space reports a sane volume", &format!("{space:?}"));
    }
    let available = must(
        "available_space reads the test volume",
        fs.available_space(base),
    );
    if available > space.total {
        fail(
            "available_space stays within the volume",
            &format!("{available} of {space:?}"),
        );
    }
    expect_err(
        "volume_space reports a missing path",
        fs.volume_space(&base.join("missing")),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
}

pub(super) fn check_volume_facts(fs: &dyn Filesystem, base: &Path) {
    let facts = must("volume_facts reads the test volume", fs.volume_facts(base));
    if facts.fs_type.is_empty() || !facts.local {
        fail(
            "volume_facts reports the test root as a local volume with a type name",
            &format!("{facts:?}"),
        );
    }
    expect_err(
        "volume_facts reports a missing path",
        fs.volume_facts(&base.join("missing")),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
}

pub(super) fn check_tree_writer(fs: &dyn Filesystem, base: &Path) {
    let dir = base.join("tree-writer");
    must("create the tree writer folder", fs.create_dir_private(&dir));
    let outside = dir.join("outside");
    must(
        "create a folder outside the tree",
        fs.create_dir_private(&outside),
    );
    let root = dir.join("tree");
    must("create the tree root", fs.create_dir_private(&root));
    let mode_of = |path: &Path| {
        must("stat a tree entry", fs.symlink_metadata(path))
            .and_then(|info| info.mode)
            .unwrap_or_else(|| fail("a tree entry exists", &path.display().to_string()))
    };

    let mut writer = must(
        "open_tree_writer opens an empty root",
        fs.open_tree_writer(&root),
    );
    let tree = writer.as_mut();
    must(
        "create_dir creates a folder",
        tree.create_dir(&entry("bin")),
    );
    expect_eq(
        "create_dir uses mode 0700",
        mode_of(&root.join("bin")),
        0o700,
    );
    let mut file = must(
        "create_file creates a file",
        tree.create_file(&entry("bin/tool")),
    );
    must_io("write a tree file", file.write_all(b"#!"));
    expect_eq(
        "finish returns the bytes written",
        must("finish a tree file", file.finish()),
        2,
    );
    expect_eq(
        "create_file uses mode 0600",
        mode_of(&root.join("bin/tool")),
        0o600,
    );
    must("create a second folder", tree.create_dir(&entry("lib")));
    let lib = must(
        "create a library",
        tree.create_file(&entry("lib/libz.dylib")),
    );
    must("finish an empty file", lib.finish());
    must(
        "create_symlink creates a symlink",
        tree.create_symlink(&entry("lib/libz.1.dylib"), "libz.dylib"),
    );
    expect_eq(
        "create_symlink writes the target as given",
        must_io(
            "read the symlink",
            std::fs::read_link(root.join("lib/libz.1.dylib")),
        ),
        Path::new("libz.dylib").to_path_buf(),
    );

    must(
        "set_mode changes a file",
        tree.set_mode(&entry("bin/tool"), mode(0o555)),
    );
    expect_eq(
        "set_mode sets file bits",
        mode_of(&root.join("bin/tool")),
        0o555,
    );
    must(
        "set_mode changes a folder",
        tree.set_mode(&entry("bin"), mode(0o755)),
    );
    expect_eq(
        "set_mode sets folder bits",
        mode_of(&root.join("bin")),
        0o755,
    );
    must(
        "set_mode accepts a symlink",
        tree.set_mode(&entry("lib/libz.1.dylib"), mode(0o777)),
    );
    expect_eq(
        "set_mode never follows a symlink",
        mode_of(&root.join("lib/libz.dylib")),
        0o600,
    );

    check_tree_writer_refusals(tree, &root, &outside);
    must("sync the tree", tree.sync());
    drop(writer);

    check_tree_root_refusals(fs, &dir, &root);
}

fn check_tree_writer_refusals(tree: &mut dyn TreeWriter, root: &Path, outside: &Path) {
    let exists = |err: &PlatformError| matches!(err, PlatformError::AlreadyExists { .. });
    expect_err(
        "create_dir refuses an existing entry",
        tree.create_dir(&entry("bin")),
        exists,
    );
    expect_err(
        "create_file refuses an existing file",
        tree.create_file(&entry("bin/tool")).map(drop),
        exists,
    );
    expect_err(
        "create_file refuses an existing symlink",
        tree.create_file(&entry("lib/libz.1.dylib")).map(drop),
        exists,
    );
    expect_err(
        "create_symlink refuses an existing entry",
        tree.create_symlink(&entry("bin/tool"), "elsewhere"),
        exists,
    );
    expect_err(
        "create_symlink refuses an empty target",
        tree.create_symlink(&entry("empty"), ""),
        |err| matches!(err, PlatformError::Io { kind, .. } if *kind == std::io::ErrorKind::InvalidInput),
    );

    must(
        "create a symlink to a folder outside the tree",
        tree.create_symlink(&entry("out"), "../outside"),
    );
    let through_symlink = |err: &PlatformError| matches!(err, PlatformError::SymlinkInPath { .. });
    expect_err(
        "create_file refuses a symlink on the way",
        tree.create_file(&entry("out/escaped")).map(drop),
        through_symlink,
    );
    expect_err(
        "create_dir refuses a symlink on the way",
        tree.create_dir(&entry("out/escaped")),
        through_symlink,
    );
    expect_err(
        "create_symlink refuses a symlink on the way",
        tree.create_symlink(&entry("out/escaped"), "x"),
        through_symlink,
    );
    expect_err(
        "set_mode refuses a symlink on the way",
        tree.set_mode(&entry("out/escaped"), mode(0o644)),
        through_symlink,
    );
    expect_absent(
        "nothing is written through a symlink",
        &outside.join("escaped"),
    );
    expect_err(
        "create_file refuses a file on the way",
        tree.create_file(&entry("bin/tool/inner")).map(drop),
        |err| matches!(err, PlatformError::NotADirectory { .. }),
    );
    expect_err(
        "create_file needs the parent to exist",
        tree.create_file(&entry("missing/file")).map(drop),
        |err| matches!(err, PlatformError::NotFound { .. }),
    );
    expect_absent("a refused create leaves nothing", &root.join("missing"));
}

fn check_tree_root_refusals(fs: &dyn Filesystem, dir: &Path, root: &Path) {
    expect_err(
        "open_tree_writer refuses a root that is not empty",
        fs.open_tree_writer(root).map(drop),
        |err| matches!(err, PlatformError::Io { kind, .. } if *kind == std::io::ErrorKind::DirectoryNotEmpty),
    );
    let empty = dir.join("empty");
    must("create an empty folder", fs.create_dir_private(&empty));
    let link = dir.join("root-link");
    must_io(
        "create a symlink to an empty folder",
        symlink(&empty, &link),
    );
    expect_err(
        "open_tree_writer refuses a symlinked root",
        fs.open_tree_writer(&link).map(drop),
        |err| matches!(err, PlatformError::SymlinkInPath { .. }),
    );
    let file = dir.join("file");
    must("write a file", fs.write_file_atomic(&file, b"x"));
    expect_err(
        "open_tree_writer refuses a file",
        fs.open_tree_writer(&file).map(drop),
        |err| matches!(err, PlatformError::NotADirectory { .. }),
    );
    let open = dir.join("open");
    must("create a folder", fs.create_dir_private(&open));
    must_io(
        "make the folder readable by others",
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)),
    );
    expect_err(
        "open_tree_writer refuses a root that is not mode 0700",
        fs.open_tree_writer(&open).map(drop),
        |err| matches!(err, PlatformError::Io { kind, .. } if *kind == std::io::ErrorKind::InvalidInput),
    );
}

fn entry(path: &str) -> EntryPath {
    EntryPath::new(path).unwrap_or_else(|err| fail("build an entry path", &err.to_string()))
}

fn mode(bits: u32) -> FileMode {
    FileMode::new(bits).unwrap_or_else(|| fail("build a file mode", &format!("{bits:#o}")))
}
