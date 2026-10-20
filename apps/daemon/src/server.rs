//! The daemon server (spec 0006, *Urutan start* steps 9 to 12, *Koneksi*,
//! *Pemeriksa socket*, and *Berhenti*).
//!
//! [`start`] binds the socket, runs the accept loop, and runs the start
//! slot in the background, so clients are served (with health
//! `starting`) while the registry opens. [`DaemonHandle::shutdown`] runs
//! the stop order. The binary calls both from `startup`; tests call them
//! in process with a test root and wrapped [`Handlers`].

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_core::operation::{EngineError, OperationEngine};
use stackly_core::{PreparedLayout, daemon_info};
use stackly_domain::{PruneReport, Registry, RegistryError, RetentionPolicy};
use stackly_ipc::{Accepted, BindState, IpcError, Listener};
use stackly_platform::{Platform, PlatformError};
use stackly_protocol::ErrorBody;
use stackly_protocol::codes::ipc;
use stackly_protocol::daemon::{DaemonHealth, DaemonInfo};
use stackly_protocol::events::{EventKind, ShutdownReason, ShuttingDown};
use stackly_storage::{OpenOptions, RecoveryReason, RegistryOpen, SqliteRegistry, StorageError};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, timeout};

use crate::connection;
use crate::dispatch::{DefaultHandlers, Handlers};
use crate::events::EventBus;
use crate::log_sink::LogSink;
use crate::map::{self, StartFacts};
use crate::shutdown::{SHUTDOWN_DRAIN, SHUTDOWN_FLUSH};
use crate::startup::EXIT_START_FAILED;
use crate::state::{DaemonCore, Phase, RegistryState};

/// How many connections are served at once; the next one gets
/// `ipc.too_many_connections`.
pub const MAX_CONNECTIONS: usize = 64;
/// How often the accept loop checks that the socket is still its own.
pub const SOCKET_CHECK_INTERVAL: Duration = Duration::from_secs(10);
/// The first wait after a failed accept; it doubles up to
/// [`ACCEPT_BACKOFF_MAX`].
pub const ACCEPT_BACKOFF_MIN: Duration = Duration::from_millis(100);
/// The longest wait after failed accepts.
pub const ACCEPT_BACKOFF_MAX: Duration = Duration::from_secs(1);
/// How often the registry history is pruned while the daemon serves.
pub const REGISTRY_PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// The waits before each new try when another process holds the registry
/// (recovery reason `registry_locked`).
pub const REGISTRY_LOCKED_RETRIES: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// Tunables of the server. The daemon uses [`DaemonConfig::default`];
/// tests shorten the intervals or hold the start slot.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DaemonConfig {
    /// How often the socket is checked.
    pub socket_check_interval: Duration,
    /// How often the registry is pruned after the first prune.
    pub prune_interval: Duration,
    /// The waits between tries while the registry is locked.
    pub registry_locked_retries: Vec<Duration>,
    /// How the start slot opens the registry; tests shorten the busy
    /// timeout.
    pub registry_open: OpenOptions,
    /// When set, the start slot waits until the value turns true, so a
    /// test can watch the daemon while it is `starting`.
    pub start_gate: Option<watch::Receiver<bool>>,
    /// The uid a peer must have; `None`, the daemon's choice, means its own
    /// uid. Tests set another uid to watch a refused peer (spec 0006 AC-2).
    pub peer_uid: Option<u32>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_check_interval: SOCKET_CHECK_INTERVAL,
            prune_interval: REGISTRY_PRUNE_INTERVAL,
            registry_locked_retries: REGISTRY_LOCKED_RETRIES.to_vec(),
            registry_open: OpenOptions::new(),
            start_gate: None,
            peer_uid: None,
        }
    }
}

/// What [`start`] needs: everything steps 1 to 8 produced.
pub struct DaemonDeps {
    platform: Platform,
    prepared: PreparedLayout,
    facts: StartFacts,
    version: String,
    log: Arc<LogSink>,
    handlers: Arc<dyn Handlers>,
    config: DaemonConfig,
}

