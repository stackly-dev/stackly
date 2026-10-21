use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::num::NonZeroU64;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_domain::{BootId, LockState, Pid, ProcessStart};
use stackly_platform::{
    CapturedOutput, ChildRole, ExitListener, ExitNotice, ExitStatus, FileIdentity, HelperHandle,
    HoldsFile, InstanceLock, LockOutcome, MainHandle, OutputStream, OutputTarget, ProcessError,
    ProcessFacts, ProcessSupervision, ProviderSignal, SecretValue, SignalTarget, SpawnRequest,
    SpoolReader, SpoolWatermark, SpoolWriter, StdinSource, SuspendedChild, UserId,
};

use crate::TestRoot;

/// The first PID the fake hands out.
const FIRST_PID: u32 = 50_000;
/// Where the start times of spawned fake processes begin:
/// 2026-01-01T00:00:00Z, the start of the fake clock.
const FIRST_START_SECONDS: u64 = 1_767_225_600;
/// The boot every fake starts in.
const DEFAULT_BOOT: &str = "5ac4e3f0-0b1d-4c8e-9a35-7d1f2e6b9c40";
/// The punch unit of the Mac this was built on: one 16 KiB VM page.
const DEFAULT_PUNCH_UNIT: u64 = 16_384;
/// The number of the signal [`ProcessSupervision::discard`] sends.
const KILL_SIGNAL_NUMBER: i32 = 9;

/// One method of [`ProcessSupervision`], or of the spool reader it opens,
/// for the call log and the faults of [`FakeProcessSupervision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessOp {
    /// [`ProcessSupervision::spawn_main`].
    SpawnMain,
    /// [`ProcessSupervision::resume`].
    Resume,
    /// [`ProcessSupervision::discard`].
    Discard,
    /// [`ProcessSupervision::spawn_helper`].
    SpawnHelper,
    /// [`ProcessSupervision::inspect`].
    Inspect,
    /// [`ProcessSupervision::holds_file`].
    HoldsFile,
    /// [`ProcessSupervision::signal`].
    Signal,
    /// [`ProcessSupervision::continue_stopped`].
    ContinueStopped,
    /// [`ProcessSupervision::terminate_helper_group`].
    TerminateHelperGroup,
    /// [`ProcessSupervision::group_members`].
    GroupMembers,
    /// [`ProcessSupervision::boot_id`].
    BootId,
    /// [`ProcessSupervision::watch_exit`].
    WatchExit,
    /// [`ProcessSupervision::lock_instance`].
    LockInstance,
    /// [`ProcessSupervision::probe_lock`].
    ProbeLock,
    /// [`ProcessSupervision::create_spool`].
    CreateSpool,
    /// [`ProcessSupervision::open_spool`].
    OpenSpool,
    /// [`SpoolReader::release_below`] on a spool the fake opened.
    ReleaseBelow,
}

/// One call that reached a [`FakeProcessSupervision`], in the order it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProcessCall {
    /// The method.
    pub op: ProcessOp,
    /// The process it targeted, or the new child of a spawn that worked.
    pub pid: Option<Pid>,
    /// The signal of a [`ProcessOp::Signal`].
    pub signal: Option<ProviderSignal>,
    /// The `force` flag of a [`ProcessOp::TerminateHelperGroup`].
    pub force: Option<bool>,
    /// The file of a lock or spool call.
    pub path: Option<PathBuf>,
    /// The error it returned, if any.
    pub error: Option<ProcessError>,
}

impl ProcessCall {
    const fn new(op: ProcessOp) -> Self {
        Self {
            op,
            pid: None,
            signal: None,
            force: None,
            path: None,
            error: None,
        }
    }
}

/// Where a process of the fake is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FakeProcessState {
    /// Spawned suspended and not resumed: it never ran.
    Suspended,
    /// Running.
    Running,
    /// Exited, or discarded. A PID that is gone has no facts.
    Exited,
}

/// What the fake recorded about one spawn that worked.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FakeSpawn {
    /// The new process.
    pub pid: Pid,
    /// Why it was spawned.
    pub role: ChildRole,
    /// The program, exactly as given.
    pub program: PathBuf,
    /// The arguments after the program, exactly as given.
    pub args: Vec<String>,
    /// The whole environment, exactly as given.
    pub env: Vec<(String, String)>,
    /// The working directory.
    pub cwd: PathBuf,
    /// The length of the secret written to stdin, or `None` for
    /// `/dev/null`. [`FakeProcessSupervision::take_stdin`] hands out the
    /// value.
    pub stdin_len: Option<usize>,
    /// The instance lock it inherited at fd 3, if any.
    pub lock: Option<FileIdentity>,
    /// The stdout and stderr spools of a main process.
    pub spools: Option<[FileIdentity; 2]>,
}

