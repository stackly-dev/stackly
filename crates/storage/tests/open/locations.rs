//! Migration 11 (spec 0032, step 2): daemon settings, storage locations,
//! and `instance_paths` rebuilt with `location_id`.

use rusqlite::Connection;
use stackly_domain::{LocationKind, Registry, StorageLocationId};
use stackly_storage::{Migration, MigrationSet, OpenOptions};

use crate::common::{Harness, ready, recovery, row_counts, schema_rows, user_version};
use crate::fixture::fixture;

const DATA: &str = "00000000-0000-7000-8000-000000000001";
const LOGS: &str = "00000000-0000-7000-8000-000000000002";
const CACHE: &str = "00000000-0000-7000-8000-000000000003";

/// Every stored path of the registry: instance, kind, root, relative path.
fn paths(conn: &Connection) -> Vec<(String, String, String, String)> {
    let mut statement = conn
        .prepare(
            "SELECT instance_id, kind, logical_root, relative_path FROM instance_paths \
             ORDER BY instance_id, kind",
        )
        .expect("instance_paths can be read");
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("instance_paths can be queried")
        .collect::<rusqlite::Result<_>>()
        .expect("every path row reads")
}

// covers: AC-25
#[test]
fn a_version_10_registry_migrates_every_path_to_the_built_in_location_of_its_kind() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(10));
    let before = paths(&harness.connect());
    assert!(!before.is_empty(), "the version 10 fixture holds instances");

    let (registry, summary) = ready(harness.open());

    assert_eq!(summary.migrated_from, Some(10));
    assert!(summary.snapshot.is_some(), "a migration takes a snapshot");
    let conn = harness.connect();
    assert_eq!(
        paths(&conn),
        before,
        "every path is the same text as before"
    );
    let located: Vec<(String, Option<String>)> = conn
        .prepare("SELECT logical_root, location_id FROM instance_paths ORDER BY logical_root")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for (root, location) in located {
        let expected = match root.as_str() {
            "data" => Some(DATA),
            "logs" => Some(LOGS),
            _ => None,
        };
        assert_eq!(
            location.as_deref(),
            expected,
            "the location of a {root} path"
        );
    }
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);

    let known = registry.known_instance_ids().unwrap().value;
    for id in known {
        let entries = registry.instance_paths(id).unwrap().value.unwrap();
        assert_eq!(entries.len(), 7);
        for entry in entries {
            let expected = match entry.root().as_code() {
                "data" => Some(LocationKind::Data.built_in_id()),
                "logs" => Some(LocationKind::Logs.built_in_id()),
                _ => None,
            };
            assert_eq!(entry.location(), expected);
        }
    }
}

// covers: AC-25
#[test]
fn migration_11_adds_the_built_in_locations_and_one_settings_row() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(10));
    let (registry, _) = ready(harness.open());

    let settings = registry.daemon_settings().unwrap().value;
    assert_eq!(settings.revision.get(), 1);
    assert!(!settings.telemetry);
    assert_eq!(settings.telemetry_copy_version, 1);
    assert_eq!(settings.data_location, LocationKind::Data.built_in_id());
    assert_eq!(settings.logs_location, LocationKind::Logs.built_in_id());
    assert_eq!(settings.cache_location, LocationKind::Cache.built_in_id());

    let locations = registry.storage_locations().unwrap().value;
    let kinds: Vec<_> = locations.iter().map(|location| location.kind()).collect();
    assert_eq!(
        kinds,
        [LocationKind::Data, LocationKind::Logs, LocationKind::Cache]
    );
    assert!(locations.iter().all(|location| location.is_built_in()));

    let usage = registry.location_usage().unwrap().value;
    let data: StorageLocationId = DATA.parse().unwrap();
    let logs: StorageLocationId = LOGS.parse().unwrap();
    let cache: StorageLocationId = CACHE.parse().unwrap();
    let count = |id| {
        usage
            .iter()
            .find(|(known, _)| *known == id)
            .map(|(_, n)| *n)
    };
    assert!(count(data).is_some_and(|n| n > 0), "instances hold data");
    assert_eq!(count(data), count(logs), "each instance has both");
    assert_eq!(count(cache), None, "no instance uses the cache");
}

