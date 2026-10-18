use std::ffi::{CStr, CString, OsStr};
use std::fs::{self, OpenOptions};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use stackly_platform::{PlatformError, REMOVE_TREE_MAX_DEPTH, RemoveReport, remove_tree_names};

/// Flags for opening a directory: never follow a symlink in the last
/// component, never block on a FIFO swapped in for it, close on exec.
const DIR_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;

/// How often one entry is examined again after it changed type between
/// its stat and its unlink or open, before the removal gives up.
const ENTRY_ATTEMPTS: usize = 8;

/// One entry after [`examine`] acted on it.
enum Entry {
    /// It was gone before it could be removed.
    Gone,
    /// It was not a directory and is now unlinked.
    Removed,
    /// It is a directory on the root's device, opened for the walk.
    Dir(OwnedFd),
}

/// One open directory of the walk and the entries still to visit in it.
struct Level {
    dir: OwnedFd,
    name: CString,
    path: PathBuf,
    pending: Vec<CString>,
}

/// [`Filesystem::remove_tree`](stackly_platform::Filesystem::remove_tree)
/// through file descriptors: every step opens, reads, or unlinks a single
/// name relative to a directory it already holds open, so no path is
/// resolved again after it was checked and a folder swapped for a symlink
/// is never followed.
pub(super) fn remove_tree(root: &Path, relative: &Path) -> Result<RemoveReport, PlatformError> {
    let names = remove_tree_names(root, relative)?;
    let mut parent = open_root(root)?;
    let device = fstat(parent.as_fd())
        .map_err(|err| io_error(root, &err))?
        .st_dev;
    let (last, on_the_way) = names
        .split_last()
        .expect("remove_tree_names returns at least one name");
    let mut path = root.to_path_buf();
    for name in on_the_way {
        path.push(name);
        let name = c_name(name, &path)?;
        match open_dir_on_the_way(parent.as_fd(), &name, &path, device)? {
            Some(dir) => parent = dir,
            None => return Ok(RemoveReport::missing()),
        }
    }
    path.push(last);
    let name = c_name(last, &path)?;
    let mut report = RemoveReport::default();
    match examine(parent.as_fd(), &name, &path, device)? {
        Entry::Gone => return Ok(RemoveReport::missing()),
        Entry::Removed => report.files = 1,
        Entry::Dir(dir) => remove_dir_tree(parent.as_fd(), name, dir, path, device, &mut report)?,
    }
    Ok(report)
}

/// Opens the root of a removal, refusing a symlink.
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

