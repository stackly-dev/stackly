//! The pause before a respawn that no operation holds (spec 0010,
//! *Restart policy*, AC-7).
//!
//! A process that dies on its own while the instance runs is handled by the
//! exit task of the actor: it records `crashed { will_retry: true }` (the
//! lifecycle becomes `recovering`) and starts the timer here. While the
//! timer waits there is no operation and no resource lock, so
//! `instance.stop` runs at once and cancels the timer. When it ends, the
//! actor begins a system `start_instance` operation (trigger
//! `restart_policy`, no dialog) that respawns from `recovering` without
//! `start_begin`. A pause inside that operation, between two attempts that
//! crash before they are ready, belongs to the operation, like the pauses of
//! a user start.

use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, JournalPayload, OperationId, OperationKind,
    OperationResource,
};
use tokio::sync::watch;
use tokio::time::Instant;

use super::actor::{ActorState, InstanceActor, restart_status};
use super::error::SupervisorError;
use super::persist::read_instance;
use super::startup::StartRequest;
use super::{Supervisor, SupervisorDeps};
use crate::operation::{BeginError, BeginRequest, BusyPolicy, Origin};

/// The journal input of a respawn, naming what started the operation.
const TRIGGER_INPUT: &str = r#"{"trigger":"restart_policy"}"#;

/// Shows when the next respawn happens, in the `restart` status.
pub(crate) fn show_pause(deps: &SupervisorDeps, state: &mut ActorState, delay: Duration) {
    let next_at = jiff::SignedDuration::try_from(delay)
        .ok()
        .and_then(|pause| deps.platform.clock().now().checked_add(pause).ok());
    let attempts = state
        .restart
        .crashes_in_window(Instant::now(), &deps.config);
    state.restart_status = restart_status(&deps.config, attempts, next_at, false);
}

/// Starts the pause before the respawn of `id`: after `delay` the actor
/// begins the respawn operation, unless a stop cancels the pause first or
/// the instance is no longer `recovering` with desired `running`.
pub(crate) fn schedule_respawn(sup: &Supervisor, id: InstanceId, delay: Duration) {
    let (actor, cancel_rx) = arm_pause(sup, id, delay);
    let sup = sup.clone();
    tokio::spawn(async move {
        pause_then_respawn(&sup, &actor, delay, cancel_rx).await;
    });
}

/// Registers the pause before a respawn with the actor, so a stop can cancel
/// it, and shows when it ends. Returns the actor and the receiver that
/// flips to `true` on a cancel.
pub(crate) fn arm_pause(
    sup: &Supervisor,
    id: InstanceId,
    delay: Duration,
) -> (Arc<InstanceActor>, watch::Receiver<bool>) {
    let deps = sup.deps();
    let actor = sup.actor(id);
    let (cancel, cancel_rx) = watch::channel(false);
    actor.update(deps, |state| {
        if let Some(earlier) = state.respawn.replace(cancel) {
            earlier.send_replace(true);
        }
        show_pause(deps, state, delay);
    });
    (actor, cancel_rx)
}

/// How one try to begin the respawn operation went.
enum Begun {
    /// The operation exists.
    Started(Option<OperationId>),
    /// The engine could not take it yet; try again after a pause.
    Later,
    /// Nothing will begin: the engine is gone or refused for good.
    GaveUp,
}

/// Waits out the pause, then begins the respawn operation and returns its
/// ID. Returns `None` when a stop cancelled the pause, the instance is no
/// longer `recovering` with desired `running`, or no operation could begin.
pub(crate) async fn pause_then_respawn(
    sup: &Supervisor,
    actor: &Arc<InstanceActor>,
    delay: Duration,
    cancel: watch::Receiver<bool>,
) -> Option<OperationId> {
    let mut degraded = false;
    let operation = pause_loop(sup, actor, delay, cancel, &mut degraded).await;
    if degraded {
        sup.deps().events.registry_degraded(false, "");
    }
    operation
}

async fn pause_loop(
    sup: &Supervisor,
    actor: &Arc<InstanceActor>,
    delay: Duration,
    mut cancel: watch::Receiver<bool>,
    degraded: &mut bool,
) -> Option<OperationId> {
    let deps = sup.deps();
    let id = actor.id;
    let mut pause = delay;
    loop {
        tokio::select! {
            () = tokio::time::sleep(pause) => {}
            _ = cancel.wait_for(|cancelled| *cancelled) => return None,
        }
        if *cancel.borrow() || sup.is_shutdown() {
            return None;
        }
        let wanted = match read_instance(deps, id).await {
            Ok(instance) => {
                if *degraded {
                    *degraded = false;
                    deps.events.registry_degraded(false, "");
                }
                instance.lifecycle() == InstanceLifecycle::Recovering
                    && instance.desired() == DesiredState::Running
            }
            // A failed read decides nothing (AC-22): the pause stays, the
            // subsystem reads `degraded`, and the read comes again.
            Err(SupervisorError::Registry(error)) => {
                tracing::warn!(instance = %id, %error, "the registry could not be read at the end of a respawn pause; trying again");
                *degraded = true;
                deps.events
                    .registry_degraded(true, "the registry cannot be read");
                pause = deps.config.restart_base_delay;
                continue;
            }
            Err(_) => false,
        };
        if !wanted {
            // A stop or a user action came first: nothing to respawn.
            clear_pause(deps, actor);
            return None;
        }
        match begin_respawn(sup, id).await {
            Begun::Started(operation) => {
                clear_pause(deps, actor);
                return operation;
            }
            Begun::GaveUp => {
                clear_pause(deps, actor);
                return None;
            }
            Begun::Later => pause = deps.config.restart_base_delay,
        }
    }
}

/// Ends the pause: no timer is pending and no time is shown.
fn clear_pause(deps: &SupervisorDeps, actor: &InstanceActor) {
    actor.update(deps, |state| {
        state.respawn = None;
        let attempts = state
            .restart
            .crashes_in_window(Instant::now(), &deps.config);
        let exhausted = state.restart_status.exhausted;
        state.restart_status = restart_status(&deps.config, attempts, None, exhausted);
    });
}

/// Begins the system `start_instance` operation of a respawn. It never
/// queues: when another operation holds the instance, the actor waits and
/// looks again, because that operation may be the stop that makes the
/// respawn unnecessary.
async fn begin_respawn(sup: &Supervisor, id: InstanceId) -> Begun {
    if sup.is_shutdown() {
        return Begun::GaveUp;
    }
    let Some(engine) = sup.operations() else {
        tracing::warn!(instance = %id, "no operation engine is attached, so the respawn does not run");
        return Begun::GaveUp;
    };
    let Ok(input) = JournalPayload::new(TRIGGER_INPUT, crate::operation::limits::INPUT_MAX) else {
        tracing::warn!(instance = %id, "the respawn input does not fit the journal");
        return Begun::GaveUp;
    };
    let request = BeginRequest {
        kind: OperationKind::StartInstance,
        resource: OperationResource::Instance(id),
        creates_resource: false,
        extra_locks: Vec::new(),
        busy: BusyPolicy::Reject,
        expected_revision: None,
        origin: Origin::Background,
        input,
        work: sup.start_work(StartRequest::respawn(id)),
        precheck: None,
    };
    match engine.begin(request).await {
        Ok(outcome) => Begun::Started(outcome.view().map(|view| view.operation.id())),
        Err(BeginError::Busy { .. } | BeginError::NotReady | BeginError::QueueFull { .. }) => {
            Begun::Later
        }
        Err(error) => {
            tracing::warn!(instance = %id, %error, "the respawn operation could not begin");
            Begun::GaveUp
        }
    }
}
