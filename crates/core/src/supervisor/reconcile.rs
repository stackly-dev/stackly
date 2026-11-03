//! Reconciliation (spec 0010, *Rekonsiliasi*, AC-9 to AC-11, AC-21): the
//! step of the daemon's start slot that finds what became of every
//! instance's process while no daemon watched, and brings the registry and
//! the actors in line with it.
//!
//! For every instance that is not retained, [`gather_evidence`] collects the
//! facts in the order the spec fixes and [`classify`] decides; [`plan_for`]
//! is the table of the spec as a pure function, and only the transitions of
//! the lifecycle table of spec 0004 appear in it. What cannot be proven is
//! never signalled, resumed, discarded, or started over: it is `unknown`.
//!
//! After the table, the secret material of instances that do not run is
//! swept and the values of the running ones are learned (spec 0011), the
//! proven processes are adopted (see [`adopt`](super::adopt)), and the
//! instances to run again wait for [`Supervisor::start_reconciled`].

use stackly_domain::{
    DesiredState, ExitKind, FailureCode, Instance, InstanceBlocker, InstanceFilter, InstanceId,
    InstanceLifecycle, LifecycleEvent, PlanDecision, ProcessObservation,
};
use tokio::time::Instant;

use super::adopt::{Adoption, adopt_process};
use super::classify::{Classification, GoneReason, UnprovenBlocker};
use super::drift;
use super::error::SupervisorError;
use super::evidence::{Gathered, gather_evidence, unproven_by};
use super::integrity::{self, FindingKind, ReconcileFinding};
use super::pending::{self, PendingKind, PendingRun};
use super::persist::{CommitMode, apply_event, blocking, commit_exit};
use super::restart::JitterFactor;
use super::{Supervisor, recheck};
use crate::secret::delivery::sweep_secret_material;

/// Whether the registry can be written to, as the daemon found when it
/// opened it (spec 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryMode {
    /// The registry is ready: reconciliation runs.
    Ready,
    /// The registry is in recovery mode: the supervisor touches no process
    /// and runs nothing.
    Recovery,
}

/// What a reconciliation did, in numbers (the counts of
/// `daemon.reconciling { phase: finished }`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ReconcileReport {
    /// True when the registry was in recovery mode and nothing ran.
    pub skipped: bool,
    /// Instances that are not retained.
    pub instances: u32,
    /// Processes adopted.
    pub adopted: u32,
    /// Instances that read `unknown` afterwards.
    pub unknown: u32,
    /// Instances that will start or restart once the engine runs.
    pub restarting: u32,
    /// The problems found and left in place, one per instance per problem
    /// (spec 0030): a missing data folder or runtime executable under a
    /// process that was adopted.
    pub findings: Vec<ReconcileFinding>,
}

/// The start or the end of a reconciliation, for `daemon.reconciling`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReconcilePhase {
    /// It begins.
    Started,
    /// It ended, with these counts.
    Finished(ReconcileReport),
}

/// Who asks for the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The reconciliation of the daemon's start slot.
    Reconcile,
    /// The reconciliation of a start slot whose session rotation was `new`
    /// (spec 0031, AC-6): a process that is gone is read as after a new
    /// boot, because logout ended it, not a crash.
    NewSession,
    /// The periodic recheck of an `unknown` instance: a vanished process
    /// is confirmed stopped but never started again on its own.
    Recheck,
}

impl Source {
    /// Whether the table runs for the daemon's start slot, in a new
    /// session or not.
    const fn is_start_slot(self) -> bool {
        matches!(self, Self::Reconcile | Self::NewSession)
    }
}

/// What a classification means for an instance (a row of the table).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    /// The lifecycle events to commit, in order.
    pub(crate) events: Vec<LifecycleEvent>,
    /// Whether the proven process is adopted.
    pub(crate) adopt: bool,
    /// What to run afterwards.
    pub(crate) then: Then,
    /// What the blocker of the instance becomes.
    pub(crate) blocker: BlockerPlan,
}

/// What follows the events of a [`Plan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Then {
    Nothing,
    /// A crash of the restart policy: the pause, then the respawn.
    Respawn,
    /// A system start.
    Start,
    /// A system stop of a proven process.
    Stop,
    /// A system start of an instance a planned restart stopped (spec
    /// 0030): trigger `planned_restart`, no dialog.
    Restore,
    /// A system stop of a proven process that a planned restart was
    /// stopping, which writes no desired state, and then the restore.
    PlannedStop,
}

/// The blocker an evaluation leaves on the instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockerPlan {
    /// No blocker of the supervisor's.
    Clear,
    /// A process that is proven runs on an instance that is not running.
    DataInUse,
    /// The classification's own blocker.
    Unproven(UnprovenBlocker),
}

