//! The process supervision port (spec 0010, *Port `ProcessSupervision`*):
//! [`ProcessSupervision`], the requests and handles that pass through it,
//! and the instance lock and log spool files it opens.
//!
//! The types make the safety rules hard to break: a main process starts
//! suspended as a [`SuspendedChild`], which only [`resume`] or [`discard`]
//! consume, so a kill never reaches a process that ran; only a
//! [`HelperHandle`] can have its group terminated; every other signal goes
//! through a [`SignalTarget`] whose identity the adapter checks again right
//! before `kill(2)`; and [`ProviderSignal`] has no `Kill`.
//!
//! [`resume`]: ProcessSupervision::resume
//! [`discard`]: ProcessSupervision::discard

use std::fmt;
use std::fs::File;
use std::io;
use std::num::NonZeroU64;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_domain::{BootId, Os, Pid, ProcessIdentity, ProcessStart};

use crate::capability::PlatformCapability;
use crate::fs::FileIdentity;
use crate::identity::UserId;
use crate::secret::SecretValue;

pub use stackly_domain::LockState;

/// The most bytes of each output stream an adapter keeps from a helper
/// process: the tail, for failure details and debug logs.
pub const HELPER_CAPTURE_MAX: usize = 65_536;

/// One output stream of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OutputStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

impl OutputStream {
    /// Both streams, stdout first.
    pub const ALL: [Self; 2] = [Self::Stdout, Self::Stderr];

    /// Returns the stable code, `stdout` or `stderr`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }

    /// Returns the file name of the stream's spool in a run folder,
    /// `stdout.spool` or `stderr.spool`.
    #[must_use]
    pub const fn spool_file_name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout.spool",
            Self::Stderr => "stderr.spool",
        }
    }
}

impl fmt::Display for OutputStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Why a child is spawned. It decides which method takes the request and
/// whether the child may inherit the instance lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildRole {
    /// The instance's long running server, spawned suspended.
    Main,
    /// A `Run` step of the init plan.
    InitStep,
    /// An `exec` readiness or health probe.
    Probe,
    /// The provider's stop command.
    StopCommand,
    /// The provider's reload command.
    ReloadCommand,
}

impl ChildRole {
    /// Returns true for the roles that may inherit the instance lock at fd
    /// 3: the main process and init steps. Probes and stop or reload
    /// commands never hold it.
    #[must_use]
    pub const fn takes_lock(self) -> bool {
        matches!(self, Self::Main | Self::InitStep)
    }
}

/// What a child reads on stdin.
#[derive(Debug)]
pub enum StdinSource {
    /// `/dev/null`.
    Null,
    /// A secret, written once to a pipe that is then closed (spec 0011).
    Bytes(SecretValue),
}

/// Where a child's stdout and stderr go.
#[derive(Debug)]
pub enum OutputTarget {
    /// The spool files of a main process, which outlive the daemon.
    Spool {
        /// The stdout spool.
        stdout: SpoolWriter,
        /// The stderr spool.
        stderr: SpoolWriter,
    },
    /// Pipes a platform thread reads into a tail of at most
    /// [`HELPER_CAPTURE_MAX`] bytes per stream, for a helper.
    Capture,
}

/// The part of a [`SpawnRequest`] that [`ProcessError::InvalidArgument`]
/// points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestPart {
    /// The program path holds a NUL byte.
    Program,
    /// The argument at this index (from 0, after the program) holds a NUL
    /// byte.
    Arg(usize),
    /// The name of the environment entry at this index is empty or holds
    /// `=` or a NUL byte.
    EnvName(usize),
    /// The value of the environment entry at this index holds a NUL byte.
    EnvValue(usize),
    /// The working directory holds a NUL byte.
    Cwd,
    /// The role does not fit the method.
    Role,
    /// A main process reads only `/dev/null`.
    Stdin,
    /// The output target does not fit the role.
    Output,
    /// The role may not inherit the instance lock.
    Lock,
}

impl fmt::Display for RequestPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Program => f.write_str("the program"),
            Self::Arg(index) => write!(f, "argument {index}"),
            Self::EnvName(index) => write!(f, "the name of environment entry {index}"),
            Self::EnvValue(index) => write!(f, "the value of environment entry {index}"),
            Self::Cwd => f.write_str("the working directory"),
            Self::Role => f.write_str("the role"),
            Self::Stdin => f.write_str("stdin"),
            Self::Output => f.write_str("the output target"),
            Self::Lock => f.write_str("the instance lock"),
        }
    }
}