/// Processes in memory with real lock and spool files in a [`TestRoot`]:
/// the [`ProcessSupervision`] port for core and daemon tests (spec 0010).
///
/// It behaves like the macOS adapter where tests can see it:
///
/// - Processes are made up, with PIDs from 50000 and facts the test can
///   change ([`update_facts`](Self::update_facts)) to act out a PID that
///   another process or user now holds. A main process starts
///   [`Suspended`](FakeProcessState::Suspended) until it is resumed, and
///   nothing exits until the test says so ([`exit`](Self::exit)).
/// - [`signal`](ProcessSupervision::signal) and
///   [`continue_stopped`](ProcessSupervision::continue_stopped) refuse with
///   `IdentityMismatch` when the start time, user, or session differs from
///   the target, and never deliver a signal then.
/// - Locks are real `flock` locks on real files, and a child really keeps
///   the lock it inherited: the fake holds a duplicate of the lock's open
///   file until the child exits, so a [`probe_lock`] sees it held after the
///   caller drops its [`InstanceLock`].
/// - Spools are real files the test appends to with
///   [`write_output`](Self::write_output). The fake cannot punch holes, so
///   it remembers how far each spool was freed, answers
///   [`first_data_offset`](SpoolReader::first_data_offset) from that, and
///   reads freed bytes as zeros, rounding with [`SpoolWatermark`] like the
///   adapter. The punch unit is 16 KiB unless the test sets another.
/// - Every lock and spool path must lie inside the test root or its run
///   folder; any other path fails with a `PermissionDenied` I/O error and
///   counts as a [violation](Self::violations).
///
/// Every call lands in a log ([`calls`](Self::calls)), so a test can check
/// that no discard reached a process that ran, that no signal reached a
/// changed identity, and that evidence was gathered in order. Every method
/// can be made to fail ([`fail_next`](Self::fail_next),
/// [`fail_always`](Self::fail_always)) after it is logged. Clones share the
/// processes, like processes outlive the daemon that started them.
///
/// [`probe_lock`]: ProcessSupervision::probe_lock
///
/// ```
/// use stackly_platform::{LockOutcome, LockState, ProcessSupervision};
/// use stackly_testkit::{FakeProcessSupervision, TestRoot};
///
/// let root = TestRoot::new()?;
/// let fake = FakeProcessSupervision::new(&root);
/// let path = root.path().join("instance.lock");
///
/// let LockOutcome::Acquired(lock) = fake.lock_instance(&path)? else {
///     panic!("a new lock file is free");
/// };
/// assert_eq!(fake.probe_lock(&path)?, LockState::Held);
/// drop(lock);
/// assert_eq!(fake.probe_lock(&path)?, LockState::Free);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub struct FakeProcessSupervision {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
}

struct State {
    roots: [PathBuf; 2],
    user: UserId,
    boot: BootId,
    punch_unit: NonZeroU64,
    next_pid: u32,
    spawn_seq: u64,
    processes: BTreeMap<Pid, FakeProcess>,
    listeners: BTreeMap<Pid, Arc<dyn ExitListener>>,
    pending: BTreeMap<Pid, ExitNotice>,
    released: HashMap<FileIdentity, u64>,
    spawns: Vec<FakeSpawn>,
    stdin: BTreeMap<Pid, SecretValue>,
    calls: Vec<ProcessCall>,
    next_faults: Vec<(ProcessOp, ProcessError)>,
    always_faults: Vec<(ProcessOp, ProcessError)>,
    /// The status the next main process dies with while still suspended.
    die_suspended: Option<ExitStatus>,
    violations: usize,
}

struct FakeProcess {
    facts: ProcessFacts,
    role: Option<ChildRole>,
    state: FakeProcessState,
    /// Open files the process holds, so their locks stay held.
    files: Vec<File>,
    held: Vec<FileIdentity>,
    output: Option<[File; 2]>,
    holds_override: Option<HoldsFile>,
    signals: Vec<ProviderSignal>,
    /// The status of a suspended main process that ended before it was
    /// resumed or discarded, which [`ProcessSupervision::discard`] answers
    /// once.
    died_suspended: Option<ExitStatus>,
}

impl FakeProcess {
    const fn is_alive(&self) -> bool {
        !matches!(self.state, FakeProcessState::Exited)
    }
}

/// An exit notice to deliver once the state lock is released.
type Delivery = Option<(Arc<dyn ExitListener>, ExitNotice)>;

