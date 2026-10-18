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

const REBUILD: Migration = Migration {
    version: 2,
    name: "rebuild_provider_revisions",
    sql: include_str!("../fixtures/migrations/0002_rebuild_provider_revisions.sql"),
};

const FAILS_MIDWAY: Migration = Migration {
    version: 2,
    name: "fails_midway",
    sql: include_str!("../fixtures/migrations/0002_fails_midway.sql"),
};

/// Five additive test migrations, versions 2 to 6.
const CHAIN: [Migration; 5] = [
    Migration {
        version: 2,
        name: "add_note_2",
        sql: "ALTER TABLE settings ADD COLUMN note_2 TEXT;",
    },
    Migration {
        version: 3,
        name: "add_note_3",
        sql: "ALTER TABLE settings ADD COLUMN note_3 TEXT;",
    },
    Migration {
        version: 4,
        name: "add_note_4",
        sql: "ALTER TABLE settings ADD COLUMN note_4 TEXT;",
    },
    Migration {
        version: 5,
        name: "add_note_5",
        sql: "ALTER TABLE settings ADD COLUMN note_5 TEXT;",
    },
    Migration {
        version: 6,
        name: "add_note_6",
        sql: "ALTER TABLE settings ADD COLUMN note_6 TEXT;",
    },
];

/// The built in set plus a version 2 whose SQL fails after its first
/// statement. Its fingerprint is never reached.
fn failing_set() -> MigrationSet {
    let builtin = MigrationSet::builtin();
    let v1 = builtin
        .fingerprint(1)
        .expect("test setup step succeeds")
        .to_owned();
    MigrationSet::new(
        vec![builtin.migrations()[0], FAILS_MIDWAY],
        vec![(1, v1), (2, "0".repeat(64))],
    )
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
fn migrating_the_v1_fixture_takes_a_verified_snapshot_and_keeps_the_data() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(1));
    let before = row_counts(&harness.connect());
    let options = OpenOptions::new().with_migrations(extended_set(&[REBUILD]));

    let (registry, summary) = ready(harness.open_with(&options));
    drop(registry);

    assert_eq!(summary.migrated_from, Some(1));
    assert_eq!(summary.schema_version, 2);
    let snapshot = summary.snapshot.expect("a migration takes a snapshot");
    let name = snapshot.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name, "v1-to-v2-20260101T000000Z.sqlite");
    assert_eq!(harness.snapshots(), [name]);
    assert_eq!(mode_of(&snapshot), 0o600);
    let copy = read_only(&snapshot);
    assert_eq!(user_version(&copy), 1);
    let application_id: i64 = copy
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .unwrap();
    assert_eq!(application_id, i64::from(APPLICATION_ID));
    assert_eq!(row_counts(&copy), before);

    let conn = harness.connect();
    assert_eq!(user_version(&conn), 2);
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

    for version in 2..=6_u32 {
        harness.fake.clock().advance(SignedDuration::from_mins(1));
        let set = extended_set(&CHAIN[..(version as usize - 1)]);
        let (registry, summary) =
            ready(harness.open_with(&OpenOptions::new().with_migrations(set)));
        drop(registry);
        assert_eq!(summary.migrated_from, Some(version - 1));
    }

    assert_eq!(
        harness.snapshots(),
        [
            "v3-to-v4-20260101T000300Z.sqlite",
            "v4-to-v5-20260101T000400Z.sqlite",
            "v5-to-v6-20260101T000500Z.sqlite",
        ]
    );
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
    harness.load_fixture(&fixture(1));
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
            assert_eq!((*from, *to, *failed_version), (1, 2, 2));
            assert!(error.contains("no such table"), "{error}");
        }
        other => panic!("expected migration_failed, got {other:?}"),
    }
    assert_eq!(state(&harness), before, "the registry is unchanged");
    assert_eq!(report.db_schema_version, Some(1));
    assert_eq!(report.app_schema_version, 2);
    let snapshot = report
        .snapshot
        .clone()
        .expect("the report names the snapshot");
    assert_eq!(snapshot.schema_version, 1);
    assert_eq!(sha256_of(&snapshot.path), snapshot.sha256);
    assert_eq!(state_of(&snapshot.path), before);
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&report.path).unwrap()).unwrap();
    assert_eq!(json["schema"], RECOVERY_REPORT_SCHEMA);
    assert_eq!(json["reason"]["code"], "migration_failed");
    assert_eq!(json["reason"]["failedVersion"], 2);
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
    harness.load_fixture(&fixture(1));
    let before = state(&harness);
    let snapshots = harness.layout.registry_snapshots_dir();
    fs::create_dir(&snapshots).unwrap();
    fs::set_permissions(&snapshots, fs::Permissions::from_mode(0o500)).unwrap();
    let options = OpenOptions::new().with_migrations(extended_set(&[REBUILD]));

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
