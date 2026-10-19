//! The secret tables of spec 0011 and the audit table: `secret_references`,
//! `secret_reference_uses`, and `audit_events`, written by the secret and
//! audit write ops and read back into the domain's records (spec 0005,
//! "Tambahan fitur 11").
//!
//! The checks follow the order the `Registry` trait documents, the same as
//! the fake: existence, the revision, references, then the unique rule of
//! a delete (`ReferenceInUse`). The foreign key from a use to its
//! reference is `RESTRICT`, the last guard behind that check.

use std::collections::BTreeMap;

use rusqlite::{Connection, params, params_from_iter};
use serde_json::{Map, Value};
use stackly_domain::{
    AuditCursor, AuditEventRecord, AuditPage, AuditQuery, AuditResource, DetailKey,
    PlatformStoreKey, RegistryError, ResourceRef, ResourceRevision, SafeDetails, SecretFieldName,
    SecretOwner, SecretOwnerKind, SecretRef, SecretReferenceFilter, SecretReferenceRecord,
    SecretReferenceView, SecretStatus, SecretUse, SecretUser, WriteOp,
};

use super::column::{Cells, collect, exists, first, int, opt_ts, sql_error, ts};

const REFERENCE_COLUMNS: &str = "id, reference, platform_store_key, owner_kind, owner_id, \
     purpose, value_origin, status, status_observed_at, value_updated_at, revision, created_at, \
     updated_at";

const AUDIT_COLUMNS: &str = "id, actor, action, resource_kind, resource_id, operation_id, result, safe_details, occurred_at";

// Writes.

/// The stored ID and revision of the row with `reference`.
fn stored(
    conn: &Connection,
    reference: &SecretRef,
) -> Result<Option<(String, ResourceRevision)>, RegistryError> {
    first(
        conn,
        "SELECT id, revision FROM secret_references WHERE reference = ?1",
        [reference.as_str()],
        "secret_references",
        |cells| Ok((cells.text("id")?, cells.revision()?)),
    )
}

/// Existence and, when `expected` is given, the revision of the row with
/// `reference`. Returns its stored ID.
fn expect_stored(
    conn: &Connection,
    reference: &SecretRef,
    expected: Option<ResourceRevision>,
) -> Result<String, RegistryError> {
    let resource = || ResourceRef::SecretReference(reference.clone());
    let (id, actual) = stored(conn, reference)?.ok_or_else(|| RegistryError::NotFound {
        resource: resource(),
    })?;
    if let Some(expected) = expected
        && actual != expected
    {
        return Err(RegistryError::RevisionConflict {
            resource: resource(),
            expected,
            actual,
        });
    }
    Ok(id)
}