impl FakeProcessSupervision {
    /// Creates a fake with no processes, confined to `root`'s test folder
    /// and run folder. The daemon's user is the owner of the test root.
    #[must_use]
    pub fn new(root: &TestRoot) -> Self {
        let uid = std::fs::metadata(root.path())
            .expect("a test root exists while the test holds it")
            .uid();
        let boot = DEFAULT_BOOT
            .parse()
            .expect("the default fake boot ID is a hyphenated UUID");
        let state = State {
            roots: [root.path().to_path_buf(), root.run_dir().to_path_buf()],
            user: UserId::Unix(uid),
            boot,
            punch_unit: NonZeroU64::new(DEFAULT_PUNCH_UNIT)
                .expect("the default punch unit is not zero"),
            next_pid: FIRST_PID,
            spawn_seq: 0,
            processes: BTreeMap::new(),
            listeners: BTreeMap::new(),
            pending: BTreeMap::new(),
            released: HashMap::new(),
            spawns: Vec::new(),
            stdin: BTreeMap::new(),
            calls: Vec::new(),
            next_faults: Vec::new(),
            always_faults: Vec::new(),
            die_suspended: None,
            violations: 0,
        };
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(state),
            }),
        }
    }

    /// Returns the user the fake daemon runs as: every process it spawns
    /// has this user.
    #[must_use]
    pub fn user(&self) -> UserId {
        self.lock_state().user.clone()
    }

    /// Makes [`boot_id`](ProcessSupervision::boot_id) answer `boot`, as
    /// after a reboot. Processes stay; a test removes them with
    /// [`exit`](Self::exit) when the reboot should end them.
    pub fn set_boot_id(&self, boot: BootId) {
        self.lock_state().boot = boot;
    }

    /// Sets the punch unit of spools opened from now on.
    pub fn set_punch_unit(&self, unit: NonZeroU64) {
        self.lock_state().punch_unit = unit;
    }

    /// Adds a running process the fake did not spawn, with exactly these
    /// facts, for example a process of another user that now holds a
    /// recorded PID, or a descendant left in a main process's group. It
    /// replaces any process with the same PID.
    pub fn insert(&self, facts: ProcessFacts) {
        let state = if facts.stopped {
            FakeProcessState::Suspended
        } else {
            FakeProcessState::Running
        };
        let process = FakeProcess {
            facts,
            role: None,
            state,
            files: Vec::new(),
            held: Vec::new(),
            output: None,
            holds_override: None,
            signals: Vec::new(),
            died_suspended: None,
        };
        let mut guard = self.lock_state();
        guard.pending.remove(&process.facts.pid);
        guard.processes.insert(process.facts.pid, process);
    }

    /// Hands `lock` to a live process, which now holds the lock and the
    /// lock file until it exits, like a process that took or inherited it.
    ///
    /// # Panics
    ///
    /// Panics when no live process has `pid`, or the lock's file cannot be
    /// duplicated.
    pub fn hand_lock(&self, pid: Pid, lock: InstanceLock) {
        let file = lock
            .file()
            .try_clone()
            .expect("a lock file the test holds can be duplicated");
        let mut state = self.lock_state();
        let process = state
            .processes
            .get_mut(&pid)
            .filter(|process| process.is_alive())
            .expect("hand_lock names a live process of the fake");
        process.files.push(file);
        process.held.push(lock.identity());
    }

    /// Changes the facts of a live process, for example its start time or
    /// user, to act out a PID that another process now holds.
    ///
    /// # Panics
    ///
    /// Panics when no live process has `pid`.
    pub fn update_facts(&self, pid: Pid, change: impl FnOnce(&mut ProcessFacts)) {
        let mut state = self.lock_state();
        let process = state
            .processes
            .get_mut(&pid)
            .filter(|process| process.is_alive())
            .expect("update_facts names a live process of the fake");
        change(&mut process.facts);
    }

    /// Makes [`holds_file`](ProcessSupervision::holds_file) answer `answer`
    /// for `pid` whatever file is asked about, for example
    /// [`HoldsFile::Unreadable`].
    pub fn set_holds(&self, pid: Pid, answer: HoldsFile) {
        if let Some(process) = self.lock_state().processes.get_mut(&pid) {
            process.holds_override = Some(answer);
        }
    }

    /// Makes the next main process die with `status` right after it is
    /// spawned, while it is still suspended, as when the OS kills a binary
    /// it rejects during exec (spec 0018, K7). A later
    /// [`discard`](ProcessSupervision::discard) answers `status` once.
    pub fn die_suspended_on_next_spawn(&self, status: ExitStatus) {
        self.lock_state().die_suspended = Some(status);
    }

    /// Ends a process with `status` (use `None` for one that is not the
    /// daemon's child, such as an adopted one). Its open files close, so a
    /// lock only it held is free again, and its watcher, if any, hears of
    /// it at once; otherwise the next [`watch_exit`] does. A process that
    /// is already gone or unknown is left alone.
    ///
    /// Ending a suspended main process acts out a child the OS killed
    /// during exec: a later [`discard`] answers `status` once, without a
    /// signal.
    ///
    /// [`watch_exit`]: ProcessSupervision::watch_exit
    /// [`discard`]: ProcessSupervision::discard
    pub fn exit(&self, pid: Pid, status: Option<ExitStatus>) {
        self.end(pid, status, None);
    }

    /// Ends a helper like [`exit`](Self::exit), with the tail of its
    /// output in the notice.
    pub fn exit_with_output(&self, pid: Pid, status: Option<ExitStatus>, output: CapturedOutput) {
        self.end(pid, status, Some(output));
    }

    /// Appends `bytes` to a main process's spool for `stream`, as if it
    /// wrote them.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::NotFound`] when no live process with spools
    /// has `pid`, and the write error otherwise.
    pub fn write_output(&self, pid: Pid, stream: OutputStream, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.lock_state();
        let output = state
            .processes
            .get_mut(&pid)
            .filter(|process| process.is_alive())
            .and_then(|process| process.output.as_mut())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no live main process"))?;
        let index = match stream {
            OutputStream::Stdout => 0,
            OutputStream::Stderr => 1,
        };
        output[index].write_all(bytes)
    }

    /// Returns where a process is in its life, or `None` for a PID the
    /// fake never knew.
    #[must_use]
    pub fn process_state(&self, pid: Pid) -> Option<FakeProcessState> {
        self.lock_state()
            .processes
            .get(&pid)
            .map(|process| process.state)
    }

    /// Returns the signals a process received, in order.
    #[must_use]
    pub fn signals(&self, pid: Pid) -> Vec<ProviderSignal> {
        self.lock_state()
            .processes
            .get(&pid)
            .map(|process| process.signals.clone())
            .unwrap_or_default()
    }

    /// Returns every spawn that worked, in order.
    #[must_use]
    pub fn spawns(&self) -> Vec<FakeSpawn> {
        self.lock_state().spawns.clone()
    }

    /// Hands out the secret a spawn wrote to the stdin of `pid`, once.
    #[must_use]
    pub fn take_stdin(&self, pid: Pid) -> Option<SecretValue> {
        self.lock_state().stdin.remove(&pid)
    }

    /// Returns every call that reached the fake, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<ProcessCall> {
        self.lock_state().calls.clone()
    }

    /// Returns how many calls of one method reached the fake.
    #[must_use]
    pub fn count(&self, op: ProcessOp) -> usize {
        self.lock_state()
            .calls
            .iter()
            .filter(|call| call.op == op)
            .count()
    }

    /// Makes the next call of `op` fail with `error`, once, after it is
    /// logged and before it has any effect. Several queue up in order.
    pub fn fail_next(&self, op: ProcessOp, error: ProcessError) {
        self.lock_state().next_faults.push((op, error));
    }

    /// Makes every call of `op` fail with `error` until
    /// [`clear_faults`](Self::clear_faults).
    pub fn fail_always(&self, op: ProcessOp, error: ProcessError) {
        self.lock_state().always_faults.push((op, error));
    }

    /// Removes every pending fault.
    pub fn clear_faults(&self) {
        let mut state = self.lock_state();
        state.next_faults.clear();
        state.always_faults.clear();
    }

    /// Returns how many lock or spool calls named a path outside the test
    /// roots.
    #[must_use]
    pub fn violations(&self) -> usize {
        self.lock_state().violations
    }

    fn lock_state(&self) -> MutexGuard<'_, State> {
        self.shared.lock()
    }

    fn end(&self, pid: Pid, status: Option<ExitStatus>, captured: Option<CapturedOutput>) {
        let delivery = {
            let mut state = self.lock_state();
            state.end(pid, status, captured)
        };
        deliver(delivery);
    }

    /// Logs a call, applies a fault, then runs `body`, and logs the error.
    fn call<T>(
        &self,
        mut call: ProcessCall,
        body: impl FnOnce(&mut State) -> Result<T, ProcessError>,
    ) -> Result<T, ProcessError> {
        let mut state = self.lock_state();
        let result = match state.take_fault(call.op) {
            Some(error) => Err(error),
            None => body(&mut state),
        };
        call.error = result.as_ref().err().cloned();
        state.calls.push(call);
        result
    }

    /// Like [`call`](Self::call) for a path, after checking the path is
    /// inside the test roots.
    fn path_call<T>(
        &self,
        op: ProcessOp,
        path: &Path,
        body: impl FnOnce(&mut State) -> Result<T, ProcessError>,
    ) -> Result<T, ProcessError> {
        let call = ProcessCall {
            path: Some(path.to_path_buf()),
            ..ProcessCall::new(op)
        };
        self.call(call, |state| {
            state.confine(path)?;
            body(state)
        })
    }

    fn checked_signal(
        &self,
        op: ProcessOp,
        target: &SignalTarget,
        signal: Option<ProviderSignal>,
    ) -> Result<(), ProcessError> {
        let call = ProcessCall {
            pid: Some(target.pid()),
            signal,
            ..ProcessCall::new(op)
        };
        self.call(call, |state| {
            let user = state.user.clone();
            let process = state.live_mut(target.pid())?;
            let expected = target.expected();
            let facts = &process.facts;
            if facts.start != expected.start
                || facts.uid != user
                || facts.session != expected.session
            {
                return Err(ProcessError::IdentityMismatch { pid: target.pid() });
            }
            match signal {
                Some(signal) => process.signals.push(signal),
                None => {
                    if process.state == FakeProcessState::Suspended {
                        process.state = FakeProcessState::Running;
                        process.facts.stopped = false;
                    }
                }
            }
            Ok(())
        })
    }

    fn spawn(&self, op: ProcessOp, request: SpawnRequest<'_>) -> Result<Pid, ProcessError> {
        let mut state = self.lock_state();
        let result = match state.take_fault(op) {
            Some(error) => Err(error),
            None if op == ProcessOp::SpawnMain => request.check_main(),
            None => request.check_helper(),
        }
        .and_then(|()| state.spawn(request));
        let call = ProcessCall {
            pid: result.as_ref().ok().copied(),
            error: result.as_ref().err().cloned(),
            ..ProcessCall::new(op)
        };
        state.calls.push(call);
        result
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    fn take_fault(&mut self, op: ProcessOp) -> Option<ProcessError> {
        if let Some(index) = self.next_faults.iter().position(|(next, _)| *next == op) {
            return Some(self.next_faults.remove(index).1);
        }
        self.always_faults
            .iter()
            .find(|(always, _)| *always == op)
            .map(|(_, error)| error.clone())
    }

    fn confine(&mut self, path: &Path) -> Result<(), ProcessError> {
        let normal = path.is_absolute()
            && !path
                .components()
                .any(|component| matches!(component, Component::ParentDir));
        if normal && self.roots.iter().any(|root| path.starts_with(root)) {
            return Ok(());
        }
        self.violations += 1;
        Err(ProcessError::Io {
            op: "confine",
            path: Some(path.to_path_buf()),
            kind: io::ErrorKind::PermissionDenied,
            message: "the path is outside the test roots".to_owned(),
        })
    }

    fn live_mut(&mut self, pid: Pid) -> Result<&mut FakeProcess, ProcessError> {
        self.processes
            .get_mut(&pid)
            .filter(|process| process.is_alive())
            .ok_or(ProcessError::NotFound { pid })
    }

    fn next_pid(&mut self) -> Result<Pid, ProcessError> {
        while self.processes.contains_key(&pid_from(self.next_pid)?) {
            self.next_pid += 1;
        }
        let pid = pid_from(self.next_pid)?;
        self.next_pid += 1;
        Ok(pid)
    }

    fn spawn(&mut self, request: SpawnRequest<'_>) -> Result<Pid, ProcessError> {
        let pid = self.next_pid()?;
        let main = request.role == ChildRole::Main;
        self.spawn_seq += 1;
        let seq = self.spawn_seq;
        let micros = u32::try_from(seq % 1_000_000).expect("a remainder of 1000000 fits a u32");
        let start = ProcessStart::new(FIRST_START_SECONDS + seq, micros)
            .expect("the microseconds of a fake start stay under one second");
        let mut process = FakeProcess {
            facts: ProcessFacts {
                pid,
                parent: Pid::new(std::process::id()).ok(),
                group: pid,
                session: pid,
                uid: self.user.clone(),
                start,
                executable: request.program.to_path_buf(),
                stopped: main,
            },
            role: Some(request.role),
            state: if main {
                FakeProcessState::Suspended
            } else {
                FakeProcessState::Running
            },
            files: Vec::new(),
            held: Vec::new(),
            output: None,
            holds_override: None,
            signals: Vec::new(),
            died_suspended: None,
        };
        let lock = match request.lock {
            Some(lock) => {
                let file = lock
                    .file()
                    .try_clone()
                    .map_err(|error| ProcessError::io("dup", Some(lock.path()), &error))?;
                process.files.push(file);
                process.held.push(lock.identity());
                Some(lock.identity())
            }
            None => None,
        };
        let spools = match request.output {
            OutputTarget::Spool { stdout, stderr } => {
                let identities = [stdout.identity(), stderr.identity()];
                process.held.extend(identities);
                process.output = Some([stdout.into_file(), stderr.into_file()]);
                Some(identities)
            }
            OutputTarget::Capture => None,
        };
        let stdin_len = match request.stdin {
            StdinSource::Null => None,
            StdinSource::Bytes(value) => {
                let len = value.len();
                self.stdin.insert(pid, value);
                Some(len)
            }
        };
        self.spawns.push(FakeSpawn {
            pid,
            role: request.role,
            program: request.program.to_path_buf(),
            args: request.args.to_vec(),
            env: request.env.to_vec(),
            cwd: request.cwd.to_path_buf(),
            stdin_len,
            lock,
            spools,
        });
        self.pending.remove(&pid);
        self.processes.insert(pid, process);
        Ok(pid)
    }

    fn end(
        &mut self,
        pid: Pid,
        status: Option<ExitStatus>,
        captured: Option<CapturedOutput>,
    ) -> Delivery {
        let process = self.live_mut(pid).ok()?;
        if process.state == FakeProcessState::Suspended && process.role == Some(ChildRole::Main) {
            process.died_suspended = status;
        }
        process.state = FakeProcessState::Exited;
        process.files.clear();
        process.output = None;
        let notice = ExitNotice {
            pid,
            status,
            captured,
        };
        match self.listeners.remove(&pid) {
            Some(listener) => Some((listener, notice)),
            None => {
                self.pending.insert(pid, notice);
                None
            }
        }
    }
}

