//! The compensator of the registry row a create inserts (spec 0020,
//! *Operasi `create_instance`*): `instance_registration`.

use std::sync::Arc;

use stackly_domain::{
    CompensatorName, CreatedItem, CreatedRecord, CustomItemKey, InstanceId, ReasonCode,
    RegistryError, ResourceRevision, WriteBatch, WriteOp,
};

use crate::operation::{
    BoxFuture, CompensationError, CompensationResult, CompensationScope, Compensator,
    check_instance_lock,
};

/// The name `instance_registration` is registered under.
pub const INSTANCE_REGISTRATION: CompensatorName =
    CompensatorName::from_static("instance_registration");

const CREATED_BY_OTHER_OPERATION: ReasonCode =
    ReasonCode::from_static("created_by_other_operation");
const INSTANCE_ACTIVE: ReasonCode = ReasonCode::from_static("instance_active");
const REMOVE_FAILED: ReasonCode = ReasonCode::from_static("remove_failed");
const BAD_KEY: ReasonCode = ReasonCode::from_static("unknown_item");
const TASK_STOPPED: ReasonCode = ReasonCode::from_static("task_stopped");
/// How often the compensator reads the instance again after a revision
/// conflict.
const ATTEMPTS: usize = 3;

/// Tells the daemon that a registry row of a failed create is gone, so it
/// can send `instance.removed` (spec 0020; the daemon implements it).
pub trait InstanceRemoved: Send + Sync {
    /// The instance row was deleted; `revision` is its last revision plus
    /// 1, the revision the event carries.
    fn removed(&self, instance: InstanceId, revision: ResourceRevision);
}

/// An [`InstanceRemoved`] that tells nobody, for tests that do not look at
/// events.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoInstanceRemoved;

impl InstanceRemoved for NoInstanceRemoved {
    fn removed(&self, _instance: InstanceId, _revision: ResourceRevision) {}
}

/// Undoes `InsertInstance` of a create: deletes the row and what hangs on
/// it with `DeleteInstanceCreatedBy`, but only while no process holds the
/// instance lock (K10), then reports `instance.removed`.
///
/// It proves ownership with `created_by` alone: compensation runs only
/// while the operation that created the row holds the lock key of that
/// instance, so no start can have happened in between.
pub struct InstanceRegistrationCompensator {
    removed: Arc<dyn InstanceRemoved>,
}

impl InstanceRegistrationCompensator {
    /// A compensator that reports every deleted row to `removed`.
    #[must_use]
    pub fn new(removed: Arc<dyn InstanceRemoved>) -> Self {
        Self { removed }
    }
}

/// The key of the custom item a create records for the instance `id`.
///
/// # Panics
///
/// Never: the hyphenated UUID is printable ASCII.
#[must_use]
pub fn registration_key(id: InstanceId) -> CustomItemKey {
    id.to_string()
        .parse()
        .expect("an instance ID is a valid custom item key")
}

impl Compensator for InstanceRegistrationCompensator {
    fn compensate(
        &self,
        scope: CompensationScope,
        record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        let removed = Arc::clone(&self.removed);
        let id = match record.item() {
            CreatedItem::Custom { key, .. } => key.as_str().parse::<InstanceId>().ok(),
            _ => None,
        };
        Box::pin(async move {
            let id = id.ok_or_else(|| {
                CompensationError::new(BAD_KEY, "the item does not name an instance")
            })?;
            tokio::task::spawn_blocking(move || delete(&scope, id, &*removed))
                .await
                .map_err(|_| CompensationError::new(TASK_STOPPED, "the removal task stopped"))?
        })
    }
}

fn delete(
    scope: &CompensationScope,
    id: InstanceId,
    removed: &dyn InstanceRemoved,
) -> Result<CompensationResult, CompensationError> {
    let registry_failed =
        |error: RegistryError| CompensationError::new(REMOVE_FAILED, error.to_string());
    for attempt in 0..ATTEMPTS {
        let Some(instance) = scope.registry.instance(id).map_err(registry_failed)?.value else {
            return Ok(CompensationResult::Compensated);
        };
        if instance.created_by() != scope.operation {
            return Ok(CompensationResult::NotOwned {
                reason: CREATED_BY_OTHER_OPERATION,
            });
        }
        check_instance_lock(
            &scope.platform,
            &scope.layout.layout().instance_paths(id).lock,
        )?;
        let revision = instance.revision();
        let op = WriteOp::DeleteInstanceCreatedBy {
            id,
            expected: revision,
            operation: scope.operation,
        };
        match scope.registry.commit(WriteBatch::new(vec![op])) {
            Ok(_) => {
                removed.removed(id, revision.next());
                return Ok(CompensationResult::Compensated);
            }
            // The row changed between the read and the delete: read it again.
            Err(error)
                if matches!(error.error, RegistryError::RevisionConflict { .. })
                    && attempt + 1 < ATTEMPTS => {}
            Err(error) => {
                return Err(match error.error {
                    RegistryError::NotFound { .. } => return Ok(CompensationResult::Compensated),
                    RegistryError::NotDeletable { .. } => CompensationError::new(
                        INSTANCE_ACTIVE,
                        "the instance may be running, so its row stays",
                    ),
                    other => registry_failed(other),
                });
            }
        }
    }
    Err(CompensationError::new(
        REMOVE_FAILED,
        "the instance kept changing while it was removed",
    ))
}