/// `InsertSecretReference` and `RestoreSecretReference`: a new ID and a new
/// reference, then the row.
pub(super) fn insert_reference(
    conn: &Connection,
    record: &SecretReferenceRecord,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    let id = record.id.to_string();
    if exists(
        conn,
        "SELECT 1 FROM secret_references WHERE id = ?1 OR reference = ?2",
        [id.as_str(), record.reference.as_str()],
    )? {
        return Err(RegistryError::DuplicateResource {
            resource: op.resource(),
        });
    }
    conn.execute(
        &format!(
            "INSERT INTO secret_references ({REFERENCE_COLUMNS}) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
        ),
        params![
            id,
            record.reference.as_str(),
            record.platform_store_key.as_str(),
            record.owner.kind().as_code(),
            record.owner.id().map(|owner| owner.to_string()),
            record.purpose.as_str(),
            record.value_origin.map(|origin| origin.as_code()),
            record.status.as_code(),
            opt_ts(record.status_observed_at),
            opt_ts(record.value_updated_at),
            int(record.revision.get(), "revision")?,
            ts(record.created_at),
            ts(record.updated_at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `UpdateSecretReference`: the row exists at `expected`; only the value
/// columns change.
pub(super) fn update_reference(
    conn: &Connection,
    record: &SecretReferenceRecord,
    expected: ResourceRevision,
) -> Result<(), RegistryError> {
    expect_stored(conn, &record.reference, Some(expected))?;
    conn.execute(
        "UPDATE secret_references SET platform_store_key = ?2, value_origin = ?3, status = ?4, \
         status_observed_at = ?5, value_updated_at = ?6, revision = ?7, updated_at = ?8 \
         WHERE reference = ?1",
        params![
            record.reference.as_str(),
            record.platform_store_key.as_str(),
            record.value_origin.map(|origin| origin.as_code()),
            record.status.as_code(),
            opt_ts(record.status_observed_at),
            opt_ts(record.value_updated_at),
            int(record.revision.get(), "revision")?,
            ts(record.updated_at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `ObserveSecretReference`: the row exists; three columns change.
pub(super) fn observe_reference(
    conn: &Connection,
    reference: &SecretRef,
    status: SecretStatus,
    status_observed_at: jiff::Timestamp,
    platform_store_key: &PlatformStoreKey,
) -> Result<(), RegistryError> {
    expect_stored(conn, reference, None)?;
    conn.execute(
        "UPDATE secret_references SET status = ?2, status_observed_at = ?3, \
         platform_store_key = ?4 WHERE reference = ?1",
        params![
            reference.as_str(),
            status.as_code(),
            ts(status_observed_at),
            platform_store_key.as_str(),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `DeleteSecretReference`: the row exists at `expected` and has no uses.
pub(super) fn delete_reference(
    conn: &Connection,
    reference: &SecretRef,
    expected: ResourceRevision,
) -> Result<(), RegistryError> {
    let id = expect_stored(conn, reference, Some(expected))?;
    let used_by = uses_of(conn, &id, reference)?;
    if !used_by.is_empty() {
        return Err(RegistryError::ReferenceInUse {
            reference: reference.clone(),
            used_by,
        });
    }
    conn.execute("DELETE FROM secret_references WHERE id = ?1", [id])
        .map_err(sql_error)?;
    Ok(())
}

/// `ReplaceSecretUses`: the user and every reference exist; then the
/// user's uses are written again whole.
pub(super) fn replace_uses(
    conn: &Connection,
    user: SecretUser,
    uses: &BTreeMap<SecretFieldName, SecretRef>,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    let (table, kind, user_id) = user_columns(user);
    if !exists(
        conn,
        &format!("SELECT 1 FROM {table} WHERE id = ?1"),
        [&user_id],
    )? {
        return Err(RegistryError::MissingReference {
            resource: op.resource(),
        });
    }
    let mut rows = Vec::with_capacity(uses.len());
    for (field, reference) in uses {
        let (id, _) = stored(conn, reference)?.ok_or_else(|| RegistryError::MissingReference {
            resource: ResourceRef::SecretReference(reference.clone()),
        })?;
        rows.push((id, field));
    }
    conn.execute(
        "DELETE FROM secret_reference_uses WHERE resource_kind = ?1 AND resource_id = ?2",
        [kind, user_id.as_str()],
    )
    .map_err(sql_error)?;
    for (id, field) in rows {
        conn.execute(
            "INSERT INTO secret_reference_uses (reference_id, resource_kind, resource_id, field) \
             VALUES (?1, ?2, ?3, ?4)",
            params![id, kind, user_id, field.as_str()],
        )
        .map_err(sql_error)?;
    }
    Ok(())
}

/// The table, stored kind, and ID of a user.
fn user_columns(user: SecretUser) -> (&'static str, &'static str, String) {
    let table = match user {
        SecretUser::Instance(_) => "instances",
        SecretUser::Environment(_) => "environments",
    };
    (table, user.kind_code(), user.id().to_string())
}

/// `InsertAuditEvent`: a new ID, then the row.
pub(super) fn insert_audit(
    conn: &Connection,
    record: &AuditEventRecord,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    let id = record.id.to_string();
    if exists(conn, "SELECT 1 FROM audit_events WHERE id = ?1", [&id])? {
        return Err(RegistryError::DuplicateResource {
            resource: op.resource(),
        });
    }
    conn.execute(
        &format!(
            "INSERT INTO audit_events ({AUDIT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        ),
        params![
            id,
            record.actor.as_code(),
            record.action.as_str(),
            record.resource.map(|resource| resource.kind_code()),
            record.resource.map(|resource| resource.id().to_string()),
            record.operation.map(|operation| operation.to_string()),
            record.result.as_code(),
            details_text(&record.safe_details)?,
            ts(record.occurred_at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// The stored form of audit details: a JSON object of strings, keys in
/// order.
fn details_text(details: &SafeDetails) -> Result<String, RegistryError> {
    let object: Map<String, Value> = details
        .iter()
        .map(|(key, value)| (key.to_string(), Value::String(value.to_owned())))
        .collect();
    serde_json::to_string(&Value::Object(object)).map_err(|err| RegistryError::Integrity {
        detail: format!("cannot encode audit details: {err}"),
    })
}

// Reads.

/// The uses of the row with stored ID `id`, environments before
/// instances, then by ID and field.
fn uses_of(
    conn: &Connection,
    id: &str,
    reference: &SecretRef,
) -> Result<Vec<SecretUse>, RegistryError> {
    collect(
        conn,
        "SELECT resource_kind, resource_id, field FROM secret_reference_uses \
         WHERE reference_id = ?1 ORDER BY resource_kind, resource_id, field",
        [id],
        "secret_reference_uses",
        |cells| {
            Ok(SecretUse::new(
                reference.clone(),
                stored_user(cells)?,
                cells.parse("field")?,
            ))
        },
    )
}

fn stored_user(cells: Cells<'_, '_>) -> Result<SecretUser, RegistryError> {
    match cells.text("resource_kind")?.as_str() {
        "instance" => Ok(SecretUser::Instance(cells.parse("resource_id")?)),
        "environment" => Ok(SecretUser::Environment(cells.parse("resource_id")?)),
        _ => Err(cells.corrupt("resource_kind", "is not instance or environment")),
    }
}

fn stored_reference(cells: Cells<'_, '_>) -> Result<SecretReferenceRecord, RegistryError> {
    let owner = match cells.parse::<SecretOwnerKind>("owner_kind")? {
        SecretOwnerKind::Instance => SecretOwner::Instance(cells.parse("owner_id")?),
        SecretOwnerKind::Environment => SecretOwner::Environment(cells.parse("owner_id")?),
        SecretOwnerKind::User => {
            if cells.opt_text("owner_id")?.is_some() {
                return Err(cells.corrupt("owner_id", "is set for a user owner"));
            }
            SecretOwner::User
        }
    };
    let mut record = SecretReferenceRecord::new(
        cells.parse("id")?,
        cells.parse("reference")?,
        cells.parse("platform_store_key")?,
        owner,
        cells.parse("purpose")?,
        cells.ts("created_at")?,
    );
    record.value_origin = cells.opt_parse("value_origin")?;
    record.status = cells.parse("status")?;
    record.status_observed_at = cells.opt_ts("status_observed_at")?;
    record.value_updated_at = cells.opt_ts("value_updated_at")?;
    record.revision = cells.revision()?;
    record.updated_at = cells.ts("updated_at")?;
    Ok(record)
}

/// The rows with their uses, for one reference or a filtered list.
fn views(
    conn: &Connection,
    clause: &str,
    params: &[String],
) -> Result<Vec<SecretReferenceView>, RegistryError> {
    let rows = collect(
        conn,
        &format!("SELECT {REFERENCE_COLUMNS} FROM secret_references {clause} ORDER BY reference"),
        params_from_iter(params),
        "secret_references",
        |cells| Ok((cells.text("id")?, stored_reference(cells)?)),
    )?;
    rows.into_iter()
        .map(|(id, record)| {
            let used_by = uses_of(conn, &id, &record.reference)?;
            Ok(SecretReferenceView::new(record, used_by))
        })
        .collect()
}

pub(super) fn reference(
    conn: &Connection,
    reference: &SecretRef,
) -> Result<Option<SecretReferenceView>, RegistryError> {
    Ok(views(
        conn,
        "WHERE reference = ?1",
        &[reference.as_str().to_owned()],
    )?
    .into_iter()
    .next())
}

pub(super) fn references(
    conn: &Connection,
    filter: SecretReferenceFilter,
) -> Result<Vec<SecretReferenceView>, RegistryError> {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    if let Some(kind) = filter.owner_kind {
        params.push(kind.as_code().to_owned());
        clauses.push(format!("owner_kind = ?{}", params.len()));
    }
    if let Some(status) = filter.status {
        params.push(status.as_code().to_owned());
        clauses.push(format!("status = ?{}", params.len()));
    }
    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    views(conn, &clause, &params)
}

pub(super) fn uses_by(
    conn: &Connection,
    user: SecretUser,
) -> Result<Vec<SecretUse>, RegistryError> {
    let (_, kind, id) = user_columns(user);
    collect(
        conn,
        "SELECT secret_references.reference, secret_reference_uses.field \
         FROM secret_reference_uses JOIN secret_references \
         ON secret_references.id = secret_reference_uses.reference_id \
         WHERE secret_reference_uses.resource_kind = ?1 \
         AND secret_reference_uses.resource_id = ?2 \
         ORDER BY secret_reference_uses.field, secret_references.reference",
        [kind, id.as_str()],
        "secret_reference_uses",
        |cells| {
            Ok(SecretUse::new(
                cells.of("secret_references").parse("reference")?,
                user,
                cells.parse("field")?,
            ))
        },
    )
}

fn stored_audit(cells: Cells<'_, '_>) -> Result<AuditEventRecord, RegistryError> {
    let resource = match cells.opt_text("resource_kind")?.as_deref() {
        None if cells.opt_text("resource_id")?.is_none() => None,
        Some("secret") => Some(AuditResource::SecretReference(cells.parse("resource_id")?)),
        None => return Err(cells.corrupt("resource_id", "is set without a resource kind")),
        Some(_) => return Err(cells.corrupt("resource_kind", "is not a known resource kind")),
    };
    let mut record = AuditEventRecord::new(
        cells.parse("id")?,
        cells.parse("actor")?,
        cells.parse("action")?,
        cells.parse("result")?,
        cells.ts("occurred_at")?,
    );
    record.resource = resource;
    record.operation = cells.opt_parse("operation_id")?;
    record.safe_details = stored_details(&cells.text("safe_details")?)
        .ok_or_else(|| cells.corrupt("safe_details", "is not an object of detail strings"))?;
    Ok(record)
}

fn stored_details(text: &str) -> Option<SafeDetails> {
    let Value::Object(object) = serde_json::from_str::<Value>(text).ok()? else {
        return None;
    };
    let entries = object
        .into_iter()
        .map(|(key, value)| match value {
            Value::String(value) => Some((key.parse::<DetailKey>().ok()?, value)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    SafeDetails::new(entries).ok()
}

pub(super) fn audit_events(
    conn: &Connection,
    query: &AuditQuery,
) -> Result<AuditPage, RegistryError> {
    let mut params = Vec::new();
    let clause = match query.before {
        Some(before) => {
            params.push(ts(before.occurred_at));
            params.push(before.id.to_string());
            "WHERE (occurred_at, id) < (?1, ?2)"
        }
        None => "",
    };
    // One row more than the page shows whether another page follows.
    let limit = usize::from(query.limit);
    let mut items = collect(
        conn,
        &format!(
            "SELECT {AUDIT_COLUMNS} FROM audit_events {clause} \
             ORDER BY occurred_at DESC, id DESC LIMIT {}",
            limit + 1
        ),
        params_from_iter(params),
        "audit_events",
        stored_audit,
    )?;
    let next = if items.len() > limit {
        items.truncate(limit);
        items.last().map(AuditCursor::of)
    } else {
        None
    };
    Ok(AuditPage { items, next })
}
