//! `audit.list` (spec 0035): one page of the audit rows the operation
//! engine and the secret service wrote, mapped to the wire.
//!
//! The registry reads the page on a blocking thread, a small page at a
//! time, so the one writer thread of spec 0005 is not held long. A page
//! stops before its JSON passes [`AUDIT_PAGE_BYTES_MAX`], with `next` at the
//! last row it holds. No value of a secret can reach a row: the registry
//! never stores one.

use stackly_domain::{
    AuditCursor, AuditEventId, AuditEventRecord, AuditQuery, AuditResource, InstanceId,
    OperationId, Registry,
};
use stackly_protocol::audit::{
    AUDIT_PAGE_BYTES_MAX, AuditDetail, AuditEventInfo, AuditPage, ListAuditParams,
};
use stackly_protocol::codes::{internal, registry, request};
use stackly_protocol::json::FieldCode;
use stackly_protocol::operation::OperationCursor;
use stackly_protocol::{Count, ErrorBody, FieldError, ResourceKind, ResourceRef, Timestamp};

use crate::dispatch::HandlerFuture;
use crate::ipc::operation::dto::{by_code, client_kind};
use crate::map;
use crate::state::DaemonCore;

/// The real `audit.list`.
pub fn list(core: &DaemonCore, params: ListAuditParams) -> HandlerFuture<'_, AuditPage> {
    Box::pin(async move {
        let query = query(&params)?;
        let registry = core.registry().ok_or_else(|| {
            Box::new(ErrorBody::new(
                &registry::UNAVAILABLE,
                "the registry is not available",
            ))
        })?;
        let page = tokio::task::spawn_blocking(move || registry.audit_events(query))
            .await
            .map_err(|_| unexpected("the audit read task failed"))?
            .map_err(|error| Box::new(map::registry_error(&error)))?
            .value;
        Ok(fit(&page.items, page.next))
    })
}

/// The registry query of `params`, already validated by the protocol: the
/// filters, the limit, and the cursor read back into the domain's.
fn query(params: &ListAuditParams) -> Result<AuditQuery, Box<ErrorBody>> {
    let invalid = |path: &str| {
        Box::new(
            ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
                .with_detail("method", "audit.list")
                .with_field(FieldError::new(path, FieldCode::InvalidFormat)),
        )
    };
    let limit = u16::try_from(params.limit_or_default()).map_err(|_| invalid("limit"))?;
    let before = match &params.before {
        None => None,
        Some(text) => {
            let cursor = OperationCursor::parse(text).ok_or_else(|| invalid("before"))?;
            Some(AuditCursor {
                occurred_at: cursor.created_at(),
                id: AuditEventId::from_uuid(cursor.id()).map_err(|_| invalid("before"))?,
            })
        }
    };
    // A UUID that is not version 7 names no row, so it matches nothing; the
    // filter keeps its meaning instead of being dropped.
    let instance = params
        .instance_id
        .map(|id| InstanceId::from_uuid(id).map_err(|_| invalid("instanceId")))
        .transpose()?;
    let operation = params
        .operation_id
        .map(|id| OperationId::from_uuid(id).map_err(|_| invalid("operationId")))
        .transpose()?;
    Ok(AuditQuery {
        instance,
        operation,
        ..AuditQuery::page(limit, before)
    })
}

/// The rows that fit one page, and the cursor after the last of them: the
/// registry's `next`, or the last row kept when the size limit cut the page.
fn fit(records: &[AuditEventRecord], next: Option<AuditCursor>) -> AuditPage {
    let mut items = Vec::with_capacity(records.len());
    let mut bytes = 0_usize;
    let mut next = next;
    for (at, record) in records.iter().enumerate() {
        let info = info(record);
        let size = serde_json::to_vec(&info)
            .map(|encoded| encoded.len())
            .unwrap_or(usize::MAX);
        if at > 0 && bytes.saturating_add(size) > AUDIT_PAGE_BYTES_MAX {
            next = Some(AuditCursor::of(&records[at - 1]));
            break;
        }
        bytes = bytes.saturating_add(size);
        items.push(info);
    }
    AuditPage {
        items,
        next: next.map(|cursor| {
            OperationCursor::new(cursor.occurred_at, cursor.id.as_uuid()).to_string()
        }),
    }
}

