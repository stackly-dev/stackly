//! [`MacosProcessSupervision`], the macOS process supervision port (spec
//! 0010, *Adapter macOS*).
//!
//! | Module | Job |
//! | --- | --- |
//! | `spawn` | the one `posix_spawn`, `pub(crate)` for the codesign helper |
//! | `stdin` | the writer of a helper's stdin pipe, the only secret reader |
//! | `inspect` | facts, the `kinfo_proc` fallback, fds, and group members |
//! | `kill` | the only file that names the kill signal |
//! | `watch` | the kqueue watcher, reaping, and the mutex per PID |
//! | `lock` | the instance lock |
//! | `spool` | log spools and their reader |
//! | `boot` | the boot ID |
//! | `sys` | declarations libc lacks, with layout asserts |

mod boot;
mod inspect;
pub(crate) mod kill;
mod lock;
pub(crate) mod spawn;
mod spool;
mod stdin;
mod sys;
mod watch;

use std::fmt;
use std::os::fd::AsFd;
use std::path::Path;
use std::sync::Arc;

use stackly_domain::{BootId, Pid};
use stackly_platform::{
    ExitListener, ExitStatus, FileIdentity, HelperHandle, HoldsFile, LockOutcome, LockState,
    MainHandle, OutputTarget, ProcessError, ProcessFacts, ProcessSupervision, ProviderSignal,
    RequestPart, SignalTarget, SpawnRequest, SpoolReader, SpoolWriter, StdinSource, SuspendedChild,
    UserId,
};

use crate::identity::current_uid;

pub use inspect::KinfoFacts;

/// Starts, watches, and signals the processes of instances on macOS (spec
/// 0010).
///
/// Every child leads its own session with default signals and only fds 0
/// to 3; a main process starts suspended, so its identity is recorded
/// before it runs. One watcher thread reaps the adapter's own children by
/// PID. A signal to a main or adopted process reads the facts again right
/// before `kill(2)`, under the PID's mutex, and is refused when the start
/// time, user, or session changed. Facts of another user's process come
/// from `kinfo_proc`, so a reused PID is proven foreign.
///
/// Drop it only when the daemon stops: the watcher thread ends then, and
/// the processes keep running.
pub struct MacosProcessSupervision {
    watcher: watch::Watcher,
}

impl MacosProcessSupervision {
    /// Creates the adapter. The watcher thread starts with the first spawn
    /// or watch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            watcher: watch::Watcher::new(),
        }
    }

    /// Lists the open descriptor numbers of `pid`, ascending. Only the AC-1
    /// integration test uses it, to prove that a suspended main process
    /// holds exactly fds 0 to 3.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] for a PID that is gone,
    /// [`ProcessError::PermissionDenied`] for a process of another user,
    /// and [`ProcessError::Io`] otherwise.
    #[doc(hidden)]
    pub fn fd_numbers(&self, pid: Pid) -> Result<Vec<i32>, ProcessError> {
        inspect::fd_numbers(pid)
    }

    /// Reads `kinfo_proc`, the source of the `inspect` fallback for another
    /// user's process, or `None` for a PID the kernel no longer knows. Only
    /// the integration test uses it, to prove that the fallback reads the
    /// same values as `PROC_PIDTBSDINFO` for its own child.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when `sysctl` fails or answers a size
    /// that is not the struct's.
    #[doc(hidden)]
    pub fn inspect_kinfo(&self, pid: Pid) -> Result<Option<KinfoFacts>, ProcessError> {
        inspect::kinfo(pid)
    }

    /// Reads the facts of `target` again and signals it only when its start
    /// time, user, and session are still the ones expected, under the
    /// PID's mutex so no reap can free the PID in between.
    fn checked_signal(
        &self,
        target: &SignalTarget,
        signal: libc::c_int,
    ) -> Result<(), ProcessError> {
        let pid = target.pid();
        self.watcher.guarded(pid, || {
            let facts = inspect::facts(pid)?.ok_or(ProcessError::NotFound { pid })?;
            let expected = target.expected();
            if facts.start != expected.start
                || facts.uid != UserId::Unix(current_uid())
                || facts.session != expected.session
            {
                return Err(ProcessError::IdentityMismatch { pid });
            }
            watch::send(pid, signal)
        })
    }
}

impl Default for MacosProcessSupervision {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MacosProcessSupervision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MacosProcessSupervision")
            .field("watching", &self.watcher.running().is_some())
            .finish_non_exhaustive()
    }
}

