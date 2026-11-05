//! The audit rows of the secret actions (spec 0011 *Value sourcing*): who,
//! which reference, and how it ended, never a value.

use stackly_domain::{
    AuditAction, AuditEventId, AuditEventRecord, AuditResource, AuditResult, DetailKey,
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
