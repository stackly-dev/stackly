//! The one `posix_spawn` of the process adapter (spec 0010, *Adapter
//! macOS*, row Spawn), `pub(crate)` so the codesign helper of feature 18
//! spawns with the same flags (spec 0004, K9).
//!
//! Every child leads its own session (`SETSID`), starts with every signal
//! at its default and none blocked (`SETSIGDEF`, `SETSIGMASK`), and keeps
//! only the descriptors mapped to fds 0 to 3 (`CLOEXEC_DEFAULT`). A main
//! process also starts suspended (`START_SUSPENDED`). No shell is involved:
//! `argv[0]` is the program path and each argument is one element.

use std::ffi::CString;
use std::io;
use std::iter;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use libc::{c_char, c_int, c_short};
use stackly_domain::Pid;
use stackly_platform::{ProcessError, RequestPart};

use super::sys;

/// The lowest number a source descriptor is duplicated to before the
/// spawn, so a source that sits at fd 0 to 3 of a daemon with one of those
/// closed never collides with a target (spec 0010, *Hasil spike*).
const SOURCE_FD_FLOOR: RawFd = 10;

/// What one spawn runs, with the descriptors the child gets.
#[derive(Debug)]
pub(crate) struct Launch<'a> {
    /// The executable, which also becomes `argv[0]`.
    pub(crate) program: &'a Path,
    /// The arguments after the program.
    pub(crate) args: &'a [String],
    /// The whole environment; nothing comes from the daemon.
    pub(crate) env: &'a [(String, String)],
    /// The working directory.
    pub(crate) cwd: &'a Path,
    /// The child's fd 0, or `None` for `/dev/null`.
    pub(crate) stdin: Option<BorrowedFd<'a>>,
    /// The child's fd 1.
    pub(crate) stdout: BorrowedFd<'a>,
    /// The child's fd 2.
    pub(crate) stderr: BorrowedFd<'a>,
    /// The child's fd 3, the instance lock, if it inherits one.
    pub(crate) lock: Option<BorrowedFd<'a>>,
    /// True to start the child suspended, before it runs any code.
    pub(crate) suspended: bool,
}

/// Spawns one child and returns its PID.
///
/// # Errors
///
/// Returns [`ProcessError::InvalidArgument`] for a NUL byte in a text,
/// [`ProcessError::SpawnFailed`] when `posix_spawn` refuses (a missing
/// program or working directory, a file that is not executable), and
/// [`ProcessError::Io`] when a descriptor or spawn attribute cannot be
/// set up.
pub(crate) fn launch(launch: &Launch<'_>) -> Result<Pid, ProcessError> {
    let program = c_text(launch.program.as_os_str().as_bytes(), RequestPart::Program)?;
    let args = launch
        .args
        .iter()
        .enumerate()
        .map(|(index, arg)| c_text(arg.as_bytes(), RequestPart::Arg(index)))
        .collect::<Result<Vec<_>, _>>()?;
    let env = launch
        .env
        .iter()
        .enumerate()
        .map(|(index, (name, value))| {
            c_text(
                format!("{name}={value}").as_bytes(),
                RequestPart::EnvValue(index),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let cwd = c_text(launch.cwd.as_os_str().as_bytes(), RequestPart::Cwd)?;
    let argv = pointers(iter::once(&program).chain(&args));
    let envp = pointers(&env);

    let stdin = launch.stdin.map(dup_high).transpose()?;
    let stdout = dup_high(launch.stdout)?;
    let stderr = dup_high(launch.stderr)?;
    let lock = launch.lock.map(dup_high).transpose()?;

    let mut actions = FileActions::new()?;
    match &stdin {
        None => actions.open_dev_null(0)?,
        Some(fd) => actions.dup2(fd, 0)?,
    }
    actions.dup2(&stdout, 1)?;
    actions.dup2(&stderr, 2)?;
    if let Some(lock) = &lock {
        actions.dup2(lock, 3)?;
    }
    actions.chdir(&cwd)?;
    let attrs = SpawnAttrs::new(launch.suspended)?;

    let mut pid: libc::pid_t = 0;
    // SAFETY: every pointer is valid for the call: `program` and the
    // strings behind `argv` and `envp` live until the end of this
    // function, both arrays end in a null pointer, and `actions` and
    // `attrs` were initialized and are destroyed only when they drop.
    let result = unsafe {
        libc::posix_spawn(
            &raw mut pid,
            program.as_ptr(),
            &raw const actions.0,
            &raw const attrs.0,
            argv.as_ptr(),
            envp.as_ptr(),
        )
    };
    if result != 0 {
        return Err(ProcessError::SpawnFailed {
            kind: io::Error::from_raw_os_error(result).kind(),
            errno: result,
        });
    }
    let pid = u32::try_from(pid)
        .ok()
        .and_then(|pid| Pid::new(pid).ok())
        .expect("posix_spawn that succeeds hands out a positive pid_t");
    Ok(pid)
}

/// Creates a pipe, both ends close on exec, and returns the read end and
/// the write end.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when the pipe cannot be created.
pub(crate) fn pipe() -> Result<(OwnedFd, OwnedFd), ProcessError> {
    let mut fds: [c_int; 2] = [-1; 2];
    // SAFETY: pipe writes two descriptors into the array of two it gets.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(last_error("pipe"));
    }
    // SAFETY: pipe succeeded, so both numbers are new descriptors that
    // nothing else owns.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&read, &write] {
        // SAFETY: fcntl only changes the flags of a descriptor this
        // function owns.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(last_error("fcntl F_SETFD"));
        }
    }
    Ok((read, write))
}

