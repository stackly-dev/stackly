//! The operation engine (spec 0007): every mutation of Stackly runs as one
//! operation that is recorded in the registry first, runs under lock keys
//! held in memory with a bounded FIFO queue, records its steps and what it
//! created in a journal, and ends with one atomic commit. Cancel,
//! compensation of what the operation itself created, resume after a
//! restart, and abandon all go through this one API.
//!
//! Registry calls run on blocking threads, and no registry transaction ever
//! stays open across an `await` (REQ-048).

pub mod codes;
mod compensation;
mod context;
mod failure;
pub mod limits;
mod lock;
mod precheck;
mod recovery;
mod redact;
mod runner;
mod scheduler;
mod state;
#[doc(hidden)]
pub mod testing;
mod work;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use stackly_domain::{ClientCode, OperationCursor};
use stackly_domain::{
    CompensatorName, CreatedRecord, IdempotencyKey, JournalPayload, JournalStep, LockKey,
    MethodName, Operation, OperationId, OperationJournal, OperationKind, OperationQuery,
    OperationResource, OperationState, Registry, RegistryError, ResourceRevision, Sha256Digest,
};
use stackly_platform::{Platform, PlatformError};
use tokio::sync::Notify;

pub use compensation::{
    CompensationError, CompensationResult, CompensationScope, Compensator,
    ENVIRONMENT_TREE_COMPENSATOR, INSTANCE_TREE_COMPENSATOR,
};
pub use context::OperationContext;
pub use limits::EngineLimits;
pub use lock::{InternalLock, LockBusy, LockHolder, LockTable};
pub use precheck::{NO_OP_STAGE, NoOpReply, Precheck, PrecheckOutcome, Rejection};
pub use redact::Redact;
pub use work::{
    BoxFuture, CreatedHandle, FinalBatch, Finalize, OperationError, OperationOutcome,
    OperationWork, PartialItem, PartialOutcome, RebuildError, StepSpec, StepStart, WorkError,
    WorkFactory, WorkOutcome, WorkResult,
};

use crate::layout::PreparedLayout;
use scheduler::Scheduler;
use state::OpHandle;

/// What the engine runs on.
#[derive(Clone)]
pub struct EngineDeps {
    /// The registry every operation is recorded in.
    pub registry: Arc<dyn Registry>,
    /// The platform ports, for the clock and the filesystem.
    pub platform: Arc<Platform>,
    /// The checked layout, for scratch folders and compensation paths.
    pub layout: Arc<PreparedLayout>,
    /// Where the engine reports every visible change after its commit.
    pub events: Arc<dyn OperationEvents>,
    /// Removes secret values from failure messages before they are stored:
    /// the daemon's shared [`Redactor`](crate::secret::Redactor).
    pub redactor: Arc<dyn Redact>,
}

impl fmt::Debug for EngineDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineDeps").finish_non_exhaustive()
    }
}

/// Receives every committed change of an operation that a client can see,
/// in revision order.
pub trait OperationEvents: Send + Sync + 'static {
    /// The operation was created or changed state.
    fn state_changed(&self, view: &OperationView);
    /// Its stage, progress, wait, point of no return, or cancel changed.
    fn progress(&self, view: &OperationView);
}

/// What to do when a lock key of a new operation is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BusyPolicy {
    /// Wait in the queue.
    #[default]
    Queue,
    /// Refuse at once with [`BeginError::Busy`].
    Reject,
}

/// The client request an operation comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOrigin {
    /// The wire method.
    pub method: MethodName,
    /// The client's idempotency key.
    pub key: IdempotencyKey,
    /// The request fingerprint (spec 0007, "Idempotency").
    pub fingerprint: Sha256Digest,
    /// The client's kind from its handshake.
    pub client: ClientCode,
    /// Whether the client can answer a prompt.
    pub interactive: bool,
}

/// Who starts an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A client request, with its idempotency key.
    Request(RequestOrigin),
    /// The daemon itself, with no key.
    Background,
}

