//! Which instances Down stops (spec 0047, *Pilihan Down*): one pure rule,
//! [`select`], that `env.preview_down`, the precheck of `env.down`, the
//! driver's check right before each child, and the final sweep all call,
//! so what the confirmation shows is what Down does.
//!
//! [`read_view`] fills the rule's input from the registry, and
//! [`DownView::of_instance`] cuts it down to one instance.

use std::collections::BTreeSet;

use jiff::Timestamp;
use stackly_domain::{
    DesiredState, Environment, EnvironmentId, EnvironmentMember, EnvironmentName, ErrorCodeName,
    Instance, InstanceFilter, InstanceId, InstanceLifecycle, InstanceName, Lease, LeaseFilter,
    LeaseHolder, LeaseState, MemberAlias, Operation, OperationKind, OperationResource,
    OperationState, Ownership, Registry, RegistryError, ResourceRevision, StopDecision,
    StopRequest, evaluate_stop,
};

use crate::instance::stop_all::DemandSource;
use crate::operation::codes::RECOVERY_REQUIRED;
use crate::supervisor::codes::IDENTITY_UNPROVEN;
use crate::supervisor::{EnvironmentHolder, environment_holders};

/// How many times [`read_view`] reads again when a commit lands between its
/// reads.
const READ_TRIES: usize = 3;

/// The membership of an instance in the environment that goes down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownMember {
    /// Its alias.
    pub alias: MemberAlias,
    /// How the environment holds it.
    pub ownership: Ownership,
}

/// What one instance contributes to the rule.
#[derive(Debug, Clone, PartialEq)]
pub struct DownInstance {
    /// The instance.
    pub id: InstanceId,
    /// Its name.
    pub name: InstanceName,
    /// Its lifecycle.
    pub lifecycle: InstanceLifecycle,
    /// Its desired state.
    pub desired: DesiredState,
    /// Whether it is retained.
    pub retained: bool,
    /// Its leases that are active or suppressed.
    pub leases: Vec<Lease>,
    /// The other environments whose lease on it is active, by name.
    pub holders: Vec<EnvironmentHolder>,
    /// Its membership in the environment that goes down; `None` for an
    /// instance that holds only a lease of it.
    pub member: Option<DownMember>,
    /// Whether a `start_instance` or `restart_instance` of it is queued,
    /// running, or being cancelled.
    pub open_start: bool,
    /// Whether an operation on it needs recovery.
    pub needs_recovery: bool,
}

/// The input of [`select`]: the environment, its members, and every
/// instance that is a member or holds one of its leases, from one snapshot
/// of the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct DownView {
    /// The environment that goes down.
    pub environment: EnvironmentId,
    /// Its name.
    pub name: EnvironmentName,
    /// Its revision.
    pub revision: ResourceRevision,
    /// Its members, `detached` ones included, with their dependency
    /// conditions.
    pub members: Vec<EnvironmentMember>,
    /// The instances, in any order.
    pub instances: Vec<DownInstance>,
    /// How many leases of the environment are active.
    pub lease_count: usize,
    /// The oldest `acquired_at` of those leases.
    pub held_since: Option<Timestamp>,
}

impl DownView {
    /// The view of the one instance `id`, or `None` when it is not there.
    #[must_use]
    pub fn of_instance(&self, id: InstanceId) -> Option<Self> {
        let instance = self.instances.iter().find(|item| item.id == id)?;
        Some(Self {
            instances: vec![instance.clone()],
            ..self.clone()
        })
    }
}

/// An instance as the selection names it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DownEntry {
    /// The instance.
    pub id: InstanceId,
    /// Its name.
    pub name: InstanceName,
    /// Its membership, when it is a member.
    pub member: Option<DownMember>,
    /// Its lifecycle.
    pub lifecycle: InstanceLifecycle,
}

/// An instance that keeps running, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DownKept {
    /// The instance.
    pub entry: DownEntry,
    /// The sources of the other active leases, without repeats.
    pub demand: Vec<DemandSource>,
    /// The other environments whose lease is active.
    pub environments: Vec<EnvironmentHolder>,
}

/// An instance Down can't stop, and the code that says why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DownBlocked {
    /// The instance.
    pub entry: DownEntry,
    /// Why.
    pub code: ErrorCodeName,
}

/// What [`select`] decided, each list ordered by name and then ID.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DownSelection {
    /// The instances to stop.
    pub stop: Vec<DownEntry>,
    /// The instances that keep running because another lease holds them.
    pub keep: Vec<DownKept>,
    /// The instances that are stopped already.
    pub idle: Vec<DownEntry>,
    /// The instances Down can't stop.
    pub blocked: Vec<DownBlocked>,
}

