//! Adopting a process that outlived the daemon (spec 0010, *Rekonsiliasi*,
//! step 5, AC-9): a process whose identity is proven gets its exit
//! watched, its spool read again from the first byte that was not freed,
//! and its readiness probed at the port that was stored.

use std::collections::BTreeMap;
use std::sync::Arc;

use stackly_domain::{
    IdentityEvidence, InstanceBlocker, InstanceId, LifecycleEvent, Pid, ProcessIdentity,
};
use stackly_platform::{FileIdentity, SignalTarget};
use tokio::sync::watch;

use super::Supervisor;
use super::actor::{ExitSlot, ProcessSlot};
use super::drain::{DrainSlot, Opening};
use super::exit::{finish_process, spawn_exit_task};
use super::persist::{CommitMode, apply_event, blocking, commit_instance};
use super::prepare::{Checks, prepare};
use super::readiness::spawn_probes;
use crate::provider::{HealthPlan, ProbePlan};

/// A proven process to adopt.
pub(crate) struct Adoption {
    /// The instance it belongs to.
    pub(crate) id: InstanceId,
    /// The recorded identity it was proven against.
    pub(crate) identity: ProcessIdentity,
    /// True when it is still suspended: the daemon died after the commit
    /// of its identity and before `SIGCONT`.
    pub(crate) stopped: bool,
    /// Its process group, when the facts were read; otherwise its own PID,
    /// because the main process is the leader of its group (`SETSID`).
    pub(crate) group: Option<Pid>,
    /// The lock file it holds.
    pub(crate) lock: Option<FileIdentity>,
}

/// Adopts the process. Returns false when it could not be adopted; the
/// instance is then `unknown` with the blocker `identityUnproven`, and the
/// process is left as it is.
pub(crate) async fn adopt_process(sup: &Supervisor, adoption: Adoption) -> bool {
    let deps = sup.deps();
    let id = adoption.id;
    let identity = adoption.identity;
    let Some(lock) = adoption.lock else {
        lose_identity(sup, id, &identity).await;
        return false;
    };
    if adoption.stopped {
        // A process held back before its first instruction (the daemon died
        // between the commit and `SIGCONT`) is let go first.
        let platform = deps.platform.clone();
        let target = SignalTarget::new(identity.clone());
        let resumed = blocking(move || platform.processes().continue_stopped(&target)).await;
        if !matches!(resumed, Ok(Ok(()))) {
            tracing::warn!(instance = %id, "a proven process that was held back could not be continued");
            lose_identity(sup, id, &identity).await;
            return false;
        }
    }

    let actor = sup.actor(id);
    let (exit_slot, exit_rx) = ExitSlot::new();
    let (done, _) = watch::channel(false);
    let (probes_stop, probes_stop_rx) = watch::channel(false);
    let (drain, logs_unavailable) = DrainSlot::start(sup, id, Opening::Resumed).await;
    let pid = identity.pid;
    let generation = actor.update(deps, |state| {
        let generation = state.next_generation;
        state.next_generation += 1;
        state.process = Some(ProcessSlot {
            generation,
            identity: identity.clone(),
            group: adoption.group.unwrap_or(pid),
            adopted: true,
            adoptable: Some(true),
            lock,
            done,
            probes_stop,
            drain,
        });
        state.op_owns_exit = false;
        state.stopping = false;
        state.leftover.clear();
        state.logs_unavailable = logs_unavailable;
        generation
    });
    let watched = {
        let platform = deps.platform.clone();
        blocking(move || platform.processes().watch_exit(pid, Arc::new(exit_slot))).await
    };
    if !matches!(watched, Ok(Ok(()))) {
        tracing::warn!(instance = %id, "the exit of a proven process could not be watched");
        finish_process(sup, &actor, generation, Vec::new());
        lose_identity(sup, id, &identity).await;
        return false;
    }
    spawn_exit_task(sup, Arc::clone(&actor), generation, exit_rx);
    match probe_plans(sup, id).await {
        Some((readiness, health)) => {
            // The first success ends the wait nobody holds here.
            drop(spawn_probes(
                sup.clone(),
                actor,
                generation,
                readiness,
                health,
                probes_stop_rx,
            ));
        }
        None => {
            tracing::warn!(instance = %id, "the probes of an adopted process could not be planned");
        }
    }
    true
}

/// Builds the readiness and health plans of the instance's pinned package
/// at the ports stored for it.
async fn probe_plans(sup: &Supervisor, id: InstanceId) -> Option<(ProbePlan, HealthPlan)> {
    let prepared = prepare(sup, id, Checks::Plan).await.ok()?;
    let context = prepared.context(&BTreeMap::new());
    let plan = prepared.package.run_plan(&context).ok()?;
    Some((plan.readiness, plan.health))
}

/// Records that the identity of the process of `id` can no longer be
/// proven: the instance becomes `unknown` with the blocker
/// `identityUnproven`, and nothing is signalled.
pub(crate) async fn lose_identity(sup: &Supervisor, id: InstanceId, identity: &ProcessIdentity) {
    let deps = sup.deps();
    let now = deps.platform.clock().now();
    let committed = commit_instance(sup, id, None, CommitMode::Actor, |changes| {
        apply_event(changes, LifecycleEvent::IdentityLost, now)?;
        Ok(Vec::new())
    })
    .await;
    if let Err(error) = committed {
        tracing::warn!(instance = %id, %error, "the loss of an identity could not be recorded");
    }
    let blocker = InstanceBlocker::IdentityUnproven {
        evidence: IdentityEvidence::new().with_pid(identity.pid),
        observed_at: now,
    };
    sup.actor(id)
        .update(deps, |state| state.set_supervisor_blocker(Some(blocker)));
}
