//! Reading and writing the registry for the supervisor (spec 0010,
//! *Commit*): every registry call runs on the blocking pool, every change
//! of an aggregate is one `UpdateInstance` per domain mutation, and an actor
//! commit retries as AC-22 says.

use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;
use stackly_domain::{
    BatchError, CommitGuard, DesiredState, Environment, ExitKind, GuardDecision, GuardView,
    Guarded, Instance, InstanceId, InstanceLifecycle, Lease, LeaseFilter, LeaseHolder,
    LifecycleEvent, RegistryError, StopDecision, StopRequest, WriteBatch, WriteOp, evaluate_stop,
    force_intent,
};
use stackly_platform::ExitStatus;

use super::actor::InstanceActor;
use super::commit::{ActorCommitRetry, CommitRetry, InstanceChanges};
use super::error::{EnvironmentHolder, SupervisorError, environment_holders};
use super::status::FailureFacts;
use super::stop::LeaseCheck;
use super::{Supervisor, SupervisorDeps};
use crate::operation::AuditNote;
use crate::session::manual_stop_ops;

/// Runs `call` on the blocking pool.
///
/// # Errors
///
/// Returns [`SupervisorError::Internal`] when the blocking task panics.
pub(crate) async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> T + Send + 'static,
) -> Result<T, SupervisorError> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|error| SupervisorError::internal(format!("a blocking task failed: {error}")))
}

/// Reads an instance.
///
/// # Errors
///
/// Returns [`SupervisorError::NotFound`] for an unknown instance and
/// [`SupervisorError::Registry`] for a registry failure.
pub(crate) async fn read_instance(
    deps: &SupervisorDeps,
    id: InstanceId,
) -> Result<Instance, SupervisorError> {
    let registry = deps.registry.clone();
    blocking(move || registry.instance(id))
        .await?
        .map_err(SupervisorError::Registry)?
        .value
        .ok_or(SupervisorError::NotFound(id))
}

/// The audit note of an operation that runs the active config revision of
/// `id` (spec 0035, *Catatan audit pekerjaan*): its number and no details,
/// or no number when the instance has no revision yet. A read that fails
/// gives an empty note, because a note never stops the work.
pub(crate) async fn active_revision_note(deps: &SupervisorDeps, id: InstanceId) -> AuditNote {
    let registry = deps.registry.clone();
    let number = blocking(move || registry.active_config_revision(id))
        .await
        .ok()
        .and_then(Result::ok)
        .and_then(|read| read.value)
        .map(|revision| revision.number);
    AuditNote::new(number)
}

/// How a commit reacts to a registry refusal.
#[derive(Clone, Copy)]
pub(crate) enum CommitMode {
    /// An operation commits against the instance it read: a conflict is an
    /// error the operation decides on.
    Operation,
    /// An actor commit (exit, crash, adoption): it reads again after a
    /// conflict, waits on `Busy`, and degrades the subsystem while the
    /// registry is full or away.
    Actor,
}

/// Applies `build` to `base`, or to the instance as the registry holds it
/// when `base` is `None`, and commits the result with the extra ops
/// `build` returns.
///
/// In [`CommitMode::Actor`] a revision conflict reads the instance again
/// and runs `build` on the new aggregate, so the event is applied only if
/// it still holds.
///
/// # Errors
///
/// Returns the error of `build`, [`SupervisorError::RevisionMismatch`] for a
/// conflict an operation must decide on, and [`SupervisorError::Registry`]
/// for any other refusal.
pub(crate) async fn commit_instance(
    sup: &Supervisor,
    id: InstanceId,
    base: Option<&Instance>,
    mode: CommitMode,
    mut build: impl FnMut(&mut InstanceChanges) -> Result<Vec<WriteOp>, SupervisorError>,
) -> Result<(), SupervisorError> {
    commit_with(sup, id, base, mode, false, |changes, _| build(changes)).await
}