/// A request to start an operation.
pub struct BeginRequest {
    /// The kind of work.
    pub kind: OperationKind,
    /// What it works on.
    pub resource: OperationResource,
    /// Whether `resource` holds a new ID the handler made.
    pub creates_resource: bool,
    /// More lock keys; the engine always adds the resource's own key.
    pub extra_locks: Vec<LockKey>,
    /// What to do when a key is taken.
    pub busy: BusyPolicy,
    /// The resource revision the request expects; `None` for the manager
    /// and for a new resource.
    pub expected_revision: Option<ResourceRevision>,
    /// Who starts it.
    pub origin: Origin,
    /// Enough to rebuild the work through its [`WorkFactory`], without any
    /// secret value.
    pub input: JournalPayload,
    /// The work.
    pub work: Box<dyn OperationWork>,
    /// The feature's own check, run in the begin gate after the key lookup
    /// and before the resource checks; `None` always proceeds.
    pub precheck: Option<Box<dyn Precheck>>,
}

impl fmt::Debug for BeginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BeginRequest")
            .field("kind", &self.kind)
            .field("resource", &self.resource)
            .field("creates_resource", &self.creates_resource)
            .field("extra_locks", &self.extra_locks)
            .field("busy", &self.busy)
            .field("expected_revision", &self.expected_revision)
            .field("origin", &self.origin)
            .field("precheck", &self.precheck.is_some())
            .finish_non_exhaustive()
    }
}

/// An operation as the engine shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationView {
    /// The operation row.
    pub operation: Operation,
    /// Its journal row.
    pub journal: OperationJournal,
    /// What the user can do about it in `needs_recovery`.
    pub recovery_actions: Vec<RecoveryAction>,
    /// Whether automatic compensation is running for it now.
    pub auto_compensating: bool,
}

/// An operation with its steps and created items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDetailView {
    /// The operation.
    pub view: OperationView,
    /// Its steps, by number.
    pub steps: Vec<JournalStep>,
    /// What it recorded as created, by number.
    pub created: Vec<CreatedRecord>,
}

/// One page of [`OperationEngine::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationListPage {
    /// The operations, newest first.
    pub items: Vec<OperationView>,
    /// Where the next page starts.
    pub next: Option<OperationCursor>,
}

/// A recovery action on an operation in `needs_recovery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryAction {
    /// Run it again from its last step.
    Resume,
    /// Undo what can be undone and close it.
    Abandon,
}

/// What [`OperationEngine::begin`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginOutcome {
    /// It created the operation.
    Created(OperationView),
    /// The idempotency key already named this operation, with the same
    /// request; nothing new ran.
    Replayed(OperationView),
    /// The feature's [`Precheck`] answered without running any work.
    NoOp(NoOpOutcome),
}

impl BeginOutcome {
    /// Returns the operation, created or replayed, or the one that records
    /// a no operation answer; `None` for a no operation answer that wrote
    /// nothing.
    #[must_use]
    pub const fn view(&self) -> Option<&OperationView> {
        match self {
            Self::Created(view) | Self::Replayed(view) => Some(view),
            Self::NoOp(outcome) => outcome.recorded.as_ref(),
        }
    }
}

/// A no operation answer of [`OperationEngine::begin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoOpOutcome {
    /// The `succeeded` operation, with no steps and stage [`NO_OP_STAGE`],
    /// that committed the answer's writes and binds the idempotency key;
    /// `None` when the answer wrote nothing.
    pub recorded: Option<OperationView>,
    /// Whether this answer replays an earlier request with the same key.
    pub replayed: bool,
}

