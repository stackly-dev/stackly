//! Reveal and delete (spec 0011, *Reveal dan copy* and *Hapus*, AC-9,
//! AC-10): the one way a value leaves the daemon, and the removal of a
//! reference, each with an audit row whether it succeeds or fails.

use stackly_domain::{ResourceRevision, SecretOwner, SecretRef, WriteBatch, WriteOp};
use stackly_platform::{SecretStoreError, SecretValue};

use super::audit::{DELETE, REVEAL};
use super::{ChangeStatus, SecretCaller, SecretChange, SecretService};
use crate::secret::error::SecretError;

/// Why a client reveals a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RevealPurpose {
    /// To show it.
    Show,
    /// To copy it.
    Copy,
}

impl RevealPurpose {
    /// Returns the stable code, `show` or `copy`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Copy => "copy",
        }
    }
}

/// What [`SecretService::remove_created`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Removed {
    /// The reference and its item are gone, or were already.
    Removed,
    /// The row belongs to another resource, so it stays.
    NotOwned,
}

impl SecretService {
    /// Removes a reference an operation created, for the `secret_ref`
    /// compensator: a row owned by the instance its reference names goes
    /// through [`delete`](Self::delete); a row of another owner stays; and
    /// without a row, the store item goes, since only the operation that
    /// recorded the intent could have made it.
    pub(crate) fn remove_created(
        &self,
        reference: &SecretRef,
        caller: &SecretCaller,
    ) -> Result<Removed, SecretError> {
        let Some(view) = Self::read(self.registry().secret_reference(reference))? else {
            let guard = self.locks.lock(reference);
            let deleted = self.store().delete(reference, caller.interaction);
            drop(guard);
            return match deleted {
                Ok(()) | Err(SecretStoreError::NotFound) => Ok(Removed::Removed),
                Err(error) => Err(SecretError::Store {
                    reference: reference.clone(),
                    error,
                }),
            };
        };
        let owned = match view.record.owner {
            SecretOwner::Instance(id) => {
                reference.is_instance_reference()
                    && reference.segments().nth(1) == Some(id.to_string().as_str())
            }
            SecretOwner::Environment(_) | SecretOwner::User => false,
        };
        if !owned {
            return Ok(Removed::NotOwned);
        }
        match self.delete(reference, Some(view.record.revision), caller) {
            Ok(()) | Err(SecretError::NotFound { .. }) => Ok(Removed::Removed),
            Err(error) => Err(error),
        }
    }

    /// Reads the value behind a registered reference for a client: the
    /// only way a value leaves the daemon (AC-9). The value is learned by
    /// the redactor, and one audit row records the actor, the client, the
    /// reference, the purpose, and the result. When that row cannot be
    /// written, the reveal fails and the value is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::NotFound`] for an unregistered reference or
    /// a missing value, [`SecretError::Store`] for any other store
    /// failure, and [`SecretError::Registry`] when the audit row cannot be
    /// written.
    pub fn reveal(
        &self,
        reference: &SecretRef,
        purpose: RevealPurpose,
        caller: &SecretCaller,
    ) -> Result<SecretValue, SecretError> {
        let purpose = Some(purpose.as_code());
        let fail = |error: SecretError| {
            self.audit_failure(REVEAL, caller, reference, purpose, &error);
            Err(error)
        };
        let view = match Self::read(self.registry().secret_reference(reference)) {
            Ok(Some(view)) => view,
            Ok(None) => {
                return fail(SecretError::NotFound {
                    reference: reference.clone(),
                });
            }
            Err(error) => return fail(error),
        };
        let value = match self.store().read(reference, caller.interaction) {
            Ok(value) => value,
            Err(SecretStoreError::NotFound) => {
                return fail(SecretError::NotFound {
                    reference: reference.clone(),
                });
            }
            Err(error) => {
                return fail(SecretError::Store {
                    reference: reference.clone(),
                    error,
                });
            }
        };
        self.deps.redactor.learn(&value);
        let audit = self.audit_row(
            REVEAL,
            caller,
            reference,
            Some(view.record.id),
            purpose,
            None,
        );
        self.registry()
            .commit(WriteBatch::new(vec![WriteOp::InsertAuditEvent(audit)]))
            .map_err(|failed| SecretError::from(failed.error))?;
        Ok(value)
    }

