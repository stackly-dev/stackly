//! What the supervisor runs on, and the supervisor itself: the registry of
//! per-instance actors every operation of spec 0010 works through.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use stackly_domain::{Instance, InstanceId, Registry, ResourceRevision};
use stackly_platform::Platform;

use super::actor::InstanceActor;
use super::sink::{LogSink, SinkSet, TailSink};
use super::status::InstanceStatus;
use super::{PortProbe, SupervisorConfig};
use crate::layout::PreparedLayout;
use crate::operation::OperationEngine;
use crate::secret::{Redactor, SecretService};

/// Receives what a client can see change, in commit order for each
/// instance. `stacklyd` publishes each call as an event and maps the values
/// to DTOs; tests record them.
pub trait SupervisorEvents: Send + Sync + 'static {
    /// A batch raised the revision of an instance (`instance.state_changed`
    /// carries the instance with the status of the moment).
    fn instance_changed(&self, instance: &Instance);

    /// The status of an instance changed in a field a client can see
    /// (`instance.health_changed`). `status.status_seq` orders it;
    /// `revision` is the instance's revision when it was made.
    fn status_changed(
        &self,
        instance: InstanceId,
        status: &InstanceStatus,
        revision: ResourceRevision,
    );

    /// The registry could not take an actor commit (`degraded` is true), or
    /// accepted one again after that (`degraded` is false). The daemon
    /// reports the `supervisor` subsystem from it.
    fn registry_degraded(&self, degraded: bool, detail: &str) {
        let _ = (degraded, detail);
    }

    /// Reconciliation began or ended (`daemon.reconciling`); the finished
    /// phase carries the counts, and the daemon reports the `supervisor`
    /// subsystem `ok` from it, with the number of `unknown` instances.
    fn reconciling(&self, phase: &super::ReconcilePhase) {
        let _ = phase;
    }
}

/// An event sink that drops everything.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSupervisorEvents;

impl SupervisorEvents for NoSupervisorEvents {
    fn instance_changed(&self, _instance: &Instance) {}

    fn status_changed(
        &self,
        _instance: InstanceId,
        _status: &InstanceStatus,
        _revision: ResourceRevision,
    ) {
    }
}

/// Everything the supervisor works with.
#[derive(Clone)]
#[non_exhaustive]
pub struct SupervisorDeps {
    /// The registry: aggregates, observations, and config revisions.
    pub registry: Arc<dyn Registry>,
    /// The platform ports: processes, filesystem, clock, identity.
    pub platform: Arc<Platform>,
    /// The checked layout.
    pub layout: Arc<PreparedLayout>,
    /// Reads the secrets an instance needs and learns their values.
    pub secrets: Arc<SecretService>,
    /// Hides known secrets in every text that leaves the supervisor.
    pub redactor: Arc<Redactor>,
    /// The in memory tail of every instance's output, the source of the
    /// failure excerpt. Put it in `sinks` too, or nothing reaches it.
    pub tail: Arc<TailSink>,
    /// Where redacted log lines go. [`SupervisorDeps::new`] makes a set
    /// that holds [`tail`](Self::tail); the daemon replaces it with a set
    /// that also holds its other sinks.
    pub sinks: Arc<dyn LogSink>,
    /// Trial binds, injectable in tests.
    pub ports: Arc<dyn PortProbe>,
    /// Where changes are reported.
    pub events: Arc<dyn SupervisorEvents>,
    /// The constants.
    pub config: SupervisorConfig,
}

impl SupervisorDeps {
    /// Bundles the dependencies, with real sockets for the port probe, no
    /// events, and the production constants. Replace a field to change one.
    #[must_use]
    pub fn new(
        registry: Arc<dyn Registry>,
        platform: Arc<Platform>,
        layout: Arc<PreparedLayout>,
        secrets: Arc<SecretService>,
        redactor: Arc<Redactor>,
    ) -> Self {
        let tail = Arc::new(TailSink::default());
        let sinks: Arc<dyn LogSink> =
            Arc::new(SinkSet::new(vec![Arc::clone(&tail) as Arc<dyn LogSink>]));
        Self {
            registry,
            platform,
            layout,
            secrets,
            redactor,
            tail,
            sinks,
            ports: Arc::new(super::SocketPortProbe::new()),
            events: Arc::new(NoSupervisorEvents),
            config: SupervisorConfig::default(),
        }
    }
}

impl fmt::Debug for SupervisorDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SupervisorDeps").finish_non_exhaustive()
    }
}

