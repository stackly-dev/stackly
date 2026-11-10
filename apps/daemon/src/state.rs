//! What the daemon knows about itself while it runs (spec 0006,
//! *Kesehatan*): its phase, its subsystems, its counters, and the facts
//! `daemon.get_info` reports.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use stackly_core::PreparedLayout;
use stackly_core::catalog::CatalogState;
use stackly_core::diagnostics::{HomeMask, Staging};
use stackly_core::instance::create::CreateHooks;
use stackly_core::metrics::{MetricsStore, SamplerConfig};
use stackly_core::secret::Redactor;
use stackly_platform::{Platform, RootKind};
use stackly_protocol::Count;
use stackly_protocol::daemon::{
    DaemonCounters, DaemonHealth, DaemonHealthStatus, DaemonInfo, RegistryRecovery, ShutdownMode,
    SubsystemHealth, SubsystemStatus, subsystem,
};
use stackly_protocol::secret::SecretStoreTarget;
use stackly_storage::SqliteRegistry;
use tokio::sync::{Notify, watch};

use crate::catalog;
use crate::connection::Connections;
use crate::detail::{DataUsageService, PinnedPackages};
use crate::events::EventBus;
use crate::ipc::logs::LogService;
use crate::log_sink::LogSink;
use crate::operations::Operations;
use crate::secrets::Secrets;
use crate::session::SessionStatus;
use crate::shutdown::Drain;
use crate::storage::Storage;
use crate::supervisor::Supervision;

/// Where the daemon is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The socket is bound and the start slot runs.
    Starting,
    /// The start slot finished.
    Ready,
    /// A stop began.
    Stopping,
}

/// Where the registry stands (spec 0005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryState {
    /// The start slot has not opened it yet.
    Opening,
    /// Open and current.
    Ready {
        /// The registry's schema version, for `DaemonInfo.registrySchema`.
        schema_version: u32,
    },
    /// Left untouched; the daemon runs in recovery mode until a restart
    /// after the registry was repaired.
    Recovery {
        /// The recovery report, for `DaemonHealth.recovery` and for
        /// `registry.recovery_mode`.
        recovery: RegistryRecovery,
    },
    /// The registry stopped working; the daemon stops.
    Failed,
}

/// The counters of `daemon.get_health` the daemon keeps itself; the bus
/// and the log writer keep theirs.
#[derive(Debug, Default)]
pub struct Counters {
    connections_open: AtomicU64,
    connections_total: AtomicU64,
    connections_rejected: AtomicU64,
    frames_rejected: AtomicU64,
    socket_rebinds: AtomicU64,
}

impl Counters {
    /// A connection was accepted, whoever the peer is.
    pub fn accepted(&self) {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
    }

    /// A connection started to be served.
    pub fn opened(&self) {
        self.connections_open.fetch_add(1, Ordering::Relaxed);
    }