/// Turns the next PID number into a [`Pid`], failing like `posix_spawn`
/// with `EAGAIN` (35 on macOS) once the fake runs out of PIDs.
fn pid_from(number: u32) -> Result<Pid, ProcessError> {
    Pid::new(number).map_err(|_| ProcessError::SpawnFailed {
        kind: io::ErrorKind::WouldBlock,
        errno: 35,
    })
}

fn deliver(delivery: Delivery) {
    if let Some((listener, notice)) = delivery {
        listener.exited(notice);
    }
}

fn file_identity(metadata: &std::fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

/// Checks a lock file the way the adapter does after `fstat`: a regular
/// file of the daemon's user.
fn check_lock_file(
    path: &Path,
    metadata: &std::fs::Metadata,
    user: &UserId,
) -> Result<(), ProcessError> {
    let invalid = |reason| ProcessError::LockFileInvalid {
        path: path.to_path_buf(),
        reason,
    };
    if metadata.file_type().is_symlink() {
        return Err(invalid("is a symlink"));
    }
    if !metadata.is_file() {
        return Err(invalid("is not a regular file"));
    }
    if &UserId::Unix(metadata.uid()) != user {
        return Err(invalid("is not owned by the current user"));
    }
    Ok(())
}

/// Reads the metadata of `path` without following a symlink, or `None`
/// when nothing is there.
fn link_metadata(op: &'static str, path: &Path) -> Result<Option<std::fs::Metadata>, ProcessError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProcessError::io(op, Some(path), &error)),
    }
}

