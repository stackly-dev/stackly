//! Reading the secrets an action needs (spec 0010 AC-18, spec 0011): once,
//! before the first side effect, with the interaction the trigger allows. A
//! secret that cannot be read becomes the instance's `needsSecret` blocker
//! and the action stops with `secret.needs_secret`, without any change of
//! the lifecycle.

use stackly_domain::InstanceId;

use super::Supervisor;
use super::error::SupervisorError;
use super::persist::blocking;
use crate::secret::{ResolvedSecrets, SecretCaller, SecretError};
use stackly_domain::InstanceBlocker;

/// Reads every secret the instance's active config revision uses, always
/// from the store.
///
/// # Errors
///
/// Returns [`SupervisorError::Secret`] with the service's error; for a
/// secret that cannot be read the blocker is set first.
pub(crate) async fn resolve(
    sup: &Supervisor,
    id: InstanceId,
    caller: &SecretCaller,
) -> Result<ResolvedSecrets, SupervisorError> {
    let deps = sup.deps();
    let service = deps.secrets.clone();
    let caller = caller.clone();
    let result = blocking(move || service.resolve_for_start(id, &caller)).await?;
    let actor = sup.actor(id);
    match result {
        Ok(resolved) => {
            actor.update(deps, |state| {
                if matches!(state.blocker, Some(InstanceBlocker::NeedsSecret { .. })) {
                    state.blocker = None;
                }
            });
            Ok(resolved)
        }
        Err(SecretError::NeedsSecret { needs }) => {
            let now = deps.platform.clock().now();
            let blocker = InstanceBlocker::needs_secret(needs.references.clone(), now);
            actor.update(deps, |state| state.offer_blocker(blocker));
            Err(SupervisorError::Secret(SecretError::NeedsSecret { needs }))
        }
        Err(other) => Err(SupervisorError::Secret(other)),
    }
}
