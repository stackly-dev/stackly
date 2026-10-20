use std::ffi::{CStr, CString};
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use stackly_platform::{EntryPath, FileMode, PlatformError, TreeFile, TreeWriter};

use super::fd::{
    DIR_FLAGS, c_name, c_path, fstat, fsync, full_fsync, is_dir, is_symlink, open_dir_at, stat_at,
};

/// Flags for creating a file: write only, never an existing entry, never
/// through a symlink, close on exec.
const FILE_FLAGS: libc::c_int =
    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;

/// [`Filesystem::open_tree_writer`](stackly_platform::Filesystem::open_tree_writer):
/// opens `root` without following a symlink, checks that it is an empty
/// directory with mode 0700, and returns a writer that works only through
/// that descriptor.
pub(super) fn open_tree_writer(root: &Path) -> Result<Box<dyn TreeWriter>, PlatformError> {
    const OP: &str = "open_tree_writer";
    let c_root = c_path(OP, root)?;
    // SAFETY: `c_root` is NUL terminated; `open` keeps no pointer, and
    // without `O_CREAT` it reads no mode.
    let fd = unsafe { libc::open(c_root.as_ptr(), DIR_FLAGS) };
    if fd < 0 {
        let err = io::Error::last_os_error();
        let path = root.to_path_buf();
        return Err(match err.raw_os_error() {
            Some(libc::ELOOP) => PlatformError::SymlinkInPath { path },
            Some(libc::ENOTDIR) => match fs::symlink_metadata(root) {
                Ok(info) if info.file_type().is_symlink() => PlatformError::SymlinkInPath { path },
                _ => PlatformError::NotADirectory { path },
            },
            _ => PlatformError::from_io(OP, root, &err),
        });
    }
    // SAFETY: `open` returned a new descriptor that nothing else owns.
    let root_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let stat = fstat(root_fd.as_fd()).map_err(|err| PlatformError::from_io(OP, root, &err))?;
    if stat.st_mode & 0o7777 != 0o700 {
        return Err(PlatformError::Io {
            op: OP,
            path: root.to_path_buf(),
            kind: io::ErrorKind::InvalidInput,
            message: format!(
                "the tree root has mode {:#o}, not 0o700",
                stat.st_mode & 0o7777
            ),
        });
    }
    // The listing goes through the path; the writer itself only ever uses
    // the descriptor opened above.
    let mut listing = fs::read_dir(root).map_err(|err| PlatformError::from_io(OP, root, &err))?;
    if listing.next().is_some() {
        return Err(PlatformError::Io {
            op: OP,
            path: root.to_path_buf(),
            kind: io::ErrorKind::DirectoryNotEmpty,
            message: "the tree root is not empty".to_owned(),
        });
    }
    Ok(Box::new(MacosTreeWriter {
        root: root_fd,
        root_path: root.to_path_buf(),
        dirs: Vec::new(),
    }))
}

/// A [`TreeWriter`] over file descriptors. Every call walks from the root
/// descriptor one component at a time with `openat(O_DIRECTORY |
/// O_NOFOLLOW)` and creates the last component relative to the directory
/// it opened, so a folder swapped for a symlink is never followed and
/// nothing is ever written outside the root.
struct MacosTreeWriter {
    root: OwnedFd,
    root_path: PathBuf,
    /// Every directory this writer created, in creation order, for
    /// [`TreeWriter::sync`].
    dirs: Vec<EntryPath>,
}

impl MacosTreeWriter {
    fn full_path(&self, path: &EntryPath) -> PathBuf {
        self.root_path.join(path.as_path())
    }

    /// Opens the directory `names` leads to from the root, or returns
    /// `None` for the root itself.
    fn open_dir<'a>(
        &self,
        op: &'static str,
        names: impl Iterator<Item = &'a str>,
    ) -> Result<Option<OwnedFd>, PlatformError> {
        let mut current: Option<OwnedFd> = None;
        let mut walked = self.root_path.clone();
        for name in names {
            walked.push(name);
            let c_name = c_name(op, name, &walked)?;
            let dir = current.as_ref().map_or(self.root.as_fd(), AsFd::as_fd);
            match open_dir_at(dir, &c_name) {
                Ok(next) => current = Some(next),
                Err(err) => return Err(walk_error(op, dir, &c_name, &walked, &err)),
            }
        }
        Ok(current)
    }

    /// Opens the parent of `path` and returns it with the last component.
    fn open_parent(
        &self,
        op: &'static str,
        path: &EntryPath,
    ) -> Result<(Option<OwnedFd>, CString), PlatformError> {
        let mut names = path.components();
        let last = names
            .next_back()
            .expect("an EntryPath holds at least one component");
        let parent = self.open_dir(op, names)?;
        let name = c_name(op, last, &self.full_path(path))?;
        Ok((parent, name))
    }

    fn parent_fd<'a>(&'a self, parent: &'a Option<OwnedFd>) -> BorrowedFd<'a> {
        parent.as_ref().map_or(self.root.as_fd(), AsFd::as_fd)
    }
}

