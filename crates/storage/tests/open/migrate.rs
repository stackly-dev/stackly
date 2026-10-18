//! Migrating after a verified snapshot, and recovery when a migration or
//! its snapshot fails (AC-4, AC-5).

use std::fs;
use std::os::unix::fs::PermissionsExt;

use jiff::SignedDuration;
use rusqlite::{Connection, OpenFlags};
use stackly_storage::{
    APPLICATION_ID, Migration, MigrationSet, OpenOptions, RECOVERY_REPORT_SCHEMA, RecoveryReason,
};
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{
    Harness, extended_set, mode_of, ready, recovery, row_counts, schema_rows, sha256_of,
    user_version,
};
use crate::fixture::fixture;

/// The newest built in schema version. The test migrations here come
/// after it, so the tests keep working when a new built in version lands.
fn latest() -> u32 {
    MigrationSet::builtin().latest()
}

/// A test migration that rebuilds `provider_revisions` the 12 step way.
fn rebuild() -> Migration {
    Migration {
        version: latest() + 1,
        name: "rebuild_provider_revisions",
        sql: include_str!("../fixtures/migrations/0002_rebuild_provider_revisions.sql"),
    }
}

/// A test migration that fails after its first statement.
fn fails_midway() -> Migration {
    Migration {
        version: latest() + 1,
        name: "fails_midway",
        sql: include_str!("../fixtures/migrations/0002_fails_midway.sql"),
    }
}

/// The names and SQL of five additive test migrations.
const CHAIN: [(&str, &str); 5] = [
    ("add_note_1", "ALTER TABLE settings ADD COLUMN note_1 TEXT;"),
    ("add_note_2", "ALTER TABLE settings ADD COLUMN note_2 TEXT;"),
    ("add_note_3", "ALTER TABLE settings ADD COLUMN note_3 TEXT;"),
    ("add_note_4", "ALTER TABLE settings ADD COLUMN note_4 TEXT;"),
    ("add_note_5", "ALTER TABLE settings ADD COLUMN note_5 TEXT;"),
];

/// The first `count` additive test migrations, after the built in ones.
fn chain(count: u32) -> Vec<Migration> {
    CHAIN
        .iter()
        .zip(1..=count)
        .map(|((name, sql), step)| Migration {
            version: latest() + step,
            name,
            sql,
        })
        .collect()
}

/// The built in set plus a next version whose SQL fails after its first
/// statement. Its fingerprint is never reached.
fn failing_set() -> MigrationSet {
    let builtin = MigrationSet::builtin();
    let mut migrations = builtin.migrations().to_vec();
    let mut fingerprints: Vec<(u32, String)> = migrations
        .iter()
        .map(|migration| {
            let fingerprint = builtin
                .fingerprint(migration.version)
                .expect("test setup step succeeds");
            (migration.version, fingerprint.to_owned())
        })
        .collect();
    migrations.push(fails_midway());
    fingerprints.push((latest() + 1, "0".repeat(64)));
    MigrationSet::new(migrations, fingerprints)
}

fn read_only(path: &std::path::Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("test setup step succeeds")
}

/// User version, stored schema, and row counts: what must not change when
/// an open ends in recovery.
type State = (
    i64,
    Vec<(String, String, Option<String>)>,
    Vec<(String, i64)>,
);

/// What must not change when an open ends in recovery.
fn state(harness: &Harness) -> State {
    let conn = harness.connect();
    (user_version(&conn), schema_rows(&conn), row_counts(&conn))
}

#[test]
fn migrating_the_latest_fixture_takes_a_verified_snapshot_and_keeps_the_data() {
    let harness = Harness::new();
    let base = latest();
    harness.load_fixture(&fixture(base));
    let before = row_counts(&harness.connect());
    let options = OpenOptions::new().with_migrations(extended_set(&[rebuild()]));

    let (registry, summary) = ready(harness.open_with(&options));
    drop(registry);

    assert_eq!(summary.migrated_from, Some(base));
    assert_eq!(summary.schema_version, base + 1);
    let snapshot = summary.snapshot.expect("a migration takes a snapshot");
    let name = snapshot.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        name,
        format!("v{base}-to-v{}-20260101T000000Z.sqlite", base + 1)
    );
    assert_eq!(harness.snapshots(), [name]);
    assert_eq!(mode_of(&snapshot), 0o600);
    let copy = read_only(&snapshot);
    assert_eq!(user_version(&copy), i64::from(base));
    let application_id: i64 = copy
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .unwrap();
    assert_eq!(application_id, i64::from(APPLICATION_ID));
    assert_eq!(row_counts(&copy), before);

    let conn = harness.connect();
    assert_eq!(user_version(&conn), i64::from(base + 1));
    let mut after = row_counts(&conn);
    after.retain(|(table, _)| table != "schema_migrations");
    let mut expected = before.clone();
    expected.retain(|(table, _)| table != "schema_migrations");
    assert_eq!(after, expected, "the data survives the migration");
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    let objects: Vec<_> = schema_rows(&conn)
        .into_iter()
        .map(|(_, name, _)| name)
        .collect();
    assert!(objects.contains(&"provider_revisions_source_id".to_owned()));
    assert!(objects.contains(&"provider_revisions_immutable".to_owned()));
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    let err = conn
        .execute("UPDATE provider_revisions SET capabilities = '[]'", [])
        .unwrap_err();
    assert!(
        err.to_string().contains("provider revision is immutable"),
        "{err}"
    );
    let err = conn
        .execute("DELETE FROM provider_revisions", [])
        .unwrap_err();
    assert!(
        err.to_string().contains("FOREIGN KEY constraint failed"),
        "{err}"
    );
    drop(conn);

    harness.fake.clock().advance(SignedDuration::from_hours(1));
    let (registry, second) = ready(harness.open_with(&options));
    drop(registry);
    assert_eq!(second.migrated_from, None);
    assert_eq!(second.snapshot, None);
    assert_eq!(
        harness.snapshots().len(),
        1,
        "a second open takes no snapshot"
    );
}