/// Why [`OperationEngine::begin`] refused, in its check order.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BeginError {
    /// The engine is still recovering.
    #[error("the operation engine is not ready")]
    NotReady,
    /// The daemon is shutting down.
    #[error("the operation engine is shutting down")]
    ShuttingDown,
    /// A caller bug, for example a new resource with an expected revision.
    #[error("invalid operation request: {reason}")]
    InvalidRequest {
        /// What is wrong.
        reason: &'static str,
    },
    /// A journal payload is over its limit.
    #[error("{field} is longer than {max} bytes")]
    PayloadTooLarge {
        /// The field.
        field: &'static str,
        /// Its limit.
        max: usize,
    },
    /// The key names another request.
    #[error("the idempotency key is bound to operation {existing}")]
    KeyReused {
        /// The operation the key names.
        existing: OperationId,
    },
    /// The feature's [`Precheck`] refused the request.
    #[error("the request was refused: {0}")]
    Rejected(Rejection),
    /// The resource does not exist.
    #[error("the resource does not exist")]
    ResourceNotFound,
    /// The resource changed since the request read it.
    #[error("the resource is at revision {actual}, not {expected}")]
    RevisionMismatch {
        /// The revision the request expects.
        expected: ResourceRevision,
        /// The revision now.
        actual: ResourceRevision,
    },
    /// An operation in `needs_recovery` holds a lock key.
    #[error("operation {operation} needs recovery and holds {key}")]
    RecoveryRequired {
        /// The operation.
        operation: OperationId,
        /// The key.
        key: LockKey,
    },
    /// A lock key is taken and the policy is `Reject`.
    #[error("the resource is busy")]
    Busy {
        /// Who holds or waits for the key.
        holder: LockHolder,
    },
    /// The queue is full.
    #[error("the operation queue is full ({max})")]
    QueueFull {
        /// The queue limit.
        max: u32,
    },
    /// The registry failed.
    #[error(transparent)]
    Registry(RegistryError),
}

/// Why an operation cannot resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotResumableReason {
    /// It is not in `needs_recovery`.
    WrongState,
    /// Its kind never resumes.
    KindNotResumable,
    /// The step it stopped in is not repeatable.
    StepNotRepeatable,
    /// No factory is registered for its kind.
    NoFactory,
    /// Its compensation failed or started.
    CompensationFailed,
    /// Automatic compensation runs for it now.
    AutoCompensating,
}

impl NotResumableReason {
    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::WrongState => "wrong_state",
            Self::KindNotResumable => "kind_not_resumable",
            Self::StepNotRepeatable => "step_not_repeatable",
            Self::NoFactory => "no_factory",
            Self::CompensationFailed => "compensation_failed",
            Self::AutoCompensating => "auto_compensating",
        }
    }
}

/// Why an operation cannot be abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotAbandonableReason {
    /// It is not in `needs_recovery`.
    WrongState,
    /// Automatic compensation runs for it now.
    AutoCompensating,
}

impl NotAbandonableReason {
    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::WrongState => "wrong_state",
            Self::AutoCompensating => "auto_compensating",
        }
    }
}

/// Error of an engine call other than [`OperationEngine::begin`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The engine is still recovering.
    #[error("the operation engine is not ready")]
    NotReady,
    /// The daemon is shutting down.
    #[error("the operation engine is shutting down")]
    ShuttingDown,
    /// The operation does not exist.
    #[error("operation {0} does not exist")]
    NotFound(OperationId),
    /// The operation changed since the caller read it.
    #[error("the operation is at revision {actual}, not {expected}")]
    RevisionMismatch {
        /// The revision the caller expects.
        expected: ResourceRevision,
        /// The revision now.
        actual: ResourceRevision,
    },
    /// Resume was refused.
    #[error("operation {operation} cannot resume: {}", reason.as_code())]
    NotResumable {
        /// The operation.
        operation: OperationId,
        /// Why.
        reason: NotResumableReason,
    },
    /// Abandon was refused.
    #[error("operation {operation} in state {state} cannot be abandoned: {}", reason.as_code())]
    NotAbandonable {
        /// The operation.
        operation: OperationId,
        /// Its state.
        state: OperationState,
        /// Why.
        reason: NotAbandonableReason,
    },
    /// More than 256 steps or 1024 created items.
    #[error("an operation records at most {max} {kind}")]
    JournalFull {
        /// `steps` or `created`.
        kind: &'static str,
        /// The limit.
        max: u16,
    },
    /// A journal payload is over its limit.
    #[error("{field} is longer than {max} bytes")]
    PayloadTooLarge {
        /// The field.
        field: &'static str,
        /// Its limit.
        max: usize,
    },
    /// A work used the context in a way the engine does not allow, for
    /// example recording an item outside a step. A bug.
    #[error("invalid use of the operation engine: {reason}")]
    InvalidUse {
        /// What is wrong.
        reason: &'static str,
    },
    /// The registry failed.
    #[error(transparent)]
    Registry(RegistryError),
    /// A filesystem port failed, for example creating the scratch folder.
    #[error(transparent)]
    Platform(PlatformError),
    /// A task of the engine stopped unexpectedly. A bug.
    #[error("the operation engine failed: {reason}")]
    Internal {
        /// What failed.
        reason: &'static str,
    },
}

