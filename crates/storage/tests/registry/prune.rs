//! Retention on the SQLite registry (spec 0005 step 12, AC-14): age and
//! row limits, rows that are never deleted, batches of 5,000 rows (500
//! operations, whose journal rows go by cascade, spec 0007), and no change
//! to the generation.

use jiff::{SignedDuration, Timestamp};
use rusqlite::Connection;
use stackly_domain::{
    AuditRetentionDays, InstanceId, OperationId, PruneReport, Registry, RetentionPolicy,
    SettingsPatch, WriteOp,
};

use stackly_testkit::evidence::{Kind, evidence};

use crate::common::Harness;

const DAY: i64 = 24 * 60 * 60;

fn days_before(now: Timestamp, days: i64) -> String {
    let at = now
        .checked_sub(SignedDuration::from_secs(days * DAY))
        .expect("test sized durations stay in range");
    format!("{at:.9}")
}

fn seed_operation(conn: &Connection, state: &str, updated_at: &str) {
    let failure = if state == "failed" {
        ("'disk_full'", "0")
    } else {
        ("NULL", "NULL")
    };
    conn.execute(
        &format!(
            "INSERT INTO operations (id, kind, resource_kind, state, failure_code, \
             failure_retryable, revision, created_at, updated_at) \
             VALUES (?1, 'update_manager', 'manager', ?2, {}, {}, 1, ?3, ?3)",
            failure.0, failure.1
        ),
        (OperationId::generate().to_string(), state, updated_at),
    )
    .expect("an operation row can be seeded");
}

fn seed_audit_event(conn: &Connection, occurred_at: &str) {
    conn.execute(
        "INSERT INTO audit_events (id, actor, action, result, occurred_at) \
         VALUES (?1, 'daemon', 'instance.start', 'succeeded', ?2)",
        (OperationId::generate().to_string(), occurred_at),
    )
    .expect("an audit event can be seeded");
}

fn seed_lease(conn: &Connection, instance: InstanceId, state: &str, changed_at: Option<&str>) {
    conn.execute(
        "INSERT INTO instance_demands (id, instance_id, source_kind, state, state_changed_at, \
         acquired_at) VALUES (?1, ?2, 'manual', ?3, ?4, '2025-01-01T00:00:00.000000000Z')",
        (
            OperationId::generate().to_string(),
            instance.to_string(),
            state,
            changed_at,
        ),
    )
    .expect("a lease can be seeded");
}

fn seed_observation(conn: &Connection, instance: InstanceId, observed_at: &str) {
    conn.execute(
        "INSERT INTO process_observations (instance_id, pid, start_identity, \
         executable_identity, boot_id, session_id, observed_at) VALUES (?1, 4242, \
         '1700000000.000000', '/opt/pg/bin/postgres', '00000000-0000-4000-8000-000000004242', \
         '4242', ?2)",
        (instance.to_string(), observed_at),
    )
    .expect("an observation can be seeded");
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0))
        .expect("the count runs")
}

