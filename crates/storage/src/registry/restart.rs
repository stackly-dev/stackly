//! The daemon restart plan tables (spec 0030): `daemon_restart_plan` and
//! `daemon_restart_plan_instance`.
//!
//! The checks follow the order the `Registry` trait documents, the same as
//! the fake: existence, then the operation the plan refers to. The partial
//! unique index that allows one open plan stays the last guard, so a second
//! open plan ends as `Integrity`.

use rusqlite::{Connection, params};
use stackly_domain::{
    PlanInstance, RegistryError, ResourceRef, RestartPlan, RestartPlanId, StoredRestartPlan,
    WriteOp,
};

use super::column::{Cells, collect, corrupt, exists, first, opt_ts, sql_error, ts};

/// The plan columns every read selects.
const PLAN_COLUMNS: &str = "id, operation_id, state, digest, boot_id, server_instance_id_before, \
     server_instance_id_after, created_at, handed_over_at, reconciled_at, finished_at, close_reason";

/// `InsertRestartPlan`: a new ID, an existing operation, then the plan row
/// and one row per instance.
pub(super) fn insert_plan(
    conn: &Connection,
    plan: &RestartPlan,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    let stored = plan.stored();
    if exists(
        conn,
        "SELECT 1 FROM daemon_restart_plan WHERE id = ?1",
        [stored.id.to_string()],
    )? {
        return Err(RegistryError::DuplicateResource {
            resource: op.resource(),
        });
    }
    if !exists(
        conn,
        "SELECT 1 FROM operations WHERE id = ?1",
        [stored.operation.to_string()],
    )? {
        return Err(RegistryError::MissingReference {
            resource: ResourceRef::Operation(stored.operation),
        });
    }
    conn.execute(
        "INSERT INTO daemon_restart_plan (id, operation_id, state, digest, boot_id, \
         server_instance_id_before, server_instance_id_after, created_at, handed_over_at, \
         reconciled_at, finished_at, close_reason) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            stored.id.to_string(),
            stored.operation.to_string(),
            stored.state.as_code(),
            stored.digest.to_string(),
            stored.boot_id.to_string(),
            stored.server_instance_id_before.hyphenated().to_string(),
            stored
                .server_instance_id_after
                .map(|id| id.hyphenated().to_string()),
            ts(stored.created_at),
            opt_ts(stored.handed_over_at),
            opt_ts(stored.reconciled_at),
            opt_ts(stored.finished_at),
            stored.close_reason.map(|reason| reason.as_code()),
        ],
    )
    .map_err(sql_error)?;
    for entry in &stored.instances {
        conn.execute(
            "INSERT INTO daemon_restart_plan_instance (plan_id, instance_id, name, decision, \
             reason, stopped_at, ready_at, outcome, failure_code) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                stored.id.to_string(),
                entry.instance_id.to_string(),
                entry.name.as_str(),
                entry.decision.as_code(),
                entry.reason.as_code(),
                opt_ts(entry.stopped_at),
                opt_ts(entry.ready_at),
                entry.outcome.map(|outcome| outcome.as_code()),
                entry.failure_code.as_ref().map(|code| code.as_str()),
            ],
        )
        .map_err(sql_error)?;
    }
    Ok(())
}