impl TreeWriter for MacosTreeWriter {
    fn create_dir(&mut self, path: &EntryPath) -> Result<(), PlatformError> {
        const OP: &str = "create_dir";
        let (parent, name) = self.open_parent(OP, path)?;
        let dir = self.parent_fd(&parent);
        // SAFETY: `dir` is an open descriptor and `name` is NUL terminated;
        // `mkdirat` keeps no pointer.
        let rc = unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            return Err(PlatformError::from_io(OP, &self.full_path(path), &err));
        }
        self.dirs.push(path.clone());
        Ok(())
    }

    fn create_file(&mut self, path: &EntryPath) -> Result<Box<dyn TreeFile + '_>, PlatformError> {
        const OP: &str = "create_file";
        let (parent, name) = self.open_parent(OP, path)?;
        let dir = self.parent_fd(&parent);
        let mode: libc::c_uint = 0o600;
        // SAFETY: `dir` is an open descriptor and `name` is NUL terminated;
        // `openat` keeps no pointer, and with `O_CREAT` it reads the one
        // mode argument passed after the flags.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), FILE_FLAGS, mode) };
        if fd < 0 {
            let err = io::Error::last_os_error();
            return Err(PlatformError::from_io(OP, &self.full_path(path), &err));
        }
        // SAFETY: `openat` returned a new descriptor that nothing else owns.
        let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        Ok(Box::new(MacosTreeFile {
            file,
            written: 0,
            path: self.full_path(path),
        }))
    }

    fn create_symlink(&mut self, path: &EntryPath, target: &str) -> Result<(), PlatformError> {
        const OP: &str = "create_symlink";
        let invalid = |message: &str| PlatformError::Io {
            op: OP,
            path: self.full_path(path),
            kind: io::ErrorKind::InvalidInput,
            message: message.to_owned(),
        };
        if target.is_empty() {
            return Err(invalid("the symlink target is empty"));
        }
        let c_target =
            CString::new(target).map_err(|_| invalid("the symlink target holds a NUL byte"))?;
        let (parent, name) = self.open_parent(OP, path)?;
        let dir = self.parent_fd(&parent);
        // SAFETY: `dir` is an open descriptor and both strings are NUL
        // terminated; `symlinkat` keeps no pointer.
        let rc = unsafe { libc::symlinkat(c_target.as_ptr(), dir.as_raw_fd(), name.as_ptr()) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            return Err(PlatformError::from_io(OP, &self.full_path(path), &err));
        }
        Ok(())
    }

    fn set_mode(&mut self, path: &EntryPath, mode: FileMode) -> Result<(), PlatformError> {
        const OP: &str = "set_mode";
        let bits = libc::mode_t::try_from(mode.bits())
            .expect("a FileMode is at most 0o777, which fits mode_t");
        let (parent, name) = self.open_parent(OP, path)?;
        let dir = self.parent_fd(&parent);
        // SAFETY: `dir` is an open descriptor and `name` is NUL terminated;
        // `fchmodat` keeps no pointer.
        let rc = unsafe {
            libc::fchmodat(
                dir.as_raw_fd(),
                name.as_ptr(),
                bits,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            let err = io::Error::last_os_error();
            return Err(PlatformError::from_io(OP, &self.full_path(path), &err));
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), PlatformError> {
        const OP: &str = "sync";
        for dir in &self.dirs {
            let opened = self
                .open_dir(OP, dir.components())?
                .expect("a created directory is below the root");
            fsync(opened.as_fd())
                .map_err(|err| PlatformError::from_io(OP, &self.full_path(dir), &err))?;
        }
        fsync(self.root.as_fd())
            .and_then(|()| full_fsync(self.root.as_fd()))
            .map_err(|err| PlatformError::from_io(OP, &self.root_path, &err))
    }
}

/// One file a [`MacosTreeWriter`] created.
struct MacosTreeFile {
    file: File,
    written: u64,
    path: PathBuf,
}

impl Write for MacosTreeFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.file.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl TreeFile for MacosTreeFile {
    /// Calls `fsync`; [`TreeWriter::sync`] issues the one `F_FULLFSYNC`
    /// for the whole tree.
    fn finish(self: Box<Self>) -> Result<u64, PlatformError> {
        fsync(self.file.as_fd())
            .map_err(|err| PlatformError::from_io("finish", &self.path, &err))?;
        Ok(self.written)
    }
}

/// Maps the error of opening `name` in `dir` on the way to an entry.
fn walk_error(
    op: &'static str,
    dir: BorrowedFd<'_>,
    name: &CStr,
    path: &Path,
    err: &io::Error,
) -> PlatformError {
    let path_buf = path.to_path_buf();
    match err.raw_os_error() {
        // O_NOFOLLOW on a symlink fails with ELOOP.
        Some(libc::ELOOP) => PlatformError::SymlinkInPath { path: path_buf },
        Some(libc::ENOTDIR) => match stat_at(dir, name) {
            Ok(stat) if is_symlink(&stat) => PlatformError::SymlinkInPath { path: path_buf },
            Ok(stat) if !is_dir(&stat) => PlatformError::NotADirectory { path: path_buf },
            // Swapped again since the open: report the open's own error.
            _ => PlatformError::from_io(op, path, err),
        },
        _ => PlatformError::from_io(op, path, err),
    }
}
