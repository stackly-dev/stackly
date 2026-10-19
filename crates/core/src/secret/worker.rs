//! The one thread every secret store call runs on, with wait limits
//! (spec 0011, *Thread Keychain dan batas tunggu*, AC-4).

use std::fmt;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use stackly_domain::SecretRef;
use stackly_platform::{
    Interaction, SecretBackend, SecretPresence, SecretStore, SecretStoreError, SecretValue,
    StoreState, WriteMode,
};

use super::redact::copy_value;

/// Name of the worker thread.
const THREAD_NAME: &str = "stackly-secret";
/// Name of the thread that runs the late change hook.
const LATE_THREAD_NAME: &str = "stackly-secret-late";

/// How long a store call may wait, counted on a monotonic clock from the
/// moment it is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerConfig {
    /// For a call that may show a dialog
    /// ([`Interaction::AllowPrompt`]).
    pub prompt_wait: Duration,
    /// For a call that never shows one ([`Interaction::Never`]) and every
    /// probe.
    pub background_wait: Duration,
}

impl WorkerConfig {
    /// The daemon's limits: 180 seconds for a dialog, 10 seconds without
    /// one.
    pub const DEFAULT: Self = Self {
        prompt_wait: Duration::from_secs(180),
        background_wait: Duration::from_secs(10),
    };

    const fn wait(&self, interaction: Interaction) -> Duration {
        match interaction {
            Interaction::AllowPrompt => self.prompt_wait,
            Interaction::Never => self.background_wait,
        }
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the late change hook gets: the reference of a write or delete that
/// finished after its caller stopped waiting.
type LateHook = Arc<dyn Fn(&SecretRef) + Send + Sync>;

/// Runs every fallible call of a [`SecretStore`] in order on one thread of
/// its own, and is itself the [`SecretStore`] the daemon installs in its
/// `Platform` (AC-4).
///
/// - [`state`](SecretStore::state), [`backend`](SecretStore::backend), and
///   [`store_key`](SecretStore::store_key) answer at once, without queueing.
/// - Every other call waits at most [`WorkerConfig::prompt_wait`] with
///   [`Interaction::AllowPrompt`], and [`WorkerConfig::background_wait`]
///   with [`Interaction::Never`] or for a probe.
/// - A call whose limit passes before its turn never runs and fails with
///   [`SecretStoreError::Busy`].
/// - A call that is running when its limit passes cannot be stopped (a
///   Keychain dialog holds the thread until the user answers). Its caller
///   gets [`SecretStoreError::Busy`] for a read or probe, and
///   [`SecretStoreError::OutcomeUnknown`] for a write or delete. When that
///   call finishes, the worker logs one line with the reference and the
///   kind of outcome, never a value, and for a write or delete runs the
///   hook set with [`on_late_change`](Self::on_late_change), which the
///   daemon points at `SecretService::probe_one`.
///
/// The thread stops once the last handle is gone and the queue is empty.
pub struct SecretWorker {
    store: Arc<dyn SecretStore>,
    config: WorkerConfig,
    sender: Sender<Job>,
    late: Arc<Mutex<Option<LateHook>>>,
}

impl fmt::Debug for SecretWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretWorker")
            .field("backend", &self.store.backend())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// Which store method a job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Probe,
    Read,
    Write,
    Delete,
}

impl Method {
    /// Whether a late finish may have changed the store.
    const fn changes(self) -> bool {
        matches!(self, Self::Write | Self::Delete)
    }

    /// What the caller gets when the call is still running at its limit.
    const fn late_error(self) -> SecretStoreError {
        if self.changes() {
            SecretStoreError::OutcomeUnknown
        } else {
            SecretStoreError::Busy
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Probe => "probe",
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
        }
    }
}

/// Where one job stands, shared by its caller and the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting in the queue.
    Queued,
    /// Never run: its limit passed first.
    Skipped,
    /// Running, with the caller waiting.
    Running,
    /// Running, and the caller stopped waiting.
    Late,
    /// Finished.
    Done,
}

/// Runs one call, sends its result to the caller, and returns the kind of
/// outcome for the log.
type Run = Box<dyn FnOnce(&dyn SecretStore) -> &'static str + Send>;

struct Job {
    method: Method,
    reference: SecretRef,
    deadline: Instant,
    stage: Arc<Mutex<Stage>>,
    run: Run,
}