#[test]
fn prune_keeps_unfinished_operations_and_bounds_history() {
    let ev = evidence!(
        covers = ["NFR-011"],
        kind = Kind::Fixture,
        expected = "prune on the SQLite registry deletes finished operations and audit events \
                    past the age or row limit, released leases past the age limit, and process \
                    observations past the newest per instance, keeps unfinished and \
                    needs_recovery operations and active leases, and leaves the generation as it \
                    was",
    );
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    let other = harness.instance(&base, "other");
    harness.commit(vec![
        WriteOp::InsertInstance(instance.clone()),
        WriteOp::InsertInstance(other.clone()),
    ]);
    let now = harness.now();
    let side = harness.side();
    for days in [31, 45, 400] {
        seed_operation(&side, "succeeded", &days_before(now, days));
    }
    for (days, state) in [
        (1, "succeeded"),
        (2, "failed"),
        (3, "cancelled"),
        (4, "succeeded"),
        (5, "succeeded"),
    ] {
        seed_operation(&side, state, &days_before(now, days));
    }
    seed_operation(&side, "needs_recovery", &days_before(now, 90));
    seed_operation(&side, "running", &days_before(now, 90));
    for days in [31, 60, 1, 2, 3, 4] {
        seed_audit_event(&side, &days_before(now, days));
    }
    seed_lease(
        &side,
        instance.id(),
        "released",
        Some(&days_before(now, 40)),
    );
    seed_lease(&side, instance.id(), "released", Some(&days_before(now, 1)));
    seed_lease(&side, instance.id(), "active", None);
    for minute in 0..25 {
        let at = now
            .checked_sub(SignedDuration::from_mins(minute))
            .expect("in range");
        seed_observation(&side, instance.id(), &format!("{at:.9}"));
    }
    seed_observation(&side, other.id(), &days_before(now, 400));
    let generation = harness.registry.known_instance_ids().unwrap().generation;
    let policy = RetentionPolicy {
        operation_max_rows: 3,
        audit_max_rows: 3,
        ..RetentionPolicy::DEFAULT
    };

    let report = harness.registry.prune(&policy, now).unwrap();

    let mut expected = PruneReport::default();
    expected.operations = 5;
    expected.audit_events = 3;
    expected.released_leases = 1;
    expected.process_observations = 5;
    assert_eq!(report, expected);
    assert_eq!(
        count(
            &side,
            "SELECT count(*) FROM operations WHERE state IN ('needs_recovery', 'running')"
        ),
        2
    );
    let kept: Vec<String> = side
        .prepare("SELECT updated_at FROM operations WHERE state IN ('succeeded', 'failed', 'cancelled') ORDER BY updated_at DESC")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(kept, [1, 2, 3].map(|days| days_before(now, days)).to_vec());
    assert_eq!(count(&side, "SELECT count(*) FROM audit_events"), 3);
    assert_eq!(
        count(
            &side,
            "SELECT count(*) FROM instance_demands WHERE state = 'active'"
        ),
        1
    );
    let observations = format!(
        "SELECT count(*) FROM process_observations WHERE instance_id = '{}'",
        instance.id()
    );
    assert_eq!(count(&side, &observations), 20);
    assert_eq!(
        count(&side, "SELECT count(*) FROM process_observations"),
        21
    );
    assert_eq!(
        harness.registry.known_instance_ids().unwrap().generation,
        generation
    );
    assert_eq!(harness.registry.prune(&policy, now).unwrap().total(), 0);
    ev.pass();
}

#[test]
fn prune_deletes_large_histories_in_several_batches() {
    let harness = Harness::new();
    let now = harness.now();
    let side = harness.side();
    let old = days_before(now, 31);
    side.execute_batch("BEGIN").unwrap();
    for _ in 0..12_001 {
        seed_audit_event(&side, &old);
    }
    seed_audit_event(&side, &days_before(now, 1));
    side.execute_batch("COMMIT").unwrap();
    let generation = harness.registry.known_instance_ids().unwrap().generation;

    let report = harness
        .registry
        .prune(&RetentionPolicy::DEFAULT, now)
        .unwrap();

    assert_eq!(report.audit_events, 12_001);
    assert_eq!(report.total(), 12_001);
    assert_eq!(count(&side, "SELECT count(*) FROM audit_events"), 1);
    assert_eq!(
        harness.registry.known_instance_ids().unwrap().generation,
        generation
    );
}

