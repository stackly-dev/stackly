//! The tables the process supervisor writes (spec 0005, "Tambahan fitur
//! 10"): `process_observations`, and `config_revisions` with the
//! instance's pointer to its active revision, `instances.config_revision_id`,
//! numbered with an endpoint snapshot and `ready_at` since spec 0029.
//!
//! The checks follow the order the `Registry` trait documents, the same as
//! the fake: existence, references, then the unique rule of a config
//! revision (one that follows none while another is active). The column
//! checks stay the last guard: an executable over 1024 characters and
//! config values that are not a JSON object end as `Integrity`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use jiff::Timestamp;
use rusqlite::{Connection, params};
use stackly_domain::{
    ConfigRevisionId, ConfigRevisionNumber, ConfigRevisionOrigin, ConfigRevisionRecord,
    ConfigRevisionStatus, ConfigValuesJson, EndpointName, EndpointRequests, Instance, InstanceId,
    OperationId, Pid, Port, PortRequest, ProcessIdentity, ProcessObservation, ProcessStart,
    RegistryError, ResourceRef, Sha256Digest, WriteOp,
};

use super::column::{Cells, collect, exists, first, int, opt_ts, sql_error, ts};

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

/// The columns every config revision read selects, in [`revision`] order.
const REVISION_COLUMNS: &str = "id, number, config_values, endpoints, rendered_sha256, \
     previous_id, status, origin, source_number, ready_at, created_by_operation, created_at";

/// The endpoint snapshot as canonical JSON object text: names sorted, each
/// `{"type":"fixed","content":<port>}` or `{"type":"auto"}` (spec 0029).
pub(super) fn endpoints_json(endpoints: &EndpointRequests) -> String {
    let mut object = serde_json::Map::new();
    for (name, request) in endpoints.iter() {
        let value = match request {
            PortRequest::Fixed(port) => {
                serde_json::json!({ "type": "fixed", "content": port.get() })
            }
            PortRequest::Auto => serde_json::json!({ "type": "auto" }),
        };
        object.insert(name.to_string(), value);
    }
    serde_json::Value::Object(object).to_string()
}

