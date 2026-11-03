//! Which instances Stop All stops (spec 0031, *Pilihan stop all*): one pure
//! rule, [`select`], that `instance.preview_stop_all`, `instance.stop_all`,
//! and the Turn Off of the background agent all call, so what the preview
//! shows is what the operation does.
//!
//! [`read_view`] fills the rule's input from the registry, and
//! [`StopAllView::of_instance`] cuts it down to one instance for the check
//! each step makes just before it stops.

use std::collections::BTreeSet;

use stackly_domain::{
    DesiredState, ErrorCodeName, Instance, InstanceFilter, InstanceId, InstanceLifecycle,
    InstanceName, Lease, LeaseFilter, LeaseHolder, LeaseState, OperationKind, OperationResource,
    OperationState, Registry, RegistryError, SessionId, StartupPolicy,
};

use crate::operation::codes::RECOVERY_REQUIRED;
use crate::session::{StartupLeaseState, startup_lease_state};
use crate::supervisor::codes::{IDENTITY_UNPROVEN, STOP_REQUIRES_OVERRIDE};

/// How many times [`read_view`] reads again when a commit lands between its
/// reads.
const READ_TRIES: usize = 3;

/// What one instance contributes to the rule.
#[derive(Debug, Clone, PartialEq)]
pub struct StopAllInstance {
    /// The instance.
    pub id: InstanceId,
    /// Its name.
    pub name: InstanceName,
    /// Its lifecycle.
    pub lifecycle: InstanceLifecycle,
    /// Its desired state.
    pub desired: DesiredState,
    /// Its startup policy.
    pub startup: StartupPolicy,
    /// Whether it is retained.
    pub retained: bool,
    /// Its leases that are active or suppressed.
    pub leases: Vec<Lease>,
    /// Whether a `start_instance` or `restart_instance` of it is queued,
    /// running, or being cancelled.
    pub open_start: bool,
    /// Whether an operation on it needs recovery.
    pub needs_recovery: bool,
}

/// The input of [`select`]: every instance, with its leases and the
/// operations that are not final, from one snapshot of the registry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StopAllView {
    /// The instances, in any order.
    pub instances: Vec<StopAllInstance>,
    /// The login session of this daemon run, for the startup lease.
    pub session: Option<SessionId>,
}

impl StopAllView {
    /// The view of the one instance `id`, or `None` when it is not there.
    #[must_use]
    pub fn of_instance(&self, id: InstanceId) -> Option<Self> {
        let instance = self.instances.iter().find(|item| item.id == id)?;
        Some(Self {
            instances: vec![instance.clone()],
            session: self.session.clone(),
        })
    }
}

/// Where the demand of an instance comes from, by its active lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum DemandSource {
    /// A person started it.
    Manual,
    /// It starts at login and this session holds the lease.
    Startup,
    /// An environment holds it.
    Environment,
}

/// An instance the rule stops.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StopAllEntry {
    /// The instance.
    pub id: InstanceId,
    /// Its name.
    pub name: InstanceName,
    /// Its lifecycle.
    pub lifecycle: InstanceLifecycle,
    /// The sources of its active leases, without repeats.
    pub demand: Vec<DemandSource>,
    /// Its startup policy.
    pub startup: StartupPolicy,
    /// The startup lease of this session.
    pub session_lease: StartupLeaseState,
}

/// An instance the rule names without stopping it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StopAllRef {
    /// The instance.
    pub id: InstanceId,
    /// Its name.
    pub name: InstanceName,
}

/// An instance the rule skips, and the code that says why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StopAllSkipped {
    /// The instance.
    pub id: InstanceId,
    /// Its name.
    pub name: InstanceName,
    /// Why.
    pub code: ErrorCodeName,
}

/// What [`select`] decided, each list ordered by name and then ID.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct StopAllSelection {
    /// The instances to stop.
    pub stop: Vec<StopAllEntry>,
    /// The instances that are stopping already, with no start or restart
    /// open.
    pub already_stopping: Vec<StopAllRef>,
    /// The instances left alone.
    pub skipped: Vec<StopAllSkipped>,
}

