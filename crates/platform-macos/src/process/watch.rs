//! Exits, reaping, and the per PID lock (spec 0010, *Adapter macOS*, rows
//! Exit and Sinyal dan reap).
//!
//! One thread, `stackly-proc-watch`, waits on a kqueue for `NOTE_EXIT` of
//! every child of this adapter and of every process a caller watches. It
//! reaps only the PIDs of its own children, with `waitpid(pid, WNOHANG)`,
//! never `waitpid(-1)`, and the daemon installs no `SIGCHLD` handler.
//!
//! Every tracked PID has a [`Slot`]. Reaping, a signal, `SIGCONT`, a
//! discard, and a helper group termination each run under the slot's
//! mutex, and a slot whose process was reaped refuses them all. A PID is
//! only free for reuse once its child is reaped, so a signal sent under
//! the slot can never reach another process.
//!
//! A child is watched from its spawn on, so it is always reaped, whether
//! or not anyone asked for its exit; the notice waits in the slot until
//! [`Watcher::watch`] hands it to a listener. Listeners run on the watcher
//! thread, or on the caller's thread when the process had already exited.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use stackly_domain::Pid;
use stackly_platform::{
    CapturedOutput, ExitListener, ExitNotice, ExitStatus, HELPER_CAPTURE_MAX, ProcessError,
};

use super::spawn::last_error;
use super::{inspect, kill};

/// The kqueue user event that stops the watcher thread. A PID is never 0,
/// so it never clashes with the per helper wake ups.
const STOP_IDENT: usize = 0;
/// How many events one `kevent` call takes.
const EVENT_BATCH: usize = 32;
/// How long one `poll` of a helper's output pipes waits.
const CAPTURE_TICK: Duration = Duration::from_millis(50);
/// How long a helper's output is still read after the helper exited, when
/// a descendant keeps the pipes open.
const CAPTURE_GRACE: Duration = Duration::from_millis(200);

/// An exit notice to deliver once every lock is released.
type Delivery = Option<(Arc<dyn ExitListener>, ExitNotice)>;

/// The watcher of one adapter. The kqueue and the thread start on first
/// use, and the thread stops when the watcher drops; the processes keep
/// running.
pub(super) struct Watcher {
    shared: Mutex<Option<Arc<Shared>>>,
}

impl Watcher {
    pub(super) const fn new() -> Self {
        Self {
            shared: Mutex::new(None),
        }
    }

    /// Returns the running watcher, starting it first if needed.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the kqueue or the thread cannot
    /// be created.
    pub(super) fn start(&self) -> Result<Arc<Shared>, ProcessError> {
        let mut guard = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(shared) = guard.as_ref() {
            return Ok(Arc::clone(shared));
        }
        // SAFETY: kqueue takes no arguments and returns a new descriptor
        // or -1.
        let kq = unsafe { libc::kqueue() };
        if kq == -1 {
            return Err(last_error("kqueue"));
        }
        let shared = Arc::new(Shared {
            // SAFETY: kqueue returned a new descriptor that nothing else
            // owns.
            kq: unsafe { OwnedFd::from_raw_fd(kq) },
            slots: Mutex::new(HashMap::new()),
            tokens: AtomicUsize::new(1),
        });
        let thread_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("stackly-proc-watch".to_owned())
            .spawn(move || thread_shared.run())
            .map_err(|error| ProcessError::io("spawn the process watcher", None, &error))?;
        *guard = Some(Arc::clone(&shared));
        Ok(shared)
    }