/// Reads the endpoint snapshot back; anything but the shape
/// [`endpoints_json`] writes is `Corrupt`.
fn parse_endpoints(cells: &Cells<'_, '_>) -> Result<EndpointRequests, RegistryError> {
    let text = cells.text("endpoints")?;
    let bad = |detail: &str| cells.corrupt("endpoints", detail);
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| bad("is not JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| bad("is not a JSON object"))?;
    let mut requests = BTreeMap::new();
    for (name, request) in object {
        let name = name
            .parse::<EndpointName>()
            .map_err(|_| bad("names an endpoint that is not valid"))?;
        let request = match (
            request.get("type").and_then(serde_json::Value::as_str),
            request.get("content"),
        ) {
            (Some("auto"), None) => PortRequest::Auto,
            (Some("fixed"), Some(content)) => content
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
                .and_then(|port| Port::new(port).ok())
                .map(PortRequest::Fixed)
                .ok_or_else(|| bad("holds a fixed port outside 1 to 65535"))?,
            _ => return Err(bad("holds a port request that is not fixed or auto")),
        };
        requests.insert(name, request);
    }
    Ok(EndpointRequests::new(requests))
}

/// Maps one row selected with [`REVISION_COLUMNS`].
fn revision(
    instance: InstanceId,
    cells: &Cells<'_, '_>,
) -> Result<ConfigRevisionRecord, RegistryError> {
    let number = cells
        .opt_u64("number")?
        .and_then(|number| ConfigRevisionNumber::new(number).ok())
        .ok_or_else(|| cells.corrupt("number", "is not a revision number from 1"))?;
    let source_number = match cells.opt_u64("source_number")? {
        None => None,
        Some(number) => Some(
            ConfigRevisionNumber::new(number).map_err(|err| cells.corrupt("source_number", err))?,
        ),
    };
    let mut record = ConfigRevisionRecord::first(
        cells.parse::<ConfigRevisionId>("id")?,
        instance,
        cells.parse::<ConfigValuesJson>("config_values")?,
        parse_endpoints(cells)?,
        cells.opt_parse::<OperationId>("created_by_operation")?,
        cells.ts("created_at")?,
    );
    record.number = number;
    record.rendered_sha256 = cells.opt_parse::<Sha256Digest>("rendered_sha256")?;
    record.previous = cells.opt_parse::<ConfigRevisionId>("previous_id")?;
    record.status = cells.parse::<ConfigRevisionStatus>("status")?;
    record.origin = cells.parse::<ConfigRevisionOrigin>("origin")?;
    record.source_number = source_number;
    record.ready_at = cells.opt_ts("ready_at")?;
    Ok(record)
}

/// Writes one row for `record`.
fn insert_row(conn: &Connection, record: &ConfigRevisionRecord) -> Result<(), RegistryError> {
    conn.execute(
        "INSERT INTO config_revisions (id, instance_id, number, config_values, endpoints, \
         rendered_sha256, previous_id, status, origin, source_number, ready_at, \
         created_by_operation, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            record.id.to_string(),
            record.instance.to_string(),
            int(record.number.get(), "number")?,
            record.values.as_str(),
            endpoints_json(&record.endpoints),
            record.rendered_sha256.map(|digest| digest.to_string()),
            record.previous.map(|previous| previous.to_string()),
            record.status.as_code(),
            record.origin.as_code(),
            record
                .source_number
                .map(|number| int(number.get(), "source_number"))
                .transpose()?,
            opt_ts(record.ready_at),
            record
                .created_by_operation
                .map(|operation| operation.to_string()),
            ts(record.created_at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

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
    insert_row(conn, record)?;
    conn.execute(
        "UPDATE instances SET config_revision_id = ?2 WHERE id = ?1",
        [&instance, &id],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// The record half of `ActivateConfigRevision`, after the instance row was
/// rewritten: a new ID; an active revision that `record` follows with the
/// next number; `record` active and its endpoints the port requests of
/// `instance`. Then the active one is superseded, the row written, and the
/// instance points to it.
pub(super) fn activate_config_revision(
    conn: &Connection,
    instance: &Instance,
    record: &ConfigRevisionRecord,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    let integrity = |detail: &str| RegistryError::Integrity {
        detail: detail.to_owned(),
    };
    if exists(
        conn,
        "SELECT 1 FROM config_revisions WHERE id = ?1",
        [record.id.to_string()],
    )? {
        return Err(RegistryError::DuplicateResource {
            resource: ResourceRef::ConfigRevision(record.id),
        });
    }
    let active =
        active_config_revision(conn, instance.id())?.ok_or(RegistryError::MissingReference {
            resource: op.resource(),
        })?;
    if record.instance != instance.id() {
        return Err(integrity("the config revision belongs to another instance"));
    }
    if record.status != ConfigRevisionStatus::Active {
        return Err(integrity("an activated config revision must be active"));
    }
    if record.previous != Some(active.id) || record.number != active.number.next() {
        return Err(integrity(
            "an activated config revision must follow the active one with the next number",
        ));
    }
    if record.endpoints != EndpointRequests::of(instance) {
        return Err(integrity(
            "the endpoint snapshot differs from the instance's port requests",
        ));
    }
    conn.execute(
        "UPDATE config_revisions SET status = ?2 WHERE id = ?1",
        [
            active.id.to_string(),
            ConfigRevisionStatus::Superseded.as_code().to_owned(),
        ],
    )
    .map_err(sql_error)?;
    insert_row(conn, record)?;
    conn.execute(
        "UPDATE instances SET config_revision_id = ?2 WHERE id = ?1",
        [instance.id().to_string(), record.id.to_string()],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `MarkConfigRevisionReady`: sets `ready_at` while it is empty.
pub(super) fn mark_config_revision_ready(
    conn: &Connection,
    revision: ConfigRevisionId,
    at: Timestamp,
) -> Result<(), RegistryError> {
    let changed = conn
        .execute(
            "UPDATE config_revisions SET ready_at = ?2 WHERE id = ?1 AND ready_at IS NULL",
            [revision.to_string(), ts(at)],
        )
        .map_err(sql_error)?;
    if changed == 0
        && !exists(
            conn,
            "SELECT 1 FROM config_revisions WHERE id = ?1",
            [revision.to_string()],
        )?
    {
        return Err(RegistryError::NotFound {
            resource: ResourceRef::ConfigRevision(revision),
        });
    }
    Ok(())
}

/// `PruneConfigRevisions`: the instance exists; every revision outside the
/// `keep` newest numbers goes, except the active one and the last known
/// good.
pub(super) fn prune_config_revisions(
    conn: &Connection,
    instance: InstanceId,
    keep: u32,
) -> Result<(), RegistryError> {
    expect_instance(conn, instance)?;
    conn.execute(
        "DELETE FROM config_revisions WHERE instance_id = ?1 \
         AND id IS NOT (SELECT config_revision_id FROM instances WHERE id = ?1) \
         AND id IS NOT (SELECT id FROM config_revisions WHERE instance_id = ?1 \
             AND ready_at IS NOT NULL ORDER BY number DESC LIMIT 1) \
         AND number NOT IN (SELECT number FROM config_revisions WHERE instance_id = ?1 \
             ORDER BY number DESC LIMIT ?2)",
        params![instance.to_string(), i64::from(keep)],
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
        &format!(
            "SELECT {REVISION_COLUMNS} FROM config_revisions WHERE id = ?1 AND instance_id = ?2"
        ),
        [id.to_string(), instance.to_string()],
        "config_revisions",
        |cells| revision(instance, &cells),
    )?;
    record.map(Some).ok_or_else(|| RegistryError::Corrupt {
        table: "instances",
        column: "config_revision_id",
        detail: "names a config revision of another instance or none".to_owned(),
    })
}

/// At most `limit` revisions of an instance, newest number first.
pub(super) fn config_revisions(
    conn: &Connection,
    instance: InstanceId,
    limit: u32,
) -> Result<Vec<ConfigRevisionRecord>, RegistryError> {
    if active_id(conn, instance)?.is_none() {
        return Err(unknown_instance(instance));
    }
    collect(
        conn,
        &format!(
            "SELECT {REVISION_COLUMNS} FROM config_revisions WHERE instance_id = ?1 \
             ORDER BY number DESC LIMIT ?2"
        ),
        params![instance.to_string(), i64::from(limit)],
        "config_revisions",
        |cells| revision(instance, &cells),
    )
}

/// The revision with `ready_at` set and the highest number, or `None`.
pub(super) fn last_known_good(
    conn: &Connection,
    instance: InstanceId,
) -> Result<Option<ConfigRevisionRecord>, RegistryError> {
    if active_id(conn, instance)?.is_none() {
        return Err(unknown_instance(instance));
    }
    first(
        conn,
        &format!(
            "SELECT {REVISION_COLUMNS} FROM config_revisions WHERE instance_id = ?1 \
             AND ready_at IS NOT NULL ORDER BY number DESC LIMIT 1"
        ),
        [instance.to_string()],
        "config_revisions",
        |cells| revision(instance, &cells),
    )
}
