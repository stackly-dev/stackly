//! The audit rows of the secret actions (spec 0011 *Value sourcing*): who,
//! which reference, and how it ended, never a value.

use stackly_domain::{
    AuditAction, AuditEventId, AuditEventRecord, AuditResource, AuditResult, DetailKey, InstanceId,
    SafeDetails, SecretRef, SecretReferenceId, WriteBatch, WriteOp,
};

use super::{SecretCaller, SecretService};
use crate::secret::error::SecretError;

/// Storing a value through `put`.
pub(super) const SET: AuditAction = AuditAction::from_static("secret.set");
/// Revealing a value.
pub(super) const REVEAL: AuditAction = AuditAction::from_static("secret.reveal");
/// Deleting a reference.
pub(super) const DELETE: AuditAction = AuditAction::from_static("secret.delete");

/// The one action specific entry an audit row may carry beside the
/// common ones.
#[derive(Clone, Copy, Debug)]
pub(super) enum AuditDetail<'a> {
    /// The reveal purpose, `show` or `copy`, as `purpose`.
    Purpose(&'a str),
    /// The source reference of a `copyFrom` put, as `copiedFrom`: the
    /// reference whose value was read, never the value (audit note 112).
    CopiedFrom(&'a SecretRef),
}

/// The instance that owns `reference`: only the daemon names a reference
/// `local:instance/<id>/<field>`, for a secret field of that instance, so
/// the owner column holds the same instance (spec 0011).
fn instance_of(reference: &SecretRef) -> Option<InstanceId> {
    if !reference.is_instance_reference() {
        return None;
    }
    reference.segments().nth(1)?.parse().ok()
}

fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("the audit detail keys of spec 0011 are camelCase constants")
}

impl SecretService {
    /// One audit row about `reference`. `resource` is the row's ID while
    /// the row exists; `detail` is the reveal purpose or the copy source;
    /// `error` makes the result `failed` with its code as `errorCode`.
    /// Every value goes through the redactor first.
    pub(super) fn audit_row(
        &self,
        action: AuditAction,
        caller: &SecretCaller,
        reference: &SecretRef,
        resource: Option<SecretReferenceId>,
        detail: Option<AuditDetail<'_>>,
        error: Option<&SecretError>,
    ) -> AuditEventRecord {
        let mut entries = vec![(key("reference"), reference.to_string())];
        match detail {
            Some(AuditDetail::Purpose(purpose)) => {
                entries.push((key("purpose"), purpose.to_owned()));
            }
            Some(AuditDetail::CopiedFrom(source)) => {
                entries.push((key("copiedFrom"), source.to_string()));
            }
            None => {}
        }
        if let Some(client) = &caller.client {
            entries.push((key("clientKind"), client.to_string()));
        }
        if let Some(uid) = caller.peer_uid {
            entries.push((key("peerUid"), uid.to_string()));
        }
        if let Some(error) = error {
            entries.push((key("errorCode"), error.code().to_string()));
        }
        let redacted = entries.into_iter().map(|(key, value)| {
            let value = self.deps.redactor.redact(&value).into_owned();
            (key, value)
        });
        let result = if error.is_some() {
            AuditResult::Failed
        } else {
            AuditResult::Succeeded
        };
        let mut record = AuditEventRecord::new(
            AuditEventId::generate(),
            caller.actor,
            action,
            result,
            self.now(),
        );
        record.resource = resource.map(AuditResource::SecretReference);
        record.operation = caller.operation;
        // Spec 0035: the client kind and, for a reference an instance owns,
        // that instance, so the row shows in the instance's Activity.
        record.client = caller.client.clone();
        record.instance = instance_of(reference);
        // At most six short entries, so the limits of SafeDetails hold.
        record.safe_details = SafeDetails::new(redacted).unwrap_or_default();
        record
    }

    /// Writes the audit row of a failure in a commit of its own. A row that
    /// cannot be written is logged and does not hide the failure.
    pub(super) fn audit_failure(
        &self,
        action: AuditAction,
        caller: &SecretCaller,
        reference: &SecretRef,
        detail: Option<AuditDetail<'_>>,
        error: &SecretError,
    ) {
        let resource = self
            .registry()
            .secret_reference(reference)
            .ok()
            .and_then(|read| read.value)
            .map(|view| view.record.id);
        let row = self.audit_row(action, caller, reference, resource, detail, Some(error));
        if let Err(written) = self
            .registry()
            .commit(WriteBatch::new(vec![WriteOp::InsertAuditEvent(row)]))
        {
            tracing::error!(
                %reference,
                code = %error.code(),
                error = %written,
                "the audit row of a failed secret action was not written"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // covers: spec 0035 *Model data* (the secret row of an instance)
    #[test]
    fn an_instance_reference_names_its_instance_and_others_name_none() {
        let id = InstanceId::generate();
        let owned: SecretRef = format!("local:instance/{id}/admin-password")
            .parse()
            .unwrap();
        assert_eq!(instance_of(&owned), Some(id));
        for other in ["local:erp/postgres/admin", "local:instance/not-an-id/admin"] {
            let reference: SecretRef = other.parse().unwrap();
            assert_eq!(instance_of(&reference), None, "{other}");
        }
    }
}