    /// Returns the running watcher, or `None` when nothing started it, so
    /// no PID is tracked.
    pub(super) fn running(&self) -> Option<Arc<Shared>> {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Runs `body` so that no reap of `pid` can happen meanwhile, under its
    /// slot when one is tracked.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when the watcher saw the tracked
    /// process end, and the error of `body` otherwise.
    pub(super) fn guarded<T>(
        &self,
        pid: Pid,
        body: impl FnOnce() -> Result<T, ProcessError>,
    ) -> Result<T, ProcessError> {
        let Some(slot) = self.running().and_then(|shared| shared.slot(pid)) else {
            return body();
        };
        let state = slot.lock();
        if state.removed || state.exited || state.reaped {
            return Err(ProcessError::NotFound { pid });
        }
        let result = body();
        drop(state);
        result
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let shared = self
            .shared
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(shared) = shared {
            // Nothing is left to tell when even the stop event fails: the
            // thread then lives on with the process.
            let _ = shared.wake(STOP_IDENT);
        }
    }
}

/// What the watcher thread and the adapter's methods share.
pub(super) struct Shared {
    kq: OwnedFd,
    slots: Mutex<HashMap<Pid, Arc<Slot>>>,
    /// The next slot token. The kqueue hands a slot's token back with its
    /// `NOTE_EXIT`, so an event left over from a process whose PID was
    /// reused never touches the new process's slot.
    tokens: AtomicUsize,
}

/// Which process a slot tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A main process this adapter spawned suspended.
    Main,
    /// A helper this adapter spawned, whose output it captures.
    Helper,
    /// A process a caller watches that is not a child, such as an adopted
    /// one. It is never reaped here, and its status is never known.
    Watched,
}

impl Kind {
    const fn is_child(self) -> bool {
        matches!(self, Self::Main | Self::Helper)
    }
}

/// Where the capture of a helper's output stands.
enum Capture {
    /// Not a helper.
    None,
    /// The capture thread still reads; it stops soon after this flag is
    /// set, once the helper exited.
    Running(Arc<AtomicBool>),
    /// The tails of both streams.
    Done(CapturedOutput),
}

/// The state of one tracked PID, behind its mutex.
pub(super) struct Slot {
    token: usize,
    state: Mutex<SlotState>,
}

struct SlotState {
    kind: Kind,
    /// The watcher saw the exit.
    exited: bool,
    /// The child was reaped, so the PID may already belong to another
    /// process.
    reaped: bool,
    status: Option<ExitStatus>,
    /// A discard took the child: nobody waits for its notice unless a
    /// listener already does.
    discarded: bool,
    /// The notice went out and the slot left the table.
    removed: bool,
    listener: Option<Arc<dyn ExitListener>>,
    capture: Capture,
}