impl ProcessSupervision for FakeProcessSupervision {
    fn spawn_main(&self, request: SpawnRequest<'_>) -> Result<SuspendedChild, ProcessError> {
        let pid = self.spawn(ProcessOp::SpawnMain, request)?;
        let mut state = self.lock_state();
        let facts = state
            .processes
            .get(&pid)
            .map(|process| process.facts.clone())
            .expect("a process the fake just spawned is in its table");
        if let Some(status) = state.die_suspended.take() {
            state.end(pid, Some(status), None);
        }
        drop(state);
        Ok(SuspendedChild::new(facts))
    }

    fn resume(&self, child: SuspendedChild) -> Result<MainHandle, ProcessError> {
        let pid = child.pid();
        let call = ProcessCall {
            pid: Some(pid),
            ..ProcessCall::new(ProcessOp::Resume)
        };
        self.call(call, |state| {
            let process = state.live_mut(pid)?;
            if process.state != FakeProcessState::Suspended || process.role != Some(ChildRole::Main)
            {
                return Err(ProcessError::NotFound { pid });
            }
            process.state = FakeProcessState::Running;
            process.facts.stopped = false;
            Ok(MainHandle::new(pid))
        })
    }

    fn discard(&self, child: SuspendedChild) -> Result<Option<ExitStatus>, ProcessError> {
        let pid = child.pid();
        let call = ProcessCall {
            pid: Some(pid),
            ..ProcessCall::new(ProcessOp::Discard)
        };
        let mut delivery = None;
        let result = self.call(call, |state| {
            let process = state
                .processes
                .get_mut(&pid)
                .filter(|process| process.role == Some(ChildRole::Main))
                .ok_or(ProcessError::NotFound { pid })?;
            let status = match process.state {
                FakeProcessState::Suspended => {
                    delivery = state.end(pid, Some(ExitStatus::Signaled(KILL_SIGNAL_NUMBER)), None);
                    None
                }
                FakeProcessState::Exited => Some(
                    process
                        .died_suspended
                        .take()
                        .ok_or(ProcessError::NotFound { pid })?,
                ),
                FakeProcessState::Running => return Err(ProcessError::NotFound { pid }),
            };
            // The adapter reaps a discarded child itself: no later watch
            // hears of it.
            state.pending.remove(&pid);
            Ok(status)
        });
        deliver(delivery);
        result
    }

