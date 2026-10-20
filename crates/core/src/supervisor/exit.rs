//! What happens when a supervised process exits (spec 0010, *Stop*, step 5,
//! and *Restart policy*): one handler, whether a stop asked for the exit or
//! the process died on its own.
//!
//! The watcher of the platform hands the exit to a [`watch`] channel; one
//! task per process waits on it. A start operation that is still waiting for
//! readiness owns the exit of its own process (it applies the restart
//! policy inside the operation) and hands it back when it ends.

use std::sync::Arc;

use stackly_domain::{FailureCode, LifecycleEvent, Pid};
use stackly_platform::{ExitNotice, ExitStatus};
use tokio::sync::watch;

use super::Supervisor;
use super::actor::InstanceActor;
use super::persist::{CommitMode, apply_event, blocking, commit_instance};
use crate::secret::delivery::remove_secret_material;

/// Starts the task that handles the exit of one process.
pub(crate) fn spawn_exit_task(
    sup: Supervisor,
    actor: Arc<InstanceActor>,
    generation: u64,
    mut exit: watch::Receiver<Option<ExitNotice>>,
) {
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
/// spool to the end (spec 0010 step 10, not built yet), removes the secret
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
            .map(|slot| (slot.group, state.stopping))
    });
    let Some((group, stopping)) = info else {
        return;
    };
    actor.update(deps, |state| {
        if let Some(slot) = &state.process {
            slot.probes_stop.send_replace(true);
        }
    });

    remove_material(sup, actor).await;
    let leftover = leftover_members(sup, group, notice.pid).await;

    let now = deps.platform.clock().now();
    let event = if stopping {
        LifecycleEvent::Exited
    } else {
        // provisional: F10 step 9. Until the restart policy runs in the
        // actor, an exit outside an operation ends the instance `failed`
        // with the reason, and the user starts it again.
        LifecycleEvent::Crashed {
            will_retry: false,
            reason: crash_reason(notice.status),
        }
    };
    let committed = commit_instance(sup, actor.id, None, CommitMode::Actor, |changes| {
        apply_event(changes, event.clone(), now)?;
        Ok(Vec::new())
    })
    .await;
    if let Err(error) = committed {
        tracing::warn!(instance = %actor.id, %error, "the exit of a process could not be recorded");
    }
    finish_process(sup, actor, generation, leftover);
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
            slot.done.send_replace(true);
        }
        state.stopping = false;
        state.op_owns_exit = false;
        state.leftover = leftover;
        state.not_running(now);
    });
    actor.released.notify_one();
}