    /// A served connection ended.
    pub fn closed(&self) {
        // Saturates at 0, though every close follows an open.
        let _ = self
            .connections_open
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |open| {
                Some(open.saturating_sub(1))
            });
    }

    /// A peer was refused, over the connection limit, or failed the
    /// handshake.
    pub fn rejected(&self) {
        self.connections_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// A frame was answered with a framing or payload error.
    pub fn frame_rejected(&self) {
        self.frames_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// The socket was bound again.
    pub fn rebound(&self) {
        self.socket_rebinds.fetch_add(1, Ordering::Relaxed);
    }

    fn get(counter: &AtomicU64) -> Count {
        Count::saturating(counter.load(Ordering::Relaxed))
    }
}

/// Records a `daemon.shutdown` request from outside a handler.
#[derive(Debug, Clone)]
pub struct ShutdownTrigger {
    shutdown: Arc<watch::Sender<Option<ShutdownMode>>>,
    plan_stopping: Arc<AtomicBool>,
}

impl ShutdownTrigger {
    /// Records `mode`; only the first request counts, and `false` means one
    /// was accepted already.
    pub fn request(&self, mode: ShutdownMode) -> bool {
        Self::request_on(&self.shutdown, mode)
    }

    /// Records that a restart plan is committed in `stopping`, or that it
    /// closed as aborted (`false`). While it is set, a `SIGTERM` follows the
    /// restart instead of ending the login session (spec 0031).
    pub fn set_plan_stopping(&self, stopping: bool) {
        self.plan_stopping.store(stopping, Ordering::SeqCst);
    }

    fn request_on(shutdown: &watch::Sender<Option<ShutdownMode>>, mode: ShutdownMode) -> bool {
        shutdown.send_if_modified(|requested| {
            if requested.is_none() {
                *requested = Some(mode);
                true
            } else {
                false
            }
        })
    }
}

/// The daemon's shared state, one per run.
pub struct DaemonCore {
    info: DaemonInfo,
    platform: Platform,
    prepared: PreparedLayout,
    bus: Arc<EventBus>,
    log: Arc<LogSink>,
    phase: Mutex<Phase>,
    registry_state: Mutex<RegistryState>,
    registry: Mutex<Option<SqliteRegistry>>,
    catalog: OnceLock<Arc<CatalogState>>,
    catalog_source: Arc<catalog::CatalogSource>,
    catalog_accesses: AtomicU64,
    pinned_packages: PinnedPackages,
    data_usage: DataUsageService,
    metrics: MetricsStore,
    sampler_config: OnceLock<SamplerConfig>,
    create_hooks: OnceLock<CreateHooks>,
    session: OnceLock<SessionStatus>,
    home: OnceLock<PathBuf>,
    ipc_problem: Mutex<Option<String>>,
    counters: Counters,
    drain: Drain,
    connections: Connections,
    operations: Operations,
    supervision: Supervision,
    logs: OnceLock<LogService>,
    redactor: Arc<Redactor>,
    secrets: Secrets,
    storage: Storage,
    doctor: stackly_core::doctor::Doctor,
    next_connection_id: AtomicU64,
    stop: watch::Sender<bool>,
    fatal: watch::Sender<Option<u8>>,
    /// Why the first fatal error happened, for the record of a failed start
    /// (spec 0034, AC-11).
    fatal_reason: OnceLock<String>,
    /// The home folder written `~`, from `HOME` as `main` read it (spec
    /// 0034, *Penggantian folder home*).
    home_mask: OnceLock<HomeMask>,
    /// The staged previews of the diagnostic bundle (spec 0034).
    bundles: Staging,
    shutdown: Arc<watch::Sender<Option<ShutdownMode>>>,
    /// Set while a `daemon.shutdown` `agent_off` runs (spec 0031, *Matikan
    /// agent*): other shutdown requests and new operations from clients get
    /// `daemon.shutting_down`.
    agent_off: AtomicBool,
    /// Set while a planned restart's plan is committed in `stopping`.
    plan_stopping: Arc<AtomicBool>,
    /// Wakes the prune loop before its interval, after a change of the
    /// audit retention (spec 0035, AC-6).
    prune_wake: Notify,
}

impl std::fmt::Debug for DaemonCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonCore")
            .field("server_instance_id", &self.info.server_instance_id)
            .field("phase", &self.phase())
            .finish_non_exhaustive()
    }
}