impl std::fmt::Debug for DaemonDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonDeps")
            .field("facts", &self.facts)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl DaemonDeps {
    /// The daemon's dependencies with [`DefaultHandlers`], the default
    /// config, and this build's version.
    #[must_use]
    pub fn new(
        platform: Platform,
        prepared: PreparedLayout,
        facts: StartFacts,
        log: Arc<LogSink>,
    ) -> Self {
        Self {
            platform,
            prepared,
            facts,
            version: daemon_info().version.to_owned(),
            log,
            handlers: Arc::new(DefaultHandlers),
            config: DaemonConfig::default(),
        }
    }

    /// Serves requests through `handlers` instead of [`DefaultHandlers`].
    #[must_use]
    pub fn with_handlers(mut self, handlers: impl Handlers) -> Self {
        self.handlers = Arc::new(handlers);
        self
    }

    /// Uses `config` instead of the default.
    #[must_use]
    pub fn with_config(mut self, config: DaemonConfig) -> Self {
        self.config = config;
        self
    }
}

/// Why the server did not start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartError {
    /// The current user could not be read.
    #[error("cannot tell the current user: {0}")]
    Identity(#[source] PlatformError),
    /// The current user has no Unix uid to check peers against.
    #[error("the current user has no unix uid")]
    NotUnix,
    /// The socket could not be bound.
    #[error("cannot bind the daemon socket: {0}")]
    Bind(#[source] IpcError),
}

impl StartError {
    /// The error code for the log line, such as `ipc.endpoint_occupied`.
    #[must_use]
    pub fn code(&self) -> Option<String> {
        match self {
            Self::Bind(error) => Some(error.to_error_body().code().to_owned()),
            Self::Identity(_) | Self::NotUnix => None,
        }
    }
}

/// How a stop went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stopped {
    /// A second signal cut the drain and the flush short.
    pub forced: bool,
}

/// The background tasks the stop waits for.
#[derive(Debug, Default)]
struct Tasks {
    prune: Mutex<Option<JoinHandle<()>>>,
    started: watch::Sender<bool>,
}

/// A running daemon server.
#[derive(Debug)]
pub struct DaemonHandle {
    core: Arc<DaemonCore>,
    accept: Option<JoinHandle<()>>,
    slot: Option<JoinHandle<()>>,
    tasks: Arc<Tasks>,
    socket: PathBuf,
}

/// Binds the socket and starts serving (steps 9 to 12).
///
/// The socket is bound before this returns; the accept loop and the start
/// slot then run in the background, and health reports `starting` until
/// the slot finished. The caller must hold the daemon lock and run inside
/// a tokio runtime.
///
/// # Errors
///
/// Returns [`StartError::Bind`] when the socket cannot be bound (for
/// example `ipc.endpoint_occupied` for a file at the socket path, which is
/// left in place), and [`StartError::Identity`] or
/// [`StartError::NotUnix`] when the current uid is unknown.
pub async fn start(deps: DaemonDeps) -> Result<DaemonHandle, StartError> {
    let DaemonDeps {
        platform,
        prepared,
        facts,
        version,
        log,
        handlers,
        config,
    } = deps;
    let user = platform
        .identity()
        .current_user()
        .map_err(StartError::Identity)?;
    let uid = map::unix_uid(&user).ok_or(StartError::NotUnix)?;
    let layout = prepared.layout().clone();
    let info = map::daemon_info(&facts, &layout, Some(uid), &version);
    let bus = Arc::new(EventBus::new(
        facts.server_instance_id,
        platform.shared_clock(),
    ));
    let core = Arc::new(DaemonCore::new(info, platform, prepared, bus, log));

    // Step 9.
    let socket = layout.daemon_socket();
    let listener =
        Listener::bind(&socket, config.peer_uid.unwrap_or(uid)).map_err(StartError::Bind)?;
    tracing::info!(socket_path = %socket.display(), "stacklyd listens");

    // Step 10.
    let accept = tokio::spawn(accept_loop(
        listener,
        Arc::clone(&core),
        Arc::clone(&handlers),
        config.socket_check_interval,
    ));

    // Steps 11 and 12.
    let tasks = Arc::new(Tasks::default());
    let slot = tokio::spawn(start_slot(
        Arc::clone(&core),
        Arc::clone(&tasks),
        handlers,
        version,
        config,
    ));
    Ok(DaemonHandle {
        core,
        accept: Some(accept),
        slot: Some(slot),
        tasks,
        socket,
    })
}

