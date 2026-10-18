//! Every recovery reason the inspection detects, each leaving the registry
//! file and its WAL byte for byte as they were (AC-6).

use std::fs::{self, OpenOptions as FileOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::time::Duration;

use stackly_storage::{OpenOptions, RecoveryReason, RecoveryReport};
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, recovery};

/// Opens the registry, expects recovery with `code`, and checks that the
/// registry and its WAL are unchanged.
fn expect_recovery(harness: &Harness, code: &str) -> RecoveryReport {
    expect_recovery_with(harness, code, &OpenOptions::new())
}

fn expect_recovery_with(harness: &Harness, code: &str, options: &OpenOptions) -> RecoveryReport {
    let before = harness.digests();
    let report = recovery(harness.open_with(options));
    assert_eq!(report.reason.code(), code, "{:#?}", report.reason);
    assert_eq!(harness.digests(), before, "the registry files changed");
    assert!(
        report
            .path
            .starts_with(harness.layout.registry_recovery_dir())
    );
    assert!(report.path.exists());
    report
}

/// Writes `bytes` at `offset` in `path`.
fn patch(path: &std::path::Path, offset: u64, bytes: &[u8]) {
    let mut file = FileOptions::new()
        .write(true)
        .open(path)
        .expect("test setup step succeeds");
    file.seek(SeekFrom::Start(offset))
        .expect("test setup step succeeds");
    file.write_all(bytes).expect("test setup step succeeds");
    file.sync_all().expect("test setup step succeeds");
}

#[test]
fn a_text_file_or_another_application_id_is_not_a_registry() {
    let text = Harness::new();
    fs::write(text.db(), b"just some text, not a database at all").unwrap();
    expect_recovery(&text, "not_a_registry");

    let foreign = Harness::new();
    let conn = foreign.connect();
    conn.execute_batch("PRAGMA application_id = 42; CREATE TABLE t (x);")
        .unwrap();
    drop(conn);
    let report = expect_recovery(&foreign, "not_a_registry");
    assert!(
        matches!(&report.reason, RecoveryReason::NotARegistry { detail } if detail.contains("0x2a")),
        "{:?}",
        report.reason
    );
}

#[test]
fn a_symlinked_registry_or_a_wal_that_is_a_folder_is_unsafe() {
    let harness = Harness::new();
    harness.create();
    let real = harness.root.path().join("elsewhere.sqlite");
    fs::rename(harness.db(), &real).unwrap();
    symlink(&real, harness.db()).unwrap();
    let real_digest = crate::common::sha256_of(&real);

    let report = recovery(harness.open());

    assert!(
        matches!(&report.reason, RecoveryReason::UnsafeRegistryFile { file, .. } if file == "stackly.sqlite"),
        "{:?}",
        report.reason
    );
    assert_eq!(crate::common::sha256_of(&real), real_digest);
    assert_eq!(report.files[0].sha256, None, "a symlink is never followed");

    let other = Harness::new();
    other.create();
    fs::create_dir(other.wal()).unwrap();
    let db_digest = crate::common::sha256_of(&other.db());
    let report = recovery(other.open());
    assert_eq!(crate::common::sha256_of(&other.db()), db_digest);
    assert!(
        matches!(&report.reason, RecoveryReason::UnsafeRegistryFile { file, .. } if file == "stackly.sqlite-wal"),
        "{:?}",
        report.reason
    );
}