/// Like [`commit_instance`], for a commit that writes `exited` (or another
/// event that records how a process ended): `build` also learns whether a
/// force stop's `kill` step began for the instance
/// ([`force_intent`](stackly_domain::force_intent)). For an instance that
/// ever had a force stop the commit runs as a guarded commit that reads the
/// journal again inside its transaction, so a force stop that began since
/// `build` ran makes it build again (spec 0026, *Handler exit:
/// `last_exit`*).
///
/// # Errors
///
/// The errors of [`commit_instance`].
pub(crate) async fn commit_exit(
    sup: &Supervisor,
    id: InstanceId,
    base: Option<&Instance>,
    mode: CommitMode,
    build: impl FnMut(&mut InstanceChanges, bool) -> Result<Vec<WriteOp>, SupervisorError>,
) -> Result<(), SupervisorError> {
    commit_with(sup, id, base, mode, true, build).await
}

/// Times a guarded exit commit builds again because a force stop began
/// between its read and its transaction.
const FORCE_REBUILD_LIMIT: u32 = 5;

/// The loop behind [`commit_instance`] and [`commit_exit`].
async fn commit_with(
    sup: &Supervisor,
    id: InstanceId,
    base: Option<&Instance>,
    mode: CommitMode,
    force_aware: bool,
    mut build: impl FnMut(&mut InstanceChanges, bool) -> Result<Vec<WriteOp>, SupervisorError>,
) -> Result<(), SupervisorError> {
    let deps = sup.deps();
    let actor = sup.actor(id);
    let mut retry = ActorCommitRetry::new(&deps.config);
    let mut degraded = false;
    let mut pinned = base.cloned();
    let mut rebuilds = 0;
    let result = loop {
        // A daemon that shut down leaves the registry as it is (AC-21).
        if sup.is_shutdown() {
            break Err(SupervisorError::internal("the daemon is shutting down"));
        }
        let instance = match pinned.clone() {
            Some(instance) => instance,
            None => match read_instance(deps, id).await {
                Ok(instance) => instance,
                // An actor never takes a failed read for an answer: it
                // waits and reads again, like it does for a refused write.
                Err(SupervisorError::Registry(error)) if matches!(mode, CommitMode::Actor) => {
                    let step = retry.next(&BatchError::batch(error.clone()));
                    if !wait_out(deps, &actor, &mut degraded, step).await {
                        break Err(SupervisorError::Registry(error));
                    }
                    continue;
                }
                Err(error) => break Err(error),
            },
        };
        // `guarded` is set when the instance ever had a force stop: only
        // then can a `kill` step begin between this read and the commit
        // that matters. Without one, a force stop that begins later finds
        // this process already gone, so the exit was not forced.
        let (force, guarded) = if force_aware {
            match read_force_history(deps, id).await {
                Ok(history) => (force_intent(&history), !history.is_empty()),
                Err(SupervisorError::Registry(error)) if matches!(mode, CommitMode::Actor) => {
                    let step = retry.next(&BatchError::batch(error.clone()));
                    if !wait_out(deps, &actor, &mut degraded, step).await {
                        break Err(SupervisorError::Registry(error));
                    }
                    continue;
                }
                Err(error) => break Err(error),
            }
        } else {
            (false, false)
        };
        let failure_before = instance.last_failure().cloned();
        let mut changes = InstanceChanges::new(instance);
        let extra = match build(&mut changes, force) {
            Ok(extra) => extra,
            Err(error) => break Err(error),
        };
        let read = changes.read_revision();
        let after = changes.instance().clone();
        let failure = changes.failure().cloned();
        let mut ops = changes.into_ops();
        ops.extend(extra);
        let lease_effect = crate::session::startup_lease_effect(id, &ops);
        if ops.is_empty() {
            break Ok(());
        }
        let registry = deps.registry.clone();
        let sent = if guarded {
            let guard: CommitGuard = Box::new(move |view: &dyn GuardView| {
                let now = force_intent(&view.force_stop_history(id)?);
                Ok(if now == force {
                    GuardDecision::Write(ops)
                } else {
                    GuardDecision::Refuse(Box::new(()))
                })
            });
            match blocking(move || registry.commit_guarded(guard)).await {
                Ok(Ok(Guarded::Committed(committed))) => Ok(Ok(committed)),
                Ok(Ok(Guarded::Refused(_))) => {
                    rebuilds += 1;
                    if rebuilds > FORCE_REBUILD_LIMIT {
                        break Err(SupervisorError::internal(
                            "force stops kept beginning while an exit was recorded",
                        ));
                    }
                    pinned = None;
                    continue;
                }
                Ok(Err(error)) => Ok(Err(error)),
                Err(error) => Err(error),
            }
        } else {
            blocking(move || registry.commit(WriteBatch::new(ops))).await
        };
        let sent = match sent {
            Ok(sent) => sent,
            Err(error) => break Err(error),
        };
        match sent {
            Ok(_) => {
                actor.note_revision(after.revision());
                note_failure_facts(
                    deps,
                    &actor,
                    failure,
                    failure_before.as_ref(),
                    after.last_failure(),
                );
                if let Some(state) = lease_effect {
                    deps.events.startup_lease_changed(id, state);
                }
                deps.events.instance_changed(&after);
                break Ok(());
            }
            Err(error) => match mode {
                CommitMode::Operation => break Err(operation_error(id, read, &error)),
                CommitMode::Actor => {
                    let step = retry.next(&error);
                    if step == CommitRetry::Reread {
                        pinned = None;
                    }
                    if !wait_out(deps, &actor, &mut degraded, step).await {
                        break Err(SupervisorError::Registry(error.error));
                    }
                }
            },
        }
    };
    if degraded {
        degrade(deps, &actor, &mut degraded, false, "");
    }
    if force_aware {
        // Audit note 100: an exit an actor could not record leaves the
        // registry with a lifecycle that may be stale, so the actor says so
        // until an exit commit lands.
        match (&result, mode) {
            (Ok(()), _) => mark_exit_unrecorded(deps, &actor, false),
            (Err(_), CommitMode::Actor) if !sup.is_shutdown() => {
                mark_exit_unrecorded(deps, &actor, true);
            }
            (Err(_), _) => {}
        }
    }
    result
}

