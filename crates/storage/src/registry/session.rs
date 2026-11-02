//! The login session table (spec 0031): `daemon_sessions`.
//!
//! The checks follow the order the `Registry` trait documents, the same as
//! the fake: existence first. The partial unique index that allows one open
//! row stays the last guard, so a second open row ends as `Integrity`.

use rusqlite::{Connection, params};
use stackly_domain::{
    DaemonSession, RegistryError, ResourceRef, SessionEndReason, SessionId, StoredSession, WriteOp,
};

use super::column::{Cells, collect, corrupt, exists, sql_error, ts};

/// The session columns every read selects.
const SESSION_COLUMNS: &str = "id, boot_id, audit_session, started_at, ended_at, end_reason";

/// `InsertSession`: a new ID, then the row.
pub(super) fn insert(
    conn: &Connection,
    session: &DaemonSession,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    if exists(
        conn,
        "SELECT 1 FROM daemon_sessions WHERE id = ?1",
        [session.id().as_str()],
    )? {
        return Err(RegistryError::DuplicateResource {
            resource: op.resource(),
        });
    }
    let (ended_at, end_reason) = match session.ended() {
        Some((at, reason)) => (Some(ts(at)), Some(reason.as_code())),
        None => (None, None),
    };
    conn.execute(
        "INSERT INTO daemon_sessions (id, boot_id, audit_session, started_at, ended_at, \
         end_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            session.id().as_str(),
            session.boot_id().to_string(),
            i64::from(session.audit_session()),
            ts(session.started_at()),
            ended_at,
            end_reason,
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `EndSession`: the row exists, then its end columns.
pub(super) fn end(
    conn: &Connection,
    id: &SessionId,
    at: jiff::Timestamp,
    reason: SessionEndReason,
) -> Result<(), RegistryError> {
    let changed = conn
        .execute(
            "UPDATE daemon_sessions SET ended_at = ?2, end_reason = ?3 WHERE id = ?1",
            params![id.as_str(), ts(at), reason.as_code()],
        )
        .map_err(sql_error)?;
    require_changed(changed, id)
}

/// `ReopenSession`: the row exists, then its end columns are cleared.
pub(super) fn reopen(conn: &Connection, id: &SessionId) -> Result<(), RegistryError> {
    let changed = conn
        .execute(
            "UPDATE daemon_sessions SET ended_at = NULL, end_reason = NULL WHERE id = ?1",
            [id.as_str()],
        )
        .map_err(sql_error)?;
    require_changed(changed, id)
}

fn require_changed(changed: usize, id: &SessionId) -> Result<(), RegistryError> {
    if changed == 0 {
        return Err(RegistryError::NotFound {
            resource: ResourceRef::Session(id.clone()),
        });
    }
    Ok(())
}

/// `PruneSessions`: the oldest rows beyond `keep`, then the `released`
/// manual and startup leases whose session is not among the kept rows.
/// Leases without a session (manual leases of a manual daemon) stay.
pub(super) fn prune(conn: &Connection, keep: u32) -> Result<(), RegistryError> {
    conn.execute(
        "DELETE FROM daemon_sessions WHERE id NOT IN \
         (SELECT id FROM daemon_sessions ORDER BY started_at DESC, id DESC LIMIT ?1)",
        [i64::from(keep)],
    )
    .map_err(sql_error)?;
    conn.execute(
        "DELETE FROM instance_demands WHERE state = 'released' \
         AND source_kind IN ('manual', 'startup') AND session_id IS NOT NULL \
         AND session_id NOT IN (SELECT id FROM daemon_sessions)",
        [],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// The newest `limit` sessions, newest first.
pub(super) fn recent(conn: &Connection, limit: u32) -> Result<Vec<DaemonSession>, RegistryError> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM daemon_sessions ORDER BY started_at DESC, id DESC LIMIT ?1"
    );
    collect(conn, &sql, [i64::from(limit)], "daemon_sessions", |cells| {
        session_row(&cells)
    })
}

/// The newest session, open or closed.
pub(super) fn latest(conn: &Connection) -> Result<Option<DaemonSession>, RegistryError> {
    Ok(recent(conn, 1)?.into_iter().next())
}

fn session_row(cells: &Cells<'_, '_>) -> Result<DaemonSession, RegistryError> {
    let audit = cells
        .opt_int("audit_session")?
        .ok_or_else(|| cells.corrupt("audit_session", "is null"))?;
    let audit_session = u32::try_from(audit)
        .map_err(|_| cells.corrupt("audit_session", "does not fit a 32 bit unsigned number"))?;
    let ended = match (cells.opt_ts("ended_at")?, cells.opt_parse("end_reason")?) {
        (Some(at), Some(reason)) => Some((at, reason)),
        (None, None) => None,
        _ => return Err(cells.corrupt("ended_at", "does not match end_reason")),
    };
    let stored = StoredSession {
        id: cells.parse("id")?,
        boot_id: cells.parse("boot_id")?,
        audit_session,
        started_at: cells.ts("started_at")?,
        ended,
    };
    DaemonSession::restore(stored).map_err(|err| corrupt("daemon_sessions", "*", err))
}