#[test]
fn a_registry_that_cannot_be_read_is_open_failed() {
    let harness = Harness::new();
    harness.create();
    let before = harness.digests();
    fs::set_permissions(harness.db(), fs::Permissions::from_mode(0o000)).unwrap();

    let report = recovery(harness.open());

    fs::set_permissions(harness.db(), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(report.reason.code(), "open_failed", "{:?}", report.reason);
    assert_eq!(harness.digests(), before);
}

#[test]
fn a_damaged_page_or_a_missing_registry_info_row_fails_integrity() {
    let damaged = Harness::new();
    damaged.create();
    let page_size: i64 = damaged
        .connect()
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .unwrap();
    patch(
        &damaged.db(),
        u64::try_from(page_size * 3).unwrap(),
        &[0xa5; 64],
    );
    expect_recovery(&damaged, "integrity_failed");

    let missing = Harness::new();
    missing.create();
    missing
        .connect()
        .execute("DELETE FROM registry_info", [])
        .unwrap();
    let report = expect_recovery(&missing, "integrity_failed");
    assert!(
        matches!(&report.reason, RecoveryReason::IntegrityFailed { detail } if detail.contains("registry_info")),
        "{:?}",
        report.reason
    );
}

#[test]
fn a_user_version_newer_than_the_app_is_schema_newer_than_app() {
    let harness = Harness::new();
    harness.create();
    patch(&harness.db(), 60, &999_u32.to_be_bytes());

    let report = expect_recovery(&harness, "schema_newer_than_app");

    assert_eq!(report.db_schema_version, Some(999));
    assert_eq!(report.app_schema_version, 1);
}

#[test]
fn a_missing_or_changed_migration_row_is_a_history_mismatch() {
    let missing = Harness::new();
    missing.create();
    missing
        .connect()
        .execute("DELETE FROM schema_migrations", [])
        .unwrap();
    expect_recovery(&missing, "migration_history_mismatch");

    let changed = Harness::new();
    changed.create();
    changed
        .connect()
        .execute(
            "UPDATE schema_migrations SET sha256 = ?1",
            [&"0".repeat(64)],
        )
        .unwrap();
    let report = expect_recovery(&changed, "migration_history_mismatch");
    assert!(
        matches!(&report.reason, RecoveryReason::MigrationHistoryMismatch { detail } if detail.contains("checksum")),
        "{:?}",
        report.reason
    );
}

#[test]
fn an_index_created_outside_the_runner_is_a_fingerprint_mismatch_even_in_an_uncheckpointed_wal() {
    let harness = Harness::new();
    harness.create();
    let writer = harness.connect();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute_batch("CREATE INDEX manual_index ON settings (updated_at);")
        .unwrap();
    let wal_len = fs::metadata(harness.wal()).unwrap().len();
    assert!(wal_len > 0, "the index lives only in the WAL");

    let report = expect_recovery(&harness, "schema_fingerprint_mismatch");

    assert_eq!(fs::metadata(harness.wal()).unwrap().len(), wal_len);
    assert_eq!(report.db_schema_version, Some(1));
    drop(writer);
}

#[test]
fn half_applied_schema_is_detected_at_start() {
    let ev = evidence!(
        covers = ["AC-065"],
        kind = Kind::Fixture,
        expected = "a registry whose schema was half changed outside the migration runner is \
                    detected at start and enters recovery without being written",
    );
    let harness = Harness::new();
    harness.create();
    harness
        .connect()
        .execute_batch("ALTER TABLE runtimes ADD COLUMN notes TEXT;")
        .unwrap();

    let report = expect_recovery(&harness, "schema_fingerprint_mismatch");

    assert!(
        matches!(&report.reason, RecoveryReason::SchemaFingerprintMismatch { expected, actual } if expected != actual),
        "{:?}",
        report.reason
    );
    ev.pass();
}

#[test]
fn a_missing_registry_with_leftovers_is_registry_missing_and_nothing_is_created() {
    let harness = Harness::new();
    harness.create();
    fs::remove_file(harness.db()).unwrap();
    fs::create_dir(harness.layout.runtimes_dir().join("postgresql")).unwrap();
    fs::write(harness.layout.instances_dir().join(".DS_Store"), b"").unwrap();

    let report = recovery(harness.open());

    match &report.reason {
        RecoveryReason::RegistryMissing { evidence } => {
            let runtime = harness.layout.runtimes_dir().join("postgresql");
            assert_eq!(evidence, &[runtime.display().to_string()]);
        }
        other => panic!("expected registry_missing, got {other:?}"),
    }
    assert!(!harness.db().exists(), "no new registry");

    let wal_only = Harness::new();
    fs::write(wal_only.wal(), b"").unwrap();
    let report = recovery(wal_only.open());
    assert_eq!(report.reason.code(), "registry_missing");
    assert!(!wal_only.db().exists());
}

#[test]
fn a_write_transaction_held_past_the_busy_timeout_is_registry_locked() {
    let harness = Harness::new();
    harness.create();
    let holder = harness.connect();
    holder.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let options = OpenOptions::new().with_busy_timeout(Duration::from_millis(50));

    expect_recovery_with(&harness, "registry_locked", &options);

    holder.execute_batch("ROLLBACK;").unwrap();
}

#[test]
fn the_same_problem_on_the_same_files_reuses_the_latest_report() {
    let harness = Harness::new();
    harness.create();
    patch(&harness.db(), 60, &999_u32.to_be_bytes());

    let first = expect_recovery(&harness, "schema_newer_than_app");
    harness
        .fake
        .clock()
        .advance(jiff::SignedDuration::from_hours(1));
    let second = expect_recovery(&harness, "schema_newer_than_app");

    assert_eq!(second.path, first.path);
    assert_eq!(harness.reports().len(), 1);
}