/// What the lease decision of a guarded stop commit let it write (spec
/// 0047, *Stop dengan guard lease*).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StopCommit {
    /// The stop committed: desired `stopped` and its lease ops. `overridden`
    /// names the environments a person's override stopped the instance
    /// against, for the audit note.
    Committed {
        /// The environments whose active lease the override passed.
        overridden: Vec<EnvironmentHolder>,
    },
    /// Another lease holds the instance, so an environment that goes down
    /// keeps it: neither desired state nor lease was written.
    Kept,
    /// Environments hold the instance and the person did not override them:
    /// nothing the stop would decide was written.
    RequiresOverride(Vec<EnvironmentHolder>),
}

/// The lease rule of one guarded stop commit: the stop's [`LeaseCheck`]
/// (never [`LeaseCheck::None`]) and whether the commit records an instance
/// that already does not run or whose process is gone (`quiet`). A quiet
/// manual stop disturbs nothing, so it never needs an override (AC-13).
#[derive(Debug, Clone)]
pub(crate) struct StopGate {
    pub(crate) check: LeaseCheck,
    pub(crate) quiet: bool,
}

/// What the guard of a stop commit decided from the leases it read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LeaseDecision {
    /// Stop, with these lease ops in the same commit.
    Stop {
        ops: Vec<WriteOp>,
        overridden: Vec<EnvironmentHolder>,
    },
    /// Keep the instance; another lease holds it.
    Kept,
    /// The person must override these environments first.
    RequiresOverride(Vec<EnvironmentHolder>),
}

/// Where the guard of a stop commit leaves its decision and the ops it
/// wrote, for the caller off the registry thread.
type DecisionSlot = Arc<Mutex<Option<(LeaseDecision, Vec<WriteOp>)>>>;

/// Why the guard of a stop commit wrote nothing.
enum StopRefusal {
    /// A force stop began since the commit was built (spec 0026): build
    /// again.
    Force,
    /// The lease decision is final and there was nothing to write.
    Decided,
}

