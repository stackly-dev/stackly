//! The tables the process supervisor writes (spec 0005, "Tambahan fitur
//! 10"): `process_observations`, and `config_revisions` with the
//! instance's pointer to its active revision, `instances.config_revision_id`.
//!
//! The checks follow the order the `Registry` trait documents, the same as
//! the fake: existence, references, then the unique rule of a config
//! revision (one that follows none while another is active). The column
//! checks stay the last guard: an executable over 1024 characters and
//! config values that are not a JSON object end as `Integrity`.

use std::path::PathBuf;

use rusqlite::{Connection, params};
use stackly_domain::{
    ConfigRevisionId, ConfigRevisionRecord, ConfigRevisionStatus, ConfigValuesJson, InstanceId,
    OperationId, Pid, ProcessIdentity, ProcessObservation, ProcessStart, RegistryError,
    ResourceRef, Sha256Digest, WriteOp,
};

use super::column::{Cells, exists, first, sql_error, ts};

/// `instances.config_revision_id` of an instance: `None` for an unknown
/// instance, `Some(None)` for one without an active revision.
fn active_id(
    conn: &Connection,
    instance: InstanceId,
) -> Result<Option<Option<ConfigRevisionId>>, RegistryError> {
    first(
        conn,
        "SELECT config_revision_id FROM instances WHERE id = ?1",
        [instance.to_string()],
        "instances",
        |cells| cells.opt_parse("config_revision_id"),
    )
}

/// Fails with `MissingReference` for an instance a write refers to that
/// does not exist (an earlier op of the same batch counts).
fn expect_instance(conn: &Connection, instance: InstanceId) -> Result<(), RegistryError> {
    if exists(
        conn,
        "SELECT 1 FROM instances WHERE id = ?1",
        [instance.to_string()],
    )? {
        Ok(())
    } else {
        Err(RegistryError::MissingReference {
            resource: ResourceRef::Instance(instance),
        })
    }
}

/// The `NotFound` of a read about an unknown instance.
const fn unknown_instance(instance: InstanceId) -> RegistryError {
    RegistryError::NotFound {
        resource: ResourceRef::Instance(instance),
    }
}

// Process observations.

