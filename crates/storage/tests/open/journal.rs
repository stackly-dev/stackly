//! Migration 2, the operation journal of spec 0007 (AC-17): migrating the
//! version 1 fixture keeps the newest operation per idempotency key, makes
//! the key unique, and backfills one journal row per operation; and the
//! journal rows of the frozen version 2 fixture read back through the
//! registry.

use rusqlite::Connection;
use stackly_domain::{
    CreatedItem, CreatedState, LockKey, OperationId, OperationKind, RecoveryCause, Registry,
    StepState, TriggerOrigin,
};

use crate::common::{Harness, ready, row_counts};
use crate::fixture::fixture;

const DUPLICATE_KEY: &str = "gui-retry-7";

/// Three operations that share a key, the newest last by `(created_at,
/// id)`: the same time breaks the tie by ID.
const DUPLICATES: [(&str, &str); 3] = [
    (
        "0199f000-0000-7000-8000-0000000000a0",
        "2026-01-01T00:00:00.000000000Z",
    ),
    (
        "0199f000-0000-7000-8000-0000000000a1",
        "2026-01-03T00:00:00.000000000Z",
    ),
    (
        "0199f000-0000-7000-8000-0000000000a2",
        "2026-01-03T00:00:00.000000000Z",
    ),
];

fn insert_duplicates(conn: &Connection) {
    for (id, at) in DUPLICATES {
        conn.execute(
            "INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, \
             state, revision, created_at, updated_at) \
             VALUES (?1, 'stop_instance', 'manager', NULL, ?2, 'succeeded', 1, ?3, ?3)",
            (id, DUPLICATE_KEY, at),
        )
        .expect("test setup step succeeds");
    }
}

fn keys(conn: &Connection) -> Vec<(String, Option<String>)> {
    conn.prepare("SELECT id, idempotency_key FROM operations ORDER BY id")
        .expect("the operations table exists")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("the key query runs")
        .collect::<rusqlite::Result<_>>()
        .expect("every key row reads")
}

#[test]
fn migrating_v1_keeps_the_newest_key_and_backfills_a_journal_per_operation() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(1));
    insert_duplicates(&harness.connect());
    // Migration 3 drops the version 1 secret_references rows on purpose
    // (spec 0005 "Tambahan fitur 11"); every other table keeps its rows.
    let kept = |table: &str| {
        table != "schema_migrations"
            && table != "compatibility_records"
            && !table.starts_with("secret_")
    };
    let mut before = row_counts(&harness.connect());
    before.retain(|(table, _)| kept(table));

    let (registry, summary) = ready(harness.open());

    assert_eq!(summary.migrated_from, Some(1));
    assert_eq!(
        summary.schema_version,
        stackly_storage::MigrationSet::builtin().latest()
    );
    let conn = harness.connect();
    let mut after = row_counts(&conn);
    after.retain(|(table, _)| kept(table) && !table.starts_with("operation_"));
    assert_eq!(after, before, "the version 1 data survives");

    let stored = keys(&conn);
    let key_of = |id: &str| {
        stored
            .iter()
            .find(|(stored_id, _)| stored_id == id)
            .and_then(|(_, key)| key.clone())
    };
    assert_eq!(key_of(DUPLICATES[0].0), None);
    assert_eq!(key_of(DUPLICATES[1].0), None);
    assert_eq!(key_of(DUPLICATES[2].0).as_deref(), Some(DUPLICATE_KEY));
    assert_eq!(
        key_of("0199f000-0000-7000-8000-000000000050").as_deref(),
        Some("cli-create-1"),
        "a key no other operation used stays"
    );
    let err = conn
        .execute(
            "UPDATE operations SET idempotency_key = ?1 WHERE id = ?2",
            (DUPLICATE_KEY, DUPLICATES[0].0),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("UNIQUE constraint failed"),
        "{err}"
    );

    let journals: Vec<(String, String, String, i64, i64, String)> = conn
        .prepare(
            "SELECT o.id, o.kind, j.trigger_origin, j.trigger_interactive, j.resumable, j.input \
             FROM operations o JOIN operation_journal j ON j.operation_id = o.id ORDER BY o.id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(journals.len(), stored.len(), "one journal per operation");
    for (id, kind, origin, interactive, resumable, input) in journals {
        let expected = kind
            .parse::<OperationKind>()
            .is_ok_and(OperationKind::resumable);
        assert_eq!(
            (origin.as_str(), interactive, resumable == 1, input.as_str()),
            ("background", 0, expected, "{}"),
            "journal of {id} ({kind})"
        );
    }

    let view = registry
        .operation_journal("0199f000-0000-7000-8000-000000000050".parse().unwrap())
        .unwrap()
        .value
        .expect("a backfilled journal reads back");
    assert_eq!(view.journal.trigger().origin(), TriggerOrigin::Background);
    assert!(view.steps.is_empty() && view.created.is_empty() && view.locks.is_empty());
}

