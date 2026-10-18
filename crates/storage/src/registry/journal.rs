//! The operation journal tables of spec 0007: `operation_journal`,
//! `operation_steps`, `operation_created`, and `operation_locks`, written
//! by the journal write ops and read back into the domain's journal types.
//!
//! Steps and created items are upserted with `ON CONFLICT DO UPDATE`,
//! never `INSERT OR REPLACE`, which would delete the row first and cascade
//! to the created items of a step. A unique rule on these tables (a step
//! name, a created item, a lock key) has no error of its own: the SQL
//! constraint refuses the write and it surfaces as `Integrity`, as in the
//! fake. Every journal read leaves out an operation without a journal row.

use rusqlite::{Connection, params};
use serde_json::{Map, Value};
use stackly_domain::{
    CreatedItem, CreatedRecord, CreatedSeq, DetailKey, FailureError, IdempotencyKey,
    JournalPayload, JournalStep, JournalView, LockKey, Operation, OperationId, OperationJournal,
    OperationTrigger, RegistryError, ResourceRef, StepSeq, StoredCreatedRecord, StoredJournal,
    StoredStep, WriteOp,
};

use super::column::{Cells, collect, exists, first, inconsistent, int, opt_ts, sql_error, ts};
use super::read::{OPERATION_COLUMNS, stored_operation};

const JOURNAL_COLUMNS: &str = "operation_id, method, request_fingerprint, trigger_origin, \
     trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, \
     point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, \
     failure_error_code, failure_message, failure_details, result";

const STEP_COLUMNS: &str = "seq, name, repeatable, reversible, state, attempts, checkpoint, \
     error_code, started_at, finished_at";

const CREATED_COLUMNS: &str =
    "seq, step_seq, item_kind, item_ref, payload, state, detail, recorded_at, settled_at";

// Writes.

/// Fails with `MissingReference` unless the operation exists.
fn expect_operation(conn: &Connection, id: OperationId) -> Result<(), RegistryError> {
    if exists(
        conn,
        "SELECT 1 FROM operations WHERE id = ?1",
        [id.to_string()],
    )? {
        Ok(())
    } else {
        Err(RegistryError::MissingReference {
            resource: ResourceRef::Operation(id),
        })
    }
}

fn has_journal(conn: &Connection, id: OperationId) -> Result<bool, RegistryError> {
    exists(
        conn,
        "SELECT 1 FROM operation_journal WHERE operation_id = ?1",
        [id.to_string()],
    )
}

/// `InsertJournal` and `UpdateJournal`: existence, then the operation.
pub(super) fn write_journal(
    conn: &Connection,
    journal: &OperationJournal,
    op: &WriteOp,
    update: bool,
) -> Result<(), RegistryError> {
    let id = journal.operation_id();
    match (has_journal(conn, id)?, update) {
        (true, false) => {
            return Err(RegistryError::DuplicateResource {
                resource: op.resource(),
            });
        }
        (false, true) => {
            return Err(RegistryError::NotFound {
                resource: op.resource(),
            });
        }
        _ => {}
    }
    expect_operation(conn, id)?;
    let error = journal.failure_error();
    let details = error.map_or_else(
        || Ok("[]".to_owned()),
        |error| details_text(error.details()),
    )?;
    let values = params![
        id.to_string(),
        journal.method().map(|method| method.as_str()),
        journal.fingerprint().map(ToString::to_string),
        journal.trigger().origin().as_code(),
        journal.trigger().client().map(|client| client.as_str()),
        journal.trigger().interactive(),
        journal.resumable(),
        journal.input().as_str(),
        journal
            .expected_revision()
            .map(|revision| int(revision.get(), "expected_revision"))
            .transpose()?,
        journal.waiting_on().map(|waiting| waiting.to_string()),
        opt_ts(journal.point_of_no_return_at()),
        opt_ts(journal.cancel_requested_at()),
        opt_ts(journal.compensation_started_at()),
        journal.recovery_cause().map(|cause| cause.as_code()),
        error.map(|error| error.code().as_str()),
        error.map(FailureError::message),
        details,
        journal.result().map(JournalPayload::as_str),
    ];
    let sql = if update {
        "UPDATE operation_journal SET method = ?2, request_fingerprint = ?3, trigger_origin = ?4, \
         trigger_client = ?5, trigger_interactive = ?6, resumable = ?7, input = ?8, \
         expected_revision = ?9, waiting_on = ?10, point_of_no_return_at = ?11, \
         cancel_requested_at = ?12, compensation_started_at = ?13, recovery_cause = ?14, \
         failure_error_code = ?15, failure_message = ?16, failure_details = ?17, result = ?18 \
         WHERE operation_id = ?1"
    } else {
        "INSERT INTO operation_journal (operation_id, method, request_fingerprint, \
         trigger_origin, trigger_client, trigger_interactive, resumable, input, \
         expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, \
         compensation_started_at, recovery_cause, failure_error_code, failure_message, \
         failure_details, result) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, \
         ?13, ?14, ?15, ?16, ?17, ?18)"
    };
    conn.execute(sql, values).map_err(sql_error)?;
    Ok(())
}