impl Plan {
    fn new(events: Vec<LifecycleEvent>) -> Self {
        Self {
            events,
            adopt: false,
            then: Then::Nothing,
            blocker: BlockerPlan::Clear,
        }
    }

    fn adopting(mut self, then: Then) -> Self {
        self.adopt = true;
        self.then = then;
        self
    }

    fn then(mut self, then: Then) -> Self {
        self.then = then;
        self
    }
}

fn interrupted() -> FailureCode {
    FailureCode::INTERRUPTED
}

/// `exited` for a process reconciliation found gone, with the
/// [`ExitKind`] its stored lifecycle gives (spec 0026, *Handler exit:
/// `last_exit`*); [`with_force`] turns it `forced` at commit time.
const fn exited(exit: ExitKind) -> LifecycleEvent {
    LifecycleEvent::Exited { exit: Some(exit) }
}

/// `confirmed_stopped`, with the [`ExitKind`] the stored lifecycle gives,
/// or `None` for an `unknown` one, which does not tell how the process
/// ended.
const fn confirmed(exit: Option<ExitKind>) -> LifecycleEvent {
    LifecycleEvent::ConfirmedStopped { exit }
}

/// Turns the exit kind of an event `forced` when a force stop's `kill`
/// step began for the instance; the table cannot know that, the journal
/// read inside the commit does.
fn with_force(event: &LifecycleEvent, force: bool) -> LifecycleEvent {
    match event {
        LifecycleEvent::Exited { exit: Some(_) } if force => exited(ExitKind::Forced),
        LifecycleEvent::ConfirmedStopped { exit: Some(_) } if force => {
            confirmed(Some(ExitKind::Forced))
        }
        other => other.clone(),
    }
}

/// The table of spec 0010 (*Rekonsiliasi*): what to do with an instance of
/// this stored `lifecycle` and `desired` state, given the classification of
/// its process. A pure function; every pair of lifecycle and event it uses
/// is in the table of spec 0004.
pub(crate) fn plan_for(
    lifecycle: InstanceLifecycle,
    desired: DesiredState,
    classification: &Classification,
    source: Source,
) -> Plan {
    use InstanceLifecycle as L;
    use LifecycleEvent as E;
    let wants_running = desired == DesiredState::Running;
    let stop_if_wanted = if wants_running {
        Then::Nothing
    } else {
        Then::Stop
    };
    let start_if_wanted = match (wants_running, source) {
        (true, Source::Reconcile | Source::NewSession) => Then::Start,
        _ => Then::Nothing,
    };
    let (proven, gone) = match classification {
        Classification::Proven { .. } => (true, None),
        Classification::Gone { reason, .. } => (false, Some(*reason)),
        Classification::Unproven { .. } => (false, None),
    };
    // A new session in the same boot reads a process that is gone as a new
    // boot does (spec 0031, AC-6): no respawn and no crash count.
    let boot_changed =
        gone == Some(GoneReason::BootChanged) || (source == Source::NewSession && gone.is_some());
    let unproven = matches!(classification, Classification::Unproven { .. });
    let plan = match lifecycle {
        L::Initializing => Plan::new(vec![E::InitFailed {
            reason: interrupted(),
        }]),
        L::Starting if proven => Plan::new(vec![E::Spawned]).adopting(stop_if_wanted),
        L::Starting if unproven => Plan::new(vec![E::IdentityLost]),
        L::Starting if boot_changed => Plan::new(vec![E::StartFailed {
            reason: interrupted(),
        }]),
        L::Starting => Plan::new(vec![E::StartFailed {
            reason: interrupted(),
        }])
        .then(start_if_wanted),
        L::Running if proven => Plan::new(Vec::new()).adopting(stop_if_wanted),
        L::Running if unproven => Plan::new(vec![E::IdentityLost]),
        L::Running if boot_changed || !wants_running => {
            Plan::new(vec![E::IdentityLost, confirmed(Some(ExitKind::Crashed))])
        }
        L::Running => Plan::new(vec![E::Crashed {
            will_retry: true,
            reason: FailureCode::EXIT_STATUS_UNKNOWN,
        }])
        .then(Then::Respawn),
        L::Recovering if proven => Plan::new(vec![E::Spawned]).adopting(stop_if_wanted),
        L::Recovering if unproven => Plan::new(vec![E::IdentityLost]),
        L::Recovering if boot_changed => {
            Plan::new(vec![E::IdentityLost, confirmed(Some(ExitKind::Crashed))])
        }
        L::Recovering if wants_running => Plan::new(Vec::new()).then(Then::Respawn),
        L::Recovering => Plan::new(vec![E::StopBegin, exited(ExitKind::Crashed)]),
        L::Stopping if proven => Plan::new(Vec::new()).adopting(Then::Stop),
        L::Stopping if unproven => Plan::new(vec![E::IdentityLost]),
        L::Stopping => Plan::new(vec![exited(ExitKind::Clean)]),
        L::Unknown if proven => Plan::new(vec![E::Adopted]).adopting(stop_if_wanted),
        L::Unknown if unproven => Plan::new(Vec::new()),
        L::Unknown if boot_changed => Plan::new(vec![confirmed(None)]),
        L::Unknown => Plan::new(vec![confirmed(None)]).then(start_if_wanted),
        L::Uninitialized | L::Stopped | L::Failed | L::Retained => Plan::new(Vec::new()),
    };
    let mut plan = plan;
    plan.blocker = match classification {
        Classification::Unproven { blocker, .. } => BlockerPlan::Unproven(*blocker),
        // A proven process that nobody adopts runs on data that no running
        // instance claims.
        Classification::Proven { .. } if !plan.adopt => BlockerPlan::DataInUse,
        _ => BlockerPlan::Clear,
    };
    plan
}

