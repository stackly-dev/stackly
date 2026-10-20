//! Reading and writing the registry for the supervisor (spec 0010,
//! *Commit*): every registry call runs on the blocking pool, every change
//! of an aggregate is one `UpdateInstance` per domain mutation, and an actor
//! commit retries as AC-22 says.

use jiff::Timestamp;
use stackly_domain::{
    BatchError, DesiredState, Instance, InstanceId, InstanceLifecycle, LifecycleEvent,
    RegistryError, WriteBatch, WriteOp,
};

use super::commit::{ActorCommitRetry, CommitRetry, InstanceChanges};
use super::error::SupervisorError;
use super::{Supervisor, SupervisorDeps};

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
    let deps = sup.deps();
    let actor = sup.actor(id);
    let mut retry = ActorCommitRetry::new(&deps.config);
    let mut degraded = false;
    let mut pinned = base.cloned();
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
        let mut changes = InstanceChanges::new(instance);
        let extra = match build(&mut changes) {
            Ok(extra) => extra,
            Err(error) => break Err(error),
        };
        let read = changes.read_revision();
        let after = changes.instance().clone();
        let mut ops = changes.into_ops();
        ops.extend(extra);
        if ops.is_empty() {
            break Ok(());
        }
        let registry = deps.registry.clone();
        let sent = match blocking(move || registry.commit(WriteBatch::new(ops))).await {
            Ok(sent) => sent,
            Err(error) => break Err(error),
        };
        match sent {
            Ok(_) => {
                actor.note_revision(after.revision());
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
    result
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
    changes.apply(|instance| {
        instance
            .apply_lifecycle(event, at)
            .map(|_| ())
            .map_err(|error| SupervisorError::internal(error.to_string()))
    })
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

/// Applies the meaning of a process exit to the aggregate the commit just
/// read, so a stop that landed between the actor's decision and its commit
/// is honoured (AC-22: the event is applied again only if it still holds).
/// A lifecycle `stopping`, or a running one whose desired state is
/// `stopped`, ends `stopped`; any other running lifecycle takes `crash`.
///
/// # Errors
///
/// Returns [`SupervisorError::Internal`] when `crash` does not fit the
/// lifecycle.
pub(crate) fn apply_exit(
    changes: &mut InstanceChanges,
    crash: LifecycleEvent,
    at: Timestamp,
) -> Result<ExitRecord, SupervisorError> {
    let desired = changes.instance().desired();
    match changes.instance().lifecycle() {
        InstanceLifecycle::Stopping => {
            apply_event(changes, LifecycleEvent::Exited, at)?;
            Ok(ExitRecord::Stopped)
        }
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering
            if desired == DesiredState::Stopped =>
        {
            apply_event(changes, LifecycleEvent::StopBegin, at)?;
            apply_event(changes, LifecycleEvent::Exited, at)?;
            Ok(ExitRecord::Stopped)
        }
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering => {
            apply_event(changes, crash, at)?;
            Ok(ExitRecord::Crashed)
        }
        _ => Ok(ExitRecord::Nothing),
    }
}
