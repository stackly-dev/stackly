//! Daemon settings and storage locations (spec 0032): the tables
//! `daemon_settings` and `storage_locations`, and the location pair a new
//! instance takes.
//!
//! The checks follow the order the `Registry` trait documents, the same as
//! the fake: the revision, then references, then the unique rules. The
//! triggers and unique indexes of the migration stay the last guard.

use jiff::Timestamp;
use rusqlite::{Connection, params};
use stackly_domain::{
    DaemonSettings, LocationKind, RegistryError, ResourceRef, ResourceRevision, SettingsPatch,
    StorageLocation, StorageLocationId, WriteOp,
};

use super::column::{Cells, collect, corrupt, exists, first, int, sql_error, ts};

/// The settings columns every read selects.
const SETTINGS_COLUMNS: &str = "default_startup, update_channel, telemetry, \
     telemetry_copy_version, data_location_id, logs_location_id, cache_location_id, revision, \
     updated_at";

/// The one settings row.
pub(super) fn settings(conn: &Connection) -> Result<DaemonSettings, RegistryError> {
    let sql = format!("SELECT {SETTINGS_COLUMNS} FROM daemon_settings WHERE id = 1");
    first(conn, &sql, [], "daemon_settings", |cells| {
        let copy = cells
            .opt_u64("telemetry_copy_version")?
            .ok_or_else(|| cells.corrupt("telemetry_copy_version", "is null"))?;
        Ok(DaemonSettings {
            default_startup: cells.parse("default_startup")?,
            update_channel: cells.parse("update_channel")?,
            telemetry: cells
                .opt_bool("telemetry")?
                .ok_or_else(|| cells.corrupt("telemetry", "is null"))?,
            telemetry_copy_version: u32::try_from(copy).map_err(|_| {
                cells.corrupt("telemetry_copy_version", "does not fit a 32 bit number")
            })?,
            data_location: cells.parse("data_location_id")?,
            logs_location: cells.parse("logs_location_id")?,
            cache_location: cells.parse("cache_location_id")?,
            revision: cells.revision()?,
            updated_at: cells.ts("updated_at")?,
        })
    })?
    .ok_or_else(|| corrupt("daemon_settings", "id", "the settings row is missing"))
}

/// Every location, built in ones first (their fixed IDs put them in the
/// order data, logs, cache), then by creation time and ID.
pub(super) fn locations(conn: &Connection) -> Result<Vec<StorageLocation>, RegistryError> {
    collect(
        conn,
        "SELECT id, kind, path, built_in, created_at FROM storage_locations \
         ORDER BY built_in DESC, created_at, id",
        [],
        "storage_locations",
        |cells| location_row(&cells),
    )
}

fn location_row(cells: &Cells<'_, '_>) -> Result<StorageLocation, RegistryError> {
    let built_in = cells
        .opt_bool("built_in")?
        .ok_or_else(|| cells.corrupt("built_in", "is null"))?;
    StorageLocation::from_stored(
        cells.parse("id")?,
        cells.parse("kind")?,
        cells.opt_text("path")?,
        built_in,
        cells.ts("created_at")?,
    )
    .map_err(|err| cells.corrupt("path", err))
}

/// How many instances each location holds, for the locations that hold
/// one. An instance counts once for its data location and once for its
/// log location.
pub(super) fn usage(conn: &Connection) -> Result<Vec<(StorageLocationId, u64)>, RegistryError> {
    collect(
        conn,
        "SELECT location_id, count(DISTINCT instance_id) AS used FROM instance_paths \
         WHERE location_id IS NOT NULL GROUP BY location_id ORDER BY location_id",
        [],
        "instance_paths",
        |cells| {
            let used = cells
                .opt_u64("used")?
                .ok_or_else(|| cells.corrupt("used", "is null"))?;
            Ok((cells.parse("location_id")?, used))
        },
    )
}

/// The data and log locations the settings name now.
pub(super) fn default_pair(
    conn: &Connection,
) -> Result<(StorageLocationId, StorageLocationId), RegistryError> {
    let settings = settings(conn)?;
    Ok((settings.data_location, settings.logs_location))
}