/// The row of the table for an instance whose data is gone and whose
/// process is not there (spec 0030, AC-4): the instance ends `stopped`
/// whatever it wanted, as if it were not wanted, and then `failed` with
/// `data_missing` (unless it already is). It never runs again on its own,
/// so the data is never replaced by an empty folder. A process that is
/// there, proven or not, keeps the ordinary row.
pub(crate) fn plan_for_missing_data(
    lifecycle: InstanceLifecycle,
    classification: &Classification,
    already_flagged: bool,
) -> Option<Plan> {
    if !matches!(classification, Classification::Gone { .. }) {
        return None;
    }
    let mut plan = plan_for(
        lifecycle,
        DesiredState::Stopped,
        classification,
        Source::Reconcile,
    );
    let ends = plan
        .events
        .iter()
        .try_fold(lifecycle, |lifecycle, event| lifecycle.apply(event))
        .ok()?;
    if matches!(ends, InstanceLifecycle::Stopped | InstanceLifecycle::Failed) && !already_flagged {
        plan.events.push(LifecycleEvent::DataMissing);
    }
    plan.then = Then::Nothing;
    Some(plan)
}

/// What settling one instance left for the caller.
pub(crate) struct Settled {
    /// The lifecycle the instance has now.
    pub(crate) lifecycle: InstanceLifecycle,
    /// What to run once the engine runs.
    pub(crate) then: Then,
    /// What is wrong under a process that is about to be adopted.
    pub(crate) findings: Vec<FindingKind>,
}

impl Supervisor {
    /// Reconciles the registry with the processes that are there (spec 0010
    /// *Rekonsiliasi*, steps 1 to 6): publishes `daemon.reconciling`, applies
    /// the table to every instance, sweeps the secret material of the
    /// instances that do not run and learns the values of the running ones,
    /// adopts the proven processes, publishes the counts, and starts the
    /// recheck of `unknown` instances. Run it in the daemon's start slot,
    /// after the registry is `ready` and the operations are recovered.
    ///
    /// The instances to run again wait for
    /// [`start_reconciled`](Self::start_reconciled). In
    /// [`RegistryMode::Recovery`] nothing is touched and nothing runs.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Registry`] when the instances cannot be
    /// read. A failure on one instance is logged and reconciliation goes on
    /// with the others.
    pub async fn reconcile(&self, mode: RegistryMode) -> Result<ReconcileReport, SupervisorError> {
        self.reconcile_in_session(mode, false).await
    }

