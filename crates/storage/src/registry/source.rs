//! Provider sources and the delete of an instance a failed create made
//! (spec 0005, *Tambahan fitur 20*; spec 0020).

use rusqlite::{Connection, params};
use stackly_domain::{
    InstanceId, InstanceLifecycle, NotDeletableReason, OperationId, ProviderSourceId,
    ProviderSourceKind, ProviderSourceRecord, RegistryError, ResourceRef, ResourceRevision,
};

use super::column::{exists, first, opt_ts, sql_error, ts};

/// The provider source with this kind and location.
pub(super) fn by_location(
    conn: &Connection,
    kind: ProviderSourceKind,
    location: &str,
) -> Result<Option<ProviderSourceRecord>, RegistryError> {
    first(
        conn,
        "SELECT id, kind, location, trust_root, last_sync_at, status, revision, created_at, \
         updated_at FROM provider_sources WHERE kind = ?1 AND location = ?2",
        (kind.as_code(), location),
        "provider_sources",
        |cells| {
            let mut record = ProviderSourceRecord::new(
                cells.parse::<ProviderSourceId>("id")?,
                cells.parse::<ProviderSourceKind>("kind")?,
                cells.text("location")?,
                cells.ts("created_at")?,
            );
            record.trust_root = cells.opt_text("trust_root")?;
            record.last_sync_at = cells.opt_ts("last_sync_at")?;
            record.status = cells.parse("status")?;
            record.revision = cells.revision()?;
            record.updated_at = cells.ts("updated_at")?;
            Ok(record)
        },
    )
}

/// `InsertProviderSource`: the ID and the kind with its location are both
/// unique, and a duplicate names the row that holds them.
pub(super) fn insert(
    conn: &Connection,
    record: &ProviderSourceRecord,
) -> Result<(), RegistryError> {
    let holder: Option<ProviderSourceId> = first(
        conn,
        "SELECT id FROM provider_sources WHERE id = ?1 OR (kind = ?2 AND location = ?3) \
         ORDER BY id = ?1 DESC LIMIT 1",
        (
            record.id.to_string(),
            record.kind.as_code(),
            record.location.as_str(),
        ),
        "provider_sources",
        |cells| cells.parse("id"),
    )?;
    if let Some(holder) = holder {
        return Err(RegistryError::DuplicateResource {
            resource: ResourceRef::ProviderSource(holder),
        });
    }
    conn.execute(
        "INSERT INTO provider_sources (id, kind, location, trust_root, last_sync_at, status, \
         revision, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            record.id.to_string(),
            record.kind.as_code(),
            record.location,
            record.trust_root,
            opt_ts(record.last_sync_at),
            record.status.as_code(),
            i64::try_from(record.revision.get()).map_err(|_| RegistryError::Integrity {
                detail: "revision is larger than the registry can store".to_owned(),
            })?,
            ts(record.created_at),
            ts(record.updated_at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `ProviderRevisionRecord.source_id` must name a row of `provider_sources`.
pub(super) fn require(conn: &Connection, source: ProviderSourceId) -> Result<(), RegistryError> {
    if exists(
        conn,
        "SELECT 1 FROM provider_sources WHERE id = ?1",
        [source.to_string()],
    )? {
        Ok(())
    } else {
        Err(RegistryError::MissingReference {
            resource: ResourceRef::ProviderSource(source),
        })
    }
}

/// `DeleteInstanceCreatedBy`: the instance, its secret uses, and what
/// cascades (endpoints, paths, config revisions, process observations,
/// leases), only while `operation` created it and nothing may run.
pub(super) fn delete_instance_created_by(
    conn: &Connection,
    id: InstanceId,
    expected: ResourceRevision,
    operation: OperationId,
) -> Result<(), RegistryError> {
    let resource = ResourceRef::Instance(id);
    let row = first(
        conn,
        "SELECT revision, lifecycle, created_by_operation FROM instances WHERE id = ?1",
        [id.to_string()],
        "instances",
        |cells| {
            Ok((
                cells.revision()?,
                cells.text("lifecycle")?,
                cells.parse::<OperationId>("created_by_operation")?,
            ))
        },
    )?;
    let Some((actual, lifecycle, created_by)) = row else {
        return Err(RegistryError::NotFound { resource });
    };
    if actual != expected {
        return Err(RegistryError::RevisionConflict {
            resource,
            expected,
            actual,
        });
    }
    let active = lifecycle
        .parse::<InstanceLifecycle>()
        .map(|lifecycle| {
            matches!(
                lifecycle,
                InstanceLifecycle::Starting
                    | InstanceLifecycle::Running
                    | InstanceLifecycle::Stopping
                    | InstanceLifecycle::Recovering
                    | InstanceLifecycle::Unknown
            )
        })
        .map_err(|err| RegistryError::Corrupt {
            table: "instances",
            column: "lifecycle",
            detail: err.to_string(),
        })?;
    let reason = if created_by != operation {
        Some(NotDeletableReason::CreatedByOtherOperation)
    } else if active {
        Some(NotDeletableReason::Active)
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(RegistryError::NotDeletable { resource, reason });
    }
    conn.execute(
        "DELETE FROM secret_reference_uses WHERE resource_kind = 'instance' AND resource_id = ?1",
        [id.to_string()],
    )
    .map_err(sql_error)?;
    conn.execute("DELETE FROM instances WHERE id = ?1", [id.to_string()])
        .map_err(sql_error)?;
    Ok(())
}