/// Returns the last OS error as [`ProcessError::Io`] for `op`.
pub(super) fn last_error(op: &'static str) -> ProcessError {
    ProcessError::io(op, None, &io::Error::last_os_error())
}

fn c_text(bytes: &[u8], position: RequestPart) -> Result<CString, ProcessError> {
    CString::new(bytes).map_err(|_| ProcessError::InvalidArgument { position })
}

/// The null terminated pointer array `posix_spawn` reads for argv or envp.
fn pointers<'a>(strings: impl IntoIterator<Item = &'a CString>) -> Vec<*mut c_char> {
    strings
        .into_iter()
        .map(|text| text.as_ptr().cast_mut())
        .chain(iter::once(ptr::null_mut()))
        .collect()
}

/// Duplicates `fd` to the lowest free number at or above
/// [`SOURCE_FD_FLOOR`], close on exec.
fn dup_high(fd: BorrowedFd<'_>) -> Result<OwnedFd, ProcessError> {
    // SAFETY: F_DUPFD_CLOEXEC only reads the borrowed descriptor and
    // returns a new one.
    let duplicate = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, SOURCE_FD_FLOOR) };
    if duplicate == -1 {
        return Err(last_error("fcntl F_DUPFD_CLOEXEC"));
    }
    // SAFETY: fcntl returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

/// Turns the result of a `posix_spawn*` setup call, an error number
/// rather than `-1` and `errno`, into a [`ProcessError::Io`].
fn spawn_setup(result: c_int, op: &'static str) -> Result<(), ProcessError> {
    if result == 0 {
        Ok(())
    } else {
        Err(ProcessError::io(
            op,
            None,
            &io::Error::from_raw_os_error(result),
        ))
    }
}

/// The file actions of one spawn, destroyed when dropped.
struct FileActions(libc::posix_spawn_file_actions_t);

impl FileActions {
    fn new() -> Result<Self, ProcessError> {
        let mut raw: libc::posix_spawn_file_actions_t = ptr::null_mut();
        // SAFETY: init fills in the object behind the pointer it gets.
        let result = unsafe { libc::posix_spawn_file_actions_init(&raw mut raw) };
        spawn_setup(result, "posix_spawn_file_actions_init")?;
        Ok(Self(raw))
    }