impl DaemonCore {
    /// The state of a run that just bound its socket: phase `starting`,
    /// registry `opening`. `redactor` is the run's one redactor, which the
    /// log writer already uses, and `secret_target` the keychain chosen at
    /// start (`None` for a store without one).
    #[must_use]
    pub fn new(
        info: DaemonInfo,
        platform: Platform,
        prepared: PreparedLayout,
        bus: Arc<EventBus>,
        log: Arc<LogSink>,
        redactor: Arc<Redactor>,
        secret_target: Option<SecretStoreTarget>,
    ) -> Self {
        let bundles = Staging::new(prepared.canonical_root(RootKind::Cache).to_path_buf());
        Self {
            info,
            platform,
            prepared,
            bus,
            log,
            phase: Mutex::new(Phase::Starting),
            registry_state: Mutex::new(RegistryState::Opening),
            registry: Mutex::new(None),
            catalog: OnceLock::new(),
            catalog_source: Arc::new(catalog::CatalogSource::default()),
            catalog_accesses: AtomicU64::new(0),
            pinned_packages: PinnedPackages::default(),
            data_usage: DataUsageService::default(),
            metrics: MetricsStore::new(),
            sampler_config: OnceLock::new(),
            create_hooks: OnceLock::new(),
            session: OnceLock::new(),
            home: OnceLock::new(),
            ipc_problem: Mutex::new(None),
            counters: Counters::default(),
            drain: Drain::default(),
            connections: Connections::default(),
            operations: Operations::default(),
            supervision: Supervision::default(),
            logs: OnceLock::new(),
            redactor,
            secrets: Secrets::new(secret_target),
            storage: Storage::default(),
            doctor: stackly_core::doctor::Doctor::default(),
            next_connection_id: AtomicU64::new(1),
            stop: watch::Sender::new(false),
            fatal: watch::Sender::new(None),
            fatal_reason: OnceLock::new(),
            home_mask: OnceLock::new(),
            bundles,
            shutdown: Arc::new(watch::Sender::new(None)),
            agent_off: AtomicBool::new(false),
            plan_stopping: Arc::new(AtomicBool::new(false)),
            prune_wake: Notify::new(),
        }
    }

    /// Whether the plan of a planned restart is committed in `stopping`.
    /// Together with [`shutdown_pending`](Self::shutdown_pending) it is the
    /// `shutdown_requested` of spec 0031, *Sinyal per konteks*.
    #[must_use]
    pub fn plan_stopping(&self) -> bool {
        self.plan_stopping.load(Ordering::SeqCst)
    }

    /// Claims the one `agent_off` a daemon runs at a time; `false` means
    /// another one runs.
    pub fn begin_agent_off(&self) -> bool {
        !self.agent_off.swap(true, Ordering::SeqCst)
    }

    /// Ends the claim of [`begin_agent_off`](Self::begin_agent_off) when the
    /// daemon stays up.
    pub fn end_agent_off(&self) {
        self.agent_off.store(false, Ordering::SeqCst);
    }

    /// Whether an `agent_off` runs right now.
    #[must_use]
    pub fn agent_off_active(&self) -> bool {
        self.agent_off.load(Ordering::SeqCst)
    }

    /// The reply of `daemon.get_info`: the facts recorded at start, plus
    /// the registry schema once the registry opened ready.
    #[must_use]
    pub fn info(&self) -> DaemonInfo {
        let mut info = self.info.clone();
        info.registry_schema = match self.registry_state() {
            RegistryState::Ready { schema_version } => Some(schema_version),
            _ => None,
        };
        info.secret_store = Some(self.secrets.store(&self.platform));
        info.session = crate::session::session_info(self.session());
        info.restart = self
            .supervision
            .restart_view()
            .filter(|view| {
                view.finished_at.is_none_or(|finished| {
                    let age = self
                        .platform
                        .clock()
                        .now()
                        .duration_since(finished.as_jiff());
                    age.as_secs()
                        < crate::ipc::restart::RESTART_SUMMARY_TTL
                            .as_secs()
                            .cast_signed()
                })
            })
            .map(Box::new);
        info
    }

    /// The facts recorded at start, which never change during a run.
    #[must_use]
    pub const fn start_info(&self) -> &DaemonInfo {
        &self.info
    }

    /// The platform the daemon runs on.
    #[must_use]
    pub const fn platform(&self) -> &Platform {
        &self.platform
    }

    /// The checked layout.
    #[must_use]
    pub const fn prepared(&self) -> &PreparedLayout {
        &self.prepared
    }

    /// The event bus.
    #[must_use]
    pub const fn bus(&self) -> &Arc<EventBus> {
        &self.bus
    }

