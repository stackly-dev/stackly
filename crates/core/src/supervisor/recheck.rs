//! The periodic look at what reconciliation could not settle (spec 0010,
//! *Rekonsiliasi*): every `UNKNOWN_RECHECK_INTERVAL` an `unknown` instance
//! is classified again with the same evidence, and an instance with a
//! `dataInUse` blocker is checked against its lock.
//!
//! `Proven` becomes `adopted`, `Gone` becomes `confirmed_stopped` without a
//! start of its own, and `Unproven` stays. A `dataInUse` blocker goes when
//! the lock is free.

use stackly_domain::{Instance, InstanceBlocker, InstanceLifecycle, LifecycleEvent};
use stackly_platform::LockState;

use super::Supervisor;
use super::actor::InstanceActor;
use super::adopt::adopt_process;
use super::persist::{
    CommitMode, apply_event, blocking, commit_exit, mark_exit_unrecorded, read_instance,
};
use super::reconcile::{Source, settle_instance};

/// Starts the recheck task, once per supervisor. It ends at shutdown.
pub(crate) fn spawn_recheck(sup: &Supervisor) {
    if sup
        .shared
        .recheck_running
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    // Weak, because the task sleeps past the daemon's stop and a strong
    // handle would keep the registry open meanwhile.
    let interval = sup.deps().config.unknown_recheck_interval;
    let sup = sup.downgrade();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let Some(sup) = sup.upgrade() else {
                return;
            };
            if sup.is_shutdown() {
                return;
            }
            for actor in sup.actors() {
                recheck_one(&sup, &actor).await;
            }
        }
    });
}

/// Looks at one instance once.
pub(crate) async fn recheck_one(sup: &Supervisor, actor: &InstanceActor) {
    let deps = sup.deps();
    let id = actor.id;
    let Ok(mut instance) = read_instance(deps, id).await else {
        return;
    };
    if lose_unrecorded_exit(sup, actor, &instance).await {
        let Ok(fresh) = read_instance(deps, id).await else {
            return;
        };
        instance = fresh;
    }
    if instance.lifecycle() == InstanceLifecycle::Unknown {
        if let Some((_, Some(adoption))) = settle_instance(sup, &instance, Source::Recheck).await
            && !adopt_process(sup, adoption).await
        {
            tracing::warn!(instance = %id, "a proven process could not be adopted");
        }
        return;
    }
    let held = actor.read(|state| matches!(state.blocker, Some(InstanceBlocker::DataInUse { .. })));
    if !held {
        return;
    }
    let platform = deps.platform.clone();
    let path = deps.layout.instance_paths(id).lock;
    let state = blocking(move || platform.processes().probe_lock(&path)).await;
    if matches!(state, Ok(Ok(LockState::Free))) {
        actor.update(deps, |state| state.set_supervisor_blocker(None));
    }
}

/// Settles an exit the actor gave up recording (spec 0010, *Commit*, audit
/// note 100). With no process and no stop or start watching one, a live
/// lifecycle in the registry is stale: it becomes `unknown` through
/// `identity_lost`, so the recheck of `unknown` confirms it stopped or
/// adopts what it finds, and nothing starts on its own. A lifecycle that
/// is no longer live only clears the mark. Returns true when it committed.
async fn lose_unrecorded_exit(
    sup: &Supervisor,
    actor: &InstanceActor,
    instance: &Instance,
) -> bool {
    let deps = sup.deps();
    let idle = actor.read(|state| {
        state.exit_unrecorded && state.process.is_none() && !state.stopping && !state.op_owns_exit
    });
    if !idle {
        return false;
    }
    if !is_live(instance.lifecycle()) {
        mark_exit_unrecorded(deps, actor, false);
        return false;
    }
    let now = deps.platform.clock().now();
    let committed = commit_exit(sup, actor.id, None, CommitMode::Actor, |changes, _| {
        if is_live(changes.instance().lifecycle()) {
            apply_event(changes, LifecycleEvent::IdentityLost, now)?;
        }
        Ok(Vec::new())
    })
    .await;
    match committed {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(instance = %actor.id, %error, "an unrecorded exit could not be settled");
            false
        }
    }
}

/// Whether `lifecycle` says a process runs or is on its way.
const fn is_live(lifecycle: InstanceLifecycle) -> bool {
    matches!(
        lifecycle,
        InstanceLifecycle::Starting
            | InstanceLifecycle::Running
            | InstanceLifecycle::Recovering
            | InstanceLifecycle::Stopping
    )
}