    fn spawn_helper(&self, request: SpawnRequest<'_>) -> Result<HelperHandle, ProcessError> {
        let role = request.role;
        let pid = self.spawn(ProcessOp::SpawnHelper, request)?;
        Ok(HelperHandle::new(pid, role))
    }

    fn inspect(&self, pid: Pid) -> Result<Option<ProcessFacts>, ProcessError> {
        let call = ProcessCall {
            pid: Some(pid),
            ..ProcessCall::new(ProcessOp::Inspect)
        };
        self.call(call, |state| {
            Ok(state
                .processes
                .get(&pid)
                .filter(|process| process.is_alive())
                .map(|process| process.facts.clone()))
        })
    }

    fn holds_file(&self, pid: Pid, file: FileIdentity) -> Result<HoldsFile, ProcessError> {
        let call = ProcessCall {
            pid: Some(pid),
            ..ProcessCall::new(ProcessOp::HoldsFile)
        };
        self.call(call, |state| {
            let user = state.user.clone();
            let process = state.live_mut(pid)?;
            Ok(if let Some(answer) = process.holds_override {
                answer
            } else if process.facts.uid != user {
                HoldsFile::Unreadable
            } else if process.held.contains(&file) {
                HoldsFile::Yes
            } else {
                HoldsFile::No
            })
        })
    }