impl Slot {
    fn new(token: usize, kind: Kind, capture: Capture) -> Arc<Self> {
        Arc::new(Self {
            token,
            state: Mutex::new(SlotState {
                kind,
                exited: false,
                reaped: false,
                status: None,
                discarded: false,
                removed: false,
                listener: None,
                capture,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, SlotState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Shared {
    /// Tracks a main process right after its spawn, reaping it at once
    /// when it is already a zombie.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the kqueue refuses the watch; the
    /// caller then gets rid of the child itself.
    pub(super) fn register_main(&self, pid: Pid) -> Result<(), ProcessError> {
        self.register_child(pid, Slot::new(self.token(), Kind::Main, Capture::None))
    }

    /// Tracks a helper right after its spawn and starts reading its
    /// output. When either fails, the helper's group is killed and
    /// reaped.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the kqueue refuses the watch or
    /// the capture thread cannot start.
    pub(super) fn register_helper(
        self: &Arc<Self>,
        pid: Pid,
        stdout: OwnedFd,
        stderr: OwnedFd,
    ) -> Result<(), ProcessError> {
        let exited = Arc::new(AtomicBool::new(false));
        let slot = Slot::new(
            self.token(),
            Kind::Helper,
            Capture::Running(Arc::clone(&exited)),
        );
        if let Err(error) = self.register_child(pid, Arc::clone(&slot)) {
            // Not tracked, so nothing else reaps it.
            let _ = kill::signal_group(pid, true);
            reap_blocking(pid);
            return Err(error);
        }
        let shared = Arc::clone(self);
        let job = CaptureJob {
            streams: [File::from(stdout), File::from(stderr)],
            exited,
        };
        let started = thread::Builder::new()
            .name("stackly-proc-capture".to_owned())
            .spawn(move || {
                let output = job.run();
                shared.capture_done(pid, output);
            });
        if let Err(error) = started {
            let delivery = {
                let mut state = slot.lock();
                state.capture = Capture::Done(CapturedOutput::default());
                state.discarded = true;
                // The slot still blocks reaping, so the group is the
                // helper's.
                let _ = kill::signal_group(pid, true);
                self.settle(pid, &slot, &mut state)
            };
            deliver(delivery);
            return Err(ProcessError::io("spawn the output capture", None, &error));
        }
        Ok(())
    }

    fn register_child(&self, pid: Pid, slot: Arc<Slot>) -> Result<(), ProcessError> {
        self.insert(pid, Arc::clone(&slot));
        match self.watch_proc(pid, slot.token, true) {
            Ok(()) => Ok(()),
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {
                // Already a zombie: the kqueue refuses it, so reap it now.
                self.exited(pid, slot.token);
                Ok(())
            }
            Err(error) => {
                self.remove(pid, &slot);
                Err(ProcessError::io("kevent EVFILT_PROC", None, &error))
            }
        }
    }

    /// Hands the exit of `pid` to `listener`: at once when it already
    /// exited, else when it does.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the kqueue refuses the watch.
    pub(super) fn watch(
        &self,
        pid: Pid,
        listener: Arc<dyn ExitListener>,
    ) -> Result<(), ProcessError> {
        if let Some(slot) = self.slot(pid) {
            let mut state = slot.lock();
            if !state.removed {
                state.listener = Some(Arc::clone(&listener));
                let delivery = self.settle(pid, &slot, &mut state);
                drop(state);
                deliver(delivery);
                return Ok(());
            }
        }
        let slot = Slot::new(self.token(), Kind::Watched, Capture::None);
        slot.lock().listener = Some(listener);
        self.insert(pid, Arc::clone(&slot));
        match self.watch_proc(pid, slot.token, false) {
            Ok(()) => Ok(()),
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {
                self.exited(pid, slot.token);
                Ok(())
            }
            Err(error) => {
                self.remove(pid, &slot);
                Err(ProcessError::io("kevent EVFILT_PROC", None, &error))
            }
        }
    }

    /// Gets rid of a suspended main process: a child that already died
    /// answers its status; a live one is killed, reaped, and answers
    /// `None`. A listener that already waits hears of the exit at once;
    /// then the PID is forgotten, so a later watch knows nothing of it.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when `pid` is not a main process
    /// of this adapter whose status is still known.
    pub(super) fn discard(&self, pid: Pid) -> Result<Option<ExitStatus>, ProcessError> {
        let not_found = ProcessError::NotFound { pid };
        let slot = self.slot(pid).ok_or_else(|| not_found.clone())?;
        let mut state = slot.lock();
        if state.removed || state.discarded || state.kind != Kind::Main {
            return Err(not_found);
        }
        let result = if state.reaped {
            state.status.map(Some).ok_or(not_found)
        } else {
            match try_reap(pid) {
                Reap::Exited(status) => {
                    state.reaped = true;
                    state.status = Some(status);
                    Ok(Some(status))
                }
                Reap::Running if inspect::exiting(pid) => {
                    // It died on its own a moment ago; the zombie is not
                    // waitable yet, so wait for it instead of killing.
                    state.status = reap_blocking(pid);
                    state.reaped = true;
                    state.status.map(Some).ok_or(not_found)
                }
                Reap::Running => {
                    // The slot keeps the child unreaped, so the PID is
                    // still this child's.
                    let _ = kill::kill_unrun(pid);
                    state.status = reap_blocking(pid);
                    state.reaped = true;
                    Ok(None)
                }
                Reap::NoChild => {
                    state.reaped = true;
                    Err(not_found)
                }
            }
        };
        // The child is reaped either way, so its exit is known now; the
        // kqueue event that may still come finds no slot.
        state.exited = true;
        state.discarded = true;
        let delivery = self.settle(pid, &slot, &mut state);
        drop(state);
        deliver(delivery);
        result
    }

    /// Sends `SIGCONT` to a suspended main process of this adapter.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when it is not one, or it was
    /// reaped.
    pub(super) fn resume(&self, pid: Pid) -> Result<(), ProcessError> {
        let slot = self.slot(pid).ok_or(ProcessError::NotFound { pid })?;
        let state = slot.lock();
        if state.removed || state.reaped || state.kind != Kind::Main {
            return Err(ProcessError::NotFound { pid });
        }
        let result = send(pid, libc::SIGCONT);
        drop(state);
        result
    }

    /// Sends `SIGTERM`, or a kill when `force` is true, to the group of a
    /// helper of this adapter that is not reaped yet.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::NotFound`] when it is not one, or it was
    /// reaped.
    pub(super) fn terminate_group(&self, pid: Pid, force: bool) -> Result<(), ProcessError> {
        let slot = self.slot(pid).ok_or(ProcessError::NotFound { pid })?;
        let state = slot.lock();
        if state.removed || state.reaped || state.kind != Kind::Helper {
            return Err(ProcessError::NotFound { pid });
        }
        let result = match kill::signal_group(pid, force) {
            Ok(()) => Ok(()),
            // Only the unreaped leader is left, as a zombie.
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
                Err(ProcessError::PermissionDenied { pid })
            }
            Err(error) => Err(ProcessError::io("kill", None, &error)),
        };
        drop(state);
        result
    }

    fn slot(&self, pid: Pid) -> Option<Arc<Slot>> {
        self.table().get(&pid).cloned()
    }

    fn insert(&self, pid: Pid, slot: Arc<Slot>) {
        self.table().insert(pid, slot);
    }

    /// Removes `slot` from the table, unless a newer slot took its PID.
    fn remove(&self, pid: Pid, slot: &Arc<Slot>) {
        let mut table = self.table();
        if table
            .get(&pid)
            .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            table.remove(&pid);
        }
    }

    fn table(&self) -> MutexGuard<'_, HashMap<Pid, Arc<Slot>>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn token(&self) -> usize {
        self.tokens.fetch_add(1, Ordering::Relaxed)
    }

    /// Registers `NOTE_EXIT` for `pid`, with `NOTE_EXITSTATUS` for a child,
    /// carrying the slot's token.
    fn watch_proc(&self, pid: Pid, token: usize, child: bool) -> io::Result<()> {
        let mut fflags = libc::NOTE_EXIT;
        if child {
            fflags |= libc::NOTE_EXITSTATUS;
        }
        let mut change = event(ident(pid), libc::EVFILT_PROC, libc::EV_ADD, fflags);
        change.udata = ptr::without_provenance_mut(token);
        self.submit(&[change])
    }

    /// Wakes the watcher thread with the user event `ident`.
    fn wake(&self, ident: usize) -> io::Result<()> {
        let add = event(ident, libc::EVFILT_USER, libc::EV_ADD | libc::EV_ONESHOT, 0);
        let trigger = event(ident, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER);
        self.submit(&[add, trigger])
    }

    fn submit(&self, changes: &[libc::kevent]) -> io::Result<()> {
        let count = libc::c_int::try_from(changes.len()).unwrap_or(libc::c_int::MAX);
        // SAFETY: the change list holds `count` initialized events, and no
        // event list is passed, so the kernel writes nothing.
        let result = unsafe {
            libc::kevent(
                self.kq.as_raw_fd(),
                changes.as_ptr(),
                count,
                ptr::null_mut(),
                0,
                ptr::null(),
            )
        };
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// The watcher thread: waits for exits and capture wake ups until the
    /// stop event.
    fn run(&self) {
        let mut events = [event(0, 0, 0, 0); EVENT_BATCH];
        let capacity = libc::c_int::try_from(EVENT_BATCH).unwrap_or(1);
        loop {
            // SAFETY: the event list is writable for `capacity` events, and
            // no change list is passed.
            let received = unsafe {
                libc::kevent(
                    self.kq.as_raw_fd(),
                    ptr::null(),
                    0,
                    events.as_mut_ptr(),
                    capacity,
                    ptr::null(),
                )
            };
            let Ok(received) = usize::try_from(received) else {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                // A kqueue that fails for another reason cannot recover.
                return;
            };
            for event in &events[..received] {
                let Some(pid) = pid_of(event.ident) else {
                    if event.filter == libc::EVFILT_USER && event.ident == STOP_IDENT {
                        return;
                    }
                    continue;
                };
                match event.filter {
                    libc::EVFILT_PROC => self.exited(pid, event.udata.addr()),
                    libc::EVFILT_USER => self.deliver_ready(pid),
                    _ => {}
                }
            }
        }
    }

    /// Handles the exit of the process a slot with `token` tracks: reaps a
    /// child, then hands out the notice when it is complete.
    fn exited(&self, pid: Pid, token: usize) {
        let Some(slot) = self.slot(pid).filter(|slot| slot.token == token) else {
            return;
        };
        let mut state = slot.lock();
        if state.removed || state.exited {
            return;
        }
        state.exited = true;
        if state.kind.is_child() && !state.reaped {
            state.status = reap(pid);
            state.reaped = true;
        }
        if let Capture::Running(exited) = &state.capture {
            exited.store(true, Ordering::Release);
        }
        let delivery = self.settle(pid, &slot, &mut state);
        drop(state);
        deliver(delivery);
    }

    /// Stores a helper's captured output, then has the watcher thread
    /// deliver the notice.
    fn capture_done(&self, pid: Pid, output: CapturedOutput) {
        let Some(slot) = self.slot(pid) else {
            return;
        };
        {
            let mut state = slot.lock();
            if matches!(state.capture, Capture::Running(_)) {
                state.capture = Capture::Done(output);
            }
        }
        if self.wake(ident(pid)).is_err() {
            self.deliver_ready(pid);
        }
    }

    fn deliver_ready(&self, pid: Pid) {
        let Some(slot) = self.slot(pid) else {
            return;
        };
        let mut state = slot.lock();
        let delivery = self.settle(pid, &slot, &mut state);
        drop(state);
        deliver(delivery);
    }

    /// Hands out the notice of an exited process once it is complete and
    /// someone waits for it, then forgets the slot. A discarded child
    /// nobody waits for is forgotten without a notice.
    fn settle(&self, pid: Pid, slot: &Arc<Slot>, state: &mut SlotState) -> Delivery {
        if state.removed || !state.exited || matches!(state.capture, Capture::Running(_)) {
            return None;
        }
        if state.listener.is_none() && !state.discarded {
            return None;
        }
        let captured = match mem::replace(&mut state.capture, Capture::None) {
            Capture::Done(output) => Some(output),
            Capture::None | Capture::Running(_) => None,
        };
        let notice = ExitNotice {
            pid,
            status: if state.kind.is_child() {
                state.status
            } else {
                None
            },
            captured,
        };
        state.removed = true;
        self.remove(pid, slot);
        state.listener.take().map(|listener| (listener, notice))
    }
}

fn deliver(delivery: Delivery) {
    if let Some((listener, notice)) = delivery {
        listener.exited(notice);
    }
}

/// Kills and reaps a suspended child that is not tracked yet, after its
/// spawn went wrong.
pub(super) fn abandon_unrun(pid: Pid) {
    // Not tracked, so nothing else reaps it and the PID is still its own.
    let _ = kill::kill_unrun(pid);
    reap_blocking(pid);
}

/// Sends `signal` to `pid`.
///
/// # Errors
///
/// Returns [`ProcessError::NotFound`] for `ESRCH`,
/// [`ProcessError::PermissionDenied`] for `EPERM`, and
/// [`ProcessError::Io`] otherwise.
pub(super) fn send(pid: Pid, signal: libc::c_int) -> Result<(), ProcessError> {
    // SAFETY: kill takes two integers and touches no memory of ours.
    if unsafe { libc::kill(pid.as_i32(), signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Err(ProcessError::NotFound { pid }),
        Some(libc::EPERM) => Err(ProcessError::PermissionDenied { pid }),
        _ => Err(ProcessError::io("kill", None, &error)),
    }
}

/// What a `waitpid(pid, WNOHANG)` found.
enum Reap {
    Exited(ExitStatus),
    /// The child has not exited.
    Running,
    /// `pid` is no child of this process, or already reaped.
    NoChild,
}

fn try_reap(pid: Pid) -> Reap {
    loop {
        let mut status: libc::c_int = 0;
        // SAFETY: waitpid writes the status into the integer it gets.
        let result = unsafe { libc::waitpid(pid.as_i32(), &raw mut status, libc::WNOHANG) };
        if result > 0 {
            return Reap::Exited(decode(status));
        }
        if result == 0 {
            return Reap::Running;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Reap::NoChild;
        }
    }
}

fn reap_blocking(pid: Pid) -> Option<ExitStatus> {
    loop {
        let mut status: libc::c_int = 0;
        // SAFETY: waitpid writes the status into the integer it gets.
        let result = unsafe { libc::waitpid(pid.as_i32(), &raw mut status, 0) };
        if result > 0 {
            return Some(decode(status));
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return None;
        }
    }
}

/// Reaps a child the kqueue reported as exited, waiting only if the zombie
/// is not visible yet.
fn reap(pid: Pid) -> Option<ExitStatus> {
    match try_reap(pid) {
        Reap::Exited(status) => Some(status),
        Reap::NoChild => None,
        Reap::Running => reap_blocking(pid),
    }
}

const fn decode(status: libc::c_int) -> ExitStatus {
    if libc::WIFSIGNALED(status) {
        ExitStatus::Signaled(libc::WTERMSIG(status))
    } else {
        ExitStatus::Exited(libc::WEXITSTATUS(status))
    }
}

const fn event(ident: usize, filter: i16, flags: u16, fflags: u32) -> libc::kevent {
    libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: ptr::null_mut(),
    }
}

fn ident(pid: Pid) -> usize {
    usize::try_from(pid.get()).unwrap_or(usize::MAX)
}

fn pid_of(ident: usize) -> Option<Pid> {
    u32::try_from(ident).ok().and_then(|pid| Pid::new(pid).ok())
}

/// The reader of one helper's stdout and stderr pipes.
struct CaptureJob {
    streams: [File; 2],
    exited: Arc<AtomicBool>,
}

impl CaptureJob {
    /// Reads both pipes into tails of at most [`HELPER_CAPTURE_MAX`] bytes
    /// until both close, or until the helper exited and nothing more came
    /// within [`CAPTURE_TICK`], or [`CAPTURE_GRACE`] after the exit.
    fn run(mut self) -> CapturedOutput {
        let mut tails: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
        let mut open = [true, true];
        for stream in &self.streams {
            // SAFETY: fcntl only changes the flags of a descriptor this
            // job owns.
            unsafe {
                let flags = libc::fcntl(stream.as_raw_fd(), libc::F_GETFL);
                libc::fcntl(stream.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        let mut grace_ends: Option<Instant> = None;
        let mut buffer = vec![0_u8; 16_384];
        while open.contains(&true) {
            let exited = self.exited.load(Ordering::Acquire);
            if exited {
                let ends = *grace_ends.get_or_insert_with(|| Instant::now() + CAPTURE_GRACE);
                if Instant::now() >= ends {
                    break;
                }
            }
            let mut fds = [0, 1].map(|index| libc::pollfd {
                fd: if open[index] {
                    self.streams[index].as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            });
            let tick = libc::c_int::try_from(CAPTURE_TICK.as_millis()).unwrap_or(50);
            // SAFETY: the array holds two pollfd entries, and poll only
            // writes their revents.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, tick) };
            if ready == 0 {
                if exited {
                    break;
                }
                continue;
            }
            if ready < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            for index in 0..2 {
                if open[index] && fds[index].revents != 0 {
                    open[index] = drain(&mut self.streams[index], &mut tails[index], &mut buffer);
                }
            }
        }
        let [stdout, stderr] = tails;
        CapturedOutput { stdout, stderr }
    }
}

/// Reads what a non blocking pipe holds into `tail`, keeping its last
/// [`HELPER_CAPTURE_MAX`] bytes. Returns false once the pipe closed.
fn drain(stream: &mut File, tail: &mut Vec<u8>, buffer: &mut [u8]) -> bool {
    loop {
        match stream.read(buffer) {
            Ok(0) => return false,
            Ok(read) => {
                tail.extend_from_slice(&buffer[..read]);
                if tail.len() > HELPER_CAPTURE_MAX {
                    let extra = tail.len() - HELPER_CAPTURE_MAX;
                    tail.drain(..extra);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return true,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}