    /// The log writer.
    #[must_use]
    pub const fn log(&self) -> &Arc<LogSink> {
        &self.log
    }

    /// The daemon's own counters.
    #[must_use]
    pub const fn counters(&self) -> &Counters {
        &self.counters
    }

    /// The requests that run right now, for the drain at stop.
    #[must_use]
    pub const fn drain(&self) -> &Drain {
        &self.drain
    }

    /// The open connections, for the flush at stop.
    #[must_use]
    pub const fn connections(&self) -> &Connections {
        &self.connections
    }

    /// The operation engine and its health (spec 0007).
    #[must_use]
    pub const fn operations(&self) -> &Operations {
        &self.operations
    }

    /// The process supervisor and its health (spec 0010).
    #[must_use]
    pub const fn supervision(&self) -> &Supervision {
        &self.supervision
    }

    /// The durable log store of the instances, once the start slot opened
    /// it (spec 0027).
    #[must_use]
    pub fn logs(&self) -> Option<&LogService> {
        self.logs.get()
    }

    /// Records the durable log store the start slot opened; a second call
    /// keeps the first.
    pub fn set_logs(&self, logs: LogService) {
        let _ = self.logs.set(logs);
    }

    /// The run's one redactor, shared by the log writer, the error bodies,
    /// the events, and the secret service (spec 0011 AC-13, AC-14).
    #[must_use]
    pub const fn redactor(&self) -> &Arc<Redactor> {
        &self.redactor
    }

    /// The secret service and the facts about its store (spec 0011).
    #[must_use]
    pub const fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    /// The single flight slot of `storage.get_capacity` (spec 0022).
    #[must_use]
    pub const fn storage(&self) -> &Storage {
        &self.storage
    }

    /// The doctor of this run (spec 0034): its two run slots and the checks
    /// that ran past their limit live as long as the run.
    #[must_use]
    pub const fn doctor(&self) -> &stackly_core::doctor::Doctor {
        &self.doctor
    }

    /// A new connection ID: a counter per run, from 1.
    pub fn next_connection_id(&self) -> u64 {
        self.next_connection_id.fetch_add(1, Ordering::Relaxed)
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        *lock(&self.phase)
    }

    /// Whether a stop began, so new requests get `daemon.shutting_down`.
    #[must_use]
    pub fn is_stopping(&self) -> bool {
        self.phase() == Phase::Stopping
    }

    /// Moves to `ready` once the start slot is done, unless a stop began.
    pub fn mark_ready(&self) {
        let mut phase = lock(&self.phase);
        if *phase == Phase::Starting {
            *phase = Phase::Ready;
        }
    }

    /// Moves to `stopping`.
    pub fn mark_stopping(&self) {
        *lock(&self.phase) = Phase::Stopping;
    }

    /// Where the registry stands.
    #[must_use]
    pub fn registry_state(&self) -> RegistryState {
        lock(&self.registry_state).clone()
    }

    /// Whether the registry is in recovery mode.
    #[must_use]
    pub fn in_recovery(&self) -> bool {
        self.recovery().is_some()
    }

    /// The recovery report while the registry is in recovery mode.
    #[must_use]
    pub fn recovery(&self) -> Option<RegistryRecovery> {
        match &*lock(&self.registry_state) {
            RegistryState::Recovery { recovery } => Some(recovery.clone()),
            _ => None,
        }
    }

    /// Records the registry state, and the handle when it is ready.
    pub fn set_registry(&self, state: RegistryState, registry: Option<SqliteRegistry>) {
        *lock(&self.registry) = registry;
        *lock(&self.registry_state) = state;
    }

    /// Marks the registry as no longer working, keeping its handle so the
    /// stop can still close it.
    pub fn mark_registry_failed(&self) {
        *lock(&self.registry_state) = RegistryState::Failed;
    }

    /// The registry handle, while it is ready.
    #[must_use]
    pub fn registry(&self) -> Option<SqliteRegistry> {
        lock(&self.registry).clone()
    }

