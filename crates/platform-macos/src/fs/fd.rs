//! Small descriptor helpers shared by the rename, volume, and tree writer
//! modules.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use stackly_platform::PlatformError;

/// Flags for opening a directory: never follow a symlink in the last
/// component, never block on a FIFO swapped in for it, close on exec.
pub(super) const DIR_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;

/// Turns a whole path into the C string the path calls take.
pub(super) fn c_path(op: &'static str, path: &Path) -> Result<CString, PlatformError> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| nul_error(op, path))
}

/// Turns one name into the C string the `*at` calls take.
pub(super) fn c_name(op: &'static str, name: &str, path: &Path) -> Result<CString, PlatformError> {
    CString::new(name).map_err(|_| nul_error(op, path))
}

fn nul_error(op: &'static str, path: &Path) -> PlatformError {
    PlatformError::Io {
        op,
        path: path.to_path_buf(),
        kind: io::ErrorKind::InvalidInput,
        message: "the path holds a NUL byte".to_owned(),
    }
}

/// Opens the directory `name` in `dir` without following a symlink.
pub(super) fn open_dir_at(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<OwnedFd> {
    // SAFETY: `dir` is an open descriptor and `name` is NUL terminated;
    // `openat` keeps no pointer, and without `O_CREAT` it reads no mode.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), DIR_FLAGS) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `openat` returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Reads the entry `name` in `dir` without following a symlink.
pub(super) fn stat_at(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `dir` is an open descriptor, `name` is NUL terminated, and
    // `stat` points to writable memory the size of `libc::stat`; neither
    // pointer is kept after the call.
    let rc = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fstatat` returned 0, so it filled `stat`.
    Ok(unsafe { stat.assume_init() })
}

/// Reads the open file `fd`.
pub(super) fn fstat(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `fd` is an open descriptor and `stat` points to writable
    // memory the size of `libc::stat`, not kept after the call.
    let rc = unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fstat` returned 0, so it filled `stat`.
    Ok(unsafe { stat.assume_init() })
}

/// Flushes the open file `fd` to the disk's cache with `fsync`.
pub(super) fn fsync(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: `fd` is an open descriptor; `fsync` touches no memory.
    let rc = unsafe { libc::fsync(fd.as_raw_fd()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Flushes the disk's cache to stable storage with `F_FULLFSYNC`.
pub(super) fn full_fsync(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: `fd` is an open descriptor; `F_FULLFSYNC` takes no argument
    // and touches no memory.
    let rc = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_FULLFSYNC) };
    if rc == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Syncs the directory at `dir` with `F_FULLFSYNC` (std's `sync_all`).
pub(super) fn sync_dir(op: &'static str, dir: &Path) -> Result<(), PlatformError> {
    File::open(dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|err| PlatformError::from_io(op, dir, &err))
}

pub(super) fn is_dir(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFDIR
}

pub(super) fn is_symlink(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFLNK
}