#[test]
fn five_migrations_in_a_row_keep_the_three_newest_snapshots() {
    let harness = Harness::new();
    harness.create();

    let base = latest();

    for step in 1..=5_u32 {
        harness.fake.clock().advance(SignedDuration::from_mins(1));
        let set = extended_set(&chain(step));
        let (registry, summary) =
            ready(harness.open_with(&OpenOptions::new().with_migrations(set)));
        drop(registry);
        assert_eq!(summary.migrated_from, Some(base + step - 1));
    }

    let expected: Vec<String> = (3..=5_u32)
        .map(|step| {
            format!(
                "v{}-to-v{}-20260101T000{step}00Z.sqlite",
                base + step - 1,
                base + step
            )
        })
        .collect();
    assert_eq!(harness.snapshots(), expected);
}

#[test]
fn failed_migration_enters_recovery_without_touching_the_registry() {
    let ev = evidence!(
        covers = ["AC-065"],
        kind = Kind::Fixture,
        expected = "a migration that fails midway rolls back, leaves the registry exactly as it \
                    was, keeps a verified snapshot, writes one recovery report, and a restart \
                    reuses both",
    );
    let harness = Harness::new();
    let base = latest();
    harness.load_fixture(&fixture(base));
    let before = state(&harness);
    let options = OpenOptions::new().with_migrations(failing_set());

    let report = recovery(harness.open_with(&options));

    match &report.reason {
        RecoveryReason::MigrationFailed {
            from,
            to,
            failed_version,
            error,
        } => {
            assert_eq!((*from, *to, *failed_version), (base, base + 1, base + 1));
            assert!(error.contains("no such table"), "{error}");
        }
        other => panic!("expected migration_failed, got {other:?}"),
    }
    assert_eq!(state(&harness), before, "the registry is unchanged");
    assert_eq!(report.db_schema_version, Some(base));
    assert_eq!(report.app_schema_version, base + 1);
    let snapshot = report
        .snapshot
        .clone()
        .expect("the report names the snapshot");
    assert_eq!(snapshot.schema_version, base);
    assert_eq!(sha256_of(&snapshot.path), snapshot.sha256);
    assert_eq!(state_of(&snapshot.path), before);
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&report.path).unwrap()).unwrap();
    assert_eq!(json["schema"], RECOVERY_REPORT_SCHEMA);
    assert_eq!(json["reason"]["code"], "migration_failed");
    assert_eq!(json["reason"]["failedVersion"], base + 1);
    assert_eq!(json["snapshot"]["sha256"], snapshot.sha256);
    let registry_file = &report.files[0];
    assert_eq!(registry_file.name, "stackly.sqlite");
    assert_eq!(
        registry_file.sha256.as_deref(),
        Some(sha256_of(&harness.db()).as_str())
    );
    assert_eq!(harness.reports().len(), 1);

    harness.fake.clock().advance(SignedDuration::from_hours(1));
    let again = recovery(harness.open_with(&options));

    assert_eq!(again.reason.code(), "migration_failed");
    assert_eq!(
        again.path, report.path,
        "the restart points at the same report"
    );
    assert_eq!(
        again.snapshot.as_ref().map(|s| &s.path),
        Some(&snapshot.path)
    );
    assert_eq!(harness.reports().len(), 1, "no new report");
    assert_eq!(
        harness.snapshots().len(),
        1,
        "no new snapshot, none deleted"
    );
    assert_eq!(state(&harness), before);
    ev.pass();
}

fn state_of(path: &std::path::Path) -> State {
    let conn = read_only(path);
    (user_version(&conn), schema_rows(&conn), row_counts(&conn))
}

#[test]
fn a_snapshot_that_cannot_be_written_is_backup_failed_and_nothing_migrates() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(latest()));
    let before = state(&harness);
    let snapshots = harness.layout.registry_snapshots_dir();
    fs::create_dir(&snapshots).unwrap();
    fs::set_permissions(&snapshots, fs::Permissions::from_mode(0o500)).unwrap();
    let options = OpenOptions::new().with_migrations(extended_set(&[rebuild()]));

    let report = recovery(harness.open_with(&options));
    fs::set_permissions(&snapshots, fs::Permissions::from_mode(0o700)).unwrap();

    assert!(
        matches!(report.reason, RecoveryReason::BackupFailed { .. }),
        "{:?}",
        report.reason
    );
    assert_eq!(report.snapshot, None);
    assert_eq!(state(&harness), before, "no migration was tried");
    assert!(harness.snapshots().is_empty());
}
