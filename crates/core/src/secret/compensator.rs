//! The `secret_ref` compensator (spec 0007, *Kompensasi*): it removes a
//! secret reference an operation created, through the delete use case of
//! spec 0011.

use std::sync::Arc;

use stackly_domain::{CompensatorName, CreatedItem, CreatedRecord, ReasonCode};

use super::error::{ConflictReason, SecretError};
use super::service::{Removed, SecretCaller, SecretService};
use crate::operation::{
    BoxFuture, CompensationError, CompensationResult, CompensationScope, Compensator,
};

/// The name the engine looks a [`CreatedItem::SecretRef`] compensator up
/// by. `stacklyd` registers a [`SecretRefCompensator`] under it.
pub const SECRET_REF_COMPENSATOR: CompensatorName = CompensatorName::from_static("secret_ref");

const OWNED_BY_ANOTHER: ReasonCode = ReasonCode::from_static("owned_by_another_resource");
const NOT_A_SECRET_REF: ReasonCode = ReasonCode::from_static("not_a_secret_ref");
const REFERENCE_IN_USE: ReasonCode = ReasonCode::from_static("reference_in_use");
const DELETE_FAILED: ReasonCode = ReasonCode::from_static("delete_failed");
const TASK_STOPPED: ReasonCode = ReasonCode::from_static("task_stopped");

/// Undoes a [`CreatedItem::SecretRef`] an operation recorded with
/// `record_secret_intent`, which only takes a reference derived from an
/// instance the operation made, after a probe found no item.
///
/// A registered row is deleted through [`SecretService::delete`], without
/// a dialog: a row owned by another resource is not this operation's and
/// stays, and a row that is still used fails the compensation. Without a
/// row, a store item that is there is deleted, since only this operation
/// could have made it; an item that is gone already counts as undone.
#[derive(Debug, Clone)]
pub struct SecretRefCompensator {
    service: Arc<SecretService>,
}

impl SecretRefCompensator {
    /// A compensator that deletes through `service`.
    #[must_use]
    pub const fn new(service: Arc<SecretService>) -> Self {
        Self { service }
    }
}

impl Compensator for SecretRefCompensator {
    fn compensate(
        &self,
        scope: CompensationScope,
        record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        let service = Arc::clone(&self.service);
        let item = record.item().clone();
        Box::pin(async move {
            let CreatedItem::SecretRef(reference) = item else {
                return Ok(CompensationResult::NotOwned {
                    reason: NOT_A_SECRET_REF,
                });
            };
            let caller = SecretCaller::background().in_operation(scope.operation);
            let removed =
                tokio::task::spawn_blocking(move || service.remove_created(&reference, &caller))
                    .await
                    .map_err(|_| {
                        CompensationError::new(TASK_STOPPED, "the secret removal task stopped")
                    })?;
            match removed {
                Ok(Removed::Removed) => Ok(CompensationResult::Compensated),
                Ok(Removed::NotOwned) => Ok(CompensationResult::NotOwned {
                    reason: OWNED_BY_ANOTHER,
                }),
                Err(
                    error @ SecretError::Conflict {
                        reason: ConflictReason::InUse(_),
                        ..
                    },
                ) => Err(CompensationError::new(REFERENCE_IN_USE, error.to_string())),
                Err(error) => Err(CompensationError::new(DELETE_FAILED, error.to_string())),
            }
        })
    }
}