/// Decides for every instance of `view`, by the first rule that fits:
///
/// 1. retained: not selected;
/// 2. `unknown`: skipped, `supervisor.identity_unproven`;
/// 3. not `starting`, `running`, or `stopping`, desired not `running`, and
///    no start or restart open: not selected;
/// 4. an operation of it needs recovery: skipped,
///    `operation.recovery_required`;
/// 5. `stopping` with no start or restart open: already stopping;
/// 6. an active environment lease: skipped,
///    `supervisor.stop_requires_override`;
/// 7. otherwise: stop.
#[must_use]
pub fn select(view: &StopAllView) -> StopAllSelection {
    let mut selection = StopAllSelection::default();
    for item in &view.instances {
        if item.retained {
            continue;
        }
        let skip = |code: ErrorCodeName| StopAllSkipped {
            id: item.id,
            name: item.name.clone(),
            code,
        };
        if item.lifecycle == InstanceLifecycle::Unknown {
            selection.skipped.push(skip(IDENTITY_UNPROVEN));
            continue;
        }
        let alive = matches!(
            item.lifecycle,
            InstanceLifecycle::Starting | InstanceLifecycle::Running | InstanceLifecycle::Stopping
        );
        if !alive && item.desired != DesiredState::Running && !item.open_start {
            continue;
        }
        if item.needs_recovery {
            selection.skipped.push(skip(RECOVERY_REQUIRED));
            continue;
        }
        if item.lifecycle == InstanceLifecycle::Stopping && !item.open_start {
            selection.already_stopping.push(StopAllRef {
                id: item.id,
                name: item.name.clone(),
            });
            continue;
        }
        let active: Vec<&Lease> = item
            .leases
            .iter()
            .filter(|lease| lease.is_active())
            .collect();
        if active
            .iter()
            .any(|lease| matches!(lease.holder(), LeaseHolder::Environment(_)))
        {
            selection.skipped.push(skip(STOP_REQUIRES_OVERRIDE));
            continue;
        }
        let mut demand: Vec<DemandSource> = active
            .iter()
            .map(|lease| match lease.holder() {
                LeaseHolder::Manual => DemandSource::Manual,
                LeaseHolder::Startup => DemandSource::Startup,
                LeaseHolder::Environment(_) => DemandSource::Environment,
            })
            .collect();
        demand.sort_unstable();
        demand.dedup();
        selection.stop.push(StopAllEntry {
            id: item.id,
            name: item.name.clone(),
            lifecycle: item.lifecycle,
            demand,
            startup: item.startup,
            session_lease: startup_lease_state(item.id, &item.leases, view.session.as_ref()),
        });
    }
    selection
        .stop
        .sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
    selection
        .already_stopping
        .sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
    selection
        .skipped
        .sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
    selection
}

/// The instances `instance.stop_all` stops when the client confirmed
/// `expected` from a preview (spec 0032 AC-7, AC-8): the `stop` list of
/// `selection` cut down to `expected`. With no `expected`, all of `stop`.
///
/// # Errors
///
/// Returns [`StopAllChanged`] when `expected` and the `stop` list are not the
/// same set. `already_stopping` and `skipped` do not count, so an instance
/// that left the list or joined it since the preview changes the answer, and
/// nothing is stopped.
pub fn confirmed_steps(
    selection: &StopAllSelection,
    expected: Option<&[InstanceId]>,
) -> Result<Vec<InstanceId>, StopAllChanged> {
    let steps: Vec<InstanceId> = selection.stop.iter().map(|entry| entry.id).collect();
    let Some(expected) = expected else {
        return Ok(steps);
    };
    let confirmed: BTreeSet<InstanceId> = expected.iter().copied().collect();
    let chosen: BTreeSet<InstanceId> = steps.iter().copied().collect();
    if confirmed != chosen {
        return Err(StopAllChanged);
    }
    Ok(steps
        .into_iter()
        .filter(|id| confirmed.contains(id))
        .collect())
}

/// The instances Stop All would stop are not the ones the client confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the instances to stop changed since the preview")]
pub struct StopAllChanged;

/// Reads the input of [`select`] from the registry: every instance, the
/// leases that are not released, and the operations that are not final. The
/// reads are separate calls, so each carries its generation and the whole is
/// read again when a commit landed between two of them.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a read.
pub fn read_view(
    registry: &dyn Registry,
    session: Option<&SessionId>,
) -> Result<StopAllView, RegistryError> {
    let mut last = None;
    for _ in 0..READ_TRIES {
        let instances = registry.instances(InstanceFilter {
            include_retained: true,
        })?;
        let operations = registry.unfinished_operations()?;
        let mut consistent = instances.generation == operations.generation;
        let mut items = Vec::with_capacity(instances.value.len());
        for instance in &instances.value {
            let leases = registry.leases(instance.id(), LeaseFilter::Unreleased)?;
            consistent &= leases.generation == instances.generation;
            let (open_start, needs_recovery) = open_work(instance, &operations.value);
            items.push(StopAllInstance {
                id: instance.id(),
                name: instance.name().clone(),
                lifecycle: instance.lifecycle(),
                desired: instance.desired(),
                startup: instance.startup(),
                retained: instance.lifecycle() == InstanceLifecycle::Retained,
                leases: leases
                    .value
                    .into_iter()
                    .filter(|lease| !matches!(lease.state(), LeaseState::Released { .. }))
                    .collect(),
                open_start,
                needs_recovery,
            });
        }
        let view = StopAllView {
            instances: items,
            session: session.cloned(),
        };
        if consistent {
            return Ok(view);
        }
        last = Some(view);
    }
    Ok(last.unwrap_or_default())
}

/// Whether a start or restart of `instance` is open, and whether an
/// operation on it needs recovery.
fn open_work(
    instance: &Instance,
    operations: &[(stackly_domain::Operation, stackly_domain::JournalView)],
) -> (bool, bool) {
    let mut open_start = false;
    let mut needs_recovery = false;
    for (operation, _) in operations {
        if operation.resource() != OperationResource::Instance(instance.id()) {
            continue;
        }
        match operation.state() {
            OperationState::NeedsRecovery => needs_recovery = true,
            OperationState::Queued | OperationState::Running | OperationState::CancelRequested => {
                open_start |= matches!(
                    operation.kind(),
                    OperationKind::StartInstance | OperationKind::RestartInstance
                );
            }
            _ => {}
        }
    }
    (open_start, needs_recovery)
}
