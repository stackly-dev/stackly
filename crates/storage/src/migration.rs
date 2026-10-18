//! Forward only migrations: SQL files embedded in the binary, applied by
//! the runner here, all pending ones in one transaction.

use std::borrow::Cow;

use jiff::Timestamp;
use rusqlite::{Connection, params};

use crate::files::sha256_hex;
use crate::fingerprint::schema_fingerprint;
use crate::time::format_timestamp;

/// The migrations this build ships, in version order.
const BUILTIN_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "registry",
    sql: include_str!("../migrations/0001_registry.sql"),
}];

/// The expected schema fingerprint after each built in version. They are
/// constants, not computed at open, because a user's registry was built by
/// an older bundled SQLite; `fingerprints_match_migrations` rebuilds every
/// version from scratch to confirm them.
const BUILTIN_FINGERPRINTS: &[(u32, &str)] = &[(
    1,
    "c7a475b672d7a678fc2274a71f56b5c6987c6d2f2e795ebbb3a9eebf3fd6ec8f",
)];

/// One forward migration: its version, a `[a-z0-9_]` name, and its SQL.
///
/// A released migration is never edited; its SHA-256 is recorded in
/// `schema_migrations` and checked at every open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// The schema version this migration produces, from 1 with no gaps.
    pub version: u32,
    /// A short name, for example `registry`.
    pub name: &'static str,
    /// The SQL text, run as one batch.
    pub sql: &'static str,
}

impl Migration {
    /// Returns the SHA-256 of the SQL text as 64 lowercase hex characters.
    #[must_use]
    pub fn sha256(&self) -> String {
        sha256_hex(self.sql.as_bytes())
    }
}

/// A failure while applying or checking a migration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("migration {version} failed: {detail}")]
#[non_exhaustive]
pub struct MigrationError {
    /// The version that failed, or the target version when a check after
    /// the last migration failed.
    pub version: u32,
    /// What failed, as SQLite or the check reported it.
    pub detail: String,
}

/// The migrations an open applies, with the expected schema fingerprint of
/// every version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationSet {
    migrations: Cow<'static, [Migration]>,
    fingerprints: Vec<(u32, String)>,
}

impl MigrationSet {
    /// Returns the migrations this build ships.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            migrations: Cow::Borrowed(BUILTIN_MIGRATIONS),
            fingerprints: BUILTIN_FINGERPRINTS
                .iter()
                .map(|(version, fingerprint)| (*version, (*fingerprint).to_owned()))
                .collect(),
        }
    }

    /// Builds another set, for tests only. `migrations` must be ordered
    /// from version 1 with no gaps; a version without a fingerprint never
    /// passes the fingerprint check.
    #[doc(hidden)]
    #[must_use]
    pub fn new(migrations: Vec<Migration>, fingerprints: Vec<(u32, String)>) -> Self {
        Self {
            migrations: Cow::Owned(migrations),
            fingerprints,
        }
    }

    /// Returns the newest schema version of the set, 0 when it is empty.
    #[must_use]
    pub fn latest(&self) -> u32 {
        self.migrations
            .last()
            .map_or(0, |migration| migration.version)
    }

    /// Returns every migration, in version order.
    #[must_use]
    pub fn migrations(&self) -> &[Migration] {
        &self.migrations
    }

    /// Returns the expected fingerprint after `version`.
    #[must_use]
    pub fn fingerprint(&self, version: u32) -> Option<&str> {
        self.fingerprints
            .iter()
            .find(|(known, _)| *known == version)
            .map(|(_, fingerprint)| fingerprint.as_str())
    }

    /// Returns the migration that produces `version`.
    pub(crate) fn get(&self, version: u32) -> Option<&Migration> {
        self.migrations
            .iter()
            .find(|migration| migration.version == version)
    }

    /// Builds every version of `migrations` from scratch in memory and
    /// returns the fingerprint after each one.
    ///
    /// # Errors
    ///
    /// Returns a [`MigrationError`] naming the first migration whose SQL
    /// fails.
    #[doc(hidden)]
    pub fn fingerprints_from_scratch(
        migrations: &[Migration],
    ) -> Result<Vec<(u32, String)>, MigrationError> {
        let failed = |version| {
            move |err: rusqlite::Error| MigrationError {
                version,
                detail: err.to_string(),
            }
        };
        let conn = Connection::open_in_memory().map_err(failed(0))?;
        migrations
            .iter()
            .map(|migration| {
                conn.execute_batch(migration.sql)
                    .map_err(failed(migration.version))?;
                let fingerprint = schema_fingerprint(&conn).map_err(failed(migration.version))?;
                Ok((migration.version, fingerprint))
            })
            .collect()
    }
}