    /// [`reconcile`](Self::reconcile) for a start slot whose session
    /// rotation was `new` when `session_changed` is true (spec 0031, AC-6):
    /// a process that is gone is read as after a new boot, so it neither
    /// respawns nor counts as a crash, and a proven process of an instance
    /// whose desired state is now `stopped` stops with the trigger
    /// `session_rotation`.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Registry`] when the instances cannot be
    /// read.
    pub async fn reconcile_in_session(
        &self,
        mode: RegistryMode,
        session_changed: bool,
    ) -> Result<ReconcileReport, SupervisorError> {
        if mode == RegistryMode::Recovery {
            return Ok(ReconcileReport {
                skipped: true,
                ..ReconcileReport::default()
            });
        }
        let deps = self.deps();
        deps.events.reconciling(&ReconcilePhase::Started);
        // Spec 0030: the plan of a restart that was under way.
        self.take_over_plan().await;
        let registry = deps.registry.clone();
        let mut instances = blocking(move || {
            registry.instances(InstanceFilter {
                include_retained: false,
            })
        })
        .await?
        .map_err(SupervisorError::Registry)?
        .value;
        instances.sort_by(|a, b| a.name().cmp(b.name()));

        let mut report = ReconcileReport {
            instances: u32::try_from(instances.len()).unwrap_or(u32::MAX),
            ..ReconcileReport::default()
        };
        let mut lifecycles = Vec::with_capacity(instances.len());
        let mut adoptions = Vec::new();
        let mut findings = Vec::new();
        let mut deferred = Vec::new();
        for instance in &instances {
            let source = if session_changed {
                Source::NewSession
            } else {
                Source::Reconcile
            };
            let settled = settle_instance(self, instance, source).await;
            let Some((settled, adoption)) = settled else {
                lifecycles.push((instance.id(), instance.lifecycle()));
                continue;
            };
            lifecycles.push((instance.id(), settled.lifecycle));
            if settled.lifecycle == InstanceLifecycle::Unknown {
                report.unknown += 1;
            }
            if matches!(
                settled.then,
                Then::Respawn | Then::Start | Then::Restore | Then::PlannedStop
            ) {
                report.restarting += 1;
            }
            if settled.then != Then::Nothing {
                deferred.push(instance.id());
            }
            self.queue_run(instance, settled.then, session_changed);
            if let Some(adoption) = adoption {
                adoptions.push(adoption);
                findings.extend(settled.findings.into_iter().map(|kind| ReconcileFinding {
                    instance_id: instance.id(),
                    name: instance.name().to_string(),
                    kind,
                }));
            }
        }

        self.sweep_and_learn(&instances, &lifecycles).await;

        for adoption in adoptions {
            let id = adoption.id;
            if adopt_process(self, adoption).await {
                report.adopted += 1;
            } else {
                findings.retain(|finding| finding.instance_id != id);
                tracing::warn!(instance = %id, "a proven process could not be adopted");
                if let Some(instance) = lifecycles.iter_mut().find(|(other, _)| *other == id) {
                    instance.1 = InstanceLifecycle::Unknown;
                }
                report.unknown += 1;
            }
        }
        // Spec 0029, *Drift*: once per instance, after the blockers above.
        for instance in &instances {
            if let Err(error) = drift::observe_active(self, instance.id()).await {
                tracing::warn!(instance = %instance.id(), %error, "the config drift check did not run");
            }
        }
        report.findings = findings;
        self.settle_plan_after_reconcile(&lifecycles, &deferred)
            .await;
        deps.events
            .reconciling(&ReconcilePhase::Finished(report.clone()));
        recheck::spawn_recheck(self);
        super::secret_retry::spawn_secret_retry(self);
        Ok(report)
    }

    /// Puts what reconciliation decided to run again on the list
    /// [`start_reconciled`](Self::start_reconciled) works through.
    fn queue_run(&self, instance: &Instance, then: Then, session_changed: bool) {
        let kind = match then {
            Then::Nothing => return,
            Then::Respawn => PendingKind::Respawn,
            Then::Start => PendingKind::Start,
            Then::Stop => PendingKind::Stop,
            Then::Restore => PendingKind::Restore,
            Then::PlannedStop => PendingKind::PlannedStop,
        };
        self.shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(PendingRun {
                id: instance.id(),
                name: instance.name().clone(),
                kind,
                trigger: if session_changed && kind == PendingKind::Stop {
                    pending::SESSION_ROTATION_INPUT
                } else {
                    pending::TRIGGER_INPUT
                },
            });
    }

    /// Removes the secret material of the instances that do not run and
    /// learns the secret values of the ones that do (spec 0011), in that
    /// order, so the logs of an adopted process stay redacted.
    async fn sweep_and_learn(
        &self,
        instances: &[Instance],
        lifecycles: &[(InstanceId, InstanceLifecycle)],
    ) {
        let deps = self.deps().clone();
        let keyed: Vec<_> = instances
            .iter()
            .zip(lifecycles)
            .map(|(instance, (_, lifecycle))| (instance.id(), *lifecycle))
            .collect();
        let done = blocking(move || {
            let swept = sweep_secret_material(
                &deps.platform,
                &deps.layout,
                keyed.iter().map(|(id, lifecycle)| (id.short_key(), *lifecycle)),
            );
            if let Err(error) = &swept {
                tracing::warn!(%error, "the secret material of the stopped instances could not be swept");
            }
            if let Err(error) = deps.secrets.learn_running(keyed.iter().copied()) {
                tracing::warn!(%error, "the secrets of the running instances could not be learned");
            }
        })
        .await;
        if let Err(error) = done {
            tracing::warn!(%error, "sweeping and learning secrets did not run");
        }
    }
}