/// What [`OperationEngine::cancel`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CancelDisposition {
    /// It never started and is now `cancelled`.
    CancelledBeforeStart,
    /// The cancel is recorded; the work stops at its next checkpoint and
    /// its effects are compensated.
    Requested,
    /// It is past its point of no return; it finishes, and the cancel stays
    /// recorded.
    ContinuingPastPointOfNoReturn,
    /// It already ended.
    AlreadyFinal,
    /// It needs recovery; resume or abandon it instead.
    RecoveryRequired,
}

impl CancelDisposition {
    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::CancelledBeforeStart => "cancelled_before_start",
            Self::Requested => "requested",
            Self::ContinuingPastPointOfNoReturn => "continuing_past_point_of_no_return",
            Self::AlreadyFinal => "already_final",
            Self::RecoveryRequired => "recovery_required",
        }
    }
}

/// What [`OperationEngine::cancel`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelOutcome {
    /// The operation after the cancel.
    pub view: OperationView,
    /// What happened.
    pub disposition: CancelDisposition,
}

/// What [`OperationEngine::abandon`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbandonOutcome {
    /// The operation, now `failed` with reason `abandoned`.
    pub view: OperationView,
    /// The items it left behind: not compensated.
    pub leftovers: Vec<CreatedRecord>,
}

/// The counters of [`OperationEngine::stats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct EngineStats {
    /// Operations running now.
    pub running: u32,
    /// Operations waiting in the queue.
    pub queued: u32,
    /// Operations in `needs_recovery`.
    pub needs_recovery: u32,
    /// The running limit.
    pub max_running: u32,
    /// The queue limit.
    pub max_queued: u32,
    /// Operations the last start found interrupted.
    pub interrupted_at_start: u32,
    /// Operations the last start queued to resume.
    pub resumed_at_start: u32,
    /// Operations the last start scheduled for automatic compensation.
    pub auto_compensated_at_start: u32,
    /// Begins refused because the queue was full.
    pub rejected_queue_full: u64,
}

/// What [`OperationEngine::recover`] found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RecoveryReport {
    /// Interrupted operations queued to resume.
    pub resumed: u32,
    /// Operations scheduled for automatic compensation.
    pub auto_compensating: u32,
    /// Operations left in `needs_recovery` for the user.
    pub needs_recovery: u32,
    /// Queued operations rebuilt to run.
    pub queued: u32,
    /// Queued operations closed because their work could not be rebuilt.
    pub rebuild_failed: u32,
    /// Queued operations closed because a key they need is held by an
    /// operation in `needs_recovery`.
    pub closed_for_recovery: u32,
    /// Interrupted operations of a kind that closes as interrupted, closed
    /// `failed` or `cancelled` with no compensation.
    pub closed_interrupted: u32,
    /// Scratch folders deleted.
    pub scratch_removed: u32,
    /// Scratch folders left alone.
    pub scratch_leftover: u32,
}

/// What [`OperationEngine::shutdown`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ShutdownReport {
    /// Resumable operations parked to run again at the next start.
    pub parked: u32,
    /// Operations stopped before their point of no return.
    pub stopped: u32,
    /// Operations still running when the grace ran out.
    pub left_running: u32,
    /// Running operations of a kind that closes as interrupted, left as
    /// they are for the next start's `recover`; shutdown never waits for
    /// them.
    pub abandoned: u32,
}

