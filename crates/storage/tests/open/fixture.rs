//! Frozen registry fixtures, one per schema version (AC-4).
//!
//! `tests/fixtures/registry-v<N>.sql` is a text dump of a registry at
//! version N holding the sample data in `tests/fixtures/seed/v<N>.sql`. It
//! is written once, when version N is made, by the test
//! `write_registry_fixture`, and never edited afterwards: its SHA-256 is
//! recorded in [`FIXTURE_SHA256`], so a changed fixture fails the tests.
//! The writer returns at once unless `STACKLY_WRITE_REGISTRY_FIXTURE=1` is
//! set, because spec 0002 keeps `#[ignore]` for native tests only:
//!
//! ```text
//! STACKLY_WRITE_REGISTRY_FIXTURE=1 cargo test -p stackly-storage --test open fixture::write_registry_fixture
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use stackly_storage::{APPLICATION_ID, MigrationSet};

use crate::common::{Harness, ready, sha256_of};

/// The SHA-256 of every frozen fixture, by schema version.
pub const FIXTURE_SHA256: &[(u32, &str)] = &[(
    1,
    "41bcbde1e817013f74f2821108b2adecc247b0f484d8c7193803d5c28bfb0458",
)];

/// When the fixture rows were applied, for the `schema_migrations` rows.
const APPLIED_AT: &str = "2026-01-01T00:00:00.000000000Z";
/// The app version the `schema_migrations` rows name.
const FIXTURE_APP_VERSION: &str = "0.1.0";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn fixture_path(version: u32) -> PathBuf {
    fixtures_dir().join(format!("registry-v{version}.sql"))
}

/// The frozen fixture of `version`.
pub fn fixture(version: u32) -> String {
    std::fs::read_to_string(fixture_path(version)).expect("the fixture exists")
}

/// Builds the registry of `version` from scratch: migrations 1 to N, their
/// `schema_migrations` rows, and the seed, all with fixed IDs and times.
fn build(version: u32) -> Connection {
    let conn = Connection::open_in_memory().expect("test setup step succeeds");
    let set = MigrationSet::builtin();
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .expect("test setup step succeeds");
    for migration in set.migrations().iter().filter(|m| m.version <= version) {
        conn.execute_batch(migration.sql)
            .expect("test setup step succeeds");
        conn.execute(
            "INSERT INTO schema_migrations (version, name, sha256, applied_at, app_version) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                migration.version,
                migration.name,
                migration.sha256(),
                APPLIED_AT,
                FIXTURE_APP_VERSION,
            ),
        )
        .expect("test setup step succeeds");
        conn.pragma_update(None, "user_version", migration.version)
            .expect("test setup step succeeds");
    }
    let seed = std::fs::read_to_string(fixtures_dir().join(format!("seed/v{version}.sql")))
        .expect("every schema version has a seed");
    // One transaction, so the deferred foreign key from instances to
    // config_revisions is checked at the end.
    conn.execute_batch(&format!("BEGIN;\n{seed}\nCOMMIT;"))
        .expect("test setup step succeeds");
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("test setup step succeeds");
    assert_eq!(violations, 0, "the seed breaks a foreign key");
    conn
}