#[test]
fn the_resumable_kinds_of_migration_2_match_the_domain() {
    let sql = stackly_storage::MigrationSet::builtin()
        .migrations()
        .iter()
        .find(|migration| migration.version == 2)
        .expect("migration 2 is built in")
        .sql;
    let case = sql
        .split("CASE WHEN kind IN (")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .expect("migration 2 derives resumable with a CASE on the kind");
    let mut in_sql: Vec<&str> = case
        .split(',')
        .map(|code| code.trim().trim_matches('\''))
        .collect();
    in_sql.sort_unstable();
    let mut in_domain: Vec<&str> = OperationKind::ALL
        .iter()
        .filter(|kind| kind.resumable())
        .map(|kind| kind.as_code())
        .collect();
    in_domain.sort_unstable();

    assert_eq!(in_sql, in_domain);
}

#[test]
fn the_v2_fixture_journal_rows_read_back_through_the_registry() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(2));
    let (registry, _) = ready(harness.open());
    let read = |id: &str| {
        registry
            .operation_journal(id.parse::<OperationId>().unwrap())
            .unwrap()
            .value
            .unwrap_or_else(|| panic!("the fixture has a journal for {id}"))
    };

    let compensating = read("0199f000-0000-7000-8000-000000000052");
    let journal = &compensating.journal;
    assert_eq!(
        journal.method().map(|method| method.as_str()),
        Some("instance.remove")
    );
    assert_eq!(
        journal.trigger().client().map(|client| client.as_str()),
        Some("gui")
    );
    assert_eq!(
        journal.recovery_cause(),
        Some(RecoveryCause::CompensationFailed)
    );
    let error = journal
        .failure_error()
        .expect("the fixture stores an error");
    assert_eq!(error.code().as_str(), "operation.compensation_failed");
    let details: Vec<(&str, &str)> = error
        .details()
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    assert_eq!(
        details,
        [("item", "instance_tree"), ("detail", "permission_denied")]
    );
    let steps: Vec<(u16, StepState, u32)> = compensating
        .steps
        .iter()
        .map(|step| (step.seq().get(), step.state(), step.attempts()))
        .collect();
    assert_eq!(steps, [(1, StepState::Done, 1), (2, StepState::Failed, 2)]);
    assert_eq!(
        compensating.steps[1].error_code().map(|code| code.as_str()),
        Some("instance.rebind_failed")
    );
    assert_eq!(compensating.created.len(), 1);
    assert_eq!(
        (
            compensating.created[0].item(),
            compensating.created[0].state()
        ),
        (
            &CreatedItem::InstanceTree("0199f000-0000-7000-8000-000000000031".parse().unwrap()),
            CreatedState::CompensationFailed
        )
    );
    assert_eq!(
        compensating.locks,
        [LockKey::Instance(
            "0199f000-0000-7000-8000-000000000031".parse().unwrap()
        )]
    );

    let running = read("0199f000-0000-7000-8000-000000000053");
    assert!(running.journal.resumable());
    assert_eq!(
        running
            .journal
            .waiting_on()
            .map(|id| id.to_string())
            .as_deref(),
        Some("0199f000-0000-7000-8000-000000000052")
    );
    assert_eq!(
        running.created[0].payload().map(|payload| payload.as_str()),
        Some(r#"{"pid":4242}"#)
    );
    assert_eq!(
        read("0199f000-0000-7000-8000-000000000055").locks,
        [LockKey::Manager]
    );
    let journals = registry
        .operation_journals(&[
            "0199f000-0000-7000-8000-000000000054".parse().unwrap(),
            "0199f000-0000-7000-8000-000000000050".parse().unwrap(),
        ])
        .unwrap()
        .value;
    assert_eq!(
        journals
            .iter()
            .map(|journal| journal.operation_id().to_string())
            .collect::<Vec<_>>(),
        [
            "0199f000-0000-7000-8000-000000000054",
            "0199f000-0000-7000-8000-000000000050"
        ]
    );
    assert_eq!(
        journals[1].result().map(|result| result.as_str()),
        Some(r#"{"resources":[]}"#)
    );
}
