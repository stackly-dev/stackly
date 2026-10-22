//! [`Filesystem::tree_usage`](stackly_platform::Filesystem::tree_usage)
//! through file descriptors (spec 0004, *Port platform*, and spec 0021,
//! *Ukuran data*): every step opens or stats a single name relative to a
//! folder the walk already holds open, so a folder swapped for a symlink
//! is never followed, and each level of the walk holds one descriptor.

use std::collections::HashSet;
use std::ffi::{CStr, CString, OsStr};
use std::fs::{self, OpenOptions};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::Instant;

use stackly_platform::{
    PlatformError, TREE_USAGE_MAX_DEPTH, TreeUsage, UsageLimits, tree_usage_root,
};

use super::fd::{fstat, is_dir, open_dir_at, stat_at};

/// Bytes in one `st_blocks` unit.
const BLOCK_BYTES: u64 = 512;

/// One open folder of the walk: its stream and its path, for errors.
struct Level {
    stream: DirStream,
    path: PathBuf,
}

/// The walk's totals and limits.
struct Walk {
    device: libc::dev_t,
    bytes: u64,
    entries: u64,
    max_entries: u64,
    deadline: Option<Instant>,
    /// False once a limit was hit or a folder was too deep to open.
    complete: bool,
    /// True once the entry limit or the deadline ended the walk.
    stopped: bool,
    /// Non folder entries with more than one link, by device and inode.
    linked: HashSet<(libc::dev_t, libc::ino_t)>,
}

/// Adds up the disk space of the tree at `root`; see the trait method.
pub(super) fn tree_usage(root: &Path, limits: UsageLimits) -> Result<TreeUsage, PlatformError> {
    tree_usage_root(root)?;
    // The deadline starts here, after the argument checks.
    let deadline = Instant::now().checked_add(limits.deadline);
    let top = open_root(root)?;
    let stat = fstat(top.as_fd()).map_err(|err| io_error(root, &err))?;
    let mut walk = Walk {
        device: stat.st_dev,
        bytes: blocks(&stat),
        entries: 0,
        max_entries: limits.max_entries,
        deadline,
        complete: true,
        stopped: false,
        linked: HashSet::new(),
    };
    let stream = DirStream::new(top).map_err(|err| io_error(root, &err))?;
    let mut stack = vec![Level {
        stream,
        path: root.to_path_buf(),
    }];
    loop {
        let depth = stack.len();
        let Some(level) = stack.last_mut() else {
            break;
        };
        let next = level
            .stream
            .next_name()
            .map_err(|err| io_error(&level.path, &err))?;
        let Some(name) = next else {
            stack.pop();
            continue;
        };
        if let Some(opened) = visit(&mut walk, level, &name, depth)? {
            stack.push(opened);
        }
        if walk.stopped {
            break;
        }
    }
    Ok(TreeUsage::new(walk.bytes, walk.complete))
}