/// Decides what a stop does to the leases of `id` (spec 0047): a manual
/// stop releases the manual leases and suppresses the startup lease of its
/// session ([`manual_stop_ops`]), or needs an override while environments
/// hold a live instance; an environment that goes down releases its own
/// leases, or keeps the instance while any other lease is active
/// ([`evaluate_stop`]). `environments` is read only when holders are named.
fn decide_leases(
    id: InstanceId,
    leases: &[Lease],
    gate: &StopGate,
    now: Timestamp,
    environments: impl FnOnce() -> Result<Vec<Environment>, RegistryError>,
) -> Result<LeaseDecision, RegistryError> {
    match &gate.check {
        LeaseCheck::None => Ok(LeaseDecision::Stop {
            ops: Vec::new(),
            overridden: Vec::new(),
        }),
        LeaseCheck::Manual {
            session,
            override_leases,
        } => {
            let passes = *override_leases || gate.quiet;
            match manual_stop_ops(id, leases, session.as_ref(), passes, now) {
                Some(ops) => {
                    // Only a live instance that environments hold is
                    // stopped against them; a quiet one disturbs nothing.
                    let overridden =
                        if *override_leases && !gate.quiet && holds_environment(id, leases) {
                            environment_holders(id, leases, &environments()?)
                        } else {
                            Vec::new()
                        };
                    Ok(LeaseDecision::Stop { ops, overridden })
                }
                None => Ok(LeaseDecision::RequiresOverride(environment_holders(
                    id,
                    leases,
                    &environments()?,
                ))),
            }
        }
        LeaseCheck::EnvironmentDown(environment) => {
            match evaluate_stop(id, leases, &StopRequest::EnvironmentDown(*environment)) {
                StopDecision::KeepRunning { .. } => Ok(LeaseDecision::Kept),
                StopDecision::Stop { release, .. } => {
                    let ops = leases
                        .iter()
                        .filter(|lease| release.contains(&lease.id()))
                        .filter_map(|lease| {
                            let mut lease = lease.clone();
                            lease
                                .release(now)
                                .ok()
                                .map(|()| WriteOp::UpdateLease(lease))
                        })
                        .collect();
                    Ok(LeaseDecision::Stop {
                        ops,
                        overridden: Vec::new(),
                    })
                }
                // `evaluate_stop` asks an override only of a manual stop.
                StopDecision::RequiresOverride { .. } => Ok(LeaseDecision::Kept),
            }
        }
    }
}

/// Whether an environment holds an active lease on `id`.
fn holds_environment(id: InstanceId, leases: &[Lease]) -> bool {
    leases.iter().any(|lease| {
        lease.instance_id() == id
            && lease.is_active()
            && matches!(lease.holder(), LeaseHolder::Environment(_))
    })
}

/// One side of a stop commit, built from the instance as read: the ops,
/// the aggregate after them, and the failure facts they wrote.
struct BuiltSide {
    ops: Vec<WriteOp>,
    after: Instance,
    failure: Option<FailureFacts>,
}

fn build_side(
    instance: &Instance,
    force: bool,
    stop: bool,
    build: &mut impl FnMut(&mut InstanceChanges, bool, bool) -> Result<Vec<WriteOp>, SupervisorError>,
) -> Result<BuiltSide, SupervisorError> {
    let mut changes = InstanceChanges::new(instance.clone());
    let extra = build(&mut changes, force, stop)?;
    let after = changes.instance().clone();
    let failure = changes.failure().cloned();
    let mut ops = changes.into_ops();
    ops.extend(extra);
    Ok(BuiltSide {
        ops,
        after,
        failure,
    })
}