impl DownSelection {
    /// The IDs of [`Self::stop`], in order.
    #[must_use]
    pub fn stop_ids(&self) -> Vec<InstanceId> {
        self.stop.iter().map(|entry| entry.id).collect()
    }

    /// Whether `expected` is the same set as [`Self::stop`] (*Precheck
    /// Down* step 4).
    #[must_use]
    pub fn matches(&self, expected: &[InstanceId]) -> bool {
        let expected: BTreeSet<InstanceId> = expected.iter().copied().collect();
        let chosen: BTreeSet<InstanceId> = self.stop.iter().map(|entry| entry.id).collect();
        expected == chosen
    }
}

/// Decides for every instance of `view`, by the first rule that fits:
///
/// 1. retained: not selected;
/// 2. `evaluate_stop(EnvironmentDown)` keeps it running: keep, with the
///    source of every other active lease;
/// 3. `unknown`: blocked, `supervisor.identity_unproven`;
/// 4. not `starting`, `running`, `recovering`, or `stopping`, desired not
///    `running`, and no start or restart open: idle;
/// 5. an operation of it needs recovery: blocked,
///    `operation.recovery_required`;
/// 6. otherwise: stop.
///
/// A `suppressed` lease is no demand, as in `evaluate_stop`.
#[must_use]
pub fn select(view: &DownView) -> DownSelection {
    let mut selection = DownSelection::default();
    let request = StopRequest::EnvironmentDown(view.environment);
    for item in &view.instances {
        if item.retained {
            continue;
        }
        let entry = DownEntry {
            id: item.id,
            name: item.name.clone(),
            member: item.member.clone(),
            lifecycle: item.lifecycle,
        };
        if let StopDecision::KeepRunning { remaining, .. } =
            evaluate_stop(item.id, &item.leases, &request)
        {
            let mut demand: Vec<DemandSource> = item
                .leases
                .iter()
                .filter(|lease| remaining.contains(&lease.id()))
                .map(|lease| match lease.holder() {
                    LeaseHolder::Manual => DemandSource::Manual,
                    LeaseHolder::Startup => DemandSource::Startup,
                    LeaseHolder::Environment(_) => DemandSource::Environment,
                })
                .collect();
            demand.sort_unstable();
            demand.dedup();
            selection.keep.push(DownKept {
                entry,
                demand,
                environments: item.holders.clone(),
            });
            continue;
        }
        if item.lifecycle == InstanceLifecycle::Unknown {
            selection.blocked.push(DownBlocked {
                entry,
                code: IDENTITY_UNPROVEN,
            });
            continue;
        }
        let alive = matches!(
            item.lifecycle,
            InstanceLifecycle::Starting
                | InstanceLifecycle::Running
                | InstanceLifecycle::Recovering
                | InstanceLifecycle::Stopping
        );
        if !alive && item.desired != DesiredState::Running && !item.open_start {
            selection.idle.push(entry);
            continue;
        }
        if item.needs_recovery {
            selection.blocked.push(DownBlocked {
                entry,
                code: RECOVERY_REQUIRED,
            });
            continue;
        }
        selection.stop.push(entry);
    }
    let by_name = |a: &DownEntry, b: &DownEntry| (&a.name, a.id).cmp(&(&b.name, b.id));
    selection.stop.sort_by(by_name);
    selection.idle.sort_by(by_name);
    selection.keep.sort_by(|a, b| by_name(&a.entry, &b.entry));
    selection
        .blocked
        .sort_by(|a, b| by_name(&a.entry, &b.entry));
    selection
}