impl DaemonHandle {
    /// The shared state, for tests that look inside.
    #[must_use]
    pub const fn core(&self) -> &Arc<DaemonCore> {
        &self.core
    }

    /// The event bus, for tests that publish.
    #[must_use]
    pub fn bus(&self) -> &Arc<EventBus> {
        self.core.bus()
    }

    /// The health right now.
    #[must_use]
    pub fn health(&self) -> DaemonHealth {
        self.core.health(std::time::Instant::now())
    }

    /// The facts `daemon.get_info` reports right now.
    #[must_use]
    pub fn info(&self) -> DaemonInfo {
        self.core.info()
    }

    /// The socket path.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Resolves once the start slot finished.
    pub async fn started(&self) {
        let mut started = self.tasks.started.subscribe();
        let _ = started.wait_for(|started| *started).await;
    }

    /// Resolves with the exit code once a fatal error means the daemon
    /// must stop, such as a registry that stopped working.
    pub async fn fatal(&self) -> u8 {
        self.core.fatal().await
    }

    /// Runs the stop order (spec 0006, *Berhenti*):
    ///
    /// 1. stop the accept loop, remove the socket (only while it is still
    ///    this daemon's), and drop the listener;
    /// 2. move to `stopping` and publish `daemon.shutting_down`;
    /// 3. from here new requests get `daemon.shutting_down`;
    /// 4. wait up to [`SHUTDOWN_DRAIN`] for running requests, and give the
    ///    operation engine its
    ///    [`OPERATION_SHUTDOWN_GRACE`](stackly_core::operation::limits::OPERATION_SHUTDOWN_GRACE)
    ///    at the same time (spec 0007, *Shutdown*);
    /// 5. give every connection [`SHUTDOWN_FLUSH`] to send what it holds,
    ///    then close it.
    ///
    /// When `interrupt` resolves first (a second signal), the drain, the
    /// engine's grace, and the flush are cut short: operations still
    /// running stay `running` in the registry for the next start's
    /// `recover`. Either way the background tasks end and the daemon's
    /// registry handle closes before this returns (an operation task the
    /// stop cut short holds its own until the runtime drops it); releasing
    /// the lock is the caller's next step.
    pub async fn shutdown(
        mut self,
        reason: ShutdownReason,
        interrupt: impl Future<Output = ()>,
    ) -> Stopped {
        tokio::pin!(interrupt);
        let core = Arc::clone(&self.core);
        let accept = self.accept.take();
        let forced = tokio::select! {
            () = stop_gracefully(&core, accept, reason) => false,
            () = &mut interrupt => {
                tracing::warn!("a second stop request arrived; skipping the drain and the flush");
                true
            }
        };
        if forced {
            core.request_stop();
            core.mark_stopping();
            core.connections().close_now();
        }
        if let Some(slot) = self.slot.take() {
            slot.abort();
            let _ = slot.await;
        }
        // The engine's tasks have ended, unless the stop was forced; its
        // last handle goes here, so its registry handle goes with it.
        core.operations().close();
        let prune = self
            .tasks
            .prune
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(prune) = prune {
            if forced {
                prune.abort();
            }
            let _ = prune.await;
        }
        if let Some(registry) = core.take_registry() {
            // Dropping the last handle joins the registry thread, which
            // checkpoints and closes the file.
            let _ = tokio::task::spawn_blocking(move || drop(registry)).await;
        }
        Stopped { forced }
    }
}