#[test]
fn prune_deletes_operations_in_batches_with_their_journal_rows() {
    let harness = Harness::new();
    let now = harness.now();
    let side = harness.side();
    let old = days_before(now, 31);
    side.execute_batch("BEGIN").unwrap();
    for index in 0..1_201 {
        let id = OperationId::generate().to_string();
        side.execute(
            "INSERT INTO operations (id, kind, resource_kind, state, revision, created_at, \
             updated_at) VALUES (?1, 'update_manager', 'manager', 'succeeded', 1, ?2, ?2)",
            (&id, &old),
        )
        .unwrap();
        side.execute(
            "INSERT INTO operation_journal (operation_id, trigger_origin, trigger_interactive, \
             resumable, input) VALUES (?1, 'background', 0, 0, '{}')",
            [&id],
        )
        .unwrap();
        side.execute(
            "INSERT INTO operation_steps (operation_id, seq, name, repeatable, reversible, \
             state, attempts, started_at, finished_at) \
             VALUES (?1, 1, 'swap', 0, 0, 'done', 1, ?2, ?2)",
            (&id, &old),
        )
        .unwrap();
        side.execute(
            "INSERT INTO operation_created (operation_id, seq, step_seq, item_kind, item_ref, \
             state, recorded_at, settled_at) \
             VALUES (?1, 1, 1, 'custom', ?2, 'kept', ?3, ?3)",
            (&id, format!("manager_binary:{index}"), &old),
        )
        .unwrap();
        side.execute(
            "INSERT INTO operation_locks (operation_id, lock_key) VALUES (?1, 'manager:self')",
            [&id],
        )
        .unwrap();
    }
    side.execute_batch("COMMIT").unwrap();

    let report = harness
        .registry
        .prune(&RetentionPolicy::DEFAULT, now)
        .unwrap();

    assert_eq!(report.operations, 1_201);
    assert_eq!(report.total(), 1_201, "journal rows are not counted");
    for table in [
        "operations",
        "operation_journal",
        "operation_steps",
        "operation_created",
        "operation_locks",
    ] {
        assert_eq!(
            count(&side, &format!("SELECT count(*) FROM {table}")),
            0,
            "{table} keeps no row of a pruned operation"
        );
    }
}

/// Every `occurred_at` of the audit rows, newest first.
fn audit_times(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT occurred_at FROM audit_events ORDER BY occurred_at DESC")
        .expect("audit_events can be read")
        .query_map([], |row| row.get(0))
        .expect("audit_events can be queried")
        .collect::<Result<_, _>>()
        .expect("every audit time reads")
}

// covers: AC-6 (spec 0035)
#[test]
fn the_audit_retention_setting_sets_the_age_of_audit_rows_and_no_other_age() {
    let ev = evidence!(
        covers = ["REQ-045"],
        kind = Kind::Fixture,
        expected = "spec 0035 AC-6: with the audit retention stored as 7, 30, or 90 days, a \
                    prune with the policy built from the stored setting deletes exactly the \
                    audit rows older than that many days, while finished operations and \
                    released leases still go after 30 days",
    );
    for (days, kept) in [
        (AuditRetentionDays::Days7, vec![1, 6]),
        (AuditRetentionDays::Days30, vec![1, 6, 8, 29]),
        (AuditRetentionDays::Days90, vec![1, 6, 8, 29, 31, 89]),
    ] {
        let harness = Harness::new();
        let base = harness.base();
        let instance = harness.instance(&base, "main");
        harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);
        let before = harness.registry.daemon_settings().unwrap().value;
        harness.commit(vec![WriteOp::UpdateDaemonSettings {
            patch: SettingsPatch {
                audit_retention: Some(days),
                ..SettingsPatch::default()
            },
            expected: before.revision,
            at: harness.now(),
        }]);
        let now = harness.now();
        let side = harness.side();
        for age in [1, 6, 8, 29, 31, 89, 91] {
            seed_audit_event(&side, &days_before(now, age));
        }
        for age in [29, 31] {
            seed_operation(&side, "succeeded", &days_before(now, age));
            seed_lease(
                &side,
                instance.id(),
                "released",
                Some(&days_before(now, age)),
            );
        }

        // The policy the daemon builds before every prune (spec 0035, AC-6).
        let stored = harness.registry.daemon_settings().unwrap().value;
        assert_eq!(stored.audit_retention, days);
        let policy = RetentionPolicy::DEFAULT.with_audit_days(stored.audit_retention);
        let report = harness.registry.prune(&policy, now).unwrap();

        assert_eq!(
            audit_times(&side),
            kept.iter()
                .map(|age| days_before(now, *age))
                .collect::<Vec<_>>(),
            "{days:?}"
        );
        assert_eq!(
            report.audit_events,
            7 - u64::try_from(kept.len()).unwrap(),
            "{days:?}"
        );
        assert_eq!(
            (report.operations, report.released_leases),
            (1, 1),
            "{days:?}: the other ages stay at 30 days"
        );
        assert_eq!(
            count(&side, "SELECT count(*) FROM operations"),
            1,
            "{days:?}"
        );
    }
    ev.pass();
}