    fn open_dev_null(&mut self, target: c_int) -> Result<(), ProcessError> {
        // SAFETY: the actions are initialized, and the path is a static C
        // string, which addopen copies.
        let result = unsafe {
            libc::posix_spawn_file_actions_addopen(
                &raw mut self.0,
                target,
                c"/dev/null".as_ptr(),
                libc::O_RDONLY,
                0,
            )
        };
        spawn_setup(result, "posix_spawn_file_actions_addopen")
    }

    fn dup2(&mut self, source: &OwnedFd, target: c_int) -> Result<(), ProcessError> {
        // SAFETY: the actions are initialized, and `source` stays open
        // until the spawn ran.
        let result = unsafe {
            libc::posix_spawn_file_actions_adddup2(&raw mut self.0, source.as_raw_fd(), target)
        };
        spawn_setup(result, "posix_spawn_file_actions_adddup2")
    }

    fn chdir(&mut self, dir: &CString) -> Result<(), ProcessError> {
        // SAFETY: the actions are initialized, and addchdir_np copies the
        // path it gets.
        let result =
            unsafe { libc::posix_spawn_file_actions_addchdir_np(&raw mut self.0, dir.as_ptr()) };
        spawn_setup(result, "posix_spawn_file_actions_addchdir_np")
    }
}

impl Drop for FileActions {
    fn drop(&mut self) {
        // SAFETY: the actions were initialized in `new` and are destroyed
        // once, here.
        unsafe { libc::posix_spawn_file_actions_destroy(&raw mut self.0) };
    }
}

/// The attributes of one spawn, destroyed when dropped.
struct SpawnAttrs(libc::posix_spawnattr_t);

impl SpawnAttrs {
    fn new(suspended: bool) -> Result<Self, ProcessError> {
        let mut raw: libc::posix_spawnattr_t = ptr::null_mut();
        // SAFETY: init fills in the object behind the pointer it gets.
        let result = unsafe { libc::posix_spawnattr_init(&raw mut raw) };
        spawn_setup(result, "posix_spawnattr_init")?;
        let mut attrs = Self(raw);
        let mut every: libc::sigset_t = 0;
        let mut none: libc::sigset_t = 0;
        // SAFETY: both functions only write the sigset_t they get.
        unsafe {
            libc::sigfillset(&raw mut every);
            libc::sigemptyset(&raw mut none);
        }
        // SAFETY: the attributes are initialized, and the sets are read
        // during the calls only.
        let results = unsafe {
            [
                (
                    libc::posix_spawnattr_setflags(&raw mut attrs.0, spawn_flags(suspended)),
                    "posix_spawnattr_setflags",
                ),
                (
                    libc::posix_spawnattr_setsigdefault(&raw mut attrs.0, &raw const every),
                    "posix_spawnattr_setsigdefault",
                ),
                (
                    libc::posix_spawnattr_setsigmask(&raw mut attrs.0, &raw const none),
                    "posix_spawnattr_setsigmask",
                ),
            ]
        };
        for (result, op) in results {
            spawn_setup(result, op)?;
        }
        Ok(attrs)
    }
}

impl Drop for SpawnAttrs {
    fn drop(&mut self) {
        // SAFETY: the attributes were initialized in `new` and are
        // destroyed once, here.
        unsafe { libc::posix_spawnattr_destroy(&raw mut self.0) };
    }
}

/// The spawn flags of spec 0010 AC-1, with `START_SUSPENDED` for a main
/// process.
const fn spawn_flags(suspended: bool) -> c_short {
    let common = sys::POSIX_SPAWN_SETSID
        | libc::POSIX_SPAWN_SETSIGDEF
        | libc::POSIX_SPAWN_SETSIGMASK
        | libc::POSIX_SPAWN_CLOEXEC_DEFAULT;
    let flags = if suspended {
        common | libc::POSIX_SPAWN_START_SUSPENDED
    } else {
        common
    };
    // Every flag sits below bit 15, so the value fits the c_short that
    // posix_spawnattr_setflags takes.
    flags as c_short
}

const _: () = assert!(spawn_flags(true) as c_int == 0x448c);
