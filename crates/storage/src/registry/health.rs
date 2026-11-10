//! The registry's `PRAGMA quick_check`, for the `registry` check of the
//! doctor (spec 0034 AC-8), and the job helper it shares with the scan of
//! [`coded`](super::coded).
//!
//! Each check runs as one job on the registry thread, in one read
//! transaction, with a progress handler that interrupts the job's own
//! statements once its stop flag is set. The registry has one connection
//! for every job, so an interrupt handle could cut whatever job runs when
//! it fires; the handler instead is installed for this job only and
//! removed before the next one runs on the connection.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, Transaction};
use stackly_domain::{QUICK_CHECK_MESSAGES_MAX, QuickCheck, RegistryError};

use super::column::sql_error;

/// How many SQLite virtual machine steps pass between two looks at a stop
/// flag.
const PROGRESS_STEPS: i32 = 1_000;

/// Runs `PRAGMA quick_check`, keeping at most
/// [`QUICK_CHECK_MESSAGES_MAX`] messages.
pub(super) fn quick_check(
    conn: &mut Connection,
    stop: Arc<AtomicBool>,
) -> Result<QuickCheck, RegistryError> {
    interruptible(conn, stop, |tx| {
        let mut statement = tx
            .prepare(&format!("PRAGMA quick_check({QUICK_CHECK_MESSAGES_MAX})"))
            .map_err(sql_error)?;
        let messages = statement
            .query_map([], |row| row.get::<_, String>(0))
            .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
            .map_err(sql_error)?;
        Ok(QuickCheck::from_messages(messages))
    })
}

/// Runs `job` in one read transaction with a progress handler that
/// interrupts its statements once `stop` is set, then removes the handler,
/// so no later job on the connection can be cut. An error while `stop` is
/// set is the interruption.
pub(super) fn interruptible<R>(
    conn: &mut Connection,
    stop: Arc<AtomicBool>,
    job: impl FnOnce(&Transaction<'_>) -> Result<R, RegistryError>,
) -> Result<R, RegistryError> {
    let watched = Arc::clone(&stop);
    conn.progress_handler(
        PROGRESS_STEPS,
        Some(move || watched.load(Ordering::Relaxed)),
    )
    .map_err(sql_error)?;
    let result = run_read(conn, job);
    let removed = conn
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(sql_error);
    let value = result.map_err(|err| {
        if stop.load(Ordering::Relaxed) {
            interrupted()
        } else {
            err
        }
    })?;
    removed?;
    Ok(value)
}

/// The error of a check that its stop flag interrupted.
pub(super) fn interrupted() -> RegistryError {
    RegistryError::Io {
        detail: "the registry check was interrupted by its stop flag".to_owned(),
    }
}

fn run_read<R>(
    conn: &mut Connection,
    job: impl FnOnce(&Transaction<'_>) -> Result<R, RegistryError>,
) -> Result<R, RegistryError> {
    let tx = conn.transaction().map_err(sql_error)?;
    let value = job(&tx)?;
    tx.commit().map_err(sql_error)?;
    Ok(value)
}