    fn signal(&self, target: &SignalTarget, signal: ProviderSignal) -> Result<(), ProcessError> {
        self.checked_signal(ProcessOp::Signal, target, Some(signal))
    }

    fn continue_stopped(&self, target: &SignalTarget) -> Result<(), ProcessError> {
        self.checked_signal(ProcessOp::ContinueStopped, target, None)
    }

    fn terminate_helper_group(
        &self,
        helper: &HelperHandle,
        force: bool,
    ) -> Result<(), ProcessError> {
        let pid = helper.pid();
        let call = ProcessCall {
            pid: Some(pid),
            force: Some(force),
            ..ProcessCall::new(ProcessOp::TerminateHelperGroup)
        };
        self.call(call, |state| {
            let process = state.live_mut(pid)?;
            match process.role {
                Some(role) if role != ChildRole::Main => Ok(()),
                _ => Err(ProcessError::NotFound { pid }),
            }
        })
    }

    fn group_members(&self, group: Pid) -> Result<Vec<Pid>, ProcessError> {
        let call = ProcessCall {
            pid: Some(group),
            ..ProcessCall::new(ProcessOp::GroupMembers)
        };
        self.call(call, |state| {
            Ok(state
                .processes
                .values()
                .filter(|process| process.is_alive() && process.facts.group == group)
                .map(|process| process.facts.pid)
                .collect())
        })
    }

    fn boot_id(&self) -> Result<BootId, ProcessError> {
        self.call(ProcessCall::new(ProcessOp::BootId), |state| Ok(state.boot))
    }

    fn watch_exit(&self, pid: Pid, listener: Arc<dyn ExitListener>) -> Result<(), ProcessError> {
        let call = ProcessCall {
            pid: Some(pid),
            ..ProcessCall::new(ProcessOp::WatchExit)
        };
        let mut delivery = None;
        let result = self.call(call, |state| {
            if state.live_mut(pid).is_ok() {
                state.listeners.insert(pid, listener);
            } else {
                let notice = state.pending.remove(&pid).unwrap_or(ExitNotice {
                    pid,
                    status: None,
                    captured: None,
                });
                delivery = Some((listener, notice));
            }
            Ok(())
        });
        deliver(delivery);
        result
    }

