//! The operation engine's place in the daemon (spec 0007, *Start daemon
//! dan pemulihan* and *Shutdown*): the start slot creates it once the
//! registry is ready, `recover` runs before `ready`, `start` runs after,
//! and the stop gives it [`OPERATION_SHUTDOWN_GRACE`] alongside the drain.
//! [`Operations`] also answers for the `operations` subsystem in
//! `daemon.get_health`.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_core::PreparedLayout;
use stackly_core::operation::{
    EngineDeps, EngineLimits, OperationEngine, Redact, ShutdownReport,
    limits::OPERATION_SHUTDOWN_GRACE,
};
use stackly_core::secret::Redactor;
use stackly_domain::Registry;
use stackly_platform::Platform;
use stackly_protocol::ErrorBody;
use stackly_protocol::codes::registry;
use stackly_protocol::daemon::{SubsystemHealth, SubsystemStatus, subsystem};
use stackly_storage::SqliteRegistry;

use crate::events::EventBus;
use crate::ipc::operation::{BusEvents, starting};

/// Where the engine stands in this daemon run.
#[derive(Debug, Clone)]
enum Slot {
    /// The registry is not open yet.
    Waiting,
    /// `recover` runs; `get` and `list` are served, everything else gets
    /// `daemon.starting` from the engine.
    Recovering(OperationEngine),
    /// Recovered; the dispatcher opens once the daemon is ready.
    Ready(OperationEngine),
    /// `recover` failed; reads still work, the rest is not ready.
    Failed(OperationEngine),
    /// No engine for this run: the registry is in recovery mode or broke.
    Unavailable(&'static str),
}

/// The daemon's operation engine and its health.
#[derive(Debug)]
pub struct Operations {
    slot: Mutex<Slot>,
}

impl Default for Operations {
    fn default() -> Self {
        Self {
            slot: Mutex::new(Slot::Waiting),
        }
    }
}

impl Operations {
    /// Creates the engine on the ready registry, publishing its events on
    /// `bus`, and holds it as recovering until [`Operations::recovered`].
    #[must_use]
    pub fn create(
        &self,
        registry: SqliteRegistry,
        platform: &Platform,
        prepared: &PreparedLayout,
        bus: &Arc<EventBus>,
    ) -> OperationEngine {
        // provisional: F11 (spec 0011 step 5 hands the engine the daemon's
        // one shared Redactor, and stacklyd registers the `secret_ref`
        // compensator on top of the secret delete use case)
        let redactor: Arc<dyn Redact> = Arc::new(Redactor::new());
        let engine = OperationEngine::new(
            EngineDeps {
                registry: Arc::new(registry) as Arc<dyn Registry>,
                platform: Arc::new(platform.clone()),
                layout: Arc::new(prepared.clone()),
                events: Arc::new(BusEvents::new(Arc::clone(bus))),
                redactor,
            },
            EngineLimits::default(),
        );
        *self.lock() = Slot::Recovering(engine.clone());
        engine
    }

    /// `recover` ended: the engine waits for `start`.
    pub fn recovered(&self) {
        let mut slot = self.lock();
        if let Slot::Recovering(engine) = &*slot {
            *slot = Slot::Ready(engine.clone());
        }
    }

    /// `recover` failed: the engine keeps serving reads.
    pub fn recovery_failed(&self) {
        let mut slot = self.lock();
        if let Slot::Recovering(engine) = &*slot {
            *slot = Slot::Failed(engine.clone());
        }
    }

    /// There is no engine in this run, for the reason in `detail`.
    pub fn unavailable(&self, detail: &'static str) {
        *self.lock() = Slot::Unavailable(detail);
    }

    /// The engine, once it exists.
    #[must_use]
    pub fn engine(&self) -> Option<OperationEngine> {
        match &*self.lock() {
            Slot::Recovering(engine) | Slot::Ready(engine) | Slot::Failed(engine) => {
                Some(engine.clone())
            }
            Slot::Waiting | Slot::Unavailable(_) => None,
        }
    }

    /// The engine for a method that only reads operations: served as soon
    /// as the registry is ready (spec 0007, `get` and `list`).
    ///
    /// # Errors
    ///
    /// Returns `daemon.starting` (subsystem `registry`) while the registry
    /// opens, and `registry.unavailable` when this run has no engine.
    pub fn reader(&self) -> Result<OperationEngine, Box<ErrorBody>> {
        self.engine_or(subsystem::REGISTRY)
    }

    /// The engine for a method that changes operations or reads the
    /// engine's memory; while it recovers, the engine itself answers
    /// `daemon.starting` with subsystem `operations`.
    ///
    /// # Errors
    ///
    /// Returns `daemon.starting` (subsystem `operations`) while the
    /// registry opens, and `registry.unavailable` when this run has no
    /// engine.
    pub fn writer(&self) -> Result<OperationEngine, Box<ErrorBody>> {
        self.engine_or(subsystem::OPERATIONS)
    }

    fn engine_or(&self, waiting_for: &str) -> Result<OperationEngine, Box<ErrorBody>> {
        match &*self.lock() {
            Slot::Recovering(engine) | Slot::Ready(engine) | Slot::Failed(engine) => {
                Ok(engine.clone())
            }
            Slot::Waiting => Err(Box::new(starting(waiting_for))),
            Slot::Unavailable(_) => Err(Box::new(ErrorBody::new(
                &registry::UNAVAILABLE,
                "the registry is not available",
            ))),
        }
    }

    /// The `operations` subsystem of `daemon.get_health` (spec 0007 AC-5):
    /// `ok`, `degraded` with the number of operations in `needs_recovery`,
    /// or `unavailable` while there is no recovered engine.
    #[must_use]
    pub fn health(&self) -> SubsystemHealth {
        let (status, detail) = match &*self.lock() {
            Slot::Waiting => (
                SubsystemStatus::Unavailable,
                Some("waiting for the registry".to_owned()),
            ),
            Slot::Recovering(_) => (
                SubsystemStatus::Unavailable,
                Some("recovering the operations of the last run".to_owned()),
            ),
            Slot::Failed(_) => (
                SubsystemStatus::Unavailable,
                Some("the operations of the last run could not be recovered".to_owned()),
            ),
            Slot::Unavailable(detail) => (SubsystemStatus::Unavailable, Some((*detail).to_owned())),
            Slot::Ready(engine) => match engine.stats().needs_recovery {
                0 => (SubsystemStatus::Ok, None),
                1 => (
                    SubsystemStatus::Degraded,
                    Some("1 operation needs recovery".to_owned()),
                ),
                count => (
                    SubsystemStatus::Degraded,
                    Some(format!("{count} operations need recovery")),
                ),
            },
        };
        SubsystemHealth {
            name: subsystem::OPERATIONS.to_owned(),
            status,
            detail,
        }
    }

    /// Stops the engine within [`OPERATION_SHUTDOWN_GRACE`] (spec 0007,
    /// *Shutdown*), or does nothing when this run has no engine.
    pub async fn shutdown(&self) -> Option<ShutdownReport> {
        let engine = self.engine()?;
        Some(engine.shutdown(OPERATION_SHUTDOWN_GRACE).await)
    }

    /// Lets go of the engine at the end of the stop, so the registry
    /// handle it holds closes once its own tasks have ended.
    pub fn close(&self) {
        *self.lock() = Slot::Unavailable("the daemon stopped");
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
