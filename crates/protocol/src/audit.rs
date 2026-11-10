//! The local audit trail (spec 0035): `audit.list` pages the rows the
//! daemon wrote for every operation that ended, went to `needs_recovery`,
//! or was cancelled, resumed, or abandoned, and for every secret action
//! (spec 0011). A row never holds a secret value.
//!
//! The trail belongs to the user who runs Stackly: the same user can delete
//! the registry file, so it is a record, not forensic evidence. A row's
//! client kind is what the client said in its handshake.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::handshake::ClientKind;
use crate::json::{FieldCode, Validate};
use crate::operation::{OperationCursor, OperationKind};
use crate::wire::{Count, ResourceRef, Timestamp};

/// Rows on a page when `limit` is absent.
pub const AUDIT_LIST_LIMIT_DEFAULT: u32 = 50;
/// The smallest `limit`.
pub const AUDIT_LIST_LIMIT_MIN: u32 = 1;
/// The largest `limit`.
pub const AUDIT_LIST_LIMIT_MAX: u32 = 200;
/// The longest `before` cursor, in bytes.
pub const AUDIT_CURSOR_MAX: usize = 80;
/// The most JSON bytes of rows one page carries; the daemon stops a page
/// before it, with `next` at the last row it holds.
pub const AUDIT_PAGE_BYTES_MAX: usize = 900 * 1024;

/// Who did what a row records. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditActorKind {
    /// A person, through a client.
    User,
    /// Stackly itself, on its own.
    Daemon,
    /// An actor this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How what a row records ended. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditResultKind {
    /// It succeeded.
    Succeeded,
    /// It failed; the `errorCode` detail names why.
    Failed,
    /// The operation was cancelled.
    Cancelled,
    /// The operation needs recovery.
    NeedsRecovery,
    /// A result this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One detail of a row: a camelCase key and its redacted value.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditDetail {
    /// The key, such as `errorCode` or `ports`.
    pub key: String,
    /// The value, at most 512 bytes; never a secret value.
    pub value: String,
}

/// One audit row.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventInfo {
    /// The row's ID.
    pub id: Uuid,
    /// When it happened.
    pub occurred_at: Timestamp,
    /// Who acted.
    pub actor: AuditActorKind,
    /// The kind of client that asked, from its handshake; absent for the
    /// daemon's own actions and for rows written before Stackly kept it.
    #[serde(default)]
    pub client: Option<ClientKind>,
    /// What happened, such as `operation.finished` or `secret.reveal`.
    pub action: String,
    /// The kind of operation, on the `operation.*` actions.
    #[serde(default)]
    pub operation_kind: Option<OperationKind>,
    /// The operation it belongs to: the correlation ID that also names it in
    /// an error and in the logs.
    #[serde(default)]
    pub operation_id: Option<Uuid>,
    /// What it was about: an instance, a runtime, an environment, or a
    /// secret reference.
    #[serde(default)]
    pub resource: Option<ResourceRef>,
    /// The instance it is about, also once the instance is removed.
    #[serde(default)]
    pub instance_id: Option<Uuid>,
    /// The number of the instance's config revision it is about.
    #[serde(default)]
    pub config_revision: Option<Count>,
    /// How it ended.
    pub result: AuditResultKind,
    /// The details, by key.
    #[serde(default)]
    pub details: Vec<AuditDetail>,
}

/// Params of `audit.list`. The filters apply together.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAuditParams {
    /// Only rows about this instance, removed ones included.
    #[serde(default)]
    pub instance_id: Option<Uuid>,
    /// Only rows of this operation.
    #[serde(default)]
    pub operation_id: Option<Uuid>,
    /// Most rows on the page, 1 to 200; absent means 50.
    #[serde(default)]
    pub limit: Option<u32>,
    /// The `next` cursor of the previous page, sent back unchanged.
    #[serde(default)]
    pub before: Option<String>,
}

impl ListAuditParams {
    /// The page size: `limit`, or [`AUDIT_LIST_LIMIT_DEFAULT`].
    #[must_use]
    pub fn limit_or_default(&self) -> u32 {
        self.limit.unwrap_or(AUDIT_LIST_LIMIT_DEFAULT)
    }
}

impl Validate for ListAuditParams {
    /// `limit` is 1 to 200, and `before` is a cursor the daemon wrote:
    /// `<occurred_at>_<id>`, the same text as an operation cursor.
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if self
            .limit
            .is_some_and(|limit| !(AUDIT_LIST_LIMIT_MIN..=AUDIT_LIST_LIMIT_MAX).contains(&limit))
        {
            fields.push(FieldError::new("limit", FieldCode::OutOfRange));
        }
        if let Some(before) = &self.before {
            if before.len() > AUDIT_CURSOR_MAX {
                fields.push(FieldError::new("before", FieldCode::TooLong));
            } else if OperationCursor::parse(before).is_none() {
                fields.push(FieldError::new("before", FieldCode::InvalidFormat));
            }
        }
        fields
    }
}

/// Reply of `audit.list`: newest first, by time then ID.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    /// The rows on this page.
    #[serde(default)]
    pub items: Vec<AuditEventInfo>,
    /// The cursor of the next page, to send back as `before`; null on the
    /// last page.
    #[serde(default)]
    pub next: Option<String>,
}

/// How long the daemon keeps audit rows (spec 0035).
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditRetention {
    /// The days a row is kept: 7, 30, or 90.
    pub days: u32,
    /// The most rows kept, whatever their age.
    pub max_rows: Count,
}

/// The days `settings.update` accepts as `auditRetentionDays`.
pub const AUDIT_RETENTION_DAYS: [u32; 3] = [7, 30, 90];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_page_fits_one_control_frame() {
        const { assert!(AUDIT_PAGE_BYTES_MAX < crate::limits::CONTROL_FRAME_MAX as usize) };
    }

    #[test]
    fn the_limit_is_1_to_200_and_the_cursor_is_one_the_daemon_wrote() {
        for limit in [None, Some(1), Some(50), Some(AUDIT_LIST_LIMIT_MAX)] {
            let params = ListAuditParams {
                limit,
                ..ListAuditParams::default()
            };
            assert!(params.validate().is_empty(), "{limit:?}");
        }
        for limit in [0, AUDIT_LIST_LIMIT_MAX + 1] {
            let params = ListAuditParams {
                limit: Some(limit),
                ..ListAuditParams::default()
            };
            assert_eq!(params.validate()[0].path(), "limit");
        }
        let cursor = OperationCursor::new(
            "2026-11-10T08:00:00Z".parse().unwrap(),
            Uuid::parse_str("0199f000-0000-7000-8000-0000000000a0").unwrap(),
        )
        .to_string();
        let good = ListAuditParams {
            before: Some(cursor),
            ..ListAuditParams::default()
        };
        assert!(good.validate().is_empty());
        for bad in ["nope".to_owned(), "x".repeat(AUDIT_CURSOR_MAX + 1)] {
            let params = ListAuditParams {
                before: Some(bad),
                ..ListAuditParams::default()
            };
            assert_eq!(params.validate()[0].path(), "before");
        }
    }

    #[test]
    fn an_unknown_actor_or_result_reads_as_unrecognized() {
        assert_eq!(
            serde_json::from_str::<AuditActorKind>("\"robot\"").unwrap(),
            AuditActorKind::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<AuditResultKind>("\"needs_recovery\"").unwrap(),
            AuditResultKind::NeedsRecovery
        );
        assert_eq!(
            serde_json::from_str::<AuditResultKind>("\"skipped\"").unwrap(),
            AuditResultKind::Unrecognized
        );
    }
}
