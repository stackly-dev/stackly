//! The process supervisor's place in the daemon (spec 0010 build step 13):
//! the start slot builds it once the secret service exists, the engine
//! gives it its operation engine, `reconcile` runs between `recover` and
//! `ready`, `start_reconciled` after the engine starts, and the stop ends
//! its actors without a signal or a commit. [`Supervision`] also answers
//! for the `supervisor` subsystem in `daemon.get_health`.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use stackly_core::PreparedLayout;
use stackly_core::secret::{Redactor, SecretService};
use stackly_core::supervisor::{
    LogSink, ReconcilePhase, ReconcileReport, RegistryMode, SinkSet, Supervisor, SupervisorConfig,
    SupervisorDeps, SupervisorError, SupervisorEvents,
};
use stackly_domain::{Instance, InstanceId, Registry, ResourceRevision};
use stackly_platform::Platform;
use stackly_protocol::codes::registry;
use stackly_protocol::daemon::{SubsystemHealth, SubsystemStatus, subsystem};
use stackly_protocol::events::EventKind;
use stackly_protocol::instance::{
    DaemonReconciling, InstanceStateChanged, ReconcilePhase as WirePhase,
};
use stackly_protocol::{Count, ErrorBody, ResourceKind, ResourceRef};

use crate::events::EventBus;
use crate::ipc::instance::dto;
use crate::ipc::operation::dto::revision;
use crate::ipc::operation::starting;

/// Where the supervisor stands in this daemon run.
#[derive(Clone)]
enum Slot {
    /// The registry is not open yet.
    Waiting,
    /// Reconciliation runs; instance methods answer `daemon.starting`.
    Reconciling(Supervisor),
    /// Reconciled; the supervisor serves.
    Ready(Supervisor),
    /// No supervisor in this run, for the reason named.
    Unavailable(&'static str),
}

/// What the `supervisor` subsystem shows besides the slot.
#[derive(Debug, Default)]
struct Report {
    /// Set while the registry cannot take an actor commit.
    degraded: Option<String>,
    /// Instances that read `unknown` after reconciliation.
    unknown: u32,
}

/// What [`Supervision::build`] needs from the start slot.
pub struct BuildInput<'a> {
    /// The ready registry.
    pub registry: Arc<dyn Registry>,
    /// The platform the daemon runs on.
    pub platform: &'a Platform,
    /// The checked layout.
    pub prepared: &'a PreparedLayout,
    /// The secret service.
    pub secrets: Arc<SecretService>,
    /// The run's one redactor.
    pub redactor: Arc<Redactor>,
    /// The event bus.
    pub bus: &'a Arc<EventBus>,
    /// Sinks that see every redacted log line besides the tail.
    pub extra_sinks: &'a [Arc<dyn LogSink>],
    /// The constants.
    pub config: SupervisorConfig,
}

/// The daemon's supervisor and the facts the health shows about it.
pub struct Supervision {
    slot: Mutex<Slot>,
    report: Arc<Mutex<Report>>,
    events: OnceLock<Arc<BusSupervisorEvents>>,
}

impl std::fmt::Debug for Supervision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervision").finish_non_exhaustive()
    }
}

impl Default for Supervision {
    fn default() -> Self {
        Self {
            slot: Mutex::new(Slot::Waiting),
            report: Arc::new(Mutex::new(Report::default())),
            events: OnceLock::new(),
        }
    }
}