/// Everything a spawn needs, exactly as the plan of spec 0009 gave it.
///
/// `argv[0]` is `program`, `argv[1..]` is `args`, the environment is
/// `env` and nothing inherited from the daemon, and no shell ever reads
/// any of it.
#[derive(Debug)]
pub struct SpawnRequest<'a> {
    /// The canonical path of the executable.
    pub program: &'a Path,
    /// The arguments after the program.
    pub args: &'a [String],
    /// The whole environment, in order.
    pub env: &'a [(String, String)],
    /// The working directory.
    pub cwd: &'a Path,
    /// What the child reads on stdin.
    pub stdin: StdinSource,
    /// Where its output goes.
    pub output: OutputTarget,
    /// The instance lock the child inherits at fd 3, only for
    /// [`ChildRole::Main`] and [`ChildRole::InitStep`].
    pub lock: Option<&'a InstanceLock>,
    /// Why the child is spawned.
    pub role: ChildRole,
}

impl SpawnRequest<'_> {
    /// Checks that the request fits [`ProcessSupervision::spawn_main`]: the
    /// [`ChildRole::Main`] role, stdin from `/dev/null`, spool output, and
    /// no NUL byte in any text. Adapters call it before any side effect.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidArgument`] naming the first part that
    /// does not fit.
    pub fn check_main(&self) -> Result<(), ProcessError> {
        self.check_text()?;
        if self.role != ChildRole::Main {
            return Err(invalid(RequestPart::Role));
        }
        if !matches!(self.stdin, StdinSource::Null) {
            return Err(invalid(RequestPart::Stdin));
        }
        if !matches!(self.output, OutputTarget::Spool { .. }) {
            return Err(invalid(RequestPart::Output));
        }
        Ok(())
    }

    /// Checks that the request fits [`ProcessSupervision::spawn_helper`]: a
    /// helper role, captured output, the instance lock only for an init
    /// step, and no NUL byte in any text. Adapters call it before any side
    /// effect.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidArgument`] naming the first part that
    /// does not fit.
    pub fn check_helper(&self) -> Result<(), ProcessError> {
        self.check_text()?;
        if self.role == ChildRole::Main {
            return Err(invalid(RequestPart::Role));
        }
        if !matches!(self.output, OutputTarget::Capture) {
            return Err(invalid(RequestPart::Output));
        }
        if self.lock.is_some() && !self.role.takes_lock() {
            return Err(invalid(RequestPart::Lock));
        }
        Ok(())
    }

    fn check_text(&self) -> Result<(), ProcessError> {
        let has_nul = |bytes: &[u8]| bytes.contains(&0);
        if has_nul(self.program.as_os_str().as_encoded_bytes()) {
            return Err(invalid(RequestPart::Program));
        }
        if let Some(index) = self.args.iter().position(|arg| has_nul(arg.as_bytes())) {
            return Err(invalid(RequestPart::Arg(index)));
        }
        for (index, (name, value)) in self.env.iter().enumerate() {
            if name.is_empty() || name.contains('=') || has_nul(name.as_bytes()) {
                return Err(invalid(RequestPart::EnvName(index)));
            }
            if has_nul(value.as_bytes()) {
                return Err(invalid(RequestPart::EnvValue(index)));
            }
        }
        if has_nul(self.cwd.as_os_str().as_encoded_bytes()) {
            return Err(invalid(RequestPart::Cwd));
        }
        Ok(())
    }
}

const fn invalid(position: RequestPart) -> ProcessError {
    ProcessError::InvalidArgument { position }
}

/// An exclusive lock on an instance's `instance.lock` file, held through
/// an open file.
///
/// Dropping it closes the file, which releases the lock unless a child
/// inherited it at fd 3; nothing ever calls `flock(LOCK_UN)`, so a lock a
/// child inherited stays held while that child or its descendants live.
#[derive(Debug)]
pub struct InstanceLock {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
}

impl InstanceLock {
    /// Wraps a file the adapter opened and locked. Only a
    /// [`ProcessSupervision`] adapter creates one.
    #[must_use]
    pub const fn new(file: File, path: PathBuf, identity: FileIdentity) -> Self {
        Self {
            file,
            path,
            identity,
        }
    }

    /// Returns the open file, for the adapter to map it to a child's fd 3.
    #[must_use]
    pub const fn file(&self) -> &File {
        &self.file
    }

    /// Returns the path the lock was taken on.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the device and inode of the lock file, which
    /// [`ProcessSupervision::holds_file`] looks for in a process.
    #[must_use]
    pub const fn identity(&self) -> FileIdentity {
        self.identity
    }
}

/// The outcome of [`ProcessSupervision::lock_instance`].
#[derive(Debug)]
pub enum LockOutcome {
    /// The lock is now held by the daemon.
    Acquired(InstanceLock),
    /// Another open file holds the lock, so nothing may start on the data.
    Held,
}

