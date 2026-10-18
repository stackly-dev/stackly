//! The schema fingerprint: a SHA-256 over the stored schema, so a table,
//! column, index, or trigger changed outside the migration runner is
//! detected at open.

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::files::hex;

/// Computes the fingerprint of the schema `conn` sees: SHA-256 over one
/// line `type \t name \t tbl_name \t sql \n` per `sqlite_schema` row whose
/// name does not start with `sqlite_`, sorted by type and then name, with
/// each `sql` [normalized](normalize).
pub(crate) fn schema_fingerprint(conn: &Connection) -> rusqlite::Result<String> {
    let mut statement = conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema")?;
    let mut rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.retain(|(_, name, _, _)| !name.starts_with("sqlite_"));
    rows.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    let mut hasher = Sha256::new();
    for (kind, name, table, sql) in rows {
        hasher.update(format!("{kind}\t{name}\t{table}\t{}\n", normalize(&sql)));
    }
    Ok(hex(&hasher.finalize()))
}

/// Drops every `"` (SQLite adds them when it renames a table), collapses
/// each run of whitespace to one space, and trims.
pub(crate) fn normalize(sql: &str) -> String {
    sql.split(|c: char| c.is_whitespace() || c == '"')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ignores_quotes_and_whitespace_runs() {
        assert_eq!(
            normalize("CREATE TABLE \"t\" (\n    a INTEGER,\tb TEXT\n)  "),
            "CREATE TABLE t ( a INTEGER, b TEXT )"
        );
    }

    #[test]
    fn fingerprint_changes_with_an_index_and_ignores_internal_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (a INTEGER PRIMARY KEY AUTOINCREMENT, b TEXT);")
            .unwrap();
        let before = schema_fingerprint(&conn).unwrap();
        conn.execute_batch("INSERT INTO t (b) VALUES ('x'); ANALYZE;")
            .unwrap();
        assert_eq!(schema_fingerprint(&conn).unwrap(), before);

        conn.execute_batch("CREATE INDEX t_b ON t (b);").unwrap();
        assert_ne!(schema_fingerprint(&conn).unwrap(), before);
    }

    #[test]
    fn a_renamed_table_has_the_same_fingerprint_as_one_created_with_that_name() {
        let direct = Connection::open_in_memory().unwrap();
        direct
            .execute_batch("CREATE TABLE items (a INTEGER NOT NULL);")
            .unwrap();
        let renamed = Connection::open_in_memory().unwrap();
        renamed
            .execute_batch(
                "CREATE TABLE items_new (a INTEGER NOT NULL);
                 ALTER TABLE items_new RENAME TO items;",
            )
            .unwrap();

        // SQLite rewrites the renamed statement as CREATE TABLE "items".
        let stored: String = renamed
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name = 'items'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.contains('"'), "{stored}");
        assert_eq!(
            schema_fingerprint(&renamed).unwrap(),
            schema_fingerprint(&direct).unwrap()
        );
    }
}