async fn stop_gracefully(
    core: &Arc<DaemonCore>,
    accept: Option<JoinHandle<()>>,
    reason: ShutdownReason,
) {
    // Step 1.
    core.request_stop();
    if let Some(accept) = accept {
        let _ = accept.await;
    }
    // Steps 2 and 3.
    core.mark_stopping();
    let shutting_down = core
        .bus()
        .publish_daemon(EventKind::DaemonShuttingDown(ShuttingDown { reason }));
    // Step 4, with the operation engine's grace at the same time.
    let drain = async {
        if timeout(SHUTDOWN_DRAIN, core.drain().idle()).await.is_err() {
            tracing::warn!(
                running = core.drain().running(),
                "requests still ran when the drain ended",
            );
        }
    };
    let ((), _) = tokio::join!(drain, core.operations().shutdown());
    // Step 5.
    core.connections()
        .close_all(SHUTDOWN_FLUSH, shutting_down)
        .await;
}

async fn accept_loop(
    mut listener: Listener,
    core: Arc<DaemonCore>,
    handlers: Arc<dyn Handlers>,
    check_interval: Duration,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut check = tokio::time::interval_at(Instant::now() + check_interval, check_interval);
    check.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut backoff = ACCEPT_BACKOFF_MIN;
    let mut accept_errors: u64 = 0;
    loop {
        tokio::select! {
            biased;
            () = core.stop_requested() => break,
            _ = check.tick() => check_socket(&mut listener, &core),
            accepted = listener.accept() => match accepted {
                Ok(Accepted::Authorized { connection, peer }) => {
                    backoff = ACCEPT_BACKOFF_MIN;
                    core.counters().accepted();
                    if let Ok(slot) = Arc::clone(&slots).try_acquire_owned() {
                        tokio::spawn(connection::serve(
                            connection,
                            peer,
                            Instant::now(),
                            Arc::clone(&core),
                            Arc::clone(&handlers),
                            slot,
                        ));
                    } else {
                        core.counters().rejected();
                        tracing::warn!(
                            peer_pid = peer.pid,
                            max = MAX_CONNECTIONS,
                            "too many connections; refusing one",
                        );
                        let error = ErrorBody::new(
                            &ipc::TOO_MANY_CONNECTIONS,
                            "the daemon serves the most connections it may",
                        )
                        .with_detail("max", MAX_CONNECTIONS.to_string());
                        tokio::spawn(async move {
                            let _ = connection.close_with_error(None, error).await;
                        });
                    }
                }
                Ok(Accepted::Rejected { uid, pid }) => {
                    // A peer that left before its credentials could be read
                    // lands here too: unreadable credentials are a refused
                    // peer (spec 0006 AC-2, fail closed), so it counts in
                    // `connectionsRejected` like any other.
                    backoff = ACCEPT_BACKOFF_MIN;
                    core.counters().accepted();
                    core.counters().rejected();
                    tracing::warn!(
                        peer_uid = uid,
                        peer_pid = pid,
                        code = ipc::PEER_REJECTED.code,
                        "a peer of another user, or one whose credentials cannot be read, was refused",
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    accept_errors += 1;
                    tracing::error!(
                        error = %error,
                        accept_errors,
                        retry_in_ms = u64::try_from(backoff.as_millis()).unwrap_or(u64::MAX),
                        "accepting a connection failed",
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(ACCEPT_BACKOFF_MAX);
                }
            },
        }
    }
    match listener.unlink_if_own() {
        Ok(_) => {}
        Err(error) => tracing::warn!(error = %error, "cannot remove the daemon socket"),
    }
    drop(listener);
}

/// The socket check (spec 0006, *Pemeriksa socket*): when the socket went
/// missing or was replaced, recreate the runtime root and bind again.
fn check_socket(listener: &mut Listener, core: &DaemonCore) {
    let state = listener.check_bound();
    if state == BindState::Bound {
        return;
    }
    let socket_path = listener.path().display().to_string();
    if let Err(error) = stackly_core::ensure_runtime_root(core.platform(), core.prepared()) {
        core.set_ipc_problem(Some("socket missing, retrying".to_owned()));
        tracing::error!(error = %error, socket_path, "cannot recreate the runtime root");
        return;
    }
    match listener.rebind() {
        Ok(()) => {
            core.counters().rebound();
            core.set_ipc_problem(None);
            tracing::warn!(
                socket_path,
                was = if state == BindState::Missing {
                    "missing"
                } else {
                    "replaced"
                },
                "socket_rebound: the socket was bound again",
            );
        }
        Err(error) => {
            core.set_ipc_problem(Some("socket missing, retrying".to_owned()));
            tracing::error!(
                error = %error,
                code = error.to_error_body().code(),
                socket_path,
                "cannot bind the socket again; retrying at the next check",
            );
        }
    }
}

/// Steps 11 and 12: the start slot, then `ready`. Once the registry is
/// ready, the operation engine recovers the last run's operations before
/// `ready` and opens its dispatcher after it (spec 0007, *Start daemon dan
/// pemulihan*).
async fn start_slot(
    core: Arc<DaemonCore>,
    tasks: Arc<Tasks>,
    handlers: Arc<dyn Handlers>,
    version: String,
    config: DaemonConfig,
) {
    if let Some(mut gate) = config.start_gate {
        let _ = gate.wait_for(|open| *open).await;
    }
    let opened = open_with_retries(
        &core,
        &version,
        &config.registry_open,
        &config.registry_locked_retries,
    )
    .await;
    let mut recovered = None;
    match opened {
        Some(Ok(RegistryOpen::Ready { registry, summary })) => {
            let pruned = prune_once(&core, &registry).await;
            if matches!(pruned, Err(RegistryError::Unavailable)) {
                registry_failed(&core);
                core.set_registry(RegistryState::Failed, Some(registry));
            } else {
                if let Err(error) = &pruned {
                    tracing::warn!(error = %error, "cannot prune the registry history; retrying at the next run");
                }
                tracing::info!(
                    registry_state = "ready",
                    pruned_rows = pruned.as_ref().ok().map(PruneReport::total),
                    schema_version = summary.schema_version,
                    created = summary.created,
                    migrated_from = summary.migrated_from,
                    snapshot = summary
                        .snapshot
                        .as_ref()
                        .map(|path| path.display().to_string()),
                    sqlite_version = summary.sqlite_version,
                    "registry ready",
                );
                core.set_registry(
                    RegistryState::Ready {
                        schema_version: summary.schema_version,
                    },
                    Some(registry.clone()),
                );
                let prune = tokio::spawn(prune_loop(Arc::clone(&core), config.prune_interval));
                *tasks.prune.lock().unwrap_or_else(PoisonError::into_inner) = Some(prune);
                recovered = recover_operations(&core, registry, handlers.as_ref()).await;
                // Spec 0010's reconciliation runs here, between `recover`
                // and `ready` (feature 10).
            }
        }
        Some(Ok(RegistryOpen::Recovery(report))) => {
            tracing::error!(
                registry_state = "recovery",
                reason = report.reason.code(),
                report = %report.path.display(),
                "the registry needs recovery; it was left untouched and stacklyd stays up",
            );
            core.set_registry(
                RegistryState::Recovery {
                    recovery: map::registry_recovery(&report),
                },
                None,
            );
            core.operations().unavailable("registry in recovery");
        }
        Some(Err(error)) => {
            tracing::error!(error = %error, "cannot open the registry");
            core.set_registry(RegistryState::Failed, None);
            core.operations().unavailable(REGISTRY_FAILED);
            core.fail(EXIT_START_FAILED);
        }
        None => {
            core.set_registry(RegistryState::Failed, None);
            core.operations().unavailable(REGISTRY_FAILED);
            core.fail(EXIT_START_FAILED);
        }
    }
    core.mark_ready();
    // A stop that began during the start slot leaves the dispatcher closed:
    // what `recover` queued runs at the next start.
    if let Some(engine) = recovered
        && core.phase() == Phase::Ready
    {
        engine.start();
    }
    tasks.started.send_replace(true);
    let health = core.health(std::time::Instant::now());
    tracing::info!(status = ?health.status, "stacklyd finished its start steps");
}

/// The `operations` detail when the registry broke.
const REGISTRY_FAILED: &str = "the registry stopped working";
/// The waits before each new try when the operation engine cannot read
/// the last run's operations.
pub const OPERATION_RECOVER_RETRIES: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// Creates the operation engine on the ready registry and recovers what
/// the last run left unfinished, trying again after each wait of
/// [`OPERATION_RECOVER_RETRIES`]. Returns the engine to start once the
/// daemon is ready, or `None` when recovery failed: a registry that
/// stopped working is fatal, and any other failure leaves the engine
/// serving reads only, with the `operations` subsystem unavailable.
async fn recover_operations(
    core: &DaemonCore,
    registry: SqliteRegistry,
    handlers: &dyn Handlers,
) -> Option<OperationEngine> {
    let engine = core
        .operations()
        .create(registry, core.platform(), core.prepared(), core.bus());
    if let Some(starting) = handlers.test_starting_method() {
        starting.register(&engine);
    }
    let mut waits = OPERATION_RECOVER_RETRIES.iter();
    loop {
        match engine.recover().await {
            Ok(_) => {
                core.operations().recovered();
                return Some(engine);
            }
            Err(EngineError::Registry(RegistryError::Unavailable)) => {
                registry_failed(core);
                core.operations().unavailable(REGISTRY_FAILED);
                return None;
            }
            Err(error) => {
                // A failed read leaves the engine as it was, so it may try
                // again.
                if let Some(wait) = waits.next() {
                    tracing::warn!(
                        error = %error,
                        retry_in_seconds = wait.as_secs(),
                        "cannot recover the operations of the last run; trying again",
                    );
                    tokio::time::sleep(*wait).await;
                    continue;
                }
                tracing::error!(error = %error, "cannot recover the operations of the last run");
                core.operations().recovery_failed();
                return None;
            }
        }
    }
}

/// Opens the registry, trying again while another process holds it.
/// `None` means the open task itself failed.
async fn open_with_retries(
    core: &DaemonCore,
    version: &str,
    options: &OpenOptions,
    retries: &[Duration],
) -> Option<Result<RegistryOpen, StorageError>> {
    let mut waits = retries.iter();
    loop {
        let platform = core.platform().clone();
        let layout = core.prepared().layout().clone();
        let version = version.to_owned();
        let options = options.clone();
        let opened =
            match tokio::task::spawn_blocking(move || options.open(&platform, &layout, &version))
                .await
            {
                Ok(opened) => opened,
                Err(error) => {
                    tracing::error!(error = %error, "the registry open task failed");
                    return None;
                }
            };
        if let Ok(RegistryOpen::Recovery(report)) = &opened
            && matches!(report.reason, RecoveryReason::RegistryLocked)
            && let Some(wait) = waits.next()
        {
            tracing::info!(
                retry_in_seconds = wait.as_secs(),
                "another process holds the registry; trying again",
            );
            tokio::time::sleep(*wait).await;
            continue;
        }
        return Some(opened);
    }
}

async fn prune_once(
    core: &DaemonCore,
    registry: &SqliteRegistry,
) -> Result<PruneReport, RegistryError> {
    let registry = registry.clone();
    let now = core.platform().clock().now();
    tokio::task::spawn_blocking(move || registry.prune(&RetentionPolicy::DEFAULT, now))
        .await
        .unwrap_or(Err(RegistryError::Unavailable))
}

/// Prunes every `interval` until the stop; a registry that stopped working
/// is fatal, any other failure waits for the next run.
async fn prune_loop(core: Arc<DaemonCore>, interval: Duration) {
    loop {
        tokio::select! {
            () = core.stop_requested() => return,
            () = tokio::time::sleep(interval) => {}
        }
        let Some(registry) = core.registry() else {
            return;
        };
        match prune_once(&core, &registry).await {
            Ok(report) => tracing::info!(pruned_rows = report.total(), "registry history pruned"),
            Err(RegistryError::Unavailable) => {
                registry_failed(&core);
                return;
            }
            Err(error) => tracing::warn!(
                error = %error,
                "cannot prune the registry history; retrying at the next run",
            ),
        }
    }
}

/// `Unavailable` from the registry is fatal: the daemon stops with the
/// usual order and exits 1.
fn registry_failed(core: &DaemonCore) {
    tracing::error!("the registry stopped working; stacklyd stops");
    core.mark_registry_failed();
    core.fail(EXIT_START_FAILED);
}