/// A new spool file, open for appending, that becomes a main process's
/// stdout or stderr.
#[derive(Debug)]
pub struct SpoolWriter {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
}

impl SpoolWriter {
    /// Wraps a spool file the adapter created. Only a
    /// [`ProcessSupervision`] adapter creates one.
    #[must_use]
    pub const fn new(file: File, path: PathBuf, identity: FileIdentity) -> Self {
        Self {
            file,
            path,
            identity,
        }
    }

    /// Returns the open file, for the adapter to map it to fd 1 or 2.
    #[must_use]
    pub const fn file(&self) -> &File {
        &self.file
    }

    /// Returns the spool's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the device and inode of the spool.
    #[must_use]
    pub const fn identity(&self) -> FileIdentity {
        self.identity
    }

    /// Returns the open file, giving up the wrapper.
    #[must_use]
    pub fn into_file(self) -> File {
        self.file
    }
}

/// Reads a spool the daemon did not create in this run, or keeps reading
/// one it did, and frees the part the log sink already accepted.
///
/// It can never write or truncate the spool. [`release_below`] frees whole
/// [`punch_unit`]s only, following [`SpoolWatermark`], so a unit the child
/// may still be writing is never freed.
///
/// [`release_below`]: SpoolReader::release_below
/// [`punch_unit`]: SpoolReader::punch_unit
#[expect(
    clippy::len_without_is_empty,
    reason = "spec 0010 fixes the reader's methods; first_data_offset tells whether data is left"
)]
pub trait SpoolReader: Send {
    /// Returns the length of the spool in bytes, freed parts included.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the length cannot be read.
    fn len(&self) -> Result<u64, ProcessError>;

    /// Reads at most `buf.len()` bytes at `offset` and returns how many it
    /// read; 0 means `offset` is at or past the end.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the read fails.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, ProcessError>;

    /// Returns the offset of the first byte that is not freed, or `None`
    /// when no data is left.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the offset cannot be read.
    fn first_data_offset(&self) -> Result<Option<u64>, ProcessError>;

    /// Returns the size of the units the reader frees, measured when the
    /// spool was opened: one VM page or one filesystem block, whichever is
    /// larger (16 KiB on Apple silicon).
    fn punch_unit(&self) -> NonZeroU64;

    /// Frees every whole unit below `offset`, the end of the last line the
    /// sink accepted. An offset at or below what is already freed does
    /// nothing, and one past the end counts as the end.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when freeing fails; the reader keeps
    /// reading.
    fn release_below(&mut self, offset: u64) -> Result<(), ProcessError>;

    /// Returns the device and inode of the spool that was opened.
    fn file_identity(&self) -> FileIdentity;
}

/// How far a [`SpoolReader`] has freed its spool: the rounding rule every
/// adapter and the testkit fake share, so they free exactly the same
/// bytes.
///
/// The watermark is always a whole number of units, and only grows.
///
/// ```
/// use std::num::NonZeroU64;
///
/// use stackly_platform::SpoolWatermark;
///
/// let unit = NonZeroU64::new(16).expect("16 is not zero");
/// let mut mark = SpoolWatermark::new(unit, Some(0), 100);
/// assert_eq!(mark.next_release(40, 100), Some(0..32));
/// mark.advance(32);
/// assert_eq!(mark.next_release(47, 100), None);
/// assert_eq!(mark.next_release(500, 100), Some(32..96));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpoolWatermark {
    unit: NonZeroU64,
    released: u64,
}

impl SpoolWatermark {
    /// Starts where a newly opened spool's data starts: `first_data`, or
    /// `len` when no data is left, rounded down to a whole unit.
    #[must_use]
    pub const fn new(unit: NonZeroU64, first_data: Option<u64>, len: u64) -> Self {
        let start = match first_data {
            Some(offset) => offset,
            None => len,
        };
        Self {
            unit,
            released: round_down(start, unit),
        }
    }

    /// Returns the unit size.
    #[must_use]
    pub const fn unit(self) -> NonZeroU64 {
        self.unit
    }

    /// Returns the offset below which everything is freed.
    #[must_use]
    pub const fn released(self) -> u64 {
        self.released
    }

    /// Returns the whole units to free for a release below `offset` on a
    /// spool of `len` bytes: from the watermark up to `offset`, cut to
    /// `len` and rounded down to a whole unit. `None` when that does not
    /// pass the watermark.
    #[must_use]
    pub const fn next_release(self, offset: u64, len: u64) -> Option<Range<u64>> {
        let limit = if offset < len { offset } else { len };
        let end = round_down(limit, self.unit);
        if end > self.released {
            Some(self.released..end)
        } else {
            None
        }
    }