impl Supervision {
    /// Creates the supervisor on the ready registry. Instance methods
    /// answer `daemon.starting` until [`reconciled`](Self::reconciled).
    #[must_use]
    pub fn build(&self, input: BuildInput<'_>) -> Supervisor {
        let handle = Arc::new(OnceLock::new());
        let events = Arc::new(BusSupervisorEvents {
            bus: Arc::clone(input.bus),
            supervisor: Arc::clone(&handle),
            report: Arc::clone(&self.report),
        });
        let mut deps = SupervisorDeps::new(
            input.registry,
            Arc::new(input.platform.clone()),
            Arc::new(input.prepared.clone()),
            input.secrets,
            input.redactor,
        );
        let mut sinks: Vec<Arc<dyn LogSink>> = vec![Arc::clone(&deps.tail) as Arc<dyn LogSink>];
        sinks.extend(input.extra_sinks.iter().cloned());
        deps.sinks = Arc::new(SinkSet::new(sinks));
        deps.events = Arc::clone(&events) as Arc<dyn SupervisorEvents>;
        deps.config = input.config;
        let supervisor = Supervisor::new(deps);
        let _ = handle.set(supervisor.clone());
        let _ = self.events.set(events);
        *self.lock() = Slot::Reconciling(supervisor.clone());
        supervisor
    }

    /// The events the supervisor publishes, once it is built.
    #[must_use]
    pub fn events(&self) -> Option<Arc<BusSupervisorEvents>> {
        self.events.get().cloned()
    }

    /// Reconciles what the last daemon left running (spec 0010,
    /// *Rekonsiliasi*), then opens the supervisor to instance methods.
    /// Returns the report; a failure leaves the supervisor unavailable.
    pub async fn reconcile(&self, supervisor: &Supervisor) -> Option<ReconcileReport> {
        match supervisor.reconcile(RegistryMode::Ready).await {
            Ok(report) => {
                self.lock_report().unknown = report.unknown;
                self.reconciled(supervisor);
                Some(report)
            }
            Err(error) => {
                tracing::error!(error = %error, "reconciliation failed; the supervisor stays down");
                self.unavailable("reconciliation failed");
                None
            }
        }
    }

    fn reconciled(&self, supervisor: &Supervisor) {
        let mut slot = self.lock();
        if matches!(&*slot, Slot::Reconciling(_)) {
            *slot = Slot::Ready(supervisor.clone());
        }
    }

    /// There is no supervisor in this run, for the reason in `detail`.
    pub fn unavailable(&self, detail: &'static str) {
        *self.lock() = Slot::Unavailable(detail);
    }

    /// The supervisor for an instance method.
    ///
    /// # Errors
    ///
    /// Returns `daemon.starting` (subsystem `supervisor`) until the start
    /// slot reconciled, and `registry.unavailable` when this run has no
    /// supervisor.
    pub fn supervisor(&self) -> Result<Supervisor, Box<ErrorBody>> {
        match &*self.lock() {
            Slot::Ready(supervisor) => Ok(supervisor.clone()),
            Slot::Waiting | Slot::Reconciling(_) => Err(Box::new(starting(subsystem::SUPERVISOR))),
            Slot::Unavailable(_) => Err(Box::new(ErrorBody::new(
                &registry::UNAVAILABLE,
                "the registry is not available",
            ))),
        }
    }

    /// The `supervisor` subsystem of `daemon.get_health`.
    #[must_use]
    pub fn health(&self) -> SubsystemHealth {
        let report = self.lock_report();
        let (status, detail) = match &*self.lock() {
            Slot::Waiting => (
                SubsystemStatus::Unavailable,
                Some("waiting for the registry".to_owned()),
            ),
            Slot::Reconciling(_) => (
                SubsystemStatus::Unavailable,
                Some("reconciling the instances".to_owned()),
            ),
            Slot::Unavailable(detail) => (SubsystemStatus::Unavailable, Some((*detail).to_owned())),
            Slot::Ready(_) => match (&report.degraded, report.unknown) {
                (Some(detail), _) => (SubsystemStatus::Degraded, Some(detail.clone())),
                (None, 0) => (SubsystemStatus::Ok, None),
                (None, 1) => (
                    SubsystemStatus::Ok,
                    Some("1 instance is unknown".to_owned()),
                ),
                (None, count) => (
                    SubsystemStatus::Ok,
                    Some(format!("{count} instances are unknown")),
                ),
            },
        };
        SubsystemHealth {
            name: subsystem::SUPERVISOR.to_owned(),
            status,
            detail,
        }
    }