/// Where the engine stands in the daemon's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Created; `recover` has not run.
    Created,
    /// `recover` runs.
    Recovering,
    /// Recovered; begins queue, and `start` opens the dispatcher.
    Ready,
    /// The dispatcher runs.
    Started,
    /// Shutdown began.
    ShuttingDown,
}

/// Counters behind [`EngineStats`].
#[derive(Debug, Default)]
struct Counters {
    interrupted_at_start: u32,
    resumed_at_start: u32,
    auto_compensated_at_start: u32,
    rejected_queue_full: u64,
}

/// The state every handle of the engine shares.
struct Inner {
    deps: EngineDeps,
    limits: EngineLimits,
    factories: RwLock<HashMap<OperationKind, Arc<dyn WorkFactory>>>,
    compensators: RwLock<HashMap<CompensatorName, Arc<dyn Compensator>>>,
    locks: LockTable,
    wake: Arc<Notify>,
    idle: Notify,
    sched: Mutex<Scheduler>,
    ops: Mutex<HashMap<OperationId, Arc<OpHandle>>>,
    begin_gate: tokio::sync::Mutex<()>,
    phase: Mutex<Phase>,
    counters: Mutex<Counters>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    fn phase(&self) -> Phase {
        *lock(&self.phase)
    }

    fn set_phase(&self, phase: Phase) {
        *lock(&self.phase) = phase;
    }

    fn sched(&self) -> MutexGuard<'_, Scheduler> {
        lock(&self.sched)
    }

    fn ops(&self) -> MutexGuard<'_, HashMap<OperationId, Arc<OpHandle>>> {
        lock(&self.ops)
    }

    fn handle(&self, id: OperationId) -> Option<Arc<OpHandle>> {
        self.ops().get(&id).cloned()
    }

    fn factory(&self, kind: OperationKind) -> Option<Arc<dyn WorkFactory>> {
        self.factories
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&kind)
            .cloned()
    }

    fn compensator(&self, name: &CompensatorName) -> Option<Arc<dyn Compensator>> {
        self.compensators
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    fn counters(&self) -> MutexGuard<'_, Counters> {
        lock(&self.counters)
    }
}

/// The operation engine. Cloning shares one engine.
#[derive(Clone)]
pub struct OperationEngine {
    inner: Arc<Inner>,
}

impl fmt::Debug for OperationEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationEngine")
            .field("phase", &self.inner.phase())
            .finish_non_exhaustive()
    }
}

impl OperationEngine {
    /// Creates an engine that has not recovered yet: every call but `get`,
    /// `list`, and `stats` answers not ready until [`recover`](Self::recover)
    /// ends.
    #[must_use]
    pub fn new(deps: EngineDeps, limits: EngineLimits) -> Self {
        let wake = Arc::new(Notify::new());
        Self {
            inner: Arc::new(Inner {
                deps,
                limits,
                factories: RwLock::new(HashMap::new()),
                compensators: RwLock::new(HashMap::new()),
                locks: LockTable::new(Arc::clone(&wake)),
                wake,
                idle: Notify::new(),
                sched: Mutex::new(Scheduler::default()),
                ops: Mutex::new(HashMap::new()),
                begin_gate: tokio::sync::Mutex::new(()),
                phase: Mutex::new(Phase::Created),
                counters: Mutex::new(Counters::default()),
            }),
        }
    }

    /// Registers how operations of `kind` are rebuilt after a restart or
    /// for a resume. A later call for the same kind replaces it.
    pub fn register_factory(&self, kind: OperationKind, factory: Arc<dyn WorkFactory>) {
        self.inner
            .factories
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(kind, factory);
    }

