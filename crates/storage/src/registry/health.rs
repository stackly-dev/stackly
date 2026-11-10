//! The registry's own health, for the `registry` check of the doctor (spec
//! 0034 AC-8): `PRAGMA quick_check`, and the scan for stored values of an
//! open code set that this build does not know (*Pindai kode tidak
//! dikenal*).
//!
//! [`CODED_COLUMNS`] names every column of an open code set (spec 0005: a
//! `code` CHECK, no `IN` list) that a row mapper reads through a set of
//! known codes. A value outside that set makes every read that loads its
//! row fail as `Corrupt`, and the scan finds it. The storage tests keep
//! the list whole: every column of the schema with a `code` CHECK is either
//! here or named as one whose values no known set limits, and every entry
//! is checked against the mapper in both directions.
//!
//! Both checks run as one job on the registry thread, in one read
//! transaction, with a progress handler that interrupts the job's own
//! statements once its stop flag is set. Only this job's statements can be
//! cut: the handler is installed for the job and removed before the next
//! one runs on the connection.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, Transaction};
use stackly_domain::{
    AuditActor, AuditResult, ConfigRevisionOrigin, ConfigRevisionStatus, OperationKind,
    ProviderSourceKind, ProviderSourceStatus, QUICK_CHECK_MESSAGES_MAX, QuickCheck, RecoveryCause,
    RecoveryHint, RegistryError, UNRECOGNIZED_ROWS_PER_COLUMN, UnrecognizedValue,
};

use super::column::sql_error;

/// How many SQLite virtual machine steps pass between two looks at a stop
/// flag.
const PROGRESS_STEPS: i32 = 1_000;

/// One column of an open code set that a row mapper reads through a set of
/// known codes.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct CodedColumn {
    /// The table, for example `operations`.
    pub table: &'static str,
    /// The column, for example `kind`.
    pub column: &'static str,
    /// The codes this build reads in the column; any other stored value
    /// makes the read of its row fail as `Corrupt`.
    pub known: fn() -> &'static [&'static str],
}

/// The codes of a `code_enum` of `stackly-domain`, in its order: exactly
/// what its `FromStr`, and so the row mapper, accepts.
macro_rules! codes_of {
    ($ty:ty) => {{
        const CODES: [&str; <$ty>::ALL.len()] = {
            let mut codes = [""; <$ty>::ALL.len()];
            let mut index = 0;
            while index < codes.len() {
                codes[index] = <$ty>::ALL[index].as_code();
                index += 1;
            }
            codes
        };
        &CODES
    }};
}

/// Every column of an open code set that a row mapper reads through a set
/// of known codes, ordered by table and column.
///
/// Most sets are the codes of a domain `code_enum`. Two are matched by hand
/// in their mapper, and their lists here must change with it:
/// `audit_events.resource_kind` (`AuditResource::kind_code`, read in
/// `stored_audit`) and `operation_created.item_kind`
/// (`CreatedItem::from_parts`, read in `stored_created`).
pub const CODED_COLUMNS: &[CodedColumn] = &[
    CodedColumn {
        table: "audit_events",
        column: "actor",
        known: || codes_of!(AuditActor),
    },
    CodedColumn {
        table: "audit_events",
        column: "resource_kind",
        known: || &["secret"],
    },
    CodedColumn {
        table: "audit_events",
        column: "result",
        known: || codes_of!(AuditResult),
    },
    CodedColumn {
        table: "config_revisions",
        column: "origin",
        known: || codes_of!(ConfigRevisionOrigin),
    },
    CodedColumn {
        table: "config_revisions",
        column: "status",
        known: || codes_of!(ConfigRevisionStatus),
    },
    CodedColumn {
        table: "operation_created",
        column: "item_kind",
        known: || &["instance_tree", "environment_tree", "secret_ref", "custom"],
    },
    CodedColumn {
        table: "operation_journal",
        column: "recovery_cause",
        known: || codes_of!(RecoveryCause),
    },
    CodedColumn {
        table: "operations",
        column: "kind",
        known: || codes_of!(OperationKind),
    },
    CodedColumn {
        table: "operations",
        column: "recovery_hint",
        known: || codes_of!(RecoveryHint),
    },
    CodedColumn {
        table: "provider_sources",
        column: "kind",
        known: || codes_of!(ProviderSourceKind),
    },
    CodedColumn {
        table: "provider_sources",
        column: "status",
        known: || codes_of!(ProviderSourceStatus),
    },
];

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

