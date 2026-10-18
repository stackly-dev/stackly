//! Single columns: SQLite values to domain values and back, and SQLite
//! errors onto `RegistryError`.
//!
//! A value that cannot be read becomes `Corrupt { table, column, detail }`.
//! The detail names what is wrong, never the stored value, because it ends
//! up in logs.

use std::fmt::Display;
use std::str::FromStr;

use jiff::Timestamp;
use rusqlite::{ErrorCode, OptionalExtension, Params, Row};
use stackly_domain::{InvariantError, Port, RegistryError, ResourceRevision};

use crate::time::{format_timestamp, parse_timestamp};

/// Maps a SQLite error: `SQLITE_FULL` to `NoSpace`, `SQLITE_BUSY` to
/// `Busy`, a constraint to `Integrity`, and anything else to `Io`.
pub(crate) fn sql_error(err: rusqlite::Error) -> RegistryError {
    match err.sqlite_error_code() {
        Some(ErrorCode::DiskFull) => RegistryError::NoSpace,
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => RegistryError::Busy,
        Some(ErrorCode::ConstraintViolation) => RegistryError::Integrity {
            detail: err.to_string(),
        },
        _ => RegistryError::Io {
            detail: err.to_string(),
        },
    }
}

/// A stored value that cannot be read.
pub(super) fn corrupt(
    table: &'static str,
    column: &'static str,
    detail: impl Display,
) -> RegistryError {
    RegistryError::Corrupt {
        table,
        column,
        detail: detail.to_string(),
    }
}

/// A stored row whose values break an invariant between fields, which a
/// domain `restore` constructor found. `*` names the whole row.
pub(super) fn inconsistent(table: &'static str, err: &InvariantError) -> RegistryError {
    corrupt(table, "*", err)
}

/// The cells of one row, read by column name. Every getter turns a missing,
/// mistyped, or invalid value into `Corrupt`.
#[derive(Clone, Copy)]
pub(super) struct Cells<'r, 's> {
    row: &'r Row<'s>,
    table: &'static str,
}

impl<'r, 's> Cells<'r, 's> {
    /// Reads `row` as a row of `table`.
    pub(super) const fn new(row: &'r Row<'s>, table: &'static str) -> Self {
        Self { row, table }
    }

    /// The same row, with the columns that follow named after `table`, for
    /// columns a join brought in.
    pub(super) const fn of(self, table: &'static str) -> Self {
        Self { table, ..self }
    }

    pub(super) fn corrupt(&self, column: &'static str, detail: impl Display) -> RegistryError {
        corrupt(self.table, column, detail)
    }

    pub(super) fn opt_text(&self, column: &'static str) -> Result<Option<String>, RegistryError> {
        self.row
            .get::<_, Option<String>>(column)
            .map_err(|err| self.corrupt(column, err))
    }

    pub(super) fn text(&self, column: &'static str) -> Result<String, RegistryError> {
        self.opt_text(column)?
            .ok_or_else(|| self.corrupt(column, "is null"))
    }