    /// Registers the compensator that undoes custom items named `name`.
    pub fn register_compensator(&self, name: CompensatorName, compensator: Arc<dyn Compensator>) {
        self.inner
            .compensators
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name, compensator);
    }

    /// Recovers what the last run left unfinished (spec 0007, "Start daemon
    /// dan pemulihan"). The daemon calls it in its start slot, after the
    /// registry opened and before the supervisor reconciles.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Registry`] when the registry cannot be read
    /// or a recovery commit keeps failing, and [`EngineError::InvalidUse`]
    /// when it already ran.
    pub async fn recover(&self) -> Result<RecoveryReport, EngineError> {
        recovery::recover(&self.inner).await
    }

    /// Opens the dispatcher, after the daemon is ready: automatic
    /// compensation and queued operations start, oldest first.
    pub fn start(&self) {
        scheduler::start(&self.inner);
    }

    /// Starts an operation, or replays the one its idempotency key names.
    /// It returns once the operation is recorded `queued`; the work runs on
    /// the engine's own tasks, so a caller that goes away cancels nothing.
    ///
    /// # Errors
    ///
    /// Returns a [`BeginError`] in the check order of spec 0007; nothing is
    /// written then.
    pub async fn begin(&self, request: BeginRequest) -> Result<BeginOutcome, BeginError> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move { scheduler::begin(&inner, request).await })
            .await
            .unwrap_or_else(|_| {
                Err(BeginError::Registry(RegistryError::Io {
                    detail: "the operation engine task stopped".to_owned(),
                }))
            })
    }

    /// Returns an operation with its steps and created items.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Registry`] when the registry cannot be read.
    pub async fn get(&self, id: OperationId) -> Result<Option<OperationDetailView>, EngineError> {
        state::detail(&self.inner, id).await
    }

    /// Returns one page of operations, newest first.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Registry`] when the registry cannot be read.
    pub async fn list(&self, query: OperationQuery) -> Result<OperationListPage, EngineError> {
        state::list(&self.inner, query).await
    }

    /// Cancels an operation (spec 0007, "Cancel").
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::NotReady`] while recovering,
    /// [`EngineError::NotFound`] for an unknown operation, and
    /// [`EngineError::Registry`] when the commit fails.
    pub async fn cancel(&self, id: OperationId) -> Result<CancelOutcome, EngineError> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move { scheduler::cancel(&inner, id).await })
            .await
            .unwrap_or(Err(EngineError::Internal {
                reason: "the cancel task stopped",
            }))
    }

    /// Queues an operation in `needs_recovery` to run again from its last
    /// step.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::NotResumable`] with the reason,
    /// [`EngineError::RevisionMismatch`] when `expected` is stale,
    /// [`EngineError::NotFound`], [`EngineError::NotReady`], or
    /// [`EngineError::Registry`].
    pub async fn resume(
        &self,
        id: OperationId,
        expected: Option<ResourceRevision>,
    ) -> Result<OperationView, EngineError> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move { recovery::resume(&inner, id, expected).await })
            .await
            .unwrap_or(Err(EngineError::Internal {
                reason: "the resume task stopped",
            }))
    }

    /// Abandons an operation in `needs_recovery`: undoes what its
    /// reversible steps created, releases its keys, and closes it `failed`
    /// with reason `abandoned`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::NotAbandonable`],
    /// [`EngineError::RevisionMismatch`], [`EngineError::NotFound`],
    /// [`EngineError::NotReady`], or [`EngineError::Registry`].
    pub async fn abandon(
        &self,
        id: OperationId,
        expected: ResourceRevision,
    ) -> Result<AbandonOutcome, EngineError> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move { recovery::abandon(&inner, id, expected).await })
            .await
            .unwrap_or(Err(EngineError::Internal {
                reason: "the abandon task stopped",
            }))
    }

    /// Returns the engine's counters.
    #[must_use]
    pub fn stats(&self) -> EngineStats {
        state::stats(&self.inner)
    }

    /// Returns the lock table, shared with the engine.
    #[must_use]
    pub fn locks(&self) -> LockTable {
        self.inner.locks.clone()
    }

    /// Stops the engine within `grace` (spec 0007, "Shutdown"): no new
    /// operation starts, resumable ones park, the rest stop before their
    /// point of no return or finish, and those that close as interrupted
    /// are left for the next start. It returns when every runner it waits
    /// for ended or the grace ran out.
    pub async fn shutdown(&self, grace: Duration) -> ShutdownReport {
        recovery::shutdown(&self.inner, grace).await
    }
}