/// Commits a stop's change of `id` with its lease decision in one guarded
/// commit (spec 0047, *Stop dengan guard lease*): the guard reads the
/// leases inside the transaction and decides once, with no rebuild.
///
/// `build` runs twice on the instance as read, like the build of
/// [`commit_exit`] with one more flag: with `stop` true it returns what the
/// stop writes when the decision lets it (desired `stopped` and the rest),
/// and with `stop` false what the commit writes anyway when the decision
/// keeps the instance (for a process that is gone, `stopping` to
/// `stopped`). The lease ops come from the guard, and the startup lease
/// effect is published from the ops it wrote. With `force_aware` the guard
/// also reads the force stop history as [`commit_exit`] does, and only a
/// force stop that began since the build makes it build again.
///
/// # Errors
///
/// The errors of [`commit_instance`] for a commit in
/// [`CommitMode::Operation`].
pub(crate) async fn commit_stop_decision(
    sup: &Supervisor,
    id: InstanceId,
    gate: StopGate,
    force_aware: bool,
    mut build: impl FnMut(&mut InstanceChanges, bool, bool) -> Result<Vec<WriteOp>, SupervisorError>,
) -> Result<StopCommit, SupervisorError> {
    let deps = sup.deps();
    let actor = sup.actor(id);
    let mut rebuilds = 0;
    let result = loop {
        if sup.is_shutdown() {
            break Err(SupervisorError::internal("the daemon is shutting down"));
        }
        let instance = match read_instance(deps, id).await {
            Ok(instance) => instance,
            Err(error) => break Err(error),
        };
        let (force, guarded_force) = if force_aware {
            match read_force_history(deps, id).await {
                Ok(history) => (force_intent(&history), !history.is_empty()),
                Err(error) => break Err(error),
            }
        } else {
            (false, false)
        };
        let failure_before = instance.last_failure().cloned();
        let read = instance.revision();
        let stopped = match build_side(&instance, force, true, &mut build) {
            Ok(side) => side,
            Err(error) => break Err(error),
        };
        let kept = match build_side(&instance, force, false, &mut build) {
            Ok(side) => side,
            Err(error) => break Err(error),
        };
        let now = deps.platform.clock().now();
        let slot: DecisionSlot = Arc::new(Mutex::new(None));
        let record = Arc::clone(&slot);
        let stop_ops = stopped.ops.clone();
        let keep_ops = kept.ops.clone();
        // An instance the commit finds at rest disturbs nobody, whatever the
        // caller saw before (AC-13); its revision guards the read.
        let at_rest = matches!(
            instance.lifecycle(),
            InstanceLifecycle::Stopped
                | InstanceLifecycle::Failed
                | InstanceLifecycle::Uninitialized
        );
        let guard_gate = StopGate {
            check: gate.check.clone(),
            quiet: gate.quiet || at_rest,
        };
        let guard: CommitGuard = Box::new(move |view: &dyn GuardView| {
            if guarded_force && force_intent(&view.force_stop_history(id)?) != force {
                return Ok(GuardDecision::Refuse(Box::new(StopRefusal::Force)));
            }
            let leases = view.leases(id, LeaseFilter::Unreleased)?;
            let decision = decide_leases(id, &leases, &guard_gate, now, || view.environments())?;
            let ops = match &decision {
                LeaseDecision::Stop { ops, .. } => {
                    let mut all = stop_ops;
                    all.extend(ops.iter().cloned());
                    all
                }
                LeaseDecision::Kept | LeaseDecision::RequiresOverride(_) => keep_ops,
            };
            *record.lock().unwrap_or_else(PoisonError::into_inner) = Some((decision, ops.clone()));
            Ok(if ops.is_empty() {
                GuardDecision::Refuse(Box::new(StopRefusal::Decided))
            } else {
                GuardDecision::Write(ops)
            })
        });
        let registry = deps.registry.clone();
        let sent = match blocking(move || registry.commit_guarded(guard)).await {
            Ok(sent) => sent,
            Err(error) => break Err(error),
        };
        let wrote = match sent {
            Ok(Guarded::Committed(_)) => true,
            Ok(Guarded::Refused(reason)) => {
                if matches!(
                    reason.downcast_ref::<StopRefusal>(),
                    Some(StopRefusal::Force)
                ) {
                    rebuilds += 1;
                    if rebuilds > FORCE_REBUILD_LIMIT {
                        break Err(SupervisorError::internal(
                            "force stops kept beginning while a stop was recorded",
                        ));
                    }
                    continue;
                }
                false
            }
            Err(error) => break Err(operation_error(id, read, &error)),
        };
        let Some((decision, written)) = slot.lock().unwrap_or_else(PoisonError::into_inner).take()
        else {
            break Err(SupervisorError::internal("the stop guard did not run"));
        };
        if wrote {
            let side = if matches!(decision, LeaseDecision::Stop { .. }) {
                &stopped
            } else {
                &kept
            };
            actor.note_revision(side.after.revision());
            note_failure_facts(
                deps,
                &actor,
                side.failure.clone(),
                failure_before.as_ref(),
                side.after.last_failure(),
            );
            if let Some(state) = crate::session::startup_lease_effect(id, &written) {
                deps.events.startup_lease_changed(id, state);
            }
            deps.events.instance_changed(&side.after);
        }
        break Ok(match decision {
            LeaseDecision::Stop { overridden, .. } => StopCommit::Committed { overridden },
            LeaseDecision::Kept => StopCommit::Kept,
            LeaseDecision::RequiresOverride(holders) => StopCommit::RequiresOverride(holders),
        });
    };
    if force_aware && result.is_ok() {
        mark_exit_unrecorded(deps, &actor, false);
    }
    result
}