    /// Moves the watermark to `end` once the units below it are freed. An
    /// `end` at or below the watermark changes nothing.
    pub const fn advance(&mut self, end: u64) {
        if end > self.released {
            self.released = round_down(end, self.unit);
        }
    }
}

const fn round_down(offset: u64, unit: NonZeroU64) -> u64 {
    offset - offset % unit.get()
}

/// What the OS says about one live process.
///
/// An adapter answers these for a process of another user too (from
/// `kinfo_proc` on macOS), so a PID that another user now holds is proven
/// not to be ours instead of left unproven.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessFacts {
    /// The process ID.
    pub pid: Pid,
    /// The parent's ID, or `None` when the kernel is the parent.
    pub parent: Option<Pid>,
    /// The process group ID.
    pub group: Pid,
    /// The session ID.
    pub session: Pid,
    /// The effective user.
    pub uid: UserId,
    /// When the process started.
    pub start: ProcessStart,
    /// The path of its executable.
    pub executable: PathBuf,
    /// True only while the process is suspended (`SSTOP`), such as a main
    /// process that was spawned and not resumed yet.
    pub stopped: bool,
}

/// A main process that was spawned suspended and has not run any code.
///
/// Only [`ProcessSupervision::resume`] and [`ProcessSupervision::discard`]
/// take it, by value, so a discard (`SIGKILL`) can never reach a process
/// that already ran.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a suspended child must be resumed or discarded"]
pub struct SuspendedChild {
    facts: ProcessFacts,
}

impl SuspendedChild {
    /// Wraps the facts the adapter read right after the spawn. Only a
    /// [`ProcessSupervision`] adapter creates one.
    pub const fn new(facts: ProcessFacts) -> Self {
        Self { facts }
    }

    /// Returns the process ID.
    #[must_use]
    pub const fn pid(&self) -> Pid {
        self.facts.pid
    }

    /// Returns the facts the adapter read while the child was suspended,
    /// the source of the identity the supervisor records.
    #[must_use]
    pub const fn facts(&self) -> &ProcessFacts {
        &self.facts
    }
}

/// A main process the daemon resumed.
#[derive(Debug, PartialEq, Eq)]
pub struct MainHandle {
    pid: Pid,
}

impl MainHandle {
    /// Wraps the PID of a resumed main process. Only a
    /// [`ProcessSupervision`] adapter creates one.
    #[must_use]
    pub const fn new(pid: Pid) -> Self {
        Self { pid }
    }

    /// Returns the process ID.
    #[must_use]
    pub const fn pid(&self) -> Pid {
        self.pid
    }
}

/// A helper process: an init step, a probe, or a stop or reload command.
/// Only a helper can have its group terminated.
#[derive(Debug, PartialEq, Eq)]
pub struct HelperHandle {
    pid: Pid,
    role: ChildRole,
}

impl HelperHandle {
    /// Wraps the PID of a helper the adapter spawned. Only a
    /// [`ProcessSupervision`] adapter creates one.
    #[must_use]
    pub const fn new(pid: Pid, role: ChildRole) -> Self {
        Self { pid, role }
    }

    /// Returns the process ID, which is also its process group ID.
    #[must_use]
    pub const fn pid(&self) -> Pid {
        self.pid
    }

    /// Returns why the helper was spawned.
    #[must_use]
    pub const fn role(&self) -> ChildRole {
        self.role
    }
}

/// Whether a process holds a file open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoldsFile {
    /// One of its descriptors refers to the file.
    Yes,
    /// None of its descriptors does.
    No,
    /// Its descriptors cannot be listed, for example for a process of
    /// another user.
    Unreadable,
}

/// The process a signal is for, with the identity it must still have.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SignalTarget {
    expected: ProcessIdentity,
}

impl SignalTarget {
    /// Targets the process `expected` describes.
    #[must_use]
    pub const fn new(expected: ProcessIdentity) -> Self {
        Self { expected }
    }

    /// Returns the process ID.
    #[must_use]
    pub const fn pid(&self) -> Pid {
        self.expected.pid
    }

    /// Returns the identity the adapter checks again right before the
    /// signal.
    #[must_use]
    pub const fn expected(&self) -> &ProcessIdentity {
        &self.expected
    }
}

/// A signal a provider's stop or reload strategy may send. There is no
/// `Kill`: a normal stop never escalates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderSignal {
    /// `SIGTERM`.
    Term,
    /// `SIGINT`.
    Int,
    /// `SIGQUIT`.
    Quit,
    /// `SIGHUP`.
    Hup,
}