impl ProcessSupervision for MacosProcessSupervision {
    fn spawn_main(&self, request: SpawnRequest<'_>) -> Result<SuspendedChild, ProcessError> {
        request.check_main()?;
        let SpawnRequest {
            program,
            args,
            env,
            cwd,
            output,
            lock,
            ..
        } = request;
        let OutputTarget::Spool { stdout, stderr } = output else {
            return Err(ProcessError::InvalidArgument {
                position: RequestPart::Output,
            });
        };
        let watcher = self.watcher.start()?;
        let pid = spawn::launch(&spawn::Launch {
            program,
            args,
            env,
            cwd,
            stdin: None,
            stdout: stdout.file().as_fd(),
            stderr: stderr.file().as_fd(),
            lock: lock.map(|lock| lock.file().as_fd()),
            suspended: true,
        })?;
        // The child holds the spools now; the daemon reads them through
        // `open_spool`.
        drop((stdout, stderr));
        let facts = match inspect::spawned_facts(pid, program) {
            Ok(facts) => facts,
            Err(error) => {
                watch::abandon_unrun(pid);
                return Err(error);
            }
        };
        if let Err(error) = watcher.register_main(pid) {
            watch::abandon_unrun(pid);
            return Err(error);
        }
        Ok(SuspendedChild::new(facts))
    }

    fn resume(&self, child: SuspendedChild) -> Result<MainHandle, ProcessError> {
        let pid = child.pid();
        let watcher = self
            .watcher
            .running()
            .ok_or(ProcessError::NotFound { pid })?;
        watcher.resume(pid)?;
        Ok(MainHandle::new(pid))
    }

    fn discard(&self, child: SuspendedChild) -> Result<Option<ExitStatus>, ProcessError> {
        let pid = child.pid();
        self.watcher
            .running()
            .ok_or(ProcessError::NotFound { pid })?
            .discard(pid)
    }

    fn spawn_helper(&self, request: SpawnRequest<'_>) -> Result<HelperHandle, ProcessError> {
        request.check_helper()?;
        let SpawnRequest {
            program,
            args,
            env,
            cwd,
            stdin,
            lock,
            role,
            ..
        } = request;
        let watcher = self.watcher.start()?;
        let stdin = match stdin {
            StdinSource::Null => None,
            StdinSource::Bytes(value) => {
                let (read, write) = spawn::pipe()?;
                stdin::feed(write, value)?;
                Some(read)
            }
        };
        let (stdout_read, stdout_write) = spawn::pipe()?;
        let (stderr_read, stderr_write) = spawn::pipe()?;
        let pid = spawn::launch(&spawn::Launch {
            program,
            args,
            env,
            cwd,
            stdin: stdin.as_ref().map(AsFd::as_fd),
            stdout: stdout_write.as_fd(),
            stderr: stderr_write.as_fd(),
            lock: lock.map(|lock| lock.file().as_fd()),
            suspended: false,
        })?;
        // Only the child keeps its ends, so the pipes close when it and its
        // descendants are done.
        drop((stdin, stdout_write, stderr_write));
        watcher.register_helper(pid, stdout_read, stderr_read)?;
        Ok(HelperHandle::new(pid, role))
    }

    fn inspect(&self, pid: Pid) -> Result<Option<ProcessFacts>, ProcessError> {
        inspect::facts(pid)
    }

    fn holds_file(&self, pid: Pid, file: FileIdentity) -> Result<HoldsFile, ProcessError> {
        inspect::holds_file(pid, file)
    }

    fn signal(&self, target: &SignalTarget, signal: ProviderSignal) -> Result<(), ProcessError> {
        let number = match signal {
            ProviderSignal::Term => libc::SIGTERM,
            ProviderSignal::Int => libc::SIGINT,
            ProviderSignal::Quit => libc::SIGQUIT,
            ProviderSignal::Hup => libc::SIGHUP,
        };
        self.checked_signal(target, number)
    }

    fn continue_stopped(&self, target: &SignalTarget) -> Result<(), ProcessError> {
        self.checked_signal(target, libc::SIGCONT)
    }

    fn terminate_helper_group(
        &self,
        helper: &HelperHandle,
        force: bool,
    ) -> Result<(), ProcessError> {
        let pid = helper.pid();
        self.watcher
            .running()
            .ok_or(ProcessError::NotFound { pid })?
            .terminate_group(pid, force)
    }

    fn group_members(&self, group: Pid) -> Result<Vec<Pid>, ProcessError> {
        inspect::group_members(group)
    }

    fn boot_id(&self) -> Result<BootId, ProcessError> {
        boot::boot_id()
    }

    fn watch_exit(&self, pid: Pid, listener: Arc<dyn ExitListener>) -> Result<(), ProcessError> {
        self.watcher.start()?.watch(pid, listener)
    }

    fn lock_instance(&self, path: &Path) -> Result<LockOutcome, ProcessError> {
        lock::lock_instance(path)
    }

    fn probe_lock(&self, path: &Path) -> Result<LockState, ProcessError> {
        lock::probe_lock(path)
    }

    fn create_spool(&self, path: &Path) -> Result<SpoolWriter, ProcessError> {
        spool::create_spool(path)
    }

    fn open_spool(&self, path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError> {
        spool::open_spool(path)
    }
}