/// Settles one instance: gathers the evidence, applies the table, and
/// records the blocker. Returns `None` when the instance could not be
/// settled and was left as it is.
pub(crate) async fn settle_instance(
    sup: &Supervisor,
    instance: &Instance,
    source: Source,
) -> Option<(Settled, Option<Adoption>)> {
    let deps = sup.deps();
    let id = instance.id();
    let actor = sup.actor(id);
    let recorded = {
        let registry = deps.registry.clone();
        blocking(move || registry.latest_process_observation(id)).await
    };
    let recorded = match recorded {
        Ok(Ok(versioned)) => Ok(versioned.value),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    let lock_path = deps.layout.instance_paths(id).lock;
    let gathered = match &recorded {
        Ok(observation) => {
            let identity = observation
                .as_ref()
                .map(|ProcessObservation { identity, .. }| identity.clone());
            let platform = deps.platform.clone();
            blocking(move || gather_evidence(&platform, &lock_path, identity.as_ref()))
                .await
                .ok()?
        }
        // Without the record there is no telling: never `Gone`.
        Err(error) => Gathered {
            classification: unproven_by(None, error),
            facts: None,
            lock_file: None,
        },
    };
    let identity = recorded
        .ok()
        .flatten()
        .map(|ProcessObservation { identity, .. }| identity);

    // Spec 0030: the data of an initialized instance must still be there.
    let checks_apply = source.is_start_slot() && integrity::applies(instance);
    let data_gone = checks_apply && integrity::data_is_missing(sup, instance).await;
    let flagged = instance.lifecycle() == InstanceLifecycle::Failed
        && instance.last_failure() == Some(&FailureCode::DATA_MISSING);
    let plan = data_gone
        .then(|| plan_for_missing_data(instance.lifecycle(), &gathered.classification, flagged))
        .flatten()
        .unwrap_or_else(|| {
            plan_for(
                instance.lifecycle(),
                instance.desired(),
                &gathered.classification,
                source,
            )
        });
    let mut plan = plan;
    // Spec 0030: what a restart plan under way says about the instance.
    // The lifecycle it ends with decides whether it starts again.
    let ends = plan
        .events
        .iter()
        .try_fold(instance.lifecycle(), |lifecycle, event| {
            lifecycle.apply(event)
        })
        .unwrap_or_else(|_| instance.lifecycle());
    if source.is_start_slot()
        && let Some(entry) = sup.planned_entry(id).await
        && entry.decision == PlanDecision::StopRestore
        && instance.desired() == DesiredState::Running
        && !data_gone
    {
        if plan.then == Then::Stop && instance.lifecycle() == InstanceLifecycle::Stopping {
            plan.then = Then::PlannedStop;
        } else if plan.then == Then::Nothing && ends == InstanceLifecycle::Stopped {
            plan.then = Then::Restore;
        }
    }
    // What is wrong under a process we keep is reported, not repaired.
    let mut findings = Vec::new();
    if plan.adopt && checks_apply {
        if data_gone {
            findings.push(FindingKind::DataMissing);
        }
        if integrity::runtime_is_missing(sup, instance).await {
            findings.push(FindingKind::RuntimeMissing);
        }
    }
    let now = deps.platform.clock().now();
    if !plan.events.is_empty() {
        let events = plan.events.clone();
        let committed = commit_exit(sup, id, None, CommitMode::Actor, |changes, force| {
            for event in &events {
                apply_event(changes, with_force(event, force), now)?;
            }
            Ok(Vec::new())
        })
        .await;
        if let Err(error) = committed {
            tracing::warn!(instance = %id, %error, "reconciliation could not record the state of an instance");
            return None;
        }
    }
    let lifecycle = plan
        .events
        .iter()
        .try_fold(instance.lifecycle(), |lifecycle, event| {
            lifecycle.apply(event)
        })
        .unwrap_or_else(|_| instance.lifecycle());

    let evidence = gathered.classification.evidence().clone();
    let blocker = match plan.blocker {
        BlockerPlan::Clear => None,
        BlockerPlan::DataInUse => Some(InstanceBlocker::DataInUse {
            evidence,
            observed_at: now,
        }),
        BlockerPlan::Unproven(UnprovenBlocker::DataInUse) => Some(InstanceBlocker::DataInUse {
            evidence,
            observed_at: now,
        }),
        BlockerPlan::Unproven(UnprovenBlocker::IdentityUnproven) => {
            Some(InstanceBlocker::IdentityUnproven {
                evidence,
                observed_at: now,
            })
        }
    };
    actor.update(deps, |state| {
        state.set_supervisor_blocker(blocker);
        if !plan.adopt {
            state.not_running(now);
        }
        // Spec 0010, *Status dan event*: the restart history is in memory
        // only, but a crash loop the registry kept still reads exhausted.
        if instance.lifecycle() == InstanceLifecycle::Failed
            && instance.last_failure() == Some(&FailureCode::CRASH_LOOP)
        {
            state.restart_status.exhausted = true;
        }
        if plan.then == Then::Respawn {
            state
                .restart
                .record_crash(Instant::now(), JitterFactor::random(), &deps.config);
        }
    });

    let adoption = match (&gathered.classification, plan.adopt, identity) {
        (Classification::Proven { evidence, .. }, true, Some(identity)) => Some(Adoption {
            id,
            stopped: evidence.stopped(),
            group: gathered.facts.as_ref().map(|facts| facts.group),
            lock: gathered.lock_file,
            identity,
        }),
        _ => None,
    };
    Some((
        Settled {
            lifecycle,
            then: plan.then,
            findings,
        },
        adoption,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use stackly_domain::{BootId, IdentityEvidence, Pid, ProcessIdentity, ProcessStart};

    use super::*;

    fn identity() -> ProcessIdentity {
        ProcessIdentity {
            pid: Pid::new(4242).unwrap(),
            start: ProcessStart::new(1_760_000_000, 1).unwrap(),
            executable: PathBuf::from("/runtimes/fake/stackly-fake-server"),
            session: Pid::new(4242).unwrap(),
            boot: "6f2a2c66-0d6b-4d62-9b0e-6a1f7c7d2a10"
                .parse::<BootId>()
                .unwrap(),
        }
    }

    fn proven() -> Classification {
        Classification::Proven {
            identity: identity(),
            evidence: IdentityEvidence::new(),
        }
    }

    fn gone(reason: GoneReason) -> Classification {
        Classification::Gone {
            reason,
            evidence: IdentityEvidence::new(),
        }
    }

    fn unproven(blocker: UnprovenBlocker) -> Classification {
        Classification::Unproven {
            blocker,
            evidence: IdentityEvidence::new(),
        }
    }

    /// The codes of the events of a plan.
    fn codes(plan: &Plan) -> Vec<&'static str> {
        plan.events.iter().map(LifecycleEvent::as_code).collect()
    }

    /// The lifecycle after the events, which must all be in the table of
    /// spec 0004.
    fn after(lifecycle: InstanceLifecycle, plan: &Plan) -> InstanceLifecycle {
        plan.events
            .iter()
            .try_fold(lifecycle, |lifecycle, event| lifecycle.apply(event))
            .unwrap_or_else(|error| {
                panic!("{lifecycle:?} has no way to {:?}: {error}", plan.events)
            })
    }

    fn plan(
        lifecycle: InstanceLifecycle,
        desired: DesiredState,
        classification: &Classification,
    ) -> Plan {
        plan_for(lifecycle, desired, classification, Source::Reconcile)
    }

    use DesiredState::{Running as Run, Stopped as Stop};
    use InstanceLifecycle as L;

    #[test]
    fn initializing_always_fails_as_interrupted() {
        for classification in [
            proven(),
            gone(GoneReason::Exited),
            gone(GoneReason::BootChanged),
            unproven(UnprovenBlocker::DataInUse),
        ] {
            let plan = plan(L::Initializing, Run, &classification);

            assert_eq!(codes(&plan), ["init_failed"]);
            assert_eq!(after(L::Initializing, &plan), L::Failed);
            assert!(!plan.adopt);
        }
    }

    #[test]
    fn initializing_keeps_the_blocker_of_a_process_that_is_still_there() {
        assert_eq!(
            plan(L::Initializing, Run, &proven()).blocker,
            BlockerPlan::DataInUse
        );
        assert_eq!(
            plan(L::Initializing, Run, &unproven(UnprovenBlocker::DataInUse)).blocker,
            BlockerPlan::Unproven(UnprovenBlocker::DataInUse)
        );
        assert_eq!(
            plan(L::Initializing, Run, &gone(GoneReason::Exited)).blocker,
            BlockerPlan::Clear
        );
    }

    #[test]
    fn starting_adopts_a_proven_process_as_spawned() {
        let plan = plan(L::Starting, Run, &proven());

        assert_eq!(codes(&plan), ["spawned"]);
        assert_eq!(after(L::Starting, &plan), L::Running);
        assert!(plan.adopt);
        assert_eq!(plan.then, Then::Nothing);
    }

    #[test]
    fn starting_that_is_gone_on_the_same_boot_fails_and_runs_again_only_when_wanted() {
        let wanted = plan(L::Starting, Run, &gone(GoneReason::Exited));
        let unwanted = plan(L::Starting, Stop, &gone(GoneReason::PidReused));

        assert_eq!(codes(&wanted), ["start_failed"]);
        assert_eq!(after(L::Starting, &wanted), L::Failed);
        assert_eq!(wanted.then, Then::Start);
        assert_eq!(unwanted.then, Then::Nothing);
    }

    #[test]
    fn starting_on_a_new_boot_fails_and_never_starts_on_its_own() {
        let plan = plan(L::Starting, Run, &gone(GoneReason::BootChanged));

        assert_eq!(codes(&plan), ["start_failed"]);
        assert_eq!(plan.then, Then::Nothing);
    }

    #[test]
    fn running_stays_running_when_proven_and_stops_when_the_user_wants_it_stopped() {
        let kept = plan(L::Running, Run, &proven());
        let stopped = plan(L::Running, Stop, &proven());

        assert!(kept.events.is_empty() && kept.adopt);
        assert_eq!(kept.then, Then::Nothing);
        assert!(stopped.adopt);
        assert_eq!(stopped.then, Then::Stop);
    }

    #[test]
    fn running_that_is_gone_on_the_same_boot_crashes_with_a_retry_when_wanted() {
        let plan = plan(L::Running, Run, &gone(GoneReason::Exited));

        assert_eq!(codes(&plan), ["crashed"]);
        assert_eq!(after(L::Running, &plan), L::Recovering);
        assert_eq!(plan.then, Then::Respawn);
        assert!(matches!(
            plan.events[0],
            LifecycleEvent::Crashed { will_retry: true, ref reason }
                if *reason == FailureCode::EXIT_STATUS_UNKNOWN
        ));
    }

    #[test]
    fn running_that_is_gone_and_not_wanted_is_confirmed_stopped() {
        let same_boot = plan(L::Running, Stop, &gone(GoneReason::Exited));
        let new_boot = plan(L::Running, Run, &gone(GoneReason::BootChanged));

        for plan in [same_boot, new_boot] {
            assert_eq!(codes(&plan), ["identity_lost", "confirmed_stopped"]);
            assert_eq!(after(L::Running, &plan), L::Stopped);
            assert_eq!(plan.then, Then::Nothing);
        }
    }

    #[test]
    fn recovering_respawns_when_gone_on_the_same_boot_and_adopts_when_proven() {
        let gone_same = plan(L::Recovering, Run, &gone(GoneReason::Exited));
        let gone_new = plan(L::Recovering, Run, &gone(GoneReason::BootChanged));
        let found = plan(L::Recovering, Run, &proven());

        assert!(gone_same.events.is_empty());
        assert_eq!(gone_same.then, Then::Respawn);
        assert_eq!(codes(&gone_new), ["identity_lost", "confirmed_stopped"]);
        assert_eq!(after(L::Recovering, &gone_new), L::Stopped);
        assert_eq!(codes(&found), ["spawned"]);
        assert_eq!(after(L::Recovering, &found), L::Running);
        assert!(found.adopt);
    }

    #[test]
    fn recovering_with_desired_stopped_ends_stopped_without_a_respawn() {
        let plan = plan(L::Recovering, Stop, &gone(GoneReason::Exited));

        assert_eq!(codes(&plan), ["stop_begin", "exited"]);
        assert_eq!(after(L::Recovering, &plan), L::Stopped);
        assert_eq!(plan.then, Then::Nothing);
    }

    #[test]
    fn stopping_stays_stopping_and_stops_again_when_proven_and_exits_when_gone() {
        let proven_plan = plan(L::Stopping, Stop, &proven());
        let gone_plan = plan(L::Stopping, Stop, &gone(GoneReason::PidReused));
        let new_boot = plan(L::Stopping, Stop, &gone(GoneReason::BootChanged));

        assert!(proven_plan.events.is_empty() && proven_plan.adopt);
        assert_eq!(proven_plan.then, Then::Stop);
        for plan in [gone_plan, new_boot] {
            assert_eq!(codes(&plan), ["exited"]);
            assert_eq!(after(L::Stopping, &plan), L::Stopped);
        }
    }

    #[test]
    fn unknown_is_adopted_when_proven_and_confirmed_stopped_when_gone() {
        let adopted = plan(L::Unknown, Run, &proven());
        let same_boot = plan(L::Unknown, Run, &gone(GoneReason::Exited));
        let new_boot = plan(L::Unknown, Run, &gone(GoneReason::BootChanged));

        assert_eq!(codes(&adopted), ["adopted"]);
        assert_eq!(after(L::Unknown, &adopted), L::Running);
        assert!(adopted.adopt);
        assert_eq!(codes(&same_boot), ["confirmed_stopped"]);
        assert_eq!(same_boot.then, Then::Start, "desired running starts again");
        assert_eq!(codes(&new_boot), ["confirmed_stopped"]);
        assert_eq!(new_boot.then, Then::Nothing, "a new boot never starts");
    }

    #[test]
    fn a_recheck_confirms_a_vanished_process_but_never_starts_it() {
        let recheck = plan_for(L::Unknown, Run, &gone(GoneReason::Exited), Source::Recheck);

        assert_eq!(codes(&recheck), ["confirmed_stopped"]);
        assert_eq!(recheck.then, Then::Nothing);
    }

    #[test]
    fn unknown_that_is_still_unproven_stays_unknown_with_its_blocker() {
        let plan = plan(
            L::Unknown,
            Run,
            &unproven(UnprovenBlocker::IdentityUnproven),
        );

        assert!(plan.events.is_empty());
        assert_eq!(
            plan.blocker,
            BlockerPlan::Unproven(UnprovenBlocker::IdentityUnproven)
        );
    }

    #[test]
    fn a_live_instance_that_cannot_be_proven_becomes_unknown() {
        for lifecycle in [L::Starting, L::Running, L::Recovering, L::Stopping] {
            let plan = plan(lifecycle, Run, &unproven(UnprovenBlocker::DataInUse));

            assert_eq!(codes(&plan), ["identity_lost"], "{lifecycle:?}");
            assert_eq!(after(lifecycle, &plan), L::Unknown);
            assert!(!plan.adopt);
            assert_eq!(plan.then, Then::Nothing);
        }
    }

    #[test]
    fn instances_that_do_not_run_change_nothing_and_only_get_a_blocker() {
        for lifecycle in [L::Stopped, L::Failed, L::Uninitialized] {
            let proven_plan = plan(lifecycle, Run, &proven());
            let gone_plan = plan(lifecycle, Run, &gone(GoneReason::Exited));
            let unproven_plan = plan(lifecycle, Run, &unproven(UnprovenBlocker::DataInUse));

            for plan in [&proven_plan, &gone_plan, &unproven_plan] {
                assert!(plan.events.is_empty(), "{lifecycle:?}");
                assert!(!plan.adopt);
                assert_eq!(plan.then, Then::Nothing);
            }
            assert_eq!(proven_plan.blocker, BlockerPlan::DataInUse);
            assert_eq!(gone_plan.blocker, BlockerPlan::Clear);
            assert_eq!(
                unproven_plan.blocker,
                BlockerPlan::Unproven(UnprovenBlocker::DataInUse)
            );
        }
    }

    #[test]
    fn missing_data_ends_every_instance_without_a_process_failed_and_never_runs_it() {
        let lifecycles = [
            L::Stopped,
            L::Failed,
            L::Starting,
            L::Running,
            L::Stopping,
            L::Recovering,
            L::Unknown,
        ];
        for lifecycle in lifecycles {
            for classification in [
                gone(GoneReason::NoRecord),
                gone(GoneReason::Exited),
                gone(GoneReason::PidReused),
                gone(GoneReason::BootChanged),
            ] {
                let plan = plan_for_missing_data(lifecycle, &classification, false)
                    .expect("a gone process gets the row");

                assert_eq!(
                    codes(&plan).last().copied(),
                    Some("data_missing"),
                    "{lifecycle:?} {classification:?}"
                );
                assert_eq!(after(lifecycle, &plan), L::Failed);
                assert_eq!(plan.then, Then::Nothing);
                assert!(!plan.adopt);
            }
        }
    }

    #[test]
    fn missing_data_leaves_a_process_that_is_there_to_the_ordinary_table() {
        assert!(plan_for_missing_data(L::Running, &proven(), false).is_none());
        assert!(
            plan_for_missing_data(L::Running, &unproven(UnprovenBlocker::DataInUse), false)
                .is_none()
        );
    }

    #[test]
    fn missing_data_already_recorded_is_not_recorded_again() {
        let plan = plan_for_missing_data(L::Failed, &gone(GoneReason::NoRecord), true)
            .expect("a gone process gets the row");

        assert!(plan.events.is_empty());
    }

    #[test]
    fn no_row_of_the_table_signals_or_adopts_what_is_not_proven() {
        let lifecycles = [
            L::Uninitialized,
            L::Initializing,
            L::Stopped,
            L::Starting,
            L::Running,
            L::Failed,
            L::Stopping,
            L::Recovering,
            L::Unknown,
        ];
        for lifecycle in lifecycles {
            for desired in [Run, Stop] {
                for classification in [
                    gone(GoneReason::NoRecord),
                    gone(GoneReason::Exited),
                    gone(GoneReason::PidReused),
                    gone(GoneReason::BootChanged),
                    unproven(UnprovenBlocker::DataInUse),
                    unproven(UnprovenBlocker::IdentityUnproven),
                ] {
                    let plan = plan(lifecycle, desired, &classification);

                    assert!(!plan.adopt, "{lifecycle:?} {classification:?}");
                    assert_ne!(plan.then, Then::Stop, "{lifecycle:?} {classification:?}");
                    let _ = after(lifecycle, &plan);
                }
            }
        }
    }
}