    pub(super) fn opt_parse<T>(&self, column: &'static str) -> Result<Option<T>, RegistryError>
    where
        T: FromStr,
        T::Err: Display,
    {
        self.opt_text(column)?
            .map(|text| text.parse().map_err(|err| self.corrupt(column, err)))
            .transpose()
    }

    pub(super) fn parse<T>(&self, column: &'static str) -> Result<T, RegistryError>
    where
        T: FromStr,
        T::Err: Display,
    {
        self.opt_parse(column)?
            .ok_or_else(|| self.corrupt(column, "is null"))
    }

    pub(super) fn opt_ts(&self, column: &'static str) -> Result<Option<Timestamp>, RegistryError> {
        self.opt_text(column)?
            .map(|text| {
                parse_timestamp(&text)
                    .ok_or_else(|| self.corrupt(column, "is not a 30 character UTC timestamp"))
            })
            .transpose()
    }

    pub(super) fn ts(&self, column: &'static str) -> Result<Timestamp, RegistryError> {
        self.opt_ts(column)?
            .ok_or_else(|| self.corrupt(column, "is null"))
    }

    pub(super) fn opt_int(&self, column: &'static str) -> Result<Option<i64>, RegistryError> {
        self.row
            .get::<_, Option<i64>>(column)
            .map_err(|err| self.corrupt(column, err))
    }

    pub(super) fn opt_u64(&self, column: &'static str) -> Result<Option<u64>, RegistryError> {
        self.opt_int(column)?
            .map(|value| u64::try_from(value).map_err(|_| self.corrupt(column, "is negative")))
            .transpose()
    }

    pub(super) fn opt_bool(&self, column: &'static str) -> Result<Option<bool>, RegistryError> {
        match self.opt_int(column)? {
            None => Ok(None),
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(_) => Err(self.corrupt(column, "is not 0 or 1")),
        }
    }

    pub(super) fn opt_port(&self, column: &'static str) -> Result<Option<Port>, RegistryError> {
        self.opt_int(column)?
            .map(|value| {
                u16::try_from(value)
                    .ok()
                    .and_then(|port| Port::new(port).ok())
                    .ok_or_else(|| self.corrupt(column, "is not a port from 1 to 65535"))
            })
            .transpose()
    }

    pub(super) fn revision(&self) -> Result<ResourceRevision, RegistryError> {
        let value = self
            .opt_u64("revision")?
            .ok_or_else(|| self.corrupt("revision", "is null"))?;
        ResourceRevision::new(value).map_err(|err| self.corrupt("revision", err))
    }
}

/// Runs a query and maps every row it returns.
pub(super) fn collect<T>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl Params,
    table: &'static str,
    mut map: impl FnMut(Cells<'_, '_>) -> Result<T, RegistryError>,
) -> Result<Vec<T>, RegistryError> {
    let mut stmt = conn.prepare(sql).map_err(sql_error)?;
    let mut rows = stmt.query(params).map_err(sql_error)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        out.push(map(Cells::new(row, table))?);
    }
    Ok(out)
}

/// Runs a query for at most one row and maps it.
pub(super) fn first<T>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl Params,
    table: &'static str,
    map: impl FnOnce(Cells<'_, '_>) -> Result<T, RegistryError>,
) -> Result<Option<T>, RegistryError> {
    let mut stmt = conn.prepare(sql).map_err(sql_error)?;
    let mut rows = stmt.query(params).map_err(sql_error)?;
    match rows.next().map_err(sql_error)? {
        Some(row) => map(Cells::new(row, table)).map(Some),
        None => Ok(None),
    }
}

/// Returns whether a query finds a row.
pub(super) fn exists(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl Params,
) -> Result<bool, RegistryError> {
    let mut stmt = conn.prepare(sql).map_err(sql_error)?;
    stmt.query_row(params, |_| Ok(()))
        .optional()
        .map(|found| found.is_some())
        .map_err(sql_error)
}

/// The stored text of a timestamp.
pub(super) fn ts(at: Timestamp) -> String {
    format_timestamp(at)
}

/// The stored text of an optional timestamp.
pub(super) fn opt_ts(at: Option<Timestamp>) -> Option<String> {
    at.map(format_timestamp)
}

/// A number the domain keeps as `u64` that SQLite stores as `i64`. A value
/// above `i64::MAX` cannot be stored, which no SQL constraint would catch.
pub(super) fn int(value: u64, column: &'static str) -> Result<i64, RegistryError> {
    i64::try_from(value).map_err(|_| RegistryError::Integrity {
        detail: format!("{column} is larger than the registry can store"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_errors_map_onto_the_registry_error_categories() {
        let failure = |code| rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None);

        assert_eq!(
            sql_error(failure(rusqlite::ffi::SQLITE_FULL)),
            RegistryError::NoSpace
        );
        assert_eq!(
            sql_error(failure(rusqlite::ffi::SQLITE_BUSY)),
            RegistryError::Busy
        );
        assert_eq!(
            sql_error(failure(rusqlite::ffi::SQLITE_LOCKED)),
            RegistryError::Busy
        );
        assert!(matches!(
            sql_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK)),
            RegistryError::Integrity { .. }
        ));
        assert!(matches!(
            sql_error(failure(rusqlite::ffi::SQLITE_IOERR)),
            RegistryError::Io { .. }
        ));
    }

    #[test]
    fn a_count_above_i64_max_cannot_be_stored() {
        assert_eq!(int(7, "progress_done"), Ok(7));
        assert!(matches!(
            int(u64::MAX, "progress_done"),
            Err(RegistryError::Integrity { detail }) if detail.contains("progress_done")
        ));
    }
}