/// One row on the wire. A secret row names its reference from the
/// `reference` detail, the form a client knows.
fn info(record: &AuditEventRecord) -> AuditEventInfo {
    let resource = match record.resource {
        Some(AuditResource::Instance(id)) => Some(ResourceRef {
            kind: ResourceKind::Instance,
            id: id.to_string(),
        }),
        Some(AuditResource::Runtime(id)) => Some(ResourceRef {
            kind: ResourceKind::Runtime,
            id: id.to_string(),
        }),
        Some(AuditResource::Environment(id)) => Some(ResourceRef {
            kind: ResourceKind::Environment,
            id: id.to_string(),
        }),
        Some(AuditResource::SecretReference(_)) => {
            record
                .safe_details
                .get("reference")
                .map(|reference| ResourceRef {
                    kind: ResourceKind::Secret,
                    id: reference.to_owned(),
                })
        }
        _ => None,
    };
    AuditEventInfo {
        id: record.id.as_uuid(),
        occurred_at: Timestamp::from_jiff(record.occurred_at),
        actor: by_code(record.actor.as_code()),
        client: record
            .client
            .as_ref()
            .and_then(|client| client_kind(client.as_str())),
        action: record.action.to_string(),
        operation_kind: record.operation_kind.map(|kind| by_code(kind.as_code())),
        operation_id: record.operation.map(|id| id.as_uuid()),
        resource,
        instance_id: record.instance.map(|id| id.as_uuid()),
        config_revision: record
            .config_revision
            .map(|number| Count::saturating(number.get())),
        result: by_code(record.result.as_code()),
        details: record
            .safe_details
            .iter()
            .map(|(key, value)| AuditDetail {
                key: key.to_string(),
                value: value.to_owned(),
            })
            .collect(),
    }
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&internal::UNEXPECTED, message))
}

#[cfg(test)]
mod tests {
    use stackly_domain::{AuditAction, AuditActor, AuditResult, ConfigRevisionNumber, SafeDetails};
    use stackly_protocol::audit::{AuditActorKind, AuditResultKind};

    use super::*;

    fn record(n: i64, details: usize) -> AuditEventRecord {
        let at: jiff::Timestamp = "2026-11-10T08:00:00Z".parse().unwrap();
        let mut record = AuditEventRecord::new(
            AuditEventId::generate(),
            AuditActor::User,
            AuditAction::OPERATION_FINISHED,
            AuditResult::NeedsRecovery,
            at.checked_sub(jiff::SignedDuration::from_secs(n)).unwrap(),
        );
        let instance = InstanceId::generate();
        record.instance = Some(instance);
        record.resource = Some(AuditResource::Instance(instance));
        record.config_revision = Some(ConfigRevisionNumber::new(3).unwrap());
        record.client = Some("gui".parse().unwrap());
        record.operation = Some(OperationId::generate());
        record.operation_kind = Some(stackly_domain::OperationKind::StartInstance);
        record.safe_details = SafeDetails::new((0..details).map(|index| {
            (
                format!("k{index:02}").parse().unwrap(),
                "x".repeat(SafeDetails::VALUE_MAX),
            )
        }))
        .unwrap();
        record
    }

    #[test]
    fn a_row_maps_every_field_to_the_wire() {
        let row = record(0, 1);
        let info = info(&row);
        assert_eq!(info.actor, AuditActorKind::User);
        assert_eq!(info.result, AuditResultKind::NeedsRecovery);
        assert_eq!(
            info.client,
            Some(stackly_protocol::handshake::ClientKind::Gui)
        );
        assert_eq!(info.config_revision.map(Count::get), Some(3));
        assert_eq!(info.instance_id, row.instance.map(|id| id.as_uuid()));
        assert_eq!(info.action, "operation.finished");
        assert_eq!(
            info.resource.map(|resource| resource.kind),
            Some(ResourceKind::Instance)
        );
        assert_eq!(info.details.len(), 1);
    }

    // covers: AC-5 (spec 0035)
    #[test]
    fn a_page_stops_before_900_kib_with_next_at_its_last_row() {
        // About 10 KiB a row: 200 rows pass the limit.
        let rows: Vec<_> = (0..200).map(|n| record(n, 19)).collect();
        let page = fit(&rows, None);
        assert!(page.items.len() < rows.len());
        let bytes: usize = page
            .items
            .iter()
            .map(|item| serde_json::to_vec(item).unwrap().len())
            .sum();
        assert!(bytes <= AUDIT_PAGE_BYTES_MAX);
        let last = &rows[page.items.len() - 1];
        let expected = OperationCursor::new(last.occurred_at, last.id.as_uuid()).to_string();
        assert_eq!(page.next, Some(expected));

        let small = fit(&rows[..3], None);
        assert_eq!(small.items.len(), 3);
        assert_eq!(
            small.next, None,
            "a page that fits keeps the registry's next"
        );
    }

    #[test]
    fn a_cursor_that_names_no_version_7_row_is_invalid_on_before() {
        let params = ListAuditParams {
            before: Some(
                OperationCursor::new("2026-11-10T08:00:00Z".parse().unwrap(), uuid::Uuid::nil())
                    .to_string(),
            ),
            ..ListAuditParams::default()
        };
        let error = query(&params).unwrap_err();
        assert_eq!(error.code(), "request.invalid_params");
        assert_eq!(error.fields()[0].path(), "before");
    }
}
