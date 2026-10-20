//! What the daemon knows about itself while it runs (spec 0006,
//! *Kesehatan*): its phase, its subsystems, its counters, and the facts
//! `daemon.get_info` reports.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use stackly_core::PreparedLayout;
use stackly_core::catalog::CatalogState;
use stackly_platform::Platform;
use stackly_protocol::Count;
use stackly_protocol::daemon::{
    DaemonCounters, DaemonHealth, DaemonHealthStatus, DaemonInfo, RegistryRecovery,
    SubsystemHealth, SubsystemStatus, subsystem,
};
use stackly_storage::SqliteRegistry;
use tokio::sync::watch;

use crate::catalog;
use crate::connection::Connections;
use crate::events::EventBus;
use crate::log_sink::LogSink;
use crate::operations::Operations;
use crate::shutdown::Drain;

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
    ipc_problem: Mutex<Option<String>>,
    counters: Counters,
    drain: Drain,
    connections: Connections,
    operations: Operations,
    next_connection_id: AtomicU64,
    stop: watch::Sender<bool>,
    fatal: watch::Sender<Option<u8>>,
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
    /// registry `opening`.
    #[must_use]
    pub fn new(
        info: DaemonInfo,
        platform: Platform,
        prepared: PreparedLayout,
        bus: Arc<EventBus>,
        log: Arc<LogSink>,
    ) -> Self {
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
            ipc_problem: Mutex::new(None),
            counters: Counters::default(),
            drain: Drain::default(),
            connections: Connections::default(),
            operations: Operations::default(),
            next_connection_id: AtomicU64::new(1),
            stop: watch::Sender::new(false),
            fatal: watch::Sender::new(None),
        }
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
        self.catalog.get().cloned()
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
        let subsystems = vec![
            ipc,
            subsystem_health(subsystem::EVENT_BUS, SubsystemStatus::Ok, None),
            self.log.health(now),
            registry,
            self.operations.health(),
            subsystem_health(subsystem::SUPERVISOR, SubsystemStatus::NotBuilt, None),
            catalog::health(self.catalog.get().map(AsRef::as_ref)),
        ];
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
