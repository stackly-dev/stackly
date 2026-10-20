use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use stackly_platform::{EntryPath, FileMode, PlatformError, TreeFile, TreeWriter};

use super::{FsOp, Shared};

/// [`Filesystem::open_tree_writer`](stackly_platform::Filesystem::open_tree_writer)
/// through `std::fs`, with the same checks on the root as the adapter.
pub(super) fn open(root: &Path, state: Shared) -> Result<Box<dyn TreeWriter>, PlatformError> {
    const OP: &str = "open_tree_writer";
    let info = fs::symlink_metadata(root).map_err(|err| PlatformError::from_io(OP, root, &err))?;
    let path = root.to_path_buf();
    if info.file_type().is_symlink() {
        return Err(PlatformError::SymlinkInPath { path });
    }
    if !info.is_dir() {
        return Err(PlatformError::NotADirectory { path });
    }
    let mode = info.permissions().mode() & 0o7777;
    if mode != 0o700 {
        return Err(PlatformError::Io {
            op: OP,
            path,
            kind: io::ErrorKind::InvalidInput,
            message: format!("the tree root has mode {mode:#o}, not 0o700"),
        });
    }
    let mut listing = fs::read_dir(root).map_err(|err| PlatformError::from_io(OP, root, &err))?;
    if listing.next().is_some() {
        return Err(PlatformError::Io {
            op: OP,
            path,
            kind: io::ErrorKind::DirectoryNotEmpty,
            message: "the tree root is not empty".to_owned(),
        });
    }
    Ok(Box::new(FakeTreeWriter {
        root: path,
        dirs: Vec::new(),
        state,
    }))
}

/// A [`TreeWriter`] through `std::fs` inside a test root. Each call checks
/// every component on the way with `symlink_metadata` before it acts, so
/// it has the adapter's semantics but not its safety against a folder
/// swapped for a symlink in between.
struct FakeTreeWriter {
    root: PathBuf,
    dirs: Vec<PathBuf>,
    state: Shared,
}

impl FakeTreeWriter {
    /// Checks the way to `path` and injected faults, and returns the full
    /// path.
    fn prepare(&self, op: FsOp, path: &EntryPath) -> Result<PathBuf, PlatformError> {
        let full = self.root.join(path.as_path());
        self.state.check_fault(op, &full)?;
        let mut names = path.components();
        names.next_back();
        let mut walked = self.root.clone();
        for name in names {
            walked.push(name);
            let info = fs::symlink_metadata(&walked)
                .map_err(|err| PlatformError::from_io(op.as_code(), &walked, &err))?;
            if info.file_type().is_symlink() {
                return Err(PlatformError::SymlinkInPath { path: walked });
            }
            if !info.is_dir() {
                return Err(PlatformError::NotADirectory { path: walked });
            }
        }
        Ok(full)
    }
}

impl TreeWriter for FakeTreeWriter {
    fn create_dir(&mut self, path: &EntryPath) -> Result<(), PlatformError> {
        let full = self.prepare(FsOp::TreeCreateDir, path)?;
        DirBuilder::new()
            .mode(0o700)
            .create(&full)
            .map_err(|err| PlatformError::from_io("create_dir", &full, &err))?;
        self.state.record(FsOp::TreeCreateDir, &full);
        self.dirs.push(full);
        Ok(())
    }

    fn create_file(&mut self, path: &EntryPath) -> Result<Box<dyn TreeFile + '_>, PlatformError> {
        let full = self.prepare(FsOp::TreeCreateFile, path)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&full)
            .map_err(|err| PlatformError::from_io("create_file", &full, &err))?;
        self.state.record(FsOp::TreeCreateFile, &full);
        Ok(Box::new(FakeTreeFile {
            file,
            written: 0,
            path: full,
        }))
    }

    fn create_symlink(&mut self, path: &EntryPath, target: &str) -> Result<(), PlatformError> {
        let full = self.root.join(path.as_path());
        if target.is_empty() || target.contains('\0') {
            return Err(PlatformError::Io {
                op: "create_symlink",
                path: full,
                kind: io::ErrorKind::InvalidInput,
                message: "the symlink target is empty or holds a NUL byte".to_owned(),
            });
        }
        let full = self.prepare(FsOp::TreeCreateSymlink, path)?;
        symlink(target, &full)
            .map_err(|err| PlatformError::from_io("create_symlink", &full, &err))?;
        self.state.record(FsOp::TreeCreateSymlink, &full);
        Ok(())
    }

    fn set_mode(&mut self, path: &EntryPath, mode: FileMode) -> Result<(), PlatformError> {
        let full = self.prepare(FsOp::TreeSetMode, path)?;
        let info = fs::symlink_metadata(&full)
            .map_err(|err| PlatformError::from_io("set_mode", &full, &err))?;
        if info.file_type().is_symlink() {
            // std cannot change a link's own bits, and following it would
            // change its target.
            return Ok(());
        }
        fs::set_permissions(&full, Permissions::from_mode(mode.bits()))
            .map_err(|err| PlatformError::from_io("set_mode", &full, &err))
    }

    fn sync(&mut self) -> Result<(), PlatformError> {
        self.state.check_fault(FsOp::TreeSync, &self.root)?;
        for dir in self.dirs.iter().chain([&self.root]) {
            File::open(dir)
                .and_then(|dir| dir.sync_all())
                .map_err(|err| PlatformError::from_io("sync", dir, &err))?;
        }
        Ok(())
    }
}

struct FakeTreeFile {
    file: File,
    written: u64,
    path: PathBuf,
}

impl Write for FakeTreeFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.file.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl TreeFile for FakeTreeFile {
    fn finish(self: Box<Self>) -> Result<u64, PlatformError> {
        self.file
            .sync_all()
            .map_err(|err| PlatformError::from_io("finish", &self.path, &err))?;
        Ok(self.written)
    }
}
