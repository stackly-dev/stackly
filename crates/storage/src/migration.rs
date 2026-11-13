//! Forward only migrations: SQL files embedded in the binary, applied by
//! the runner here, all pending ones in one transaction.

use std::borrow::Cow;

use jiff::Timestamp;
use rusqlite::{Connection, params};

use crate::files::sha256_hex;
use crate::fingerprint::schema_fingerprint;
use crate::time::format_timestamp;

/// The migrations this build ships, in version order.
const BUILTIN_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "registry",
        sql: include_str!("../migrations/0001_registry.sql"),
    },
    Migration {
        version: 2,
        name: "operation_journal",
        sql: include_str!("../migrations/0002_operation_journal.sql"),
    },
    Migration {
        version: 3,
        name: "secret_references",
        sql: include_str!("../migrations/0003_secret_references.sql"),
    },
    Migration {
        version: 4,
        name: "operation_failure",
        sql: include_str!("../migrations/0004_operation_failure.sql"),
    },
    Migration {
        version: 5,
        name: "runtime_install",
        sql: include_str!("../migrations/0005_runtime_install.sql"),
    },
    Migration {
        version: 6,
        name: "instance_last_exit",
        sql: include_str!("../migrations/0006_instance_last_exit.sql"),
    },
    Migration {
        version: 7,
        name: "instance_restart_total",
        sql: include_str!("../migrations/0007_instance_restart_total.sql"),
    },
    Migration {
        version: 8,
        name: "config_revision_history",
        sql: include_str!("../migrations/0008_config_revision_history.sql"),
    },
    Migration {
        version: 9,
        name: "daemon_restart_plan",
        sql: include_str!("../migrations/0009_daemon_restart_plan.sql"),
    },
    Migration {
        version: 10,
        name: "daemon_sessions",
        sql: include_str!("../migrations/0010_daemon_sessions.sql"),
    },
    Migration {
        version: 11,
        name: "settings_locations",
        sql: include_str!("../migrations/0011_settings_locations.sql"),
    },
    Migration {
        version: 12,
        name: "audit_operation_fields",
        sql: include_str!("../migrations/0012_audit_operation_fields.sql"),
    },
    Migration {
        version: 13,
        name: "provider_imports",
        sql: include_str!("../migrations/0013_provider_imports.sql"),
    },
    Migration {
        version: 14,
        name: "environment_manifest",
        sql: include_str!("../migrations/0014_environment_manifest.sql"),
    },
    Migration {
        version: 15,
        name: "environment_up",
        sql: include_str!("../migrations/0015_environment_up.sql"),
    },
];

/// The expected schema fingerprint after each built in version. They are
/// constants, not computed at open, because a user's registry was built by
/// an older bundled SQLite; `fingerprints_match_migrations` rebuilds every
/// version from scratch to confirm them.
const BUILTIN_FINGERPRINTS: &[(u32, &str)] = &[
    (
        1,
        "c7a475b672d7a678fc2274a71f56b5c6987c6d2f2e795ebbb3a9eebf3fd6ec8f",
    ),
    (
        2,
        "b2075db92cc9fea179ce9926b42b90d585030b0e0448aa21676feb00dca0f74a",
    ),
    (
        3,
        "c12e265313c19ed8a4654b5dfff6c01d0817934c662c4c399d303662f2107131",
    ),
    (
        4,
        "a854c1f9020b9c46bfe6753f34cfe415f60e892d989349395424084d717832ab",
    ),
    (
        5,
        "9d8d855fb4418c39ff9aa9fc84e10d02154a819145e4fafdbe4604c0cb1f60db",
    ),
    (
        6,
        "388f7ecd75733cd923a9ada1426226032a52d1f49455b9b886a414ad70093451",
    ),
    (
        7,
        "15733ad01129754b2249639bc2287f969bcc5f1ec078aab92761af25590eb1da",
    ),
    (
        8,
        "ecba327fde38ec332681dd5b3b46f90c4a9fa0d6eec23450a64d3a91ba4ac9f1",
    ),
    (
        9,
        "4ec55bc96e1f47750f8bc1c25a6dca4bd25b709528171111960e3360a1ba4a77",
    ),
    (
        10,
        "60be692e75571387774c265eb804faaf55173b9484371d420778f5bec6c3be7c",
    ),
    (
        11,
        "c542ea64cbcf0575bcf80f0d77eff1feeeda30accb8ac4853e701668bfcde5dd",
    ),
    (
        12,
        "1b00ec6f9445027e1b3aeb4a82ff71ce0ba9f2e6cf9274b6beac72755b332d4d",
    ),
    (
        13,
        "4227517716f3d57b63206ea830542e0d7e2647eed3a9a1b833f9413aa76fd9f6",
    ),
    (
        14,
        "da91a3a3cac4476a7ac41daefaed5a4c20142193a6a28f233e5c9dd976f10c0d",
    ),
    (
        15,
        "ce93c75e5e9d66287d0b0a69320f75d81bab60139d9570fa116dc7d388c49ff5",
    ),
];

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
        assert_eq!(set.latest(), 15);
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