/// Opens a directory on the way to the target, or returns `None` when it
/// does not exist.
fn open_dir_on_the_way(
    parent: BorrowedFd<'_>,
    name: &CStr,
    path: &Path,
    device: libc::dev_t,
) -> Result<Option<OwnedFd>, PlatformError> {
    match open_dir_at(parent, name) {
        Ok(dir) => {
            expect_device(dir.as_fd(), path, device)?;
            Ok(Some(dir))
        }
        Err(err) if err.raw_os_error() == Some(libc::ENOENT) => Ok(None),
        Err(err) if matches!(err.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {
            let path = path.to_path_buf();
            match stat_at(parent, name) {
                Ok(stat) if is_symlink(&stat) => Err(PlatformError::SymlinkInPath { path }),
                Ok(_) => Err(PlatformError::NotADirectory { path }),
                Err(err) if err.raw_os_error() == Some(libc::ENOENT) => Ok(None),
                Err(err) => Err(io_error(&path, &err)),
            }
        }
        Err(err) => Err(io_error(path, &err)),
    }
}

/// Removes one entry of `dir` that is not a directory, or opens it when it
/// is one. An entry that changes type in between is examined again.
fn examine(
    dir: BorrowedFd<'_>,
    name: &CStr,
    path: &Path,
    device: libc::dev_t,
) -> Result<Entry, PlatformError> {
    for _ in 0..ENTRY_ATTEMPTS {
        let stat = match stat_at(dir, name) {
            Ok(stat) => stat,
            Err(err) if err.raw_os_error() == Some(libc::ENOENT) => return Ok(Entry::Gone),
            Err(err) => return Err(io_error(path, &err)),
        };
        if stat.st_dev != device {
            return Err(PlatformError::CrossesDevice {
                path: path.to_path_buf(),
            });
        }
        if is_dir(&stat) {
            match open_dir_at(dir, name) {
                Ok(opened) => {
                    expect_device(opened.as_fd(), path, device)?;
                    return Ok(Entry::Dir(opened));
                }
                Err(err) if err.raw_os_error() == Some(libc::ENOENT) => return Ok(Entry::Gone),
                // Swapped for a symlink or a file since the stat.
                Err(err) if matches!(err.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {}
                Err(err) => return Err(io_error(path, &err)),
            }
        } else {
            match unlink_at(dir, name, 0) {
                Ok(()) => return Ok(Entry::Removed),
                Err(err) if err.raw_os_error() == Some(libc::ENOENT) => return Ok(Entry::Gone),
                // macOS refuses to unlink a directory with EPERM: swapped
                // for a directory since the stat.
                Err(err)
                    if matches!(err.raw_os_error(), Some(libc::EPERM | libc::EISDIR))
                        && stat_at(dir, name).is_ok_and(|stat| is_dir(&stat)) => {}
                Err(err) => return Err(io_error(path, &err)),
            }
        }
    }
    Err(PlatformError::Io {
        op: "remove_tree",
        path: path.to_path_buf(),
        kind: io::ErrorKind::Other,
        message: "the entry kept changing type while it was being removed".to_owned(),
    })
}

/// Removes the directory `top`, already open as `dir`, and everything in
/// it, depth first with at most [`REMOVE_TREE_MAX_DEPTH`] directories open.
fn remove_dir_tree(
    parent: BorrowedFd<'_>,
    name: CString,
    dir: OwnedFd,
    top: PathBuf,
    device: libc::dev_t,
    report: &mut RemoveReport,
) -> Result<(), PlatformError> {
    let pending = read_names(dir.as_fd(), &top)?;
    let mut stack = vec![Level {
        dir,
        name,
        path: top,
        pending,
    }];
    while let Some(level) = stack.last_mut() {
        if let Some(entry) = level.pending.pop() {
            let path = level.path.join(OsStr::from_bytes(entry.to_bytes()));
            match examine(level.dir.as_fd(), &entry, &path, device)? {
                Entry::Gone => {}
                Entry::Removed => report.files += 1,
                Entry::Dir(dir) => {
                    if stack.len() >= REMOVE_TREE_MAX_DEPTH {
                        return Err(PlatformError::TooDeep {
                            path,
                            max: REMOVE_TREE_MAX_DEPTH,
                        });
                    }
                    let pending = read_names(dir.as_fd(), &path)?;
                    stack.push(Level {
                        dir,
                        name: entry,
                        path,
                        pending,
                    });
                }
            }
        } else {
            let done = stack
                .pop()
                .expect("the loop only runs while the stack has a level");
            drop(done.dir);
            let holder = stack.last().map_or(parent, |level| level.dir.as_fd());
            match unlink_at(holder, &done.name, libc::AT_REMOVEDIR) {
                Ok(()) => report.dirs += 1,
                Err(err) if err.raw_os_error() == Some(libc::ENOENT) => {}
                Err(err) => return Err(io_error(&done.path, &err)),
            }
        }
    }
    Ok(())
}

/// Lists the names in an open directory, without `.` and `..`, through a
/// second descriptor so the directory's own descriptor keeps its offset.
fn read_names(dir: BorrowedFd<'_>, path: &Path) -> Result<Vec<CString>, PlatformError> {
    let listing = open_dir_at(dir, c".").map_err(|err| io_error(path, &err))?;
    let mut stream = DirStream::new(listing).map_err(|err| io_error(path, &err))?;
    stream.names().map_err(|err| io_error(path, &err))
}

/// A `DIR` stream from `fdopendir`, closed on drop.
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

    fn names(&mut self) -> io::Result<Vec<CString>> {
        let mut names = Vec::new();
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
                    Some(0) => Ok(names),
                    _ => Err(err),
                };
            }
            // SAFETY: `readdir` returned a valid entry whose `d_name` is NUL
            // terminated and stays valid until the next `readdir` or
            // `closedir` on this stream; the name is copied before either.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name != c"." && name != c".." {
                names.push(name.to_owned());
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

/// Opens the directory `name` in `dir` without following a symlink.
fn open_dir_at(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<OwnedFd> {
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
fn stat_at(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<libc::stat> {
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
fn fstat(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
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

/// Unlinks the entry `name` in `dir`; `flags` is 0 or `AT_REMOVEDIR`.
fn unlink_at(dir: BorrowedFd<'_>, name: &CStr, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: `dir` is an open descriptor and `name` is NUL terminated;
    // `unlinkat` keeps no pointer.
    let rc = unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Fails when the open directory lies on another device than the root.
fn expect_device(
    dir: BorrowedFd<'_>,
    path: &Path,
    device: libc::dev_t,
) -> Result<(), PlatformError> {
    let stat = fstat(dir).map_err(|err| io_error(path, &err))?;
    if stat.st_dev == device {
        Ok(())
    } else {
        Err(PlatformError::CrossesDevice {
            path: path.to_path_buf(),
        })
    }
}

fn is_dir(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFDIR
}

fn is_symlink(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFLNK
}

/// Turns one path component into the C string the `*at` calls take.
fn c_name(name: &OsStr, path: &Path) -> Result<CString, PlatformError> {
    CString::new(name.as_bytes()).map_err(|_| PlatformError::Io {
        op: "remove_tree",
        path: path.to_path_buf(),
        kind: io::ErrorKind::InvalidInput,
        message: "a path component holds a NUL byte".to_owned(),
    })
}

fn io_error(path: &Path, err: &io::Error) -> PlatformError {
    PlatformError::from_io("remove_tree", path, err)
}