/// Migration 11 with a statement that fails after the rebuild.
fn failing_set() -> MigrationSet {
    let builtin = MigrationSet::builtin();
    let mut migrations = builtin.migrations().to_vec();
    let last = migrations.pop().expect("there are built in migrations");
    let sql: &'static str = Box::leak(
        format!("{}\nSELECT * FROM table_that_does_not_exist;", last.sql).into_boxed_str(),
    );
    migrations.push(Migration {
        version: last.version,
        name: last.name,
        sql,
    });
    let fingerprints = migrations
        .iter()
        .map(|migration| {
            let known = builtin.fingerprint(migration.version).unwrap_or_default();
            (migration.version, known.to_owned())
        })
        .collect();
    MigrationSet::new(migrations, fingerprints)
}

// covers: AC-25
#[test]
fn a_migration_11_that_fails_after_the_copy_leaves_the_registry_as_it_was() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(10));
    let conn = harness.connect();
    let before = (user_version(&conn), schema_rows(&conn), row_counts(&conn));
    drop(conn);

    let report = recovery(harness.open_with(&OpenOptions::new().with_migrations(failing_set())));

    assert_eq!(report.reason.code(), "migration_failed");
    let conn = harness.connect();
    let after = (user_version(&conn), schema_rows(&conn), row_counts(&conn));
    assert_eq!(after, before, "nothing of the migration stays");
    let snapshot = report.snapshot.expect("the report names the snapshot");
    assert_eq!(snapshot.schema_version, 10);
}

fn migrated() -> (Harness, Connection) {
    let harness = Harness::new();
    harness.load_fixture(&fixture(10));
    drop(ready(harness.open()));
    let conn = harness.connect();
    conn.pragma_update(None, "foreign_keys", true)
        .expect("foreign keys can be turned on");
    (harness, conn)
}

// covers: AC-25
#[test]
fn the_tables_refuse_what_the_data_model_forbids() {
    let (_harness, conn) = migrated();
    let refused = |sql: &str| conn.execute(sql, []).unwrap_err().to_string();

    // A second row of settings, a second built in row of a kind, and a path
    // on a built in row.
    assert!(
        refused(
            "INSERT INTO daemon_settings (id, default_startup, update_channel, telemetry, \
             telemetry_copy_version, data_location_id, logs_location_id, cache_location_id, \
             revision, updated_at) VALUES (2, 'manual', 'stable', 0, 1, \
             '00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000002', \
             '00000000-0000-7000-8000-000000000003', 1, '2026-01-01T00:00:00.000000000Z')"
        )
        .contains("CHECK constraint failed")
    );
    assert!(
        refused(
            "INSERT INTO storage_locations (id, kind, path, built_in, created_at) VALUES \
             ('0199f000-0000-7000-8000-0000000000c0', 'data', NULL, 1, \
             '2026-01-01T00:00:00.000000000Z')"
        )
        .contains("UNIQUE constraint failed")
    );
    assert!(
        refused(
            "INSERT INTO storage_locations (id, kind, path, built_in, created_at) VALUES \
             ('0199f000-0000-7000-8000-0000000000c1', 'data', '/x/Stackly', 1, \
             '2026-01-01T00:00:00.000000000Z')"
        )
        .contains("CHECK constraint failed")
    );

    // A location is never changed or deleted, and the one row of settings
    // is never deleted.
    conn.execute(
        "INSERT INTO storage_locations (id, kind, path, built_in, created_at) VALUES \
         ('0199f000-0000-7000-8000-0000000000c2', 'logs', '/Volumes/Work/Stackly', 0, \
         '2026-01-01T00:00:00.000000000Z')",
        [],
    )
    .unwrap();
    assert!(
        refused("UPDATE storage_locations SET path = '/y' WHERE built_in = 0")
            .contains("never changed")
    );
    assert!(refused("DELETE FROM storage_locations WHERE built_in = 0").contains("never deleted"));
    assert!(refused("DELETE FROM daemon_settings").contains("never deleted"));

    // A default location of another kind than its column.
    assert!(
        refused(
            "UPDATE daemon_settings SET data_location_id = \
             '0199f000-0000-7000-8000-0000000000c2'"
        )
        .contains("another kind")
    );
    // A path names a location of its own root's kind, also when it changes.
    assert!(
        refused(
            "UPDATE instance_paths SET location_id = '0199f000-0000-7000-8000-0000000000c2' \
             WHERE kind = 'data'"
        )
        .contains("another kind than its root")
    );
    // A location in use cannot be replaced by nothing.
    assert!(
        refused("UPDATE instance_paths SET location_id = NULL WHERE kind = 'data'")
            .contains("CHECK constraint failed")
    );
}