impl ProviderSignal {
    /// Every signal, in code order.
    pub const ALL: [Self; 4] = [Self::Term, Self::Int, Self::Quit, Self::Hup];

    /// Returns the stable code, for example `term`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::Int => "int",
            Self::Quit => "quit",
            Self::Hup => "hup",
        }
    }
}

impl fmt::Display for ProviderSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// How a child of the daemon ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExitStatus {
    /// It exited with this code.
    Exited(i32),
    /// This signal ended it.
    Signaled(i32),
}

/// The tail of a helper's output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapturedOutput {
    /// The last bytes of stdout, at most [`HELPER_CAPTURE_MAX`].
    pub stdout: Vec<u8>,
    /// The last bytes of stderr, at most [`HELPER_CAPTURE_MAX`].
    pub stderr: Vec<u8>,
}

/// One process exit, as the watcher reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitNotice {
    /// The process that exited.
    pub pid: Pid,
    /// How it ended, only for a child of this daemon; an adopted process
    /// always has `None`.
    pub status: Option<ExitStatus>,
    /// The tail of its output, only for a helper.
    pub captured: Option<CapturedOutput>,
}

/// Receives the exit of one watched process.
pub trait ExitListener: Send + Sync {
    /// Called once, from the adapter's watcher thread, when the process
    /// exits; at once when it already had.
    fn exited(&self, notice: ExitNotice);
}

/// Error returned by a [`ProcessSupervision`] method.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProcessError {
    /// The OS has no process supervision adapter.
    #[error("{capability} is not supported on {os}")]
    NotSupported {
        /// Always [`PlatformCapability::ProcessSupervision`].
        capability: PlatformCapability,
        /// The OS that lacks it.
        os: Os,
    },
    /// The OS refused to spawn the process.
    #[error("the spawn failed with errno {errno} ({kind})")]
    SpawnFailed {
        /// The kind of the OS error.
        kind: io::ErrorKind,
        /// The OS error number.
        errno: i32,
    },
    /// A part of the spawn request is invalid, such as a NUL byte in an
    /// argument; nothing was spawned.
    #[error("the spawn request is invalid: {position}")]
    InvalidArgument {
        /// The part that is invalid.
        position: RequestPart,
    },
    /// The process no longer has the identity the caller expected, so it
    /// was not signalled.
    #[error("process {pid} no longer has the expected identity")]
    IdentityMismatch {
        /// The process ID.
        pid: Pid,
    },
    /// No live process has this ID, or the handle was already reaped.
    #[error("process {pid} does not exist")]
    NotFound {
        /// The process ID.
        pid: Pid,
    },
    /// The OS denied access to the process.
    #[error("permission denied for process {pid}")]
    PermissionDenied {
        /// The process ID.
        pid: Pid,
    },
    /// The lock file is a symlink, not a regular file, or not owned by the
    /// current user.
    #[error("lock file {} is invalid: {reason}", path.display())]
    LockFileInvalid {
        /// The lock file.
        path: PathBuf,
        /// Why, for example `is a symlink`.
        reason: &'static str,
    },
    /// Any other OS failure.
    #[error("{op} failed{}: {message}", for_path(path.as_deref()))]
    Io {
        /// The operation or step that failed, for example `proc_pidinfo`.
        op: &'static str,
        /// The file it was working on, if any.
        path: Option<PathBuf>,
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// The OS error message.
        message: String,
    },
}