/// Reads the input of [`select`] for `environment` from the registry: its
/// members, the instances that are members or hold one of its active
/// leases, their leases that are not released, and the operations that are
/// not final. The reads are separate calls, so each carries its generation
/// and the whole is read again when a commit landed between two of them.
/// `None` when the environment is not registered.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a read.
pub fn read_view(
    registry: &dyn Registry,
    environment: EnvironmentId,
) -> Result<Option<DownView>, RegistryError> {
    let mut last = None;
    for _ in 0..READ_TRIES {
        let environments = registry.environments()?;
        let Some(own) = environments
            .value
            .iter()
            .find(|candidate| candidate.id() == environment)
        else {
            return Ok(None);
        };
        let own_leases = registry.leases_of_environment(environment, LeaseFilter::Unreleased)?;
        let instances = registry.instances(InstanceFilter {
            include_retained: true,
        })?;
        let operations = registry.unfinished_operations()?;
        let mut consistent = environments.generation == instances.generation
            && own_leases.generation == instances.generation
            && operations.generation == instances.generation;
        let active: Vec<&Lease> = own_leases
            .value
            .iter()
            .filter(|lease| lease.is_active())
            .collect();
        let mut wanted: BTreeSet<InstanceId> = own
            .members()
            .iter()
            .map(EnvironmentMember::instance_id)
            .collect();
        wanted.extend(active.iter().map(|lease| lease.instance_id()));
        let mut items = Vec::with_capacity(wanted.len());
        for instance in instances
            .value
            .iter()
            .filter(|instance| wanted.contains(&instance.id()))
        {
            let leases = registry.leases(instance.id(), LeaseFilter::Unreleased)?;
            consistent &= leases.generation == instances.generation;
            let leases: Vec<Lease> = leases
                .value
                .into_iter()
                .filter(|lease| !matches!(lease.state(), LeaseState::Released { .. }))
                .collect();
            items.push(item(
                own,
                instance,
                leases,
                &environments.value,
                &operations.value,
            ));
        }
        let view = DownView {
            environment,
            name: own.name().clone(),
            revision: own.revision(),
            members: own.members().to_vec(),
            instances: items,
            lease_count: active.len(),
            held_since: active.iter().map(|lease| lease.acquired_at()).min(),
        };
        if consistent {
            return Ok(Some(view));
        }
        last = Some(view);
    }
    Ok(last)
}

fn item(
    own: &Environment,
    instance: &Instance,
    leases: Vec<Lease>,
    environments: &[Environment],
    operations: &[(Operation, stackly_domain::JournalView)],
) -> DownInstance {
    let (open_start, needs_recovery) = open_work(instance.id(), operations);
    let holders = environment_holders(instance.id(), &leases, environments)
        .into_iter()
        .filter(|holder| holder.environment != own.id())
        .collect();
    let member = own
        .members()
        .iter()
        .find(|member| member.instance_id() == instance.id())
        .map(|member| DownMember {
            alias: member.alias().clone(),
            ownership: member.ownership(),
        });
    DownInstance {
        id: instance.id(),
        name: instance.name().clone(),
        lifecycle: instance.lifecycle(),
        desired: instance.desired(),
        retained: instance.lifecycle() == InstanceLifecycle::Retained,
        leases,
        holders,
        member,
        open_start,
        needs_recovery,
    }
}