/// The pair a create used: both locations exist and hold the right kind.
pub(super) fn check_pair(
    conn: &Connection,
    data: StorageLocationId,
    logs: StorageLocationId,
) -> Result<(), RegistryError> {
    check_kind(conn, data, LocationKind::Data)?;
    check_kind(conn, logs, LocationKind::Logs)
}

fn check_kind(
    conn: &Connection,
    id: StorageLocationId,
    expected: LocationKind,
) -> Result<(), RegistryError> {
    let kind: Option<LocationKind> = first(
        conn,
        "SELECT kind FROM storage_locations WHERE id = ?1",
        [id.to_string()],
        "storage_locations",
        |cells| cells.parse("kind"),
    )?;
    match kind {
        None => Err(RegistryError::MissingReference {
            resource: ResourceRef::StorageLocation(id),
        }),
        Some(kind) if kind != expected => Err(RegistryError::WrongLocationKind {
            location: ResourceRef::StorageLocation(id),
            expected,
        }),
        Some(_) => Ok(()),
    }
}

/// The revision of the settings must be `expected`.
fn expect_revision(conn: &Connection, expected: ResourceRevision) -> Result<(), RegistryError> {
    let actual = settings(conn)?.revision;
    if actual == expected {
        Ok(())
    } else {
        Err(RegistryError::RevisionConflict {
            resource: ResourceRef::DaemonSettings,
            expected,
            actual,
        })
    }
}

/// `UpdateDaemonSettings`: the revision, then the patch. A patch that
/// changes nothing writes nothing.
pub(super) fn update(
    conn: &Connection,
    patch: &SettingsPatch,
    expected: ResourceRevision,
    at: Timestamp,
) -> Result<(), RegistryError> {
    expect_revision(conn, expected)?;
    let current = settings(conn)?;
    match current.patched(patch, at) {
        Some(next) => write(conn, &next, expected),
        None => Ok(()),
    }
}

/// `SetDefaultLocation`: the revision, the location, its kind, then the
/// column of the kind.
pub(super) fn set_default(
    conn: &Connection,
    kind: LocationKind,
    id: StorageLocationId,
    expected: ResourceRevision,
    at: Timestamp,
) -> Result<(), RegistryError> {
    expect_revision(conn, expected)?;
    check_kind(conn, id, kind)?;
    let current = settings(conn)?;
    match current.with_location(kind, id, at) {
        Some(next) => write(conn, &next, expected),
        None => Ok(()),
    }
}

fn write(
    conn: &Connection,
    next: &DaemonSettings,
    expected: ResourceRevision,
) -> Result<(), RegistryError> {
    conn.execute(
        "UPDATE daemon_settings SET default_startup = ?1, update_channel = ?2, telemetry = ?3, \
         telemetry_copy_version = ?4, data_location_id = ?5, logs_location_id = ?6, \
         cache_location_id = ?7, revision = ?8, updated_at = ?9 WHERE id = 1 AND revision = ?10",
        params![
            next.default_startup.as_code(),
            next.update_channel.as_code(),
            i64::from(next.telemetry),
            i64::from(next.telemetry_copy_version),
            next.data_location.to_string(),
            next.logs_location.to_string(),
            next.cache_location.to_string(),
            int(next.revision.get(), "revision")?,
            ts(next.updated_at),
            int(expected.get(), "revision")?,
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `InsertStorageLocation`: a new ID and a new kind and path, then the row.
pub(super) fn insert_location(
    conn: &Connection,
    location: &StorageLocation,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    if exists(
        conn,
        "SELECT 1 FROM storage_locations WHERE id = ?1",
        [location.id().to_string()],
    )? {
        return Err(RegistryError::DuplicateResource {
            resource: op.resource(),
        });
    }
    if let Some(path) = location.path()
        && let Some(existing) = first(
            conn,
            "SELECT id FROM storage_locations WHERE kind = ?1 AND path = ?2",
            params![location.kind().as_code(), path],
            "storage_locations",
            |cells| cells.parse::<StorageLocationId>("id"),
        )?
    {
        return Err(RegistryError::DuplicateResource {
            resource: ResourceRef::StorageLocation(existing),
        });
    }
    conn.execute(
        "INSERT INTO storage_locations (id, kind, path, built_in, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            location.id().to_string(),
            location.kind().as_code(),
            location.path(),
            i64::from(location.is_built_in()),
            ts(location.created_at()),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}