/// Hands the actor the failure facts of a committed batch (spec 0033, AC-3):
/// the facts of the failure event it wrote replace the ones the actor
/// holds, and a batch that changed the stored `last_failure` without a
/// failure event (`spawned`, `adopted`, a refused restore) clears them, so
/// the actor never holds facts of a failure the registry no longer names.
/// The actor publishes the change before the commit's `instance.changed`.
fn note_failure_facts(
    deps: &SupervisorDeps,
    actor: &InstanceActor,
    failure: Option<FailureFacts>,
    before: Option<&stackly_domain::FailureCode>,
    after: Option<&stackly_domain::FailureCode>,
) {
    match failure {
        Some(facts) => actor.update(deps, |state| state.last_failure = Some(facts)),
        None if before != after => actor.update(deps, |state| state.last_failure = None),
        None => {}
    }
}

/// Records whether the actor gave up recording how a process ended (spec
/// 0010, *Commit*, audit note 100).
pub(crate) fn mark_exit_unrecorded(
    deps: &SupervisorDeps,
    actor: &super::actor::InstanceActor,
    unrecorded: bool,
) {
    actor.update(deps, |state| state.exit_unrecorded = unrecorded);
}

/// Reads the newest force stop of `id` and the operations after it.
///
/// # Errors
///
/// Returns [`SupervisorError::Registry`] when the journal cannot be read.
async fn read_force_history(
    deps: &SupervisorDeps,
    id: InstanceId,
) -> Result<Vec<(stackly_domain::Operation, stackly_domain::JournalView)>, SupervisorError> {
    let registry = deps.registry.clone();
    Ok(blocking(move || registry.force_stop_history(id))
        .await?
        .map_err(SupervisorError::Registry)?
        .value)
}

/// Waits as `step` says. Returns false when the actor must give up.
async fn wait_out(
    deps: &SupervisorDeps,
    actor: &super::actor::InstanceActor,
    degraded: &mut bool,
    step: CommitRetry,
) -> bool {
    match step {
        CommitRetry::Reread => true,
        CommitRetry::Wait(delay) => {
            degrade(deps, actor, degraded, true, "the registry is busy");
            tokio::time::sleep(delay).await;
            true
        }
        CommitRetry::Degraded(delay) => {
            degrade(
                deps,
                actor,
                degraded,
                true,
                "the registry cannot take a write",
            );
            tokio::time::sleep(delay).await;
            true
        }
        CommitRetry::GiveUp => false,
    }
}

/// Reads an instance for an actor (AC-22): a refused read is never an
/// answer. It waits as an actor commit does and reads again, and the
/// subsystem reads `degraded` meanwhile.
///
/// # Errors
///
/// Returns [`SupervisorError::NotFound`] for an unknown instance,
/// [`SupervisorError::Registry`] for an error no wait cures, and
/// [`SupervisorError::Internal`] once the daemon shuts down.
pub(crate) async fn read_instance_actor(
    sup: &Supervisor,
    id: InstanceId,
) -> Result<Instance, SupervisorError> {
    let deps = sup.deps();
    let actor = sup.actor(id);
    let mut retry = ActorCommitRetry::new(&deps.config);
    let mut degraded = false;
    let result = loop {
        if sup.is_shutdown() {
            break Err(SupervisorError::internal("the daemon is shutting down"));
        }
        match read_instance(deps, id).await {
            Ok(instance) => break Ok(instance),
            Err(SupervisorError::Registry(error)) => {
                let step = retry.next(&BatchError::batch(error.clone()));
                if !wait_out(deps, &actor, &mut degraded, step).await {
                    break Err(SupervisorError::Registry(error));
                }
            }
            Err(error) => break Err(error),
        }
    };
    if degraded {
        degrade(deps, &actor, &mut degraded, false, "");
    }
    result
}