/// The state every clone of a [`Supervisor`] shares.
pub(crate) struct Shared {
    pub(crate) deps: SupervisorDeps,
    actors: Mutex<HashMap<InstanceId, Arc<InstanceActor>>>,
    /// The engine the actors begin their own operations through (a
    /// respawn after a crash, a start after reconciliation); set once by
    /// the daemon.
    operations: OnceLock<OperationEngine>,
    /// Set when the daemon shuts down: nothing new runs, and no actor
    /// commits a lifecycle any more.
    shutdown: AtomicBool,
    /// What reconciliation decided to run again, taken once by
    /// [`Supervisor::start_reconciled`].
    pub(crate) pending: Mutex<Vec<super::pending::PendingRun>>,
    /// Whether the recheck task of `unknown` instances runs.
    pub(crate) recheck_running: AtomicBool,
}

/// The process supervisor: one actor per instance, and the operations that
/// start, stop, and reload an instance's process and initialize its data.
///
/// Clones share their actors. The daemon makes one and hands clones to its
/// operation works.
#[derive(Clone)]
pub struct Supervisor {
    pub(crate) shared: Arc<Shared>,
}

impl fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Supervisor").finish_non_exhaustive()
    }
}

impl Supervisor {
    /// Creates a supervisor with no actors yet.
    #[must_use]
    pub fn new(deps: SupervisorDeps) -> Self {
        Self {
            shared: Arc::new(Shared {
                deps,
                actors: Mutex::new(HashMap::new()),
                operations: OnceLock::new(),
                shutdown: AtomicBool::new(false),
                pending: Mutex::new(Vec::new()),
                recheck_running: AtomicBool::new(false),
            }),
        }
    }

    /// Hands the supervisor the operation engine its actors start system
    /// operations through: the respawn that follows a crash (trigger
    /// `restart_policy`) and the starts of reconciliation. The engine is
    /// built after the supervisor, because its operation works hold a
    /// clone of it, so the daemon calls this once, before the engine
    /// starts. Until then a crashed instance stays `recovering` with
    /// nothing scheduled, and the supervisor logs a warning. A second call
    /// changes nothing.
    pub fn attach_operations(&self, engine: OperationEngine) {
        if self.shared.operations.set(engine).is_err() {
            tracing::warn!("the supervisor already has an operation engine");
        }
    }

    pub(crate) fn operations(&self) -> Option<&OperationEngine> {
        self.shared.operations.get()
    }

    /// Stops the supervisor for a daemon shutdown (spec 0010 AC-21): the
    /// probes, the spool readers, the respawn pauses, the recheck, and the
    /// handling of exits end, and no instance process is signalled and no
    /// lifecycle committed. An operation that still runs is left for the
    /// engine to end; the next daemon closes it and reconciliation adopts
    /// the process. Calling it again changes nothing.
    pub fn shutdown(&self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        let actors: Vec<_> = self
            .shared
            .actors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for actor in actors {
            actor.stop_background(self.deps());
        }
    }

    /// Returns true once [`shutdown`](Self::shutdown) was called.
    pub(crate) fn is_shutdown(&self) -> bool {
        self.shared.shutdown.load(Ordering::SeqCst)
    }

    /// Returns the actors that exist now, in no order.
    pub(crate) fn actors(&self) -> Vec<Arc<InstanceActor>> {
        self.shared
            .actors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    pub(crate) fn deps(&self) -> &SupervisorDeps {
        &self.shared.deps
    }

    /// Returns the actor of `id`, made on first use.
    pub(crate) fn actor(&self, id: InstanceId) -> Arc<InstanceActor> {
        let mut actors = self
            .shared
            .actors
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Arc::clone(actors.entry(id).or_insert_with(|| {
            Arc::new(InstanceActor::new(
                id,
                self.shared.deps.platform.clock().now(),
            ))
        }))
    }

    /// Returns the failure excerpt of `id`: the last lines it wrote,
    /// stderr first, then stdout, already redacted, at most
    /// [`FAILURE_EXCERPT_LINES`](super::FAILURE_EXCERPT_LINES) of at most
    /// [`FAILURE_EXCERPT_LINE_CHARS`](super::FAILURE_EXCERPT_LINE_CHARS)
    /// characters. Empty when the instance wrote nothing since it last
    /// started.
    #[must_use]
    pub fn failure_excerpt(&self, id: InstanceId) -> Vec<String> {
        self.deps()
            .tail
            .excerpt_of(id, self.deps().config.failure_excerpt_lines)
    }

    /// Returns the status of `id` as read now: readiness and health through
    /// `effective`, so a value older than
    /// [`readiness_max_age`](SupervisorConfig::readiness_max_age) reads as
    /// `unknown` with reason `stale`. An instance the supervisor has not
    /// touched yet reads as `unknown` with reason `daemon_restarted`.
    #[must_use]
    pub fn status(&self, id: InstanceId) -> InstanceStatus {
        self.actor(id).snapshot(
            self.deps().platform.clock().now(),
            self.deps().config.readiness_max_age,
        )
    }
}