    /// Takes the registry handle out, so it can be closed before the lock
    /// is released.
    pub fn take_registry(&self) -> Option<SqliteRegistry> {
        lock(&self.registry).take()
    }

    /// The catalog the start slot loaded, or `None` while it loads.
    #[must_use]
    pub fn catalog(&self) -> Option<Arc<CatalogState>> {
        self.catalog_accesses.fetch_add(1, Ordering::Relaxed);
        self.catalog.get().cloned()
    }

    /// The row of the loaded catalog in `provider_sources` (spec 0020
    /// AC-14).
    #[must_use]
    pub fn catalog_source(&self) -> &Arc<catalog::CatalogSource> {
        self.catalog_accesses.fetch_add(1, Ordering::Relaxed);
        &self.catalog_source
    }

    /// How many times code asked for the loaded catalog or its source row
    /// since the daemon started. The offline test of spec 0021 (AC-10)
    /// reads it before and after start, status, detail, and connection to
    /// show they never look at the catalog.
    #[doc(hidden)]
    #[must_use]
    pub fn catalog_accesses(&self) -> u64 {
        self.catalog_accesses.load(Ordering::Relaxed)
    }

    /// The pinned packages the detail methods have read (spec 0021).
    #[must_use]
    pub const fn pinned_packages(&self) -> &PinnedPackages {
        &self.pinned_packages
    }

    /// The sizes of data folders (spec 0021).
    #[must_use]
    pub const fn data_usage(&self) -> &DataUsageService {
        &self.data_usage
    }

    /// The metrics histories of the running instances (spec 0028).
    #[must_use]
    pub const fn metrics(&self) -> &MetricsStore {
        &self.metrics
    }

    /// The constants of the metrics sampler of this run: what the start
    /// slot recorded, else the defaults.
    #[must_use]
    pub fn sampler_config(&self) -> SamplerConfig {
        self.sampler_config.get().copied().unwrap_or_default()
    }

    /// Records the sampler constants of this run. The first call wins.
    pub fn set_sampler_config(&self, config: SamplerConfig) {
        let _ = self.sampler_config.set(config);
    }

    /// The hooks a test hung on `instance.create`; none in a normal run.
    #[must_use]
    pub fn create_hooks(&self) -> CreateHooks {
        self.create_hooks.get().cloned().unwrap_or_default()
    }

    /// Records the create hooks of this run. The first call wins.
    pub fn set_create_hooks(&self, hooks: CreateHooks) {
        let _ = self.create_hooks.set(hooks);
    }

    /// Records the home folder of the user for this run, which the start
    /// resolved once from the user database. The first call wins.
    pub fn set_home(&self, home: PathBuf) {
        let _ = self.home.set(home);
    }

    /// The home folder of the user, for the protected folders a storage
    /// location must stay out of; `None` when the start recorded none.
    #[must_use]
    pub fn home(&self) -> Option<&Path> {
        self.home.get().map(PathBuf::as_path)
    }

    /// Records the login session of this run (spec 0031); the start slot
    /// calls it once, before reconciliation.
    pub fn set_session(&self, session: SessionStatus) {
        let _ = self.session.set(session);
    }

    /// The login session of this run: skipped for a daemon that launchd did
    /// not start, until the start slot records it.
    #[must_use]
    pub fn session(&self) -> &SessionStatus {
        self.session.get().unwrap_or(&SessionStatus::Skipped)
    }

    /// Records the catalog of this run. The first call wins: the catalog
    /// never changes while the daemon runs.
    pub fn set_catalog(&self, state: CatalogState) {
        let _ = self.catalog.set(Arc::new(state));
    }

    /// Marks the socket listener working (`None`) or not, with a safe
    /// sentence for the health detail.
    pub fn set_ipc_problem(&self, problem: Option<String>) {
        *lock(&self.ipc_problem) = problem;
    }

    /// Asks every loop to stop: the accept loop and the prune task.
    pub fn request_stop(&self) {
        self.stop.send_replace(true);
    }