/// Counts one name read from `level`. Returns the folder to descend into,
/// when there is one. `depth` is the number of open levels.
fn visit(
    walk: &mut Walk,
    level: &Level,
    name: &CStr,
    depth: usize,
) -> Result<Option<Level>, PlatformError> {
    walk.entries += 1;
    if walk.entries > walk.max_entries || walk.deadline.is_some_and(|at| Instant::now() >= at) {
        walk.complete = false;
        walk.stopped = true;
        return Ok(None);
    }
    let path = level.path.join(OsStr::from_bytes(name.to_bytes()));
    let parent = level.stream.dir_fd();
    let stat = match retry(|| stat_at(parent, name)) {
        Ok(stat) => stat,
        Err(err) if err.raw_os_error() == Some(libc::ENOENT) => return Ok(None),
        Err(err) => return Err(io_error(&path, &err)),
    };
    if stat.st_dev != walk.device {
        return Ok(None);
    }
    if !is_dir(&stat) {
        let count = stat.st_nlink <= 1 || walk.linked.insert((stat.st_dev, stat.st_ino));
        if count {
            walk.bytes = walk.bytes.saturating_add(blocks(&stat));
        }
        return Ok(None);
    }
    walk.bytes = walk.bytes.saturating_add(blocks(&stat));
    if depth >= TREE_USAGE_MAX_DEPTH {
        walk.complete = false;
        return Ok(None);
    }
    match retry(|| open_dir_at(parent, name)) {
        Ok(dir) => {
            // Another device after the open: the entry is counted by its
            // stat and not entered.
            let opened = fstat(dir.as_fd()).map_err(|err| io_error(&path, &err))?;
            if opened.st_dev != walk.device {
                return Ok(None);
            }
            let stream = DirStream::new(dir).map_err(|err| io_error(&path, &err))?;
            Ok(Some(Level { stream, path }))
        }
        // Gone since the stat: it left the tree, so its block does not count.
        Err(err) if err.raw_os_error() == Some(libc::ENOENT) => {
            walk.bytes = walk.bytes.saturating_sub(blocks(&stat));
            Ok(None)
        }
        // Swapped for a symlink or a file since the stat: not entered.
        Err(err) if matches!(err.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => Ok(None),
        Err(err) => Err(io_error(&path, &err)),
    }
}

/// The bytes one stat result occupies on disk.
fn blocks(stat: &libc::stat) -> u64 {
    u64::try_from(stat.st_blocks)
        .unwrap_or(0)
        .saturating_mul(BLOCK_BYTES)
}

/// Runs `call` again while it fails with `EINTR`.
fn retry<T>(mut call: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    loop {
        match call() {
            Err(err) if err.raw_os_error() == Some(libc::EINTR) => {}
            other => return other,
        }
    }
}

/// Opens the root, refusing a symlink.
fn open_root(root: &Path) -> Result<OwnedFd, PlatformError> {
    match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(root)
    {
        Ok(file) => Ok(OwnedFd::from(file)),
        Err(err) if matches!(err.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {
            let path = root.to_path_buf();
            Err(match fs::symlink_metadata(root) {
                Ok(info) if info.file_type().is_symlink() => PlatformError::SymlinkInPath { path },
                Ok(_) => PlatformError::NotADirectory { path },
                Err(err) => io_error(root, &err),
            })
        }
        Err(err) => Err(io_error(root, &err)),
    }
}

fn io_error(path: &Path, err: &io::Error) -> PlatformError {
    PlatformError::from_io("tree_usage", path, err)
}

/// A `DIR` stream from `fdopendir` over the folder's own descriptor,
/// closed on drop. The same descriptor serves `openat` and `fstatat`
/// through [`dir_fd`](Self::dir_fd), so a level costs one descriptor.
struct DirStream(NonNull<libc::DIR>);

impl DirStream {
    fn new(dir: OwnedFd) -> io::Result<Self> {
        let raw = dir.into_raw_fd();
        // SAFETY: `raw` is an open directory descriptor this function owns;
        // on success `fdopendir` takes it over and `closedir` releases it.
        let stream = unsafe { libc::fdopendir(raw) };
        match NonNull::new(stream) {
            Some(stream) => Ok(Self(stream)),
            None => {
                let err = io::Error::last_os_error();
                // SAFETY: `fdopendir` failed, so `raw` is still open and owned
                // here alone; wrapping it closes it exactly once.
                drop(unsafe { OwnedFd::from_raw_fd(raw) });
                Err(err)
            }
        }
    }

    /// The descriptor of the folder this stream reads.
    fn dir_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: `self.0` is a live stream, so `dirfd` returns the open
        // descriptor it owns; the borrow ends before the stream is dropped
        // because it is tied to `&self`.
        let fd = unsafe { libc::dirfd(self.0.as_ptr()) };
        // SAFETY: `fd` stays open for as long as the stream, which `&self`
        // keeps alive.
        unsafe { BorrowedFd::borrow_raw(fd) }
    }

    /// Reads the next name, skipping `.` and `..`, or `None` at the end.
    /// Only one name is held at a time.
    fn next_name(&mut self) -> io::Result<Option<CString>> {
        loop {
            // SAFETY: `__error` returns the address of this thread's errno,
            // which is always valid to write. `readdir` reports its end and
            // its errors both as NULL, so errno is cleared to tell them apart.
            unsafe { *libc::__error() = 0 };
            // SAFETY: `self.0` is a live stream from `fdopendir`, used only
            // through `&mut self` on this thread.
            let entry = unsafe { libc::readdir(self.0.as_ptr()) };
            if entry.is_null() {
                let err = io::Error::last_os_error();
                return match err.raw_os_error() {
                    Some(0) => Ok(None),
                    Some(libc::EINTR) => continue,
                    _ => Err(err),
                };
            }
            // SAFETY: `readdir` returned a valid entry whose `d_name` is NUL
            // terminated and stays valid until the next `readdir` or
            // `closedir` on this stream; the name is copied before either.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name != c"." && name != c".." {
                return Ok(Some(name.to_owned()));
            }
        }
    }
}

impl Drop for DirStream {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `fdopendir` and is closed exactly once,
        // here.
        unsafe { libc::closedir(self.0.as_ptr()) };
    }
}