/// Reports the registry as degraded or back, and marks the actor's commit
/// as pending while it waits.
fn degrade(
    deps: &SupervisorDeps,
    actor: &super::actor::InstanceActor,
    degraded: &mut bool,
    now_degraded: bool,
    detail: &str,
) {
    if *degraded != now_degraded {
        *degraded = now_degraded;
        deps.events.registry_degraded(now_degraded, detail);
    }
    actor.update(deps, |state| state.pending_commit = now_degraded);
}

/// Maps the refusal of an operation's commit.
fn operation_error(
    id: InstanceId,
    read: stackly_domain::ResourceRevision,
    error: &BatchError,
) -> SupervisorError {
    match &error.error {
        RegistryError::RevisionConflict { actual, .. } => SupervisorError::RevisionMismatch {
            instance: id,
            expected: read,
            actual: *actual,
        },
        other => SupervisorError::Registry(other.clone()),
    }
}

/// Applies one lifecycle event to the changes of a commit.
///
/// # Errors
///
/// Returns [`SupervisorError::Internal`] when the event does not fit the
/// lifecycle the instance is in: the caller moved on since it read.
pub(crate) fn apply_event(
    changes: &mut InstanceChanges,
    event: LifecycleEvent,
    at: Timestamp,
) -> Result<(), SupervisorError> {
    apply_event_after(changes, event, at, None)
}

/// Applies one lifecycle event that follows `exit`, the exit status of the
/// process whose end led to it, or `None` when no process ended. A failure
/// event (`init_failed`, `start_failed`, `crashed`) also notes its
/// [`FailureFacts`] in the changes: its code, `at`, and that exit (spec
/// 0033, AC-3).
///
/// # Errors
///
/// The errors of [`apply_event`].
pub(crate) fn apply_event_after(
    changes: &mut InstanceChanges,
    event: LifecycleEvent,
    at: Timestamp,
    exit: Option<ExitStatus>,
) -> Result<(), SupervisorError> {
    let failure = match &event {
        LifecycleEvent::InitFailed { reason }
        | LifecycleEvent::StartFailed { reason }
        | LifecycleEvent::Crashed { reason, .. } => Some(reason.clone()),
        _ => None,
    };
    changes.apply(|instance| {
        instance
            .apply_lifecycle(event, at)
            .map(|_| ())
            .map_err(|error| SupervisorError::internal(error.to_string()))
    })?;
    if let Some(code) = failure {
        changes.note_failure(FailureFacts::new(code, at, exit));
    }
    Ok(())
}

/// What [`apply_exit`] recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitRecord {
    /// The crash event applied: the lifecycle is `recovering` or `failed`.
    Crashed,
    /// The exit ended a stop, asked for or not: the lifecycle is `stopped`.
    Stopped,
    /// The lifecycle had nothing to record.
    Nothing,
}

/// How the process that ended was seen to end, for [`exit_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitSeen {
    /// The exit status of a child of this daemon; `None` for an adopted
    /// process, whose status is never known.
    pub(crate) status: Option<ExitStatus>,
    /// The number of the stop signal the daemon sent it, if any.
    pub(crate) sent_signal: Option<i32>,
}

impl ExitSeen {
    /// An exit with nothing known about how the process ended: a process
    /// found gone, or one a stop saw leave.
    pub(crate) const UNSEEN: Self = Self {
        status: None,
        sent_signal: None,
    };
}

/// Returns the [`ExitKind`] an exit records (spec 0026, *Handler exit:
/// `last_exit`*), from the lifecycle the commit read: `forced` whenever a
/// force stop's `kill` step began; while `stopping`, `crashed` for a
/// child that a signal the daemon did not send ended, else `clean`; and
/// `crashed` for every exit while `starting`, `running`, or `recovering`.
pub(crate) fn exit_kind(force: bool, lifecycle: InstanceLifecycle, seen: ExitSeen) -> ExitKind {
    if force {
        return ExitKind::Forced;
    }
    match lifecycle {
        InstanceLifecycle::Stopping => match seen.status {
            Some(ExitStatus::Signaled(signal)) if Some(signal) != seen.sent_signal => {
                ExitKind::Crashed
            }
            _ => ExitKind::Clean,
        },
        _ => ExitKind::Crashed,
    }
}