/// Whether a start or restart of `instance` is open, and whether an
/// operation on it needs recovery (the rule of `stop_all::read_view`).
fn open_work(
    instance: InstanceId,
    operations: &[(Operation, stackly_domain::JournalView)],
) -> (bool, bool) {
    let mut open_start = false;
    let mut needs_recovery = false;
    for (operation, _) in operations {
        if operation.resource() != OperationResource::Instance(instance) {
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

#[cfg(test)]
mod tests {
    use stackly_testkit::LeaseBuilder;

    use super::*;

    const ENV: u128 = 0x0190_0000_0000_7000_8000_0000_0000_0001;

    fn env() -> EnvironmentId {
        EnvironmentId::from_uuid(uuid::Uuid::from_u128(ENV)).unwrap()
    }

    fn other() -> EnvironmentId {
        EnvironmentId::from_uuid(uuid::Uuid::from_u128(ENV + 1)).unwrap()
    }

    fn instance(name: &str, lifecycle: InstanceLifecycle, desired: DesiredState) -> DownInstance {
        DownInstance {
            id: InstanceId::generate(),
            name: name.parse().unwrap(),
            lifecycle,
            desired,
            retained: lifecycle == InstanceLifecycle::Retained,
            leases: Vec::new(),
            holders: Vec::new(),
            member: Some(DownMember {
                alias: name.parse().unwrap(),
                ownership: Ownership::Dedicated,
            }),
            open_start: false,
            needs_recovery: false,
        }
    }

    fn running(name: &str) -> DownInstance {
        instance(name, InstanceLifecycle::Running, DesiredState::Running)
    }

    fn leased(
        mut item: DownInstance,
        builder: impl Fn(LeaseBuilder) -> LeaseBuilder,
    ) -> DownInstance {
        let lease = builder(LeaseBuilder::new(item.id)).build();
        item.leases.push(lease);
        item
    }

    fn own(item: DownInstance) -> DownInstance {
        leased(item, |lease| lease.environment(env()))
    }

    fn view(instances: Vec<DownInstance>) -> DownView {
        DownView {
            environment: env(),
            name: "shop".parse().unwrap(),
            revision: ResourceRevision::INITIAL,
            members: Vec::new(),
            instances,
            lease_count: 0,
            held_since: None,
        }
    }

    fn names(entries: &[DownEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn an_instance_only_this_environment_holds_is_stopped() {
        let selection = select(&view(vec![own(running("db"))]));

        assert_eq!(names(&selection.stop), ["db"]);
        assert!(selection.keep.is_empty() && selection.idle.is_empty());
    }

    #[test]
    fn another_environment_a_manual_start_or_a_login_start_keeps_it_running() {
        let shared = leased(own(running("shared")), |lease| lease.environment(other()));
        let manual = leased(own(running("manual")), |lease| lease.manual(None));
        let startup = leased(own(running("login")), |lease| lease.startup("s-1"));

        let selection = select(&view(vec![shared, manual, startup]));

        assert!(selection.stop.is_empty());
        let kept: Vec<(&str, Vec<DemandSource>)> = selection
            .keep
            .iter()
            .map(|kept| (kept.entry.name.as_str(), kept.demand.clone()))
            .collect();
        assert_eq!(
            kept,
            [
                ("login", vec![DemandSource::Startup]),
                ("manual", vec![DemandSource::Manual]),
                ("shared", vec![DemandSource::Environment]),
            ],
            "by name"
        );
    }

    #[test]
    fn a_suppressed_lease_is_no_demand() {
        let at = jiff::Timestamp::from_second(1_800_000_000).unwrap();
        let item = leased(own(running("db")), |lease| {
            lease.startup("s-1").suppressed(at)
        });

        assert_eq!(names(&select(&view(vec![item])).stop), ["db"]);
    }

    #[test]
    fn an_instance_still_needed_is_kept_before_any_block() {
        let mut item = leased(own(running("db")), |lease| lease.manual(None));
        item.lifecycle = InstanceLifecycle::Unknown;
        item.needs_recovery = true;

        let selection = select(&view(vec![item]));

        assert_eq!(selection.keep.len(), 1);
        assert!(selection.blocked.is_empty());
    }

    #[test]
    fn an_unknown_instance_and_one_that_needs_recovery_are_blocked() {
        let unknown = own(instance(
            "ghost",
            InstanceLifecycle::Unknown,
            DesiredState::Running,
        ));
        let mut recovering = own(running("db"));
        recovering.needs_recovery = true;

        let selection = select(&view(vec![unknown, recovering]));

        let blocked: Vec<(&str, &str)> = selection
            .blocked
            .iter()
            .map(|b| (b.entry.name.as_str(), b.code.as_str()))
            .collect();
        assert_eq!(
            blocked,
            [
                ("db", "operation.recovery_required"),
                ("ghost", "supervisor.identity_unproven"),
            ]
        );
    }

    #[test]
    fn a_stopped_instance_is_idle_unless_its_desired_state_or_a_start_says_running() {
        let idle = instance("idle", InstanceLifecycle::Stopped, DesiredState::Stopped);
        let waiting = instance("waiting", InstanceLifecycle::Failed, DesiredState::Running);
        let mut queued = instance("queued", InstanceLifecycle::Stopped, DesiredState::Stopped);
        queued.open_start = true;

        let selection = select(&view(vec![idle, waiting, queued]));

        assert_eq!(names(&selection.idle), ["idle"]);
        assert_eq!(names(&selection.stop), ["queued", "waiting"]);
    }

    #[test]
    fn a_retained_instance_is_not_selected_and_every_live_state_is_stopped() {
        let retained = instance("gone", InstanceLifecycle::Retained, DesiredState::Running);
        let live: Vec<DownInstance> = [
            ("a", InstanceLifecycle::Starting),
            ("b", InstanceLifecycle::Running),
            ("c", InstanceLifecycle::Recovering),
            ("d", InstanceLifecycle::Stopping),
        ]
        .into_iter()
        .map(|(name, lifecycle)| instance(name, lifecycle, DesiredState::Stopped))
        .collect();
        let mut all = live;
        all.push(retained);

        let selection = select(&view(all));

        assert_eq!(names(&selection.stop), ["a", "b", "c", "d"]);
        assert!(selection.idle.is_empty() && selection.blocked.is_empty());
    }

    #[test]
    fn the_stop_set_matches_only_the_same_ids_in_any_order() {
        let a = own(running("a"));
        let b = own(running("b"));
        let (ia, ib) = (a.id, b.id);
        let selection = select(&view(vec![a, b]));

        assert!(selection.matches(&[ib, ia]));
        assert!(!selection.matches(&[ia]));
        assert!(!selection.matches(&[ia, ib, InstanceId::generate()]));
        assert_eq!(selection.stop_ids(), [ia, ib]);
    }

    #[test]
    fn the_view_of_one_instance_keeps_the_environment() {
        let a = own(running("a"));
        let id = a.id;
        let whole = view(vec![a, own(running("b"))]);

        let single = whole.of_instance(id).unwrap();

        assert_eq!(single.instances.len(), 1);
        assert_eq!(single.environment, env());
        assert!(whole.of_instance(InstanceId::generate()).is_none());
    }
}