    /// Runs the prune loop now instead of at its next interval; a wake while
    /// a prune runs makes the next one start right after it (spec 0035,
    /// AC-6). The settings handler calls it once a change of the audit
    /// retention committed.
    pub fn wake_prune(&self) {
        self.prune_wake.notify_one();
    }

    /// Resolves at the next [`wake_prune`](Self::wake_prune).
    pub async fn prune_woken(&self) {
        self.prune_wake.notified().await;
    }

    /// Resolves once a stop was requested.
    pub async fn stop_requested(&self) {
        let mut stop = self.stop.subscribe();
        // The sender lives as long as `self`, so this only ends on `true`.
        let _ = stop.wait_for(|stop| *stop).await;
    }

    /// Records a fatal error: the daemon must stop and exit with `code`.
    /// The first code wins.
    pub fn fail(&self, code: u8) {
        self.fatal.send_if_modified(|fatal| {
            if fatal.is_none() {
                *fatal = Some(code);
                true
            } else {
                false
            }
        });
    }

    /// Records a fatal error like [`fail`](Self::fail), with the reason
    /// the record of a failed start keeps (spec 0034, AC-11). The reason of
    /// the first fatal error wins.
    pub fn fail_because(&self, code: u8, reason: impl Into<String>) {
        if self.fatal.borrow().is_none() {
            let _ = self.fatal_reason.set(reason.into());
        }
        self.fail(code);
    }

    /// Records the home folder mask of this run; the first call wins.
    pub fn set_home_mask(&self, mask: HomeMask) {
        let _ = self.home_mask.set(mask);
    }

    /// The home folder mask of this run; one that masks nothing when the
    /// start recorded none.
    #[must_use]
    pub fn home_mask(&self) -> &HomeMask {
        static NONE: HomeMask = HomeMask::none();
        self.home_mask.get().unwrap_or(&NONE)
    }

    /// The staged previews of the diagnostic bundle (spec 0034).
    #[must_use]
    pub const fn bundles(&self) -> &Staging {
        &self.bundles
    }

    /// The reason of the first fatal error, when it was recorded with one.
    #[must_use]
    pub fn fatal_reason(&self) -> Option<&str> {
        self.fatal_reason.get().map(String::as_str)
    }

    /// Records the mode of a `daemon.shutdown` request. Only the first
    /// request counts; `false` means one was already accepted, so the
    /// daemon is as good as stopping.
    pub fn request_shutdown(&self, mode: ShutdownMode) -> bool {
        ShutdownTrigger::request_on(&self.shutdown, mode)
    }

    /// Whether a `daemon.shutdown` request was accepted already.
    #[must_use]
    pub fn shutdown_pending(&self) -> bool {
        self.shutdown.borrow().is_some()
    }

    /// A handle that records a shutdown request without borrowing the
    /// core, for work that outlives the request (a planned restart hands
    /// the daemon over from a task of the operation).
    #[must_use]
    pub fn shutdown_trigger(&self) -> ShutdownTrigger {
        ShutdownTrigger {
            shutdown: Arc::clone(&self.shutdown),
            plan_stopping: Arc::clone(&self.plan_stopping),
        }
    }

    /// Resolves with the mode once a `daemon.shutdown` request was
    /// accepted.
    pub async fn shutdown_requested(&self) -> ShutdownMode {
        let mut requested = self.shutdown.subscribe();
        match requested.wait_for(Option::is_some).await {
            Ok(mode) => mode.unwrap_or(ShutdownMode::Stop),
            // The sender lives as long as `self`, so this cannot happen.
            Err(_) => ShutdownMode::Stop,
        }
    }

    /// The exit code of a fatal error, if one happened.
    #[must_use]
    pub fn fatal_code(&self) -> Option<u8> {
        *self.fatal.borrow()
    }

    /// Resolves with the exit code once a fatal error happened.
    pub async fn fatal(&self) -> u8 {
        let mut fatal = self.fatal.subscribe();
        match fatal.wait_for(Option::is_some).await {
            Ok(code) => code.unwrap_or(1),
            Err(_) => 1,
        }
    }