    /// Ends the actors, probes, spool readers, and exit handling without
    /// a signal or a commit (spec 0010 AC-21). Calling it twice changes
    /// nothing.
    pub fn shutdown(&self) {
        let supervisor = match &*self.lock() {
            Slot::Reconciling(supervisor) | Slot::Ready(supervisor) => Some(supervisor.clone()),
            Slot::Waiting | Slot::Unavailable(_) => None,
        };
        if let Some(supervisor) = supervisor {
            supervisor.shutdown();
        }
    }

    /// Lets go of the supervisor at the end of the stop, so the registry
    /// handle it holds closes.
    pub fn close(&self) {
        *self.lock() = Slot::Unavailable("the daemon stopped");
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_report(&self) -> MutexGuard<'_, Report> {
        self.report.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Publishes what the supervisor reports as events on the bus (spec 0010,
/// *Status dan event*) and keeps the `supervisor` subsystem's degraded
/// flag.
pub struct BusSupervisorEvents {
    bus: Arc<EventBus>,
    supervisor: Arc<OnceLock<Supervisor>>,
    report: Arc<Mutex<Report>>,
}

impl std::fmt::Debug for BusSupervisorEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusSupervisorEvents")
            .finish_non_exhaustive()
    }
}

impl BusSupervisorEvents {
    fn resource(id: InstanceId) -> ResourceRef {
        ResourceRef {
            kind: ResourceKind::Instance,
            id: id.to_string(),
        }
    }
}

impl SupervisorEvents for BusSupervisorEvents {
    fn instance_changed(&self, instance: &Instance) {
        let Some(supervisor) = self.supervisor.get() else {
            return;
        };
        let status = supervisor.status(instance.id());
        let excerpt = if instance.last_failure().is_some() {
            supervisor
                .failure_excerpt(instance.id())
                .iter()
                .map(|line| dto::excerpt_line(line))
                .collect()
        } else {
            Vec::new()
        };
        let summary = dto::summary(instance, &status, excerpt);
        self.bus.publish_resource(
            EventKind::InstanceStateChanged(Box::new(InstanceStateChanged { instance: summary })),
            Self::resource(instance.id()),
            revision(instance.revision()),
        );
    }

    fn status_changed(
        &self,
        instance: InstanceId,
        status: &stackly_core::supervisor::InstanceStatus,
        at: ResourceRevision,
    ) {
        self.bus.publish_resource(
            EventKind::InstanceHealthChanged(Box::new(dto::health_changed(instance, status))),
            Self::resource(instance),
            revision(at),
        );
    }

    fn registry_degraded(&self, degraded: bool, detail: &str) {
        self.report
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .degraded = degraded.then(|| detail.to_owned());
    }

    fn reconciling(&self, phase: &ReconcilePhase) {
        let content = match phase {
            ReconcilePhase::Started => DaemonReconciling {
                phase: WirePhase::Started,
                instances: Count::saturating(0),
                adopted: Count::saturating(0),
                unknown: Count::saturating(0),
                restarting: Count::saturating(0),
            },
            ReconcilePhase::Finished(report) => DaemonReconciling {
                phase: WirePhase::Finished,
                instances: Count::saturating(u64::from(report.instances)),
                adopted: Count::saturating(u64::from(report.adopted)),
                unknown: Count::saturating(u64::from(report.unknown)),
                restarting: Count::saturating(u64::from(report.restarting)),
            },
            _ => return,
        };
        self.bus
            .publish_daemon(EventKind::DaemonReconciling(content));
    }
}

/// The error body of a supervisor failure that a method answers directly.
#[must_use]
pub fn supervisor_error(error: &SupervisorError) -> ErrorBody {
    crate::ipc::operation::operation_error_body(error)
}