impl SecretWorker {
    /// Starts the worker thread in front of `store`.
    ///
    /// # Errors
    ///
    /// Returns the I/O error of a thread that could not start.
    pub fn spawn(store: Arc<dyn SecretStore>, config: WorkerConfig) -> std::io::Result<Arc<Self>> {
        let (sender, receiver) = mpsc::channel();
        let late: Arc<Mutex<Option<LateHook>>> = Arc::new(Mutex::new(None));
        let thread_store = Arc::clone(&store);
        let thread_late = Arc::clone(&late);
        std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || serve(thread_store.as_ref(), &receiver, &thread_late))?;
        Ok(Arc::new(Self {
            store,
            config,
            sender,
            late,
        }))
    }

    /// Sets what runs, on a thread of its own, after a write or delete
    /// finished late, with the reference it changed. A later call replaces
    /// the hook.
    pub fn on_late_change(&self, hook: impl Fn(&SecretRef) + Send + Sync + 'static) {
        *lock(&self.late) = Some(Arc::new(hook));
    }

    /// Queues one call and waits for it up to the limit of `interaction`.
    fn submit<T: Send + 'static>(
        &self,
        method: Method,
        reference: &SecretRef,
        interaction: Interaction,
        call: impl FnOnce(&dyn SecretStore) -> Result<T, SecretStoreError> + Send + 'static,
    ) -> Result<T, SecretStoreError> {
        let wait = self.config.wait(interaction);
        let deadline = Instant::now() + wait;
        let stage = Arc::new(Mutex::new(Stage::Queued));
        let (reply, result) = mpsc::sync_channel(1);
        let job = Job {
            method,
            reference: reference.clone(),
            deadline,
            stage: Arc::clone(&stage),
            run: Box::new(move |store| {
                let answer = call(store);
                let outcome = outcome_kind(answer.as_ref().err());
                // The caller may have stopped waiting; nothing to do then.
                let _ = reply.send(answer);
                outcome
            }),
        };
        if self.sender.send(job).is_err() {
            return Err(SecretStoreError::Busy);
        }
        match result.recv_timeout(wait) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                let mut stage = lock(&stage);
                match *stage {
                    Stage::Queued => {
                        *stage = Stage::Skipped;
                        Err(SecretStoreError::Busy)
                    }
                    Stage::Running => {
                        *stage = Stage::Late;
                        Err(method.late_error())
                    }
                    Stage::Done => result.try_recv().unwrap_or(Err(method.late_error())),
                    Stage::Skipped | Stage::Late => Err(SecretStoreError::Busy),
                }
            }
        }
    }
}

impl SecretStore for SecretWorker {
    fn backend(&self) -> SecretBackend {
        self.store.backend()
    }

    fn state(&self) -> StoreState {
        self.store.state()
    }

    fn store_key(&self, reference: &SecretRef) -> String {
        self.store.store_key(reference)
    }

    fn probe(&self, reference: &SecretRef) -> Result<SecretPresence, SecretStoreError> {
        let target = reference.clone();
        self.submit(Method::Probe, reference, Interaction::Never, move |store| {
            store.probe(&target)
        })
    }

    fn read(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<SecretValue, SecretStoreError> {
        let target = reference.clone();
        self.submit(Method::Read, reference, interaction, move |store| {
            store.read(&target, interaction)
        })
    }

    fn write(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
        mode: WriteMode,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        let target = reference.clone();
        // The job may outlive this call, so it holds its own copy, wiped
        // when the job drops.
        let value = copy_value(value);
        self.submit(Method::Write, reference, interaction, move |store| {
            store.write(&target, &value, mode, interaction)
        })
    }

    fn delete(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        let target = reference.clone();
        self.submit(Method::Delete, reference, interaction, move |store| {
            store.delete(&target, interaction)
        })
    }
}

/// The worker thread: one job at a time, in queue order.
fn serve(store: &dyn SecretStore, receiver: &Receiver<Job>, late: &Mutex<Option<LateHook>>) {
    while let Ok(job) = receiver.recv() {
        {
            let mut stage = lock(&job.stage);
            if *stage != Stage::Queued {
                continue;
            }
            if Instant::now() >= job.deadline {
                *stage = Stage::Skipped;
                continue;
            }
            *stage = Stage::Running;
        }
        let outcome = (job.run)(store);
        let finished_late = {
            let mut stage = lock(&job.stage);
            let finished_late = *stage == Stage::Late;
            *stage = Stage::Done;
            finished_late
        };
        if finished_late {
            tracing::info!(
                reference = %job.reference,
                call = job.method.as_str(),
                outcome,
                "a secret store call finished after its caller stopped waiting"
            );
            if job.method.changes() {
                run_late_hook(late, job.reference);
            }
        }
    }
}

/// Runs the late change hook on a thread of its own, since it calls the
/// worker again and must not wait on the worker thread itself.
fn run_late_hook(late: &Mutex<Option<LateHook>>, reference: SecretRef) {
    let Some(hook) = lock(late).clone() else {
        return;
    };
    let started = std::thread::Builder::new()
        .name(LATE_THREAD_NAME.to_owned())
        .spawn(move || hook(&reference));
    if let Err(error) = started {
        tracing::warn!(%error, "the secret store check after a late change did not start");
    }
}

/// The kind of outcome of a call, for the log.
const fn outcome_kind(error: Option<&SecretStoreError>) -> &'static str {
    match error {
        None => "succeeded",
        Some(SecretStoreError::NotFound) => "not_found",
        Some(SecretStoreError::AlreadyExists) => "already_exists",
        Some(SecretStoreError::Locked) => "locked",
        Some(SecretStoreError::AccessDenied { .. }) => "access_denied",
        Some(SecretStoreError::Busy) => "busy",
        Some(SecretStoreError::OutcomeUnknown) => "outcome_unknown",
        Some(SecretStoreError::NotSupported { .. }) => "not_supported",
        Some(_) => "backend",
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