/// Lists the stored values outside the known set of every
/// [`CODED_COLUMNS`] entry, at most [`UNRECOGNIZED_ROWS_PER_COLUMN`] per
/// column, in the order of the list and then by `rowid`.
pub(super) fn scan_unrecognized(
    conn: &mut Connection,
    stop: Arc<AtomicBool>,
) -> Result<Vec<UnrecognizedValue>, RegistryError> {
    let watched = Arc::clone(&stop);
    interruptible(conn, stop, move |tx| {
        let mut found = Vec::new();
        for entry in CODED_COLUMNS {
            // A column's statement may be too short for the progress
            // handler to run, so the flag is also read between columns.
            if watched.load(Ordering::Relaxed) {
                return Err(interrupted());
            }
            scan_column(tx, entry, &mut found)?;
        }
        Ok(found)
    })
}

/// Runs `job` in one read transaction with a progress handler that
/// interrupts its statements once `stop` is set, then removes the handler,
/// so no later job on the connection can be cut. An error while `stop` is
/// set is the interruption.
fn interruptible<R>(
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

fn run_read<R>(
    conn: &mut Connection,
    job: impl FnOnce(&Transaction<'_>) -> Result<R, RegistryError>,
) -> Result<R, RegistryError> {
    let tx = conn.transaction().map_err(sql_error)?;
    let value = job(&tx)?;
    tx.commit().map_err(sql_error)?;
    Ok(value)
}

fn scan_column(
    tx: &Transaction<'_>,
    entry: &CodedColumn,
    found: &mut Vec<UnrecognizedValue>,
) -> Result<(), RegistryError> {
    let known = (entry.known)();
    let placeholders = vec!["?"; known.len()].join(", ");
    // The table and column come from the constant list, never from input.
    let sql = format!(
        "SELECT rowid, {column} FROM {table} WHERE {column} NOT IN ({placeholders}) \
         ORDER BY rowid LIMIT {UNRECOGNIZED_ROWS_PER_COLUMN}",
        table = entry.table,
        column = entry.column,
    );
    let mut statement = tx.prepare(&sql).map_err(sql_error)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(known))
        .map_err(sql_error)?;
    while let Some(row) = rows.next().map_err(sql_error)? {
        let row_id: i64 = row.get(0).map_err(sql_error)?;
        // Every coded table is STRICT, so a stored value is text; any
        // other type is named rather than shown.
        let value = match row.get_ref(1).map_err(sql_error)? {
            ValueRef::Null => continue,
            ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            other => format!("<{}>", other.data_type()),
        };
        found.push(UnrecognizedValue::new(
            entry.table,
            row_id,
            entry.column,
            &value,
        ));
    }
    Ok(())
}

fn interrupted() -> RegistryError {
    RegistryError::Io {
        detail: "the registry check was interrupted by its stop flag".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_entry_has_known_codes_without_duplicates_and_the_list_is_ordered() {
        let mut previous = None;
        for entry in CODED_COLUMNS {
            let known = (entry.known)();
            assert!(!known.is_empty(), "{}.{}", entry.table, entry.column);
            let unique: BTreeSet<_> = known.iter().collect();
            assert_eq!(
                unique.len(),
                known.len(),
                "{}.{}",
                entry.table,
                entry.column
            );
            let key = (entry.table, entry.column);
            assert!(previous < Some(key), "{key:?} is out of order");
            previous = Some(key);
        }
    }

    #[test]
    fn a_code_enum_set_is_exactly_what_its_parser_accepts() {
        let known = codes_of!(RecoveryHint);
        assert_eq!(known.len(), RecoveryHint::ALL.len());
        for code in known {
            assert!(code.parse::<RecoveryHint>().is_ok(), "{code}");
        }
    }

    fn known_of(table: &str, column: &str) -> &'static [&'static str] {
        let entry = CODED_COLUMNS
            .iter()
            .find(|entry| (entry.table, entry.column) == (table, column))
            .expect("the entry is listed");
        (entry.known)()
    }

    #[test]
    fn the_created_item_kinds_are_the_ones_the_domain_reads() {
        let samples = [
            ("instance_tree", "0199f000-0000-7000-8000-000000000031"),
            ("environment_tree", "0199f000-0000-7000-8000-000000000032"),
            ("secret_ref", "local:erp/postgres/admin"),
            ("custom", "supervisor_spawned_process:pid:4242"),
        ];
        for (kind, item_ref) in samples {
            assert!(
                stackly_domain::CreatedItem::from_parts(kind, item_ref).is_ok(),
                "{kind}"
            );
        }
        let sampled: Vec<&str> = samples.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(known_of("operation_created", "item_kind"), sampled);
        assert!(
            stackly_domain::CreatedItem::from_parts("zzz_not_known", "x").is_err(),
            "a kind outside the list is refused"
        );
    }

    #[test]
    fn the_audit_resource_kinds_are_the_ones_the_domain_writes() {
        let id: stackly_domain::SecretReferenceId =
            "0199f000-0000-7000-8000-000000000061".parse().unwrap();
        let resource = stackly_domain::AuditResource::SecretReference(id);

        assert_eq!(
            known_of("audit_events", "resource_kind"),
            [resource.kind_code()]
        );
    }
}