    fn lock_instance(&self, path: &Path) -> Result<LockOutcome, ProcessError> {
        self.path_call(ProcessOp::LockInstance, path, |state| {
            if let Some(metadata) = link_metadata("lock_instance", path)? {
                check_lock_file(path, &metadata, &state.user)?;
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .map_err(|error| ProcessError::io("lock_instance", Some(path), &error))?;
            let metadata = file
                .metadata()
                .map_err(|error| ProcessError::io("lock_instance", Some(path), &error))?;
            check_lock_file(path, &metadata, &state.user)?;
            match file.try_lock() {
                Ok(()) => Ok(LockOutcome::Acquired(InstanceLock::new(
                    file,
                    path.to_path_buf(),
                    file_identity(&metadata),
                ))),
                Err(TryLockError::WouldBlock) => Ok(LockOutcome::Held),
                Err(TryLockError::Error(error)) => {
                    Err(ProcessError::io("flock", Some(path), &error))
                }
            }
        })
    }

    fn probe_lock(&self, path: &Path) -> Result<LockState, ProcessError> {
        self.path_call(ProcessOp::ProbeLock, path, |state| {
            let Some(metadata) = link_metadata("probe_lock", path)? else {
                return Ok(LockState::Free);
            };
            check_lock_file(path, &metadata, &state.user)?;
            let file = match OpenOptions::new().read(true).write(true).open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(LockState::Free);
                }
                Err(error) => return Err(ProcessError::io("probe_lock", Some(path), &error)),
            };
            match file.try_lock() {
                Ok(()) => Ok(LockState::Free),
                Err(TryLockError::WouldBlock) => Ok(LockState::Held),
                Err(TryLockError::Error(error)) => {
                    Err(ProcessError::io("flock", Some(path), &error))
                }
            }
        })
    }

    fn create_spool(&self, path: &Path) -> Result<SpoolWriter, ProcessError> {
        self.path_call(ProcessOp::CreateSpool, path, |_| {
            let io_error = |error: io::Error| ProcessError::io("create_spool", Some(path), &error);
            let file = OpenOptions::new()
                .append(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(io_error)?;
            let metadata = file.metadata().map_err(io_error)?;
            Ok(SpoolWriter::new(
                file,
                path.to_path_buf(),
                file_identity(&metadata),
            ))
        })
    }

    fn open_spool(&self, path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError> {
        let shared = Arc::clone(&self.shared);
        self.path_call(ProcessOp::OpenSpool, path, |state| {
            let io_error = |error: io::Error| ProcessError::io("open_spool", Some(path), &error);
            let Some(metadata) = link_metadata("open_spool", path)? else {
                return Ok(None);
            };
            if !metadata.is_file() || UserId::Unix(metadata.uid()) != state.user {
                return Err(ProcessError::Io {
                    op: "open_spool",
                    path: Some(path.to_path_buf()),
                    kind: io::ErrorKind::InvalidInput,
                    message: "the spool is not a regular file of the current user".to_owned(),
                });
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(io_error)?;
            let metadata = file.metadata().map_err(io_error)?;
            let identity = file_identity(&metadata);
            let released = state.released.get(&identity).copied().unwrap_or(0);
            let len = metadata.len();
            let first_data = (released < len).then_some(released);
            let reader = FakeSpoolReader {
                file,
                path: path.to_path_buf(),
                identity,
                watermark: SpoolWatermark::new(state.punch_unit, first_data, len),
                shared,
            };
            Ok(Some(Box::new(reader) as Box<dyn SpoolReader>))
        })
    }
}

impl fmt::Debug for FakeProcessSupervision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock_state();
        f.debug_struct("FakeProcessSupervision")
            .field("processes", &state.processes.len())
            .field("calls", &state.calls.len())
            .field("boot", &state.boot)
            .finish_non_exhaustive()
    }
}

/// A spool reader over a real file, which records how far the spool was
/// freed in the fake instead of punching holes.
struct FakeSpoolReader {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
    watermark: SpoolWatermark,
    shared: Arc<Shared>,
}

impl FakeSpoolReader {
    fn io_error(&self, op: &'static str, error: &io::Error) -> ProcessError {
        ProcessError::io(op, Some(&self.path), error)
    }

    fn released(&self) -> u64 {
        self.shared
            .lock()
            .released
            .get(&self.identity)
            .copied()
            .unwrap_or(0)
    }
}

impl SpoolReader for FakeSpoolReader {
    fn len(&self) -> Result<u64, ProcessError> {
        self.file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|error| self.io_error("fstat", &error))
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, ProcessError> {
        let read = self
            .file
            .read_at(buf, offset)
            .map_err(|error| self.io_error("pread", &error))?;
        let freed = self.released().saturating_sub(offset);
        let freed = usize::try_from(freed).unwrap_or(usize::MAX).min(read);
        buf[..freed].fill(0);
        Ok(read)
    }

    fn first_data_offset(&self) -> Result<Option<u64>, ProcessError> {
        let len = self.len()?;
        let released = self.released();
        Ok((released < len).then_some(released))
    }

    fn punch_unit(&self) -> NonZeroU64 {
        self.watermark.unit()
    }

    fn release_below(&mut self, offset: u64) -> Result<(), ProcessError> {
        let len = self.len()?;
        let mut state = self.shared.lock();
        let call = ProcessCall {
            path: Some(self.path.clone()),
            ..ProcessCall::new(ProcessOp::ReleaseBelow)
        };
        let result = match state.take_fault(ProcessOp::ReleaseBelow) {
            Some(error) => Err(error),
            None => {
                if let Some(range) = self.watermark.next_release(offset, len) {
                    let released = state.released.entry(self.identity).or_insert(0);
                    *released = (*released).max(range.end);
                    self.watermark.advance(range.end);
                }
                Ok(())
            }
        };
        state.calls.push(ProcessCall {
            error: result.as_ref().err().cloned(),
            ..call
        });
        result
    }

    fn file_identity(&self) -> FileIdentity {
        self.identity
    }
}