impl ProcessError {
    /// Wraps an I/O error from `op` on `path` as [`ProcessError::Io`].
    #[must_use]
    pub fn io(op: &'static str, path: Option<&Path>, error: &io::Error) -> Self {
        Self::Io {
            op,
            path: path.map(Path::to_path_buf),
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

fn for_path(path: Option<&Path>) -> String {
    path.map(|path| format!(" for {}", path.display()))
        .unwrap_or_default()
}

/// Starting, watching, and signalling the processes of instances (spec
/// 0010).
///
/// Only the supervisor in `stacklyd` calls it. Every method is sync and
/// object safe, and safe to call from any thread; core calls it through
/// `spawn_blocking`. No method takes a process name, and no method can
/// send `SIGKILL` to a process that ran, except to a helper's group.
pub trait ProcessSupervision: Send + Sync {
    /// Spawns a main process suspended, in its own session, with the
    /// request's argv, environment, and working directory, its output in
    /// the spools, and the lock at fd 3.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidArgument`] when the request does not
    /// pass [`SpawnRequest::check_main`], and
    /// [`ProcessError::SpawnFailed`] when the OS refuses.
    fn spawn_main(&self, request: SpawnRequest<'_>) -> Result<SuspendedChild, ProcessError>;

    /// Resumes a suspended main process with `SIGCONT`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when the child no longer exists.
    fn resume(&self, child: SuspendedChild) -> Result<MainHandle, ProcessError>;

    /// Kills and reaps a suspended main process that never ran.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when the child no longer exists.
    fn discard(&self, child: SuspendedChild) -> Result<(), ProcessError>;

    /// Spawns a helper, running at once, in its own session.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::InvalidArgument`] when the request does not
    /// pass [`SpawnRequest::check_helper`], and
    /// [`ProcessError::SpawnFailed`] when the OS refuses.
    fn spawn_helper(&self, request: SpawnRequest<'_>) -> Result<HelperHandle, ProcessError>;

    /// Reads the facts of a live process, or `None` when no process or only
    /// a zombie has the ID. A process of another user still has facts.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] or [`ProcessError::PermissionDenied`]
    /// when the facts cannot be read; the caller treats that as unproven,
    /// never as gone.
    fn inspect(&self, pid: Pid) -> Result<Option<ProcessFacts>, ProcessError>;

    /// Tells whether a process holds `file` open. A process whose
    /// descriptors cannot be listed answers [`HoldsFile::Unreadable`].
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when the process is gone, and
    /// [`ProcessError::Io`] when the listing fails otherwise.
    fn holds_file(&self, pid: Pid, file: FileIdentity) -> Result<HoldsFile, ProcessError>;

    /// Sends a provider signal after reading the target's facts again
    /// right before `kill(2)`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::IdentityMismatch`] without signalling when
    /// the start time, user, or session differs (a PID another user now
    /// holds included), and [`ProcessError::NotFound`] when the process is
    /// gone.
    fn signal(&self, target: &SignalTarget, signal: ProviderSignal) -> Result<(), ProcessError>;

    /// Sends `SIGCONT` to a suspended process the daemon adopts, after the
    /// same check as [`signal`](Self::signal).
    ///
    /// # Errors
    ///
    /// The errors of [`signal`](Self::signal).
    fn continue_stopped(&self, target: &SignalTarget) -> Result<(), ProcessError>;

    /// Sends `SIGTERM` to a helper's process group, or `SIGKILL` when
    /// `force` is true, only while the helper is not reaped yet.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when the helper is already
    /// reaped.
    fn terminate_helper_group(
        &self,
        helper: &HelperHandle,
        force: bool,
    ) -> Result<(), ProcessError>;

    /// Lists the live members of a process group, in PID order, to report
    /// the processes a main process left behind. It never signals them.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the list cannot be read.
    fn group_members(&self, group: Pid) -> Result<Vec<Pid>, ProcessError>;

    /// Returns the ID of the current boot.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when it cannot be read.
    fn boot_id(&self) -> Result<BootId, ProcessError>;

    /// Watches a process and calls `listener` once when it exits, at once
    /// when it already has.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the watch cannot be registered.
    fn watch_exit(&self, pid: Pid, listener: Arc<dyn ExitListener>) -> Result<(), ProcessError>;

    /// Opens or creates the lock file (mode 0600, never through a symlink,
    /// a regular file of the current user) and takes an exclusive lock
    /// without waiting.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::LockFileInvalid`] for a symlink, a file of
    /// another kind, or one owned by another user, and
    /// [`ProcessError::Io`] when the file cannot be opened.
    fn lock_instance(&self, path: &Path) -> Result<LockOutcome, ProcessError>;

    /// Tells whether anyone holds the lock, through a new open file that it
    /// closes at once; it never keeps the lock. A missing file is free.
    ///
    /// # Errors
    ///
    /// The errors of [`lock_instance`](Self::lock_instance).
    fn probe_lock(&self, path: &Path) -> Result<LockState, ProcessError>;

    /// Creates a new spool (append only, mode 0600), failing when the path
    /// exists.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the file exists or cannot be
    /// created.
    fn create_spool(&self, path: &Path) -> Result<SpoolWriter, ProcessError>;

    /// Opens an existing spool for reading and freeing, or returns `None`
    /// when there is none.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when it exists but cannot be opened, is
    /// not a regular file of the current user, or the punch unit cannot be
    /// measured.
    fn open_spool(&self, path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError>;
}

/// The process supervision of an OS without an adapter: every method
/// answers [`ProcessError::NotSupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedProcessSupervision {
    os: Os,
}

impl UnsupportedProcessSupervision {
    /// Creates the port for `os`.
    #[must_use]
    pub const fn new(os: Os) -> Self {
        Self { os }
    }

    const fn not_supported(&self) -> ProcessError {
        ProcessError::NotSupported {
            capability: PlatformCapability::ProcessSupervision,
            os: self.os,
        }
    }
}

impl ProcessSupervision for UnsupportedProcessSupervision {
    fn spawn_main(&self, _request: SpawnRequest<'_>) -> Result<SuspendedChild, ProcessError> {
        Err(self.not_supported())
    }

    fn resume(&self, _child: SuspendedChild) -> Result<MainHandle, ProcessError> {
        Err(self.not_supported())
    }

    fn discard(&self, _child: SuspendedChild) -> Result<(), ProcessError> {
        Err(self.not_supported())
    }

    fn spawn_helper(&self, _request: SpawnRequest<'_>) -> Result<HelperHandle, ProcessError> {
        Err(self.not_supported())
    }

    fn inspect(&self, _pid: Pid) -> Result<Option<ProcessFacts>, ProcessError> {
        Err(self.not_supported())
    }

    fn holds_file(&self, _pid: Pid, _file: FileIdentity) -> Result<HoldsFile, ProcessError> {
        Err(self.not_supported())
    }

    fn signal(&self, _target: &SignalTarget, _signal: ProviderSignal) -> Result<(), ProcessError> {
        Err(self.not_supported())
    }

    fn continue_stopped(&self, _target: &SignalTarget) -> Result<(), ProcessError> {
        Err(self.not_supported())
    }

    fn terminate_helper_group(
        &self,
        _helper: &HelperHandle,
        _force: bool,
    ) -> Result<(), ProcessError> {
        Err(self.not_supported())
    }

    fn group_members(&self, _group: Pid) -> Result<Vec<Pid>, ProcessError> {
        Err(self.not_supported())
    }

    fn boot_id(&self) -> Result<BootId, ProcessError> {
        Err(self.not_supported())
    }

    fn watch_exit(&self, _pid: Pid, _listener: Arc<dyn ExitListener>) -> Result<(), ProcessError> {
        Err(self.not_supported())
    }

    fn lock_instance(&self, _path: &Path) -> Result<LockOutcome, ProcessError> {
        Err(self.not_supported())
    }

    fn probe_lock(&self, _path: &Path) -> Result<LockState, ProcessError> {
        Err(self.not_supported())
    }

    fn create_spool(&self, _path: &Path) -> Result<SpoolWriter, ProcessError> {
        Err(self.not_supported())
    }

    fn open_spool(&self, _path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError> {
        Err(self.not_supported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(size: u64) -> NonZeroU64 {
        NonZeroU64::new(size).unwrap()
    }

    fn request<'a>(
        role: ChildRole,
        args: &'a [String],
        env: &'a [(String, String)],
        output: OutputTarget,
    ) -> SpawnRequest<'a> {
        SpawnRequest {
            program: Path::new("fake-server"),
            args,
            env,
            cwd: Path::new("."),
            stdin: StdinSource::Null,
            output,
            lock: None,
            role,
        }
    }

    fn invalid_at(result: Result<(), ProcessError>) -> RequestPart {
        match result {
            Err(ProcessError::InvalidArgument { position }) => position,
            other => panic!("expected an invalid argument, got {other:?}"),
        }
    }

    #[test]
    fn a_watermark_starts_at_the_first_data_rounded_down_to_a_unit() {
        assert_eq!(SpoolWatermark::new(unit(16), Some(0), 100).released(), 0);
        assert_eq!(SpoolWatermark::new(unit(16), Some(40), 100).released(), 32);
        assert_eq!(SpoolWatermark::new(unit(16), None, 100).released(), 96);
        assert_eq!(SpoolWatermark::new(unit(16), None, 0).released(), 0);
    }

    #[test]
    fn a_release_frees_only_whole_units_past_the_watermark_and_within_the_spool() {
        let mut mark = SpoolWatermark::new(unit(16), Some(16), 100);

        assert_eq!(mark.next_release(15, 100), None);
        assert_eq!(mark.next_release(31, 100), None);
        assert_eq!(mark.next_release(32, 100), Some(16..32));
        assert_eq!(mark.next_release(1_000, 100), Some(16..96));
        mark.advance(96);
        assert_eq!(mark.released(), 96);
        assert_eq!(mark.next_release(1_000, 100), None);
        mark.advance(48);
        assert_eq!(mark.released(), 96);
        assert_eq!(mark.next_release(130, 140), Some(96..128));
    }

    #[test]
    fn a_main_request_needs_the_main_role_null_stdin_and_spool_output() {
        let args = ["serve".to_owned()];
        let env = [("FAKE_LABEL".to_owned(), "a b".to_owned())];

        assert_eq!(
            invalid_at(request(ChildRole::Probe, &args, &env, OutputTarget::Capture).check_main()),
            RequestPart::Role
        );
        assert_eq!(
            invalid_at(request(ChildRole::Main, &args, &env, OutputTarget::Capture).check_main()),
            RequestPart::Output
        );
        let mut secret = request(ChildRole::Main, &args, &env, OutputTarget::Capture);
        secret.stdin =
            StdinSource::Bytes(SecretValue::new("not-a-real-secret".to_owned()).unwrap());
        assert_eq!(invalid_at(secret.check_main()), RequestPart::Stdin);
    }

    #[test]
    fn a_helper_request_refuses_the_main_role_spool_output_and_a_lock_it_may_not_take() {
        let lock_file = File::open(".").unwrap();
        let lock = InstanceLock::new(
            lock_file,
            PathBuf::from("instance.lock"),
            FileIdentity {
                device: 1,
                inode: 2,
            },
        );
        let args = [];
        let env = [];

        assert_eq!(
            request(ChildRole::Probe, &args, &env, OutputTarget::Capture).check_helper(),
            Ok(())
        );
        assert_eq!(
            invalid_at(request(ChildRole::Main, &args, &env, OutputTarget::Capture).check_helper()),
            RequestPart::Role
        );
        for role in [
            ChildRole::Probe,
            ChildRole::StopCommand,
            ChildRole::ReloadCommand,
        ] {
            let mut with_lock = request(role, &args, &env, OutputTarget::Capture);
            with_lock.lock = Some(&lock);
            assert_eq!(invalid_at(with_lock.check_helper()), RequestPart::Lock);
        }
        let mut init = request(ChildRole::InitStep, &args, &env, OutputTarget::Capture);
        init.lock = Some(&lock);
        assert_eq!(init.check_helper(), Ok(()));
        assert!(ChildRole::Main.takes_lock());
    }

    #[test]
    fn nul_bytes_and_bad_environment_names_are_refused_by_position() {
        let good = ["ok".to_owned(), "spaces and ü".to_owned()];
        let bad = ["ok".to_owned(), "nul\0inside".to_owned()];
        let env = |name: &str, value: &str| vec![(name.to_owned(), value.to_owned())];
        let check = |args: &[String], env: &[(String, String)]| {
            request(ChildRole::Probe, args, env, OutputTarget::Capture).check_helper()
        };

        assert_eq!(check(&good, &env("A", "1")), Ok(()));
        assert_eq!(invalid_at(check(&bad, &[])), RequestPart::Arg(1));
        for name in ["", "A=B", "A\0"] {
            assert_eq!(
                invalid_at(check(&good, &env(name, "1"))),
                RequestPart::EnvName(0)
            );
        }
        assert_eq!(
            invalid_at(check(&good, &env("A", "1\0"))),
            RequestPart::EnvValue(0)
        );
        let mut nul_program = request(ChildRole::Probe, &good, &[], OutputTarget::Capture);
        nul_program.program = Path::new("fake\0server");
        assert_eq!(invalid_at(nul_program.check_helper()), RequestPart::Program);
        let mut nul_cwd = request(ChildRole::Probe, &good, &[], OutputTarget::Capture);
        nul_cwd.cwd = Path::new("run\0dir");
        assert_eq!(invalid_at(nul_cwd.check_helper()), RequestPart::Cwd);
    }

    #[test]
    fn process_errors_read_as_lowercase_sentences() {
        let pid = Pid::new(4242).unwrap();
        let io_error = io::Error::other("boom");

        assert_eq!(
            ProcessError::IdentityMismatch { pid }.to_string(),
            "process 4242 no longer has the expected identity"
        );
        assert_eq!(
            invalid(RequestPart::EnvName(2)).to_string(),
            "the spawn request is invalid: the name of environment entry 2"
        );
        assert_eq!(
            ProcessError::io("proc_pidinfo", None, &io_error).to_string(),
            "proc_pidinfo failed: boom"
        );
        assert_eq!(
            ProcessError::io("open", Some(Path::new("stdout.spool")), &io_error).to_string(),
            "open failed for stdout.spool: boom"
        );
    }

    #[test]
    fn streams_and_signals_have_stable_codes() {
        let streams: Vec<_> = OutputStream::ALL
            .iter()
            .map(|stream| (stream.as_code(), stream.spool_file_name()))
            .collect();
        assert_eq!(
            streams,
            [("stdout", "stdout.spool"), ("stderr", "stderr.spool")]
        );
        let signals: Vec<_> = ProviderSignal::ALL
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(signals, ["term", "int", "quit", "hup"]);
    }
}