/// `UpdateRestartPlan`: the plan exists, then its state columns and the
/// recorded fields of each instance row.
pub(super) fn update_plan(conn: &Connection, plan: &RestartPlan) -> Result<(), RegistryError> {
    let stored = plan.stored();
    let changed = conn
        .execute(
            "UPDATE daemon_restart_plan SET state = ?2, server_instance_id_after = ?3, \
             handed_over_at = ?4, reconciled_at = ?5, finished_at = ?6, close_reason = ?7 \
             WHERE id = ?1",
            params![
                stored.id.to_string(),
                stored.state.as_code(),
                stored
                    .server_instance_id_after
                    .map(|id| id.hyphenated().to_string()),
                opt_ts(stored.handed_over_at),
                opt_ts(stored.reconciled_at),
                opt_ts(stored.finished_at),
                stored.close_reason.map(|reason| reason.as_code()),
            ],
        )
        .map_err(sql_error)?;
    if changed == 0 {
        return Err(RegistryError::NotFound {
            resource: ResourceRef::RestartPlan(stored.id),
        });
    }
    for entry in &stored.instances {
        conn.execute(
            "UPDATE daemon_restart_plan_instance SET stopped_at = ?3, ready_at = ?4, \
             outcome = ?5, failure_code = ?6 WHERE plan_id = ?1 AND instance_id = ?2",
            params![
                stored.id.to_string(),
                entry.instance_id.to_string(),
                opt_ts(entry.stopped_at),
                opt_ts(entry.ready_at),
                entry.outcome.map(|outcome| outcome.as_code()),
                entry.failure_code.as_ref().map(|code| code.as_str()),
            ],
        )
        .map_err(sql_error)?;
    }
    Ok(())
}

/// The open plan, if any.
pub(super) fn open_plan(conn: &Connection) -> Result<Option<RestartPlan>, RegistryError> {
    plan_where(
        conn,
        "WHERE state IN ('stopping', 'handed_over', 'restoring')",
    )
}

/// The newest plan by `created_at` and then ID.
pub(super) fn latest_plan(conn: &Connection) -> Result<Option<RestartPlan>, RegistryError> {
    plan_where(conn, "ORDER BY created_at DESC, id DESC")
}

fn plan_where(conn: &Connection, tail: &str) -> Result<Option<RestartPlan>, RegistryError> {
    let sql = format!("SELECT {PLAN_COLUMNS} FROM daemon_restart_plan {tail} LIMIT 1");
    let Some(mut stored) = first(conn, &sql, [], "daemon_restart_plan", |cells| {
        plan_row(&cells)
    })?
    else {
        return Ok(None);
    };
    stored.instances = instances(conn, stored.id)?;
    RestartPlan::restore(stored)
        .map(Some)
        .map_err(|err| corrupt("daemon_restart_plan", "*", err))
}

fn plan_row(cells: &Cells<'_, '_>) -> Result<StoredRestartPlan, RegistryError> {
    Ok(StoredRestartPlan {
        id: cells.parse("id")?,
        operation: cells.parse("operation_id")?,
        state: cells.parse("state")?,
        digest: cells.parse("digest")?,
        boot_id: cells.parse("boot_id")?,
        server_instance_id_before: cells.parse("server_instance_id_before")?,
        server_instance_id_after: cells.opt_parse("server_instance_id_after")?,
        created_at: cells.ts("created_at")?,
        handed_over_at: cells.opt_ts("handed_over_at")?,
        reconciled_at: cells.opt_ts("reconciled_at")?,
        finished_at: cells.opt_ts("finished_at")?,
        close_reason: cells.opt_parse("close_reason")?,
        instances: Vec::new(),
    })
}

fn instances(conn: &Connection, plan: RestartPlanId) -> Result<Vec<PlanInstance>, RegistryError> {
    collect(
        conn,
        "SELECT instance_id, name, decision, reason, stopped_at, ready_at, outcome, failure_code \
         FROM daemon_restart_plan_instance WHERE plan_id = ?1 ORDER BY instance_id",
        [plan.to_string()],
        "daemon_restart_plan_instance",
        |cells| {
            Ok(PlanInstance {
                instance_id: cells.parse("instance_id")?,
                name: cells.parse("name")?,
                decision: cells.parse("decision")?,
                reason: cells.parse("reason")?,
                stopped_at: cells.opt_ts("stopped_at")?,
                ready_at: cells.opt_ts("ready_at")?,
                outcome: cells.opt_parse("outcome")?,
                failure_code: cells.opt_parse("failure_code")?,
            })
        },
    )
}
