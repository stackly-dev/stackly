//! Creating a registry in a clean data root (AC-1, AC-2, AC-3).

use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::{Arc, Barrier};

use stackly_storage::{APPLICATION_ID, MigrationSet, OpenOptions, RegistryOpen, StorageError};

use crate::common::{APP_VERSION, Harness, mode_of, ready, suffixed};

const TEMP: &str = ".stackly.sqlite.creating";

#[test]
fn a_clean_root_creates_the_registry_with_the_fake_clock_and_private_files() {
    let harness = Harness::new();
    let now = jiff::Timestamp::constant(1_792_300_000, 123_456_789);
    harness.fake.clock().set(now);

    let (registry, summary) = ready(harness.open());

    assert!(summary.created);
    assert_eq!(summary.migrated_from, None);
    assert_eq!(summary.snapshot, None);
    assert_eq!(summary.schema_version, 1);
    assert_eq!(summary.opened_at, now);
    assert_eq!(summary.sqlite_version, rusqlite::version());
    assert!(!harness.layout.registry_dir().join(TEMP).exists());
    for path in [harness.db(), harness.wal(), suffixed(&harness.db(), "-shm")] {
        assert_eq!(mode_of(&path), 0o600, "{}", path.display());
    }
    for dir in [
        harness.layout.registry_snapshots_dir(),
        harness.layout.registry_recovery_dir(),
    ] {
        assert_eq!(mode_of(&dir), 0o700, "{}", dir.display());
    }
    let conn = harness.connect();
    let (id, created_at, version, generation): (String, String, String, i64) = conn
        .query_row(
            "SELECT registry_id, created_at, created_by_version, generation FROM registry_info",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(id, summary.registry_id.to_string());
    assert_eq!(created_at, "2026-10-18T05:06:40.123456789Z");
    assert_eq!((version.as_str(), generation), (APP_VERSION, 0));
    let migration = MigrationSet::builtin().migrations()[0];
    let (sha256, applied_at, app_version): (String, String, String) = conn
        .query_row(
            "SELECT sha256, applied_at, app_version FROM schema_migrations WHERE version = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(sha256, migration.sha256());
    assert_eq!(applied_at, created_at);
    assert_eq!(app_version, APP_VERSION);
    let application_id: i64 = conn
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .unwrap();
    assert_eq!(application_id, i64::from(APPLICATION_ID));
    drop(conn);
    drop(registry);
}

#[test]
fn closing_the_registry_checkpoints_and_removes_the_wal() {
    let harness = Harness::new();
    let (registry, _) = ready(harness.open());
    assert!(harness.wal().exists());

    drop(registry);

    assert!(!harness.wal().exists());
}

#[test]
fn a_second_open_finds_the_registry_and_creates_nothing() {
    let harness = Harness::new();
    let first = harness.create();

    let (registry, second) = ready(harness.open());

    assert!(!second.created);
    assert_eq!(second.registry_id, first.registry_id);
    assert_eq!(second.migrated_from, None);
    assert!(harness.snapshots().is_empty());
    assert!(harness.reports().is_empty());
    drop(registry);
}

#[test]
fn a_leftover_create_file_whose_lock_is_free_is_removed_first() {
    let harness = Harness::new();
    let temp = harness.layout.registry_dir().join(TEMP);
    fs::write(&temp, b"half written").unwrap();
    fs::write(suffixed(&temp, "-journal"), b"journal").unwrap();

    let (registry, summary) = ready(harness.open());

    assert!(summary.created);
    assert!(!temp.exists());
    assert!(!suffixed(&temp, "-journal").exists());
    drop(registry);
}

#[test]
fn a_create_file_locked_by_someone_else_means_create_in_progress() {
    let harness = Harness::new();
    let temp = harness.layout.registry_dir().join(TEMP);
    let held = File::create(&temp).unwrap();
    held.try_lock().unwrap();

    let err = harness.try_open_with(&OpenOptions::new()).unwrap_err();

    assert_eq!(err, StorageError::CreateInProgress { path: temp.clone() });
    assert!(!harness.db().exists());
    assert!(temp.exists());
}

#[test]
fn concurrent_opens_in_a_clean_root_create_at_most_one_registry() {
    let harness = Arc::new(Harness::new());
    let threads = 8;
    let barrier = Arc::new(Barrier::new(threads));

    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let harness = Arc::clone(&harness);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                match harness.try_open_with(&OpenOptions::new()) {
                    Ok(RegistryOpen::Ready { summary, .. }) => {
                        Ok((summary.created, summary.registry_id))
                    }
                    Ok(RegistryOpen::Recovery(report)) => panic!("unexpected recovery {report:#?}"),
                    Err(err) => Err(err),
                }
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();

    let created = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok((true, _))))
        .count();
    assert!(created <= 1, "{outcomes:?}");
    for outcome in &outcomes {
        assert!(
            matches!(outcome, Ok(_) | Err(StorageError::CreateInProgress { .. })),
            "{outcome:?}"
        );
    }
    let (registry, last) = ready(harness.open());
    for (_, id) in outcomes.iter().flatten() {
        assert_eq!(*id, last.registry_id, "a registry was replaced");
    }
    drop(registry);
}

#[test]
fn a_registry_folder_open_to_others_is_unusable() {
    let harness = Harness::new();
    let dir = harness.layout.registry_dir();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

    let err = harness.try_open_with(&OpenOptions::new()).unwrap_err();

    assert_eq!(
        err,
        StorageError::RegistryDirUnusable {
            path: dir,
            reason: "group or others have access",
        }
    );
    assert!(!harness.db().exists());
}

#[test]
fn a_symlinked_snapshot_folder_is_unusable_and_a_missing_registry_folder_too() {
    let harness = Harness::new();
    let elsewhere = harness.root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, harness.layout.registry_snapshots_dir()).unwrap();

    let err = harness.try_open_with(&OpenOptions::new()).unwrap_err();
    assert_eq!(
        err,
        StorageError::RegistryDirUnusable {
            path: harness.layout.registry_snapshots_dir(),
            reason: "a symlink",
        }
    );

    let bare = Harness::new();
    fs::remove_dir(bare.layout.registry_dir()).unwrap();
    let err = bare.try_open_with(&OpenOptions::new()).unwrap_err();
    assert!(
        matches!(err, StorageError::RegistryDirUnusable { ref path, .. } if *path == bare.layout.registry_dir()),
        "{err:?}"
    );
}