    /// Deletes a reference and the value behind it (AC-10): first the row,
    /// in a commit that fails while any resource uses the reference, then
    /// the store item, where an item that is already gone counts as
    /// deleted. When the store fails, the row comes back exactly as it was
    /// with its revision up by 1, and the store error is returned. Every
    /// delete writes one audit row.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::NotFound`] for an unregistered reference,
    /// [`SecretError::RevisionMismatch`], [`SecretError::Conflict`] with
    /// every use while the reference is used, [`SecretError::Store`] when
    /// the store item could not be deleted, and [`SecretError::Registry`].
    pub fn delete(
        &self,
        reference: &SecretRef,
        expected_revision: Option<ResourceRevision>,
        caller: &SecretCaller,
    ) -> Result<(), SecretError> {
        let guard = self.locks.lock(reference);
        let (result, change) = self.delete_held(reference, expected_revision, caller);
        drop(guard);
        if let Some(change) = change {
            self.report(&change);
        }
        result
    }

    /// The delete under the lock, and the change it made: the deleted row,
    /// or the restored one after a store failure.
    fn delete_held(
        &self,
        reference: &SecretRef,
        expected: Option<ResourceRevision>,
        caller: &SecretCaller,
    ) -> (Result<(), SecretError>, Option<SecretChange>) {
        let fail = |error: SecretError| {
            self.audit_failure(DELETE, caller, reference, None, &error);
            (Err(error), None)
        };
        let view = match Self::read(self.registry().secret_reference(reference)) {
            Ok(Some(view)) => view,
            Ok(None) => {
                return fail(SecretError::NotFound {
                    reference: reference.clone(),
                });
            }
            Err(error) => return fail(error),
        };
        let stored = view.record.revision;
        if let Some(expected) = expected
            && expected != stored
        {
            return fail(SecretError::RevisionMismatch {
                reference: reference.clone(),
                expected,
                actual: stored,
            });
        }
        let deleted =
            self.registry()
                .commit(WriteBatch::new(vec![WriteOp::DeleteSecretReference {
                    reference: reference.clone(),
                    expected: stored,
                }]));
        if let Err(failed) = deleted {
            return fail(SecretError::from(failed.error));
        }
        match self.store().delete(reference, caller.interaction) {
            Ok(()) | Err(SecretStoreError::NotFound) => {}
            Err(store) => {
                let error = SecretError::Store {
                    reference: reference.clone(),
                    error: store,
                };
                let mut restored = view.record.clone();
                restored.revision = stored.next();
                let audit = self.audit_row(
                    DELETE,
                    caller,
                    reference,
                    Some(restored.id),
                    None,
                    Some(&error),
                );
                let committed = self.registry().commit(WriteBatch::new(vec![
                    WriteOp::RestoreSecretReference(restored.clone()),
                    WriteOp::InsertAuditEvent(audit),
                ]));
                return match committed {
                    Ok(_) => {
                        let mut view = view;
                        view.record = restored;
                        (Err(error), Some(Self::change_of(&view)))
                    }
                    Err(failed) => {
                        tracing::error!(
                            %reference,
                            error = %failed,
                            "a reference row was not restored after its store item could not be \
                             deleted"
                        );
                        (Err(error), None)
                    }
                };
            }
        }
        let audit = self.audit_row(DELETE, caller, reference, None, None, None);
        if let Err(failed) = self
            .registry()
            .commit(WriteBatch::new(vec![WriteOp::InsertAuditEvent(audit)]))
        {
            tracing::error!(
                %reference,
                error = %failed,
                "the audit row of a secret delete was not written"
            );
        }
        let change = SecretChange {
            reference: reference.clone(),
            status: ChangeStatus::Deleted,
            status_observed_at: view.record.status_observed_at,
            revision: stored,
        };
        (Ok(()), Some(change))
    }
}