/// Dumps a registry as SQL text: the header pragmas, every schema statement
/// as `sqlite_schema` stores it (tables, then indexes, then triggers), and
/// one `INSERT` per row, table by table in key order.
fn dump(conn: &Connection) -> String {
    let pragma = |name: &str| -> i64 {
        conn.pragma_query_value(None, name, |row| row.get(0))
            .expect("test setup step succeeds")
    };
    let mut out = String::new();
    writeln!(
        out,
        "-- Frozen registry fixture, schema version {}.",
        pragma("user_version")
    )
    .expect("test setup step succeeds");
    writeln!(
        out,
        "-- Written by write_registry_fixture; never edit it by hand."
    )
    .expect("test setup step succeeds");
    writeln!(out, "PRAGMA application_id = {};", pragma("application_id"))
        .expect("test setup step succeeds");
    writeln!(out, "PRAGMA user_version = {};", pragma("user_version"))
        .expect("test setup step succeeds");
    writeln!(out, "BEGIN;").expect("test setup step succeeds");
    let schema: Vec<(String, String, String)> = conn
        .prepare(
            "SELECT type, name, sql FROM sqlite_schema WHERE sql IS NOT NULL \
             AND name NOT LIKE 'sqlite_%' ORDER BY CASE type WHEN 'table' THEN 0 \
             WHEN 'index' THEN 1 WHEN 'trigger' THEN 2 ELSE 3 END, name",
        )
        .expect("test setup step succeeds")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("test setup step succeeds")
        .collect::<rusqlite::Result<_>>()
        .expect("test setup step succeeds");
    for (_, _, sql) in &schema {
        writeln!(out, "{sql};").expect("test setup step succeeds");
    }
    let tables = schema
        .iter()
        .filter(|(kind, _, _)| kind == "table")
        .map(|(_, name, _)| name);
    for table in tables {
        let key: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")
            .expect("test setup step succeeds")
            .query_map([table], |row| row.get(0))
            .expect("test setup step succeeds")
            .collect::<rusqlite::Result<_>>()
            .expect("test setup step succeeds");
        let order = if key.is_empty() {
            "rowid".to_owned()
        } else {
            key.join(", ")
        };
        let mut statement = conn
            .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY {order}"))
            .expect("test setup step succeeds");
        let columns: Vec<String> = statement
            .column_names()
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let mut rows = statement.query([]).expect("test setup step succeeds");
        while let Some(row) = rows.next().expect("test setup step succeeds") {
            let values: Vec<String> = (0..columns.len())
                .map(|index| literal(row.get_ref(index).expect("test setup step succeeds")))
                .collect();
            writeln!(
                out,
                "INSERT INTO {table} ({}) VALUES ({});",
                columns.join(", "),
                values.join(", ")
            )
            .expect("test setup step succeeds");
        }
    }
    writeln!(out, "COMMIT;").expect("test setup step succeeds");
    out
}

fn literal(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "NULL".to_owned(),
        ValueRef::Integer(number) => number.to_string(),
        ValueRef::Real(number) => format!("{number:?}"),
        ValueRef::Text(text) => {
            format!("'{}'", String::from_utf8_lossy(text).replace('\'', "''"))
        }
        ValueRef::Blob(bytes) => {
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("X'{hex}'")
        }
    }
}

/// Writes the frozen fixture of the newest schema version. Run it by hand
/// once, when a new schema version is made; without
/// `STACKLY_WRITE_REGISTRY_FIXTURE=1` it does nothing.
#[test]
fn write_registry_fixture() {
    if std::env::var("STACKLY_WRITE_REGISTRY_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let version = MigrationSet::builtin().latest();
    let path = fixture_path(version);
    assert!(
        !path.exists(),
        "{} is frozen; a released fixture is never written again",
        path.display()
    );
    std::fs::write(&path, dump(&build(version))).unwrap();
    println!("wrote {} with sha256 {}", path.display(), sha256_of(&path));
}

#[test]
fn every_builtin_schema_version_has_a_frozen_fixture() {
    for migration in MigrationSet::builtin().migrations() {
        let version = migration.version;
        assert!(
            FIXTURE_SHA256.iter().any(|(known, _)| *known == version),
            "schema version {version} has no FIXTURE_SHA256 entry"
        );
        assert!(
            fixture_path(version).exists(),
            "schema version {version} has no fixture"
        );
    }
}

#[test]
fn frozen_fixtures_are_unchanged() {
    for (version, expected) in FIXTURE_SHA256 {
        assert_eq!(
            sha256_of(&fixture_path(*version)),
            *expected,
            "registry-v{version}.sql changed; released fixtures are frozen"
        );
    }
}

#[test]
fn the_dumper_reproduces_the_fixture_from_its_seed() {
    for (version, _) in FIXTURE_SHA256 {
        assert_eq!(
            dump(&build(*version)),
            fixture(*version),
            "registry-v{version}.sql"
        );
    }
}

#[test]
fn the_latest_fixture_opens_ready_without_migrating() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(MigrationSet::builtin().latest()));

    let (registry, summary) = ready(harness.open());

    assert!(!summary.created);
    assert_eq!(summary.migrated_from, None);
    assert_eq!(
        summary.registry_id.to_string(),
        "0199f000-0000-7000-8000-000000000001"
    );
    drop(registry);
}