/// The stored form of failure details: `[{"key": ..., "value": ...}]`, in
/// order.
fn details_text(details: &[(DetailKey, String)]) -> Result<String, RegistryError> {
    let rows: Vec<Value> = details
        .iter()
        .map(|(key, value)| {
            let mut row = Map::new();
            row.insert("key".to_owned(), Value::String(key.to_string()));
            row.insert("value".to_owned(), Value::String(value.clone()));
            Value::Object(row)
        })
        .collect();
    serde_json::to_string(&rows).map_err(|err| RegistryError::Integrity {
        detail: format!("cannot encode failure_details: {err}"),
    })
}

/// `UpsertStep`: the operation, then the row, by `(operation_id, seq)`.
pub(super) fn upsert_step(
    conn: &Connection,
    operation: OperationId,
    step: &JournalStep,
) -> Result<(), RegistryError> {
    expect_operation(conn, operation)?;
    conn.execute(
        "INSERT INTO operation_steps (operation_id, seq, name, repeatable, reversible, state, \
         attempts, checkpoint, error_code, started_at, finished_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT (operation_id, seq) DO UPDATE SET name = excluded.name, \
         repeatable = excluded.repeatable, reversible = excluded.reversible, \
         state = excluded.state, attempts = excluded.attempts, \
         checkpoint = excluded.checkpoint, error_code = excluded.error_code, \
         started_at = excluded.started_at, finished_at = excluded.finished_at",
        params![
            operation.to_string(),
            step.seq().get(),
            step.name().as_str(),
            step.repeatable(),
            step.reversible(),
            step.state().as_code(),
            step.attempts(),
            step.checkpoint().map(JournalPayload::as_str),
            step.error_code().map(|code| code.as_str()),
            ts(step.started_at()),
            opt_ts(step.finished_at()),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `UpsertCreated`: the operation and its step, then the row, by
/// `(operation_id, seq)`.
pub(super) fn upsert_created(
    conn: &Connection,
    operation: OperationId,
    record: &CreatedRecord,
) -> Result<(), RegistryError> {
    expect_operation(conn, operation)?;
    if !exists(
        conn,
        "SELECT 1 FROM operation_steps WHERE operation_id = ?1 AND seq = ?2",
        params![operation.to_string(), record.step_seq().get()],
    )? {
        return Err(RegistryError::MissingReference {
            resource: ResourceRef::OperationStep(operation, record.step_seq()),
        });
    }
    conn.execute(
        "INSERT INTO operation_created (operation_id, seq, step_seq, item_kind, item_ref, \
         payload, state, detail, recorded_at, settled_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT (operation_id, seq) DO UPDATE SET step_seq = excluded.step_seq, \
         item_kind = excluded.item_kind, item_ref = excluded.item_ref, \
         payload = excluded.payload, state = excluded.state, detail = excluded.detail, \
         recorded_at = excluded.recorded_at, settled_at = excluded.settled_at",
        params![
            operation.to_string(),
            record.seq().get(),
            record.step_seq().get(),
            record.item().kind_code(),
            record.item().item_ref(),
            record.payload().map(JournalPayload::as_str),
            record.state().as_code(),
            record.detail().map(|detail| detail.as_str()),
            ts(record.recorded_at()),
            opt_ts(record.settled_at()),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `InsertLocks`: the operation, then one row per key. `check_shape`
/// already refused a key with no text form.
pub(super) fn insert_locks(
    conn: &Connection,
    operation: OperationId,
    keys: &[LockKey],
) -> Result<(), RegistryError> {
    expect_operation(conn, operation)?;
    let id = operation.to_string();
    for key in keys {
        conn.execute(
            "INSERT INTO operation_locks (operation_id, lock_key) VALUES (?1, ?2)",
            params![id, key.to_string()],
        )
        .map_err(sql_error)?;
    }
    Ok(())
}

/// `DeleteLocks`: the operation, then every key it recorded.
pub(super) fn delete_locks(conn: &Connection, operation: OperationId) -> Result<(), RegistryError> {
    expect_operation(conn, operation)?;
    conn.execute(
        "DELETE FROM operation_locks WHERE operation_id = ?1",
        [operation.to_string()],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// The unique rule of an operation's idempotency key: no other operation
/// holds it. The unique index is the last guard.
pub(super) fn check_idempotency_key(
    conn: &Connection,
    operation: &Operation,
) -> Result<(), RegistryError> {
    let Some(key) = operation.idempotency_key() else {
        return Ok(());
    };
    let existing = first(
        conn,
        "SELECT id FROM operations WHERE idempotency_key = ?1 AND id <> ?2",
        params![key.as_str(), operation.id().to_string()],
        "operations",
        |cells| cells.parse::<OperationId>("id"),
    )?;
    match existing {
        Some(existing) => Err(RegistryError::IdempotencyKeyTaken {
            key: key.clone(),
            existing,
        }),
        None => Ok(()),
    }
}

// Reads.

/// The journal row of an operation, without its other rows.
pub(super) fn journal(
    conn: &Connection,
    id: OperationId,
) -> Result<Option<OperationJournal>, RegistryError> {
    first(
        conn,
        &format!("SELECT {JOURNAL_COLUMNS} FROM operation_journal WHERE operation_id = ?1"),
        [id.to_string()],
        "operation_journal",
        stored_journal,
    )
}

/// The journal rows of `ids`, in their order; an ID without one is left
/// out.
pub(super) fn journals(
    conn: &Connection,
    ids: &[OperationId],
) -> Result<Vec<OperationJournal>, RegistryError> {
    let mut journals = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(journal) = journal(conn, *id)? {
            journals.push(journal);
        }
    }
    Ok(journals)
}

/// The journal of an operation with its steps and created items by `seq`
/// and its lock keys by text.
pub(super) fn journal_view(
    conn: &Connection,
    id: OperationId,
) -> Result<Option<JournalView>, RegistryError> {
    let Some(journal) = journal(conn, id)? else {
        return Ok(None);
    };
    let id = id.to_string();
    let steps = collect(
        conn,
        &format!("SELECT {STEP_COLUMNS} FROM operation_steps WHERE operation_id = ?1 ORDER BY seq"),
        [&id],
        "operation_steps",
        stored_step,
    )?;
    let created = collect(
        conn,
        &format!(
            "SELECT {CREATED_COLUMNS} FROM operation_created WHERE operation_id = ?1 ORDER BY seq"
        ),
        [&id],
        "operation_created",
        stored_created,
    )?;
    let locks = collect(
        conn,
        "SELECT lock_key FROM operation_locks WHERE operation_id = ?1 ORDER BY lock_key",
        [&id],
        "operation_locks",
        |cells| cells.parse::<LockKey>("lock_key"),
    )?;
    Ok(Some(JournalView::new(journal, steps, created, locks)))
}

/// The operation holding `key`, with its journal row.
pub(super) fn by_idempotency_key(
    conn: &Connection,
    key: &IdempotencyKey,
) -> Result<Option<(Operation, OperationJournal)>, RegistryError> {
    let operation = first(
        conn,
        &format!("SELECT {OPERATION_COLUMNS} FROM operations WHERE idempotency_key = ?1"),
        [key.as_str()],
        "operations",
        stored_operation,
    )?;
    let Some(operation) = operation else {
        return Ok(None);
    };
    Ok(journal(conn, operation.id())?.map(|journal| (operation, journal)))
}

/// Every operation not yet final, other than `succeeded`, `failed`, and
/// `cancelled`, that has a journal, oldest first, with its journal.
pub(super) fn unfinished(
    conn: &Connection,
) -> Result<Vec<(Operation, JournalView)>, RegistryError> {
    let operations = collect(
        conn,
        &format!(
            "SELECT {OPERATION_COLUMNS} FROM operations \
             WHERE state IN ('queued', 'running', 'cancel_requested', 'needs_recovery') \
             AND EXISTS (SELECT 1 FROM operation_journal WHERE operation_id = operations.id) \
             ORDER BY created_at, id"
        ),
        [],
        "operations",
        stored_operation,
    )?;
    let mut unfinished = Vec::with_capacity(operations.len());
    for operation in operations {
        if let Some(view) = journal_view(conn, operation.id())? {
            unfinished.push((operation, view));
        }
    }
    Ok(unfinished)
}

fn payload(
    cells: Cells<'_, '_>,
    column: &'static str,
    max: usize,
) -> Result<Option<JournalPayload>, RegistryError> {
    cells
        .opt_text(column)?
        .map(|text| JournalPayload::new(text, max).map_err(|err| cells.corrupt(column, err)))
        .transpose()
}

fn number<T>(
    cells: Cells<'_, '_>,
    column: &'static str,
    make: impl FnOnce(u16) -> Result<T, stackly_domain::ValueError>,
) -> Result<T, RegistryError> {
    let value = cells
        .opt_u64(column)?
        .ok_or_else(|| cells.corrupt(column, "is null"))?;
    u16::try_from(value)
        .map_err(|_| cells.corrupt(column, "is out of range"))
        .and_then(|value| make(value).map_err(|err| cells.corrupt(column, err)))
}

fn flag(cells: Cells<'_, '_>, column: &'static str) -> Result<bool, RegistryError> {
    cells
        .opt_bool(column)?
        .ok_or_else(|| cells.corrupt(column, "is null"))
}

fn stored_journal(cells: Cells<'_, '_>) -> Result<OperationJournal, RegistryError> {
    let failure_error = match (
        cells.opt_parse("failure_error_code")?,
        cells.opt_text("failure_message")?,
    ) {
        (None, None) => None,
        (Some(code), Some(message)) => {
            let details = stored_details(cells)?;
            Some(
                FailureError::new(code, message, details)
                    .map_err(|err| cells.corrupt("failure_message", err))?,
            )
        }
        _ => {
            return Err(cells.corrupt(
                "failure_message",
                "is set without failure_error_code, or the other way round",
            ));
        }
    };
    let stored = StoredJournal {
        operation_id: cells.parse("operation_id")?,
        method: cells.opt_parse("method")?,
        fingerprint: cells.opt_parse("request_fingerprint")?,
        trigger: OperationTrigger::new(
            cells.parse("trigger_origin")?,
            cells.opt_parse("trigger_client")?,
            flag(cells, "trigger_interactive")?,
        ),
        resumable: flag(cells, "resumable")?,
        input: payload(cells, "input", OperationJournal::INPUT_MAX)?
            .ok_or_else(|| cells.corrupt("input", "is null"))?,
        expected_revision: cells
            .opt_u64("expected_revision")?
            .map(|value| {
                stackly_domain::ResourceRevision::new(value)
                    .map_err(|err| cells.corrupt("expected_revision", err))
            })
            .transpose()?,
        waiting_on: cells.opt_parse("waiting_on")?,
        point_of_no_return_at: cells.opt_ts("point_of_no_return_at")?,
        cancel_requested_at: cells.opt_ts("cancel_requested_at")?,
        compensation_started_at: cells.opt_ts("compensation_started_at")?,
        recovery_cause: cells.opt_parse("recovery_cause")?,
        failure_error,
        result: payload(cells, "result", OperationJournal::RESULT_MAX)?,
    };
    OperationJournal::restore(stored).map_err(|err| inconsistent("operation_journal", &err))
}

fn stored_details(cells: Cells<'_, '_>) -> Result<Vec<(DetailKey, String)>, RegistryError> {
    let malformed = || cells.corrupt("failure_details", "is not a list of key and value objects");
    let text = cells.text("failure_details")?;
    let Value::Array(rows) = serde_json::from_str(&text).map_err(|_| malformed())? else {
        return Err(malformed());
    };
    rows.into_iter()
        .map(|row| {
            let Value::Object(mut row) = row else {
                return Err(malformed());
            };
            match (row.remove("key"), row.remove("value")) {
                (Some(Value::String(key)), Some(Value::String(value))) if row.is_empty() => {
                    let key = key
                        .parse()
                        .map_err(|err| cells.corrupt("failure_details", err))?;
                    Ok((key, value))
                }
                _ => Err(malformed()),
            }
        })
        .collect()
}

fn stored_step(cells: Cells<'_, '_>) -> Result<JournalStep, RegistryError> {
    let attempts = cells
        .opt_u64("attempts")?
        .ok_or_else(|| cells.corrupt("attempts", "is null"))?;
    let stored = StoredStep {
        seq: number(cells, "seq", StepSeq::new)?,
        name: cells.parse("name")?,
        repeatable: flag(cells, "repeatable")?,
        reversible: flag(cells, "reversible")?,
        state: cells.parse("state")?,
        attempts: u32::try_from(attempts).map_err(|_| cells.corrupt("attempts", "is too large"))?,
        checkpoint: payload(cells, "checkpoint", JournalStep::CHECKPOINT_MAX)?,
        error_code: cells.opt_parse("error_code")?,
        started_at: cells.ts("started_at")?,
        finished_at: cells.opt_ts("finished_at")?,
    };
    JournalStep::restore(stored).map_err(|err| inconsistent("operation_steps", &err))
}

fn stored_created(cells: Cells<'_, '_>) -> Result<CreatedRecord, RegistryError> {
    let item = CreatedItem::from_parts(&cells.text("item_kind")?, &cells.text("item_ref")?)
        .map_err(|err| cells.corrupt("item_kind", err))?;
    let stored = StoredCreatedRecord {
        seq: number(cells, "seq", CreatedSeq::new)?,
        step_seq: number(cells, "step_seq", StepSeq::new)?,
        item,
        payload: payload(cells, "payload", CreatedRecord::PAYLOAD_MAX)?,
        state: cells.parse("state")?,
        detail: cells.opt_parse("detail")?,
        recorded_at: cells.ts("recorded_at")?,
        settled_at: cells.opt_ts("settled_at")?,
    };
    CreatedRecord::restore(stored).map_err(|err| inconsistent("operation_created", &err))
}
