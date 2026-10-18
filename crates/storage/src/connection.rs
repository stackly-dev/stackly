//! The four kinds of SQLite connection and their pragma sets
//! (spec 0005, "Open, migrasi, dan recovery").

use std::ffi::CStr;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::error::StorageError;

/// The VFS of the create connection. The creator holds an exclusive
/// `flock` on the temporary file, and on macOS a `flock` also blocks
/// SQLite's own `fcntl` locks, so the create connection takes no locks of
/// its own. Nothing but the lock holder ever opens that file.
const CREATE_VFS: &CStr = c"unix-none";

/// Busy timeout of the inspection and write connections in production.
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// A failure while opening the write connection.
#[derive(Debug)]
pub(crate) enum WriteOpenError {
    /// SQLite refused, for example because the file is locked.
    Sqlite(rusqlite::Error),
    /// A pragma did not take effect.
    Pragma(StorageError),
}

impl From<rusqlite::Error> for WriteOpenError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

/// Flags shared by every connection: no SQLite mutex (one thread owns
/// each connection), extended result codes, and never a symlink anywhere
/// in the path.
fn base_flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_EXRESCODE
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
}

/// Opens the read only connection that decides between ready and
/// recovery. It never writes the registry file or its WAL.
pub(crate) fn open_inspect(path: &Path, busy_timeout: Duration) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(path, base_flags() | OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.pragma_update(None, "query_only", true)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    conn.busy_timeout(busy_timeout)?;
    Ok(conn)
}

/// Opens the read only connection that verifies a snapshot.
pub(crate) fn open_verify(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(path, base_flags() | OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.pragma_update(None, "query_only", true)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    Ok(conn)
}

/// Opens the connection that builds a new registry in the temporary
/// create file: rollback journal, full sync, and foreign keys off while
/// the migrations run.
pub(crate) fn open_create(path: &Path) -> Result<Connection, StorageError> {
    let failed = |err: rusqlite::Error| StorageError::CreateFailed {
        detail: err.to_string(),
    };
    let conn = Connection::open_with_flags_and_vfs(
        path,
        base_flags() | OpenFlags::SQLITE_OPEN_READ_WRITE,
        CREATE_VFS,
    )
    .map_err(failed)?;
    let mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))
        .map_err(failed)?;
    expect_text("journal_mode", "delete", &mode)?;
    for (name, value) in [
        ("synchronous", 2),
        ("fullfsync", 1),
        ("foreign_keys", 0),
        ("trusted_schema", 0),
    ] {
        conn.pragma_update(None, name, value).map_err(failed)?;
        let actual = read_int(&conn, name).map_err(failed)?;
        expect_int(name, value, actual)?;
    }
    Ok(conn)
}

/// Opens the write connection of a registry that exists, sets its pragmas,
/// and reads every one of them back.
pub(crate) fn open_write(
    path: &Path,
    busy_timeout: Duration,
) -> Result<Connection, WriteOpenError> {
    let conn = Connection::open_with_flags(path, base_flags() | OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let busy_ms = i64::try_from(busy_timeout.as_millis()).unwrap_or(i64::MAX);
    conn.busy_timeout(busy_timeout)?;
    let settings = [
        ("trusted_schema", 0),
        ("foreign_keys", 1),
        ("synchronous", 2),
        ("fullfsync", 1),
        ("checkpoint_fullfsync", 1),
    ];
    for (name, value) in settings {
        conn.pragma_update(None, name, value)?;
    }
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    expect_text("journal_mode", "wal", &mode).map_err(WriteOpenError::Pragma)?;
    for (name, value) in settings.into_iter().chain([("busy_timeout", busy_ms)]) {
        let actual = read_int(&conn, name)?;
        expect_int(name, value, actual).map_err(WriteOpenError::Pragma)?;
    }
    let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    expect_text("journal_mode", "wal", &mode).map_err(WriteOpenError::Pragma)?;
    Ok(conn)
}

/// Switches foreign key enforcement and reads it back. Only takes effect
/// outside a transaction.
pub(crate) fn set_foreign_keys(conn: &Connection, on: bool) -> Result<(), WriteOpenError> {
    let value = i64::from(on);
    conn.pragma_update(None, "foreign_keys", value)?;
    let actual = read_int(conn, "foreign_keys")?;
    expect_int("foreign_keys", value, actual).map_err(WriteOpenError::Pragma)
}

/// Reads an integer pragma.
pub(crate) fn read_int(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, name, |row| row.get(0))
}

fn expect_int(name: &'static str, expected: i64, actual: i64) -> Result<(), StorageError> {
    if expected == actual {
        Ok(())
    } else {
        Err(StorageError::PragmaNotApplied {
            name,
            expected: expected.to_string(),
            actual: actual.to_string(),
        })
    }
}

fn expect_text(name: &'static str, expected: &str, actual: &str) -> Result<(), StorageError> {
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(StorageError::PragmaNotApplied {
            name,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}