/// `RecordProcessObservation`: the instance exists, then one row.
pub(super) fn record_observation(
    conn: &Connection,
    observation: &ProcessObservation,
) -> Result<(), RegistryError> {
    expect_instance(conn, observation.instance)?;
    let identity = &observation.identity;
    let executable = identity
        .executable
        .to_str()
        .ok_or(RegistryError::NotUtf8Path {
            field: "executable_identity",
        })?;
    conn.execute(
        "INSERT INTO process_observations (instance_id, pid, start_identity, \
         executable_identity, boot_id, session_id, observed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            observation.instance.to_string(),
            identity.pid.get(),
            identity.start.to_string(),
            executable,
            identity.boot.to_string(),
            identity.session.to_string(),
            ts(observation.observed_at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// The observation recorded last for an instance: the largest row ID, never
/// the latest `observed_at`.
pub(super) fn latest_observation(
    conn: &Connection,
    instance: InstanceId,
) -> Result<Option<ProcessObservation>, RegistryError> {
    if active_id(conn, instance)?.is_none() {
        return Err(unknown_instance(instance));
    }
    first(
        conn,
        "SELECT pid, start_identity, executable_identity, boot_id, session_id, observed_at \
         FROM process_observations WHERE instance_id = ?1 ORDER BY id DESC LIMIT 1",
        [instance.to_string()],
        "process_observations",
        |cells| observation(instance, &cells),
    )
}

fn observation(
    instance: InstanceId,
    cells: &Cells<'_, '_>,
) -> Result<ProcessObservation, RegistryError> {
    let pid = cells
        .opt_u64("pid")?
        .and_then(|pid| u32::try_from(pid).ok())
        .and_then(|pid| Pid::new(pid).ok())
        .ok_or_else(|| cells.corrupt("pid", "is not a process ID from 1 to 2147483647"))?;
    Ok(ProcessObservation {
        instance,
        identity: ProcessIdentity {
            pid,
            start: cells.parse::<ProcessStart>("start_identity")?,
            executable: PathBuf::from(cells.text("executable_identity")?),
            session: cells.parse::<Pid>("session_id")?,
            boot: cells.parse("boot_id")?,
        },
        observed_at: cells.ts("observed_at")?,
    })
}

// Config revisions.

/// `InsertConfigRevision`: a new ID; the instance exists and `previous` is
/// a revision of it; no active revision unless `previous` is set. Then the
/// row, and the instance points to it. The instance's revision and
/// `updated_at` stay: the config revision is not part of the aggregate.
pub(super) fn insert_config_revision(
    conn: &Connection,
    record: &ConfigRevisionRecord,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    let id = record.id.to_string();
    let instance = record.instance.to_string();
    if exists(conn, "SELECT 1 FROM config_revisions WHERE id = ?1", [&id])? {
        return Err(RegistryError::DuplicateResource {
            resource: op.resource(),
        });
    }
    let active = active_id(conn, record.instance)?.ok_or(RegistryError::MissingReference {
        resource: ResourceRef::Instance(record.instance),
    })?;
    match (record.previous, active) {
        (Some(previous), _) => {
            if !exists(
                conn,
                "SELECT 1 FROM config_revisions WHERE id = ?1 AND instance_id = ?2",
                [previous.to_string(), instance.clone()],
            )? {
                return Err(RegistryError::MissingReference {
                    resource: ResourceRef::ConfigRevision(previous),
                });
            }
        }
        (None, Some(active)) => {
            return Err(RegistryError::DuplicateResource {
                resource: ResourceRef::ConfigRevision(active),
            });
        }
        (None, None) => {}
    }
    conn.execute(
        "INSERT INTO config_revisions (id, instance_id, config_values, rendered_sha256, \
         previous_id, status, created_by_operation, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            instance,
            record.values.as_str(),
            record.rendered_sha256.map(|digest| digest.to_string()),
            record.previous.map(|previous| previous.to_string()),
            record.status.as_code(),
            record
                .created_by_operation
                .map(|operation| operation.to_string()),
            ts(record.created_at),
        ],
    )
    .map_err(sql_error)?;
    conn.execute(
        "UPDATE instances SET config_revision_id = ?2 WHERE id = ?1",
        [&instance, &id],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// The revision `instances.config_revision_id` points to, or `None` while
/// it is empty.
pub(super) fn active_config_revision(
    conn: &Connection,
    instance: InstanceId,
) -> Result<Option<ConfigRevisionRecord>, RegistryError> {
    let Some(active) = active_id(conn, instance)? else {
        return Err(unknown_instance(instance));
    };
    let Some(id) = active else {
        return Ok(None);
    };
    let record = first(
        conn,
        "SELECT config_values, rendered_sha256, previous_id, status, created_by_operation, \
         created_at FROM config_revisions WHERE id = ?1 AND instance_id = ?2",
        [id.to_string(), instance.to_string()],
        "config_revisions",
        |cells| {
            let mut record = ConfigRevisionRecord::first(
                id,
                instance,
                cells.parse::<ConfigValuesJson>("config_values")?,
                cells.opt_parse::<OperationId>("created_by_operation")?,
                cells.ts("created_at")?,
            );
            record.rendered_sha256 = cells.opt_parse::<Sha256Digest>("rendered_sha256")?;
            record.previous = cells.opt_parse::<ConfigRevisionId>("previous_id")?;
            record.status = cells.parse::<ConfigRevisionStatus>("status")?;
            Ok(record)
        },
    )?;
    record.map(Some).ok_or_else(|| RegistryError::Corrupt {
        table: "instances",
        column: "config_revision_id",
        detail: "names a config revision of another instance or none".to_owned(),
    })
}