/// Applies `exited` with the [`ExitKind`] of a process that left while the
/// instance is `stopping` and nothing more is known about it (a stop or
/// a force stop that found its process gone).
///
/// # Errors
///
/// Returns [`SupervisorError::Internal`] when the lifecycle is not
/// `stopping`.
pub(crate) fn apply_stopping_exit(
    changes: &mut InstanceChanges,
    force: bool,
    at: Timestamp,
) -> Result<(), SupervisorError> {
    let exit = exit_kind(force, InstanceLifecycle::Stopping, ExitSeen::UNSEEN);
    apply_event(changes, LifecycleEvent::Exited { exit: Some(exit) }, at)
}

/// Applies the meaning of a process exit to the aggregate the commit just
/// read, so a stop that landed between the actor's decision and its commit
/// is honoured (AC-22: the event is applied again only if it still holds).
/// A lifecycle `stopping`, or a running one whose desired state is
/// `stopped`, ends `stopped`; any other running lifecycle takes `crash`.
/// The `exited` event carries the [`ExitKind`] of [`exit_kind`] when `seen`
/// describes a process that ended, and none otherwise.
///
/// # Errors
///
/// Returns [`SupervisorError::Internal`] when `crash` does not fit the
/// lifecycle.
pub(crate) fn apply_exit(
    changes: &mut InstanceChanges,
    crash: LifecycleEvent,
    force: bool,
    seen: Option<ExitSeen>,
    at: Timestamp,
) -> Result<ExitRecord, SupervisorError> {
    let desired = changes.instance().desired();
    let lifecycle = changes.instance().lifecycle();
    // Without a process that ended (`seen` is `None`), `exited` leaves
    // `last_exit` as it is.
    let exit = seen.map(|seen| exit_kind(force, lifecycle, seen));
    match lifecycle {
        InstanceLifecycle::Stopping => {
            apply_event(changes, LifecycleEvent::Exited { exit }, at)?;
            Ok(ExitRecord::Stopped)
        }
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering
            if desired == DesiredState::Stopped =>
        {
            apply_event(changes, LifecycleEvent::StopBegin, at)?;
            apply_event(changes, LifecycleEvent::Exited { exit }, at)?;
            Ok(ExitRecord::Stopped)
        }
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering => {
            apply_event_after(changes, crash, at, seen.and_then(|seen| seen.status))?;
            Ok(ExitRecord::Crashed)
        }
        _ => Ok(ExitRecord::Nothing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn seen(status: Option<ExitStatus>, sent_signal: Option<i32>) -> ExitSeen {
        ExitSeen {
            status,
            sent_signal,
        }
    }

    #[test]
    fn a_force_intent_makes_every_exit_forced() {
        for lifecycle in InstanceLifecycle::ALL {
            assert_eq!(
                exit_kind(true, *lifecycle, seen(Some(ExitStatus::Signaled(9)), None)),
                ExitKind::Forced
            );
        }
    }

    #[test]
    fn an_exit_while_stopping_is_clean_unless_a_foreign_signal_ended_it() {
        let stopping = InstanceLifecycle::Stopping;

        assert_eq!(
            exit_kind(false, stopping, seen(Some(ExitStatus::Exited(0)), Some(15))),
            ExitKind::Clean
        );
        assert_eq!(
            exit_kind(
                false,
                stopping,
                seen(Some(ExitStatus::Signaled(15)), Some(15))
            ),
            ExitKind::Clean
        );
        assert_eq!(
            exit_kind(
                false,
                stopping,
                seen(Some(ExitStatus::Signaled(11)), Some(15))
            ),
            ExitKind::Crashed
        );
        assert_eq!(
            exit_kind(false, stopping, ExitSeen::UNSEEN),
            ExitKind::Clean,
            "an adopted process has no status, so it reads clean"
        );
    }

    #[test]
    fn an_exit_while_running_starting_or_recovering_is_crashed() {
        for lifecycle in [
            InstanceLifecycle::Starting,
            InstanceLifecycle::Running,
            InstanceLifecycle::Recovering,
        ] {
            assert_eq!(
                exit_kind(false, lifecycle, seen(Some(ExitStatus::Exited(0)), None)),
                ExitKind::Crashed
            );
        }
    }
}
