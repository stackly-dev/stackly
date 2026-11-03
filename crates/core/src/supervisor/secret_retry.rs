//! The wait for a locked Keychain (spec 0031, AC-15): a system start that
//! failed because the store was locked or busy leaves the instance `stopped`
//! with desired `running` and a `needsSecret` blocker. Every
//! [`LOGIN_SECRET_RETRY_INTERVAL`](super::LOGIN_SECRET_RETRY_INTERVAL) the
//! supervisor reads the secrets of such an instance again, with no dialog and
//! no operation. When they read, it begins `start_instance` with the trigger
//! `secret_retry`. The wait ends when desired becomes `stopped`, when the
//! start works, or when the reason becomes `missing` or `access_denied`.

use stackly_domain::{
    DesiredState, InstanceBlocker, InstanceId, InstanceLifecycle, NeedsSecretReason, OperationKind,
};

use super::Supervisor;
use super::actor::InstanceActor;
use super::error::SupervisorError;
use super::pending::{SECRET_RETRY_INPUT, begin, wait_final};
use super::persist::read_instance_actor;
use super::secrets::resolve;
use super::startup::StartRequest;
use crate::secret::{SecretCaller, SecretError};

/// Starts the retry task, once per supervisor. It ends at shutdown.
pub(crate) fn spawn_secret_retry(sup: &Supervisor) {
    if sup
        .shared
        .secret_retry_running
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    // Weak, because the task sleeps past the daemon's stop and a strong
    // handle would keep the registry open meanwhile.
    let interval = sup.deps().config.login_secret_retry_interval;
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
                if waits_for_the_store(&actor) && claim(&sup, actor.id) {
                    let sup = sup.clone();
                    tokio::spawn(async move {
                        retry_one(&sup, &actor).await;
                        release(&sup, actor.id);
                    });
                }
            }
        }
    });
}

/// Whether the blocker of `actor` says the store was locked or busy, and
/// nothing else is wrong with the secrets: a `missing` or `access_denied`
/// reference ends the wait because only the person can fix it.
fn waits_for_the_store(actor: &InstanceActor) -> bool {
    actor.read(|state| match &state.blocker {
        Some(InstanceBlocker::NeedsSecret { references, .. }) => {
            !references.is_empty()
                && references.iter().all(|item| {
                    matches!(
                        item.reason,
                        NeedsSecretReason::StoreLocked | NeedsSecretReason::StoreBusy
                    )
                })
        }
        _ => false,
    })
}

fn claim(sup: &Supervisor, id: InstanceId) -> bool {
    sup.shared
        .secret_retrying
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id)
}

fn release(sup: &Supervisor, id: InstanceId) {
    sup.shared
        .secret_retrying
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
}

/// One look at one waiting instance.
async fn retry_one(sup: &Supervisor, actor: &InstanceActor) {
    let deps = sup.deps();
    let id = actor.id;
    let Ok(instance) = read_instance_actor(sup, id).await else {
        return;
    };
    if instance.desired() != DesiredState::Running {
        // A stop ended the wait: the blocker told what the next start would
        // meet, and there is no next start.
        actor.update(deps, |state| {
            if matches!(state.blocker, Some(InstanceBlocker::NeedsSecret { .. })) {
                state.blocker = None;
            }
        });
        return;
    }
    if instance.lifecycle() != InstanceLifecycle::Stopped {
        return;
    }
    // No dialog, and the read refreshes the blocker: it goes when the
    // secrets read, and it takes the new reason when they do not.
    match resolve(sup, id, &SecretCaller::background()).await {
        Ok(secrets) => drop(secrets),
        Err(SupervisorError::Secret(SecretError::NeedsSecret { .. })) => return,
        Err(error) => {
            tracing::debug!(instance = %id, %error, "the secrets could not be read again");
            return;
        }
    }
    let make = || sup.start_work(StartRequest::background(id));
    if let Some(operation) = begin(
        sup,
        id,
        OperationKind::StartInstance,
        SECRET_RETRY_INPUT,
        &make,
        None,
    )
    .await
    {
        wait_final(sup, operation).await;
    }
}