/// Applies every migration after `from` on `conn`, inside a transaction
/// the caller began with foreign keys off: each migration's SQL, its
/// `schema_migrations` row, and `PRAGMA user_version`. Then the foreign
/// key check must be empty and the fingerprint must equal the set's value
/// for the target version. The caller commits on success and rolls back
/// on error. Returns the target version.
pub(crate) fn apply_pending(
    conn: &Connection,
    set: &MigrationSet,
    from: u32,
    now: Timestamp,
    app_version: &str,
) -> Result<u32, MigrationError> {
    let to = set.latest();
    let applied_at = format_timestamp(now);
    for migration in set.migrations().iter().filter(|m| m.version > from) {
        let failed = |err: rusqlite::Error| MigrationError {
            version: migration.version,
            detail: err.to_string(),
        };
        conn.execute_batch(migration.sql).map_err(failed)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, name, sha256, applied_at, app_version) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                migration.version,
                migration.name,
                migration.sha256(),
                applied_at,
                app_version
            ],
        )
        .map_err(failed)?;
        conn.pragma_update(None, "user_version", migration.version)
            .map_err(failed)?;
    }
    let check_failed = |detail: String| MigrationError {
        version: to,
        detail,
    };
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|err| check_failed(err.to_string()))?;
    if violations > 0 {
        return Err(check_failed(format!(
            "the foreign key check found {violations} violation(s)"
        )));
    }
    let actual = schema_fingerprint(conn).map_err(|err| check_failed(err.to_string()))?;
    match set.fingerprint(to) {
        Some(expected) if expected == actual => Ok(to),
        Some(expected) => Err(check_failed(format!(
            "schema fingerprint {actual} does not match {expected}, the value for version {to}"
        ))),
        None => Err(check_failed(format!(
            "no fingerprint is known for version {to}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_match_migrations() {
        let set = MigrationSet::builtin();
        let computed = MigrationSet::fingerprints_from_scratch(set.migrations()).unwrap();

        for (version, fingerprint) in computed {
            assert_eq!(
                set.fingerprint(version),
                Some(fingerprint.as_str()),
                "fingerprint of version {version}"
            );
        }
    }

    #[test]
    fn builtin_versions_start_at_1_with_no_gaps_and_valid_names() {
        let set = MigrationSet::builtin();

        for (index, migration) in set.migrations().iter().enumerate() {
            assert_eq!(migration.version as usize, index + 1);
            assert!(
                !migration.name.is_empty()
                    && migration
                        .name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "{}",
                migration.name
            );
        }
        assert_eq!(set.latest(), 1);
    }

    #[test]
    fn migrations_use_only_the_allowed_statements() {
        for migration in MigrationSet::builtin().migrations() {
            let upper: String = migration
                .sql
                .lines()
                .map(|line| line.split("--").next().unwrap_or_default().to_uppercase())
                .collect::<Vec<_>>()
                .join("\n");
            let banned_statements = [
                "RENAME COLUMN",
                "DROP COLUMN",
                "PRAGMA",
                "BEGIN;",
                "BEGIN TRANSACTION",
                "BEGIN IMMEDIATE",
                "BEGIN EXCLUSIVE",
                "BEGIN DEFERRED",
                "COMMIT",
                "ROLLBACK",
                "SAVEPOINT",
                "ATTACH",
                "VACUUM",
            ];
            for banned in banned_statements {
                assert!(
                    !upper.contains(banned),
                    "migration {} uses {banned}",
                    migration.version
                );
            }
            for clock in ["CURRENT_TIMESTAMP", "CURRENT_DATE", "CURRENT_TIME", "'NOW'"] {
                assert!(
                    !upper.contains(clock),
                    "migration {} reads the time from SQLite",
                    migration.version
                );
            }
        }
    }
}