    /// The reply of `daemon.get_health` at `now` (spec 0006,
    /// *Kesehatan*): `starting` and `stopping` follow the phase; in
    /// between, `degraded` when a built subsystem is degraded or
    /// unavailable, else `ready`. `not_built` never lowers the status.
    #[must_use]
    pub fn health(&self, now: Instant) -> DaemonHealth {
        let ipc = match lock(&self.ipc_problem).clone() {
            None => subsystem_health(subsystem::IPC, SubsystemStatus::Ok, None),
            Some(detail) => {
                subsystem_health(subsystem::IPC, SubsystemStatus::Unavailable, Some(detail))
            }
        };
        let registry_state = self.registry_state();
        let registry = match &registry_state {
            RegistryState::Ready { .. } => {
                subsystem_health(subsystem::REGISTRY, SubsystemStatus::Ok, None)
            }
            RegistryState::Opening => subsystem_health(
                subsystem::REGISTRY,
                SubsystemStatus::Unavailable,
                Some("the registry is opening".to_owned()),
            ),
            RegistryState::Recovery { recovery } => subsystem_health(
                subsystem::REGISTRY,
                SubsystemStatus::Unavailable,
                Some(format!("the registry needs recovery ({})", recovery.reason)),
            ),
            RegistryState::Failed => subsystem_health(
                subsystem::REGISTRY,
                SubsystemStatus::Unavailable,
                Some("the registry stopped working".to_owned()),
            ),
        };
        let mut subsystems = vec![
            ipc,
            subsystem_health(subsystem::EVENT_BUS, SubsystemStatus::Ok, None),
            self.log.health(now),
            registry,
            self.operations.health(),
            self.supervision.health(),
            catalog::health(
                self.catalog.get().map(AsRef::as_ref),
                self.catalog_source.degraded().as_deref(),
            ),
            crate::ipc::logs::health(self.logs().map(|logs| logs.log().health())),
        ];
        if let SessionStatus::Unavailable(detail) = self.session() {
            subsystems.push(subsystem_health(
                subsystem::SESSION,
                SubsystemStatus::Degraded,
                Some(format!("platform.session_unavailable: {detail}")),
            ));
        }
        let status = match self.phase() {
            Phase::Starting => DaemonHealthStatus::Starting,
            Phase::Stopping => DaemonHealthStatus::Stopping,
            Phase::Ready => {
                let lowered = subsystems.iter().any(|health| {
                    matches!(
                        health.status,
                        SubsystemStatus::Degraded | SubsystemStatus::Unavailable
                    )
                });
                if lowered {
                    DaemonHealthStatus::Degraded
                } else {
                    DaemonHealthStatus::Ready
                }
            }
        };
        let counters = &self.counters;
        let logs_health = self.logs().map(|logs| logs.log().health());
        DaemonHealth {
            status,
            subsystems,
            counters: DaemonCounters {
                connections_open: Counters::get(&counters.connections_open),
                connections_total: Counters::get(&counters.connections_total),
                connections_rejected: Counters::get(&counters.connections_rejected),
                frames_rejected: Counters::get(&counters.frames_rejected),
                events_published: self.bus.published(),
                events_dropped: self.bus.dropped(),
                log_lines_dropped: self.log.dropped(),
                socket_rebinds: Counters::get(&counters.socket_rebinds),
                instance_log_truncations: Count::saturating(
                    logs_health.map_or(0, |health| health.truncations),
                ),
                instance_log_bytes_truncated: Count::saturating(
                    logs_health.map_or(0, |health| health.bytes_truncated),
                ),
            },
            recovery: match registry_state {
                RegistryState::Recovery { recovery } => Some(recovery),
                _ => None,
            },
        }
    }
}

fn subsystem_health(
    name: &str,
    status: SubsystemStatus,
    detail: Option<String>,
) -> SubsystemHealth {
    SubsystemHealth {
        name: name.to_owned(),
        status,
        detail,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
