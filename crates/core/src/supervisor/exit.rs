//! What happens when a supervised process exits (spec 0010, *Stop*, step 5,
//! and *Restart policy*): one handler, whether a stop asked for the exit or
//! the process died on its own. A crash outside any operation goes through
//! the restart policy here and, when the budget allows, starts the pause of
//! [`respawn`](super::respawn).
//!
//! The watcher of the platform hands the exit to a [`watch`] channel; one
//! task per process waits on it. A start operation that is still waiting for
//! readiness owns the exit of its own process (it applies the restart
//! policy inside the operation) and hands it back when it ends.

use std::sync::Arc;

use stackly_domain::{
    DesiredState, FailureCode, InstanceId, InstanceLifecycle, LifecycleEvent, Pid,
};
use stackly_platform::{ExitNotice, ExitStatus};
use tokio::sync::watch;

use super::Supervisor;
use super::actor::InstanceActor;
use super::error::SupervisorError;
use super::persist::{CommitMode, apply_exit, blocking, commit_instance, read_instance_actor};
use super::respawn::schedule_respawn;
use super::startup::{Next, after_unrequested_exit};
use crate::secret::delivery::remove_secret_material;

/// Starts the task that handles the exit of one process.
pub(crate) fn spawn_exit_task(
    sup: &Supervisor,
    actor: Arc<InstanceActor>,
    generation: u64,
    mut exit: watch::Receiver<Option<ExitNotice>>,
) {
    // A process outlives a daemon that shuts down, so this task may wait
    // for the exit past the stop. It holds the supervisor weakly: a strong
    // handle would keep the registry open until the process is gone.
    let sup = sup.downgrade();
    tokio::spawn(async move {
        let notice = match exit.wait_for(Option::is_some).await {
            Ok(seen) => seen.clone(),
            // The watcher dropped its listener without an exit: nothing to
            // handle, and nothing to claim.
            Err(_) => return,
        };
        let Some(notice) = notice else { return };
        loop {
            if !actor.read(|state| state.op_owns_exit) {
                break;
            }
            actor.released.notified().await;
        }
        // A shut down daemon leaves the registry as it is: the next daemon
        // finds the process gone or adopts it.
        let Some(sup) = sup.upgrade() else { return };
        if sup.is_shutdown() {
            return;
        }
        handle_exit(&sup, &actor, generation, &notice).await;
    });
}

/// Returns the failure code a crash with this status records.
pub(crate) fn crash_reason(status: Option<ExitStatus>) -> FailureCode {
    match status {
        Some(ExitStatus::Exited(_)) => FailureCode::PROCESS_EXITED,
        Some(ExitStatus::Signaled(_)) => FailureCode::PROCESS_SIGNALED,
        None => FailureCode::EXIT_STATUS_UNKNOWN,
    }
}

/// Handles the exit of the process of generation `generation`: reads the
/// spools to the end, removes the secret
/// material, reports the members of the group that are left, records the
/// lifecycle event, and lets everyone waiting for the process know.
pub(crate) async fn handle_exit(
    sup: &Supervisor,
    actor: &Arc<InstanceActor>,
    generation: u64,
    notice: &ExitNotice,
) {
    let deps = sup.deps();
    let info = actor.read(|state| {
        state
            .process
            .as_ref()
            .filter(|slot| slot.generation == generation)
            .map(|slot| (slot.group, Arc::clone(&slot.drain)))
    });
    let Some((group, drain)) = info else {
        return;
    };
    actor.update(deps, |state| {
        if let Some(slot) = &state.process {
            slot.probes_stop.send_replace(true);
        }
    });

    drain.finish(sup, actor.id).await;
    remove_material(sup, actor).await;
    let leftover = leftover_members(sup, group, notice.pid).await;

    let next = settle(sup, actor.id, notice).await;
    finish_process(sup, actor, generation, leftover);
    match next {
        Some(Next::Retry(delay)) => schedule_respawn(sup, actor.id, delay),
        Some(Next::Final(error)) => {
            tracing::info!(instance = %actor.id, %error, "the instance is not restarted");
        }
        Some(Next::Settled) | None => {}
    }
}

/// Records what the exit means for the lifecycle, from the aggregate as the
/// registry holds it now: the memory of the actor is never the basis (a stop
/// can land at any moment) and a refused read is never an answer (AC-22).
/// With desired `running` and a running lifecycle it is a crash and the
/// restart policy decides (spec 0010, *Restart policy*). A stop in progress,
/// or a desired `stopped`, ends the instance `stopped`.
async fn settle(sup: &Supervisor, id: InstanceId, notice: &ExitNotice) -> Option<Next> {
    let deps = sup.deps();
    let instance = match read_instance_actor(sup, id).await {
        Ok(instance) => instance,
        // The instance is gone: nothing to record.
        Err(SupervisorError::NotFound(_)) => return None,
        Err(error) => {
            tracing::warn!(instance = %id, %error, "the exit of a process could not be recorded");
            return Some(Next::Final(error));
        }
    };
    if instance.desired() == DesiredState::Running
        && matches!(
            instance.lifecycle(),
            InstanceLifecycle::Starting
                | InstanceLifecycle::Running
                | InstanceLifecycle::Recovering
        )
    {
        return match after_unrequested_exit(sup, id, notice, CommitMode::Actor).await {
            Next::Settled => None,
            next => Some(next),
        };
    }
    let now = deps.platform.clock().now();
    let crash = LifecycleEvent::Crashed {
        will_retry: false,
        reason: crash_reason(notice.status),
    };
    let committed = commit_instance(sup, id, None, CommitMode::Actor, |changes| {
        apply_exit(changes, crash.clone(), now)?;
        Ok(Vec::new())
    })
    .await;
    if let Err(error) = committed {
        tracing::warn!(instance = %id, %error, "the exit of a process could not be recorded");
    }
    None
}

/// Removes the instance's secret material after its process exited.
pub(crate) async fn remove_material(sup: &Supervisor, actor: &InstanceActor) {
    let deps = sup.deps().clone();
    let key = actor.id.short_key();
    let removed = blocking(move || remove_secret_material(&deps.platform, &deps.layout, key)).await;
    match removed {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(instance = %actor.id, %error, "secret material could not be removed");
        }
        Err(error) => {
            tracing::warn!(instance = %actor.id, %error, "secret material removal did not run");
        }
    }
}

/// Lists the members left in the group after the main process exited. They
/// are reported and never signalled.
async fn leftover_members(sup: &Supervisor, group: Pid, main: Pid) -> Vec<Pid> {
    let platform = sup.deps().platform.clone();
    match blocking(move || platform.processes().group_members(group)).await {
        Ok(Ok(members)) => members.into_iter().filter(|pid| *pid != main).collect(),
        _ => Vec::new(),
    }
}

/// Ends the actor's record of the process: readiness and health read
/// `unknown` with the reason `not_running`, and every task waiting for the
/// process is released.
pub(crate) fn finish_process(
    sup: &Supervisor,
    actor: &InstanceActor,
    generation: u64,
    leftover: Vec<Pid>,
) {
    let deps = sup.deps();
    let now = deps.platform.clock().now();
    actor.update(deps, |state| {
        let ours = state
            .process
            .as_ref()
            .is_some_and(|slot| slot.generation == generation);
        if !ours {
            return;
        }
        if let Some(slot) = state.process.take() {
            slot.probes_stop.send_replace(true);
            slot.drain.stop();
            slot.done.send_replace(true);
        }
        state.stopping = false;
        state.op_owns_exit = false;
        state.leftover = leftover;
        state.not_running(now);
    });
    actor.released.notify_one();
}
