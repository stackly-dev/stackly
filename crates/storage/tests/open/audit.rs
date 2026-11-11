//! Migration 12 (spec 0035, step 1): the operation fields of an audit row and
//! the audit retention setting, added by `ALTER TABLE ... ADD COLUMN`.

use rusqlite::Connection;
use stackly_domain::{AuditQuery, AuditRetentionDays, OperationKind, Registry};

use crate::common::{Harness, ready, row_counts, schema_rows};
use crate::fixture::fixture;

/// Every audit row, by ID: the new columns as text.
fn audit_columns(conn: &Connection) -> Vec<[Option<String>; 4]> {
    conn.prepare(
        "SELECT instance_id, operation_kind, CAST(config_revision AS TEXT), client \
         FROM audit_events ORDER BY id",
    )
    .expect("audit_events can be read")
    .query_map([], |row| {
        Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?])
    })
    .expect("audit_events can be queried")
    .collect::<rusqlite::Result<_>>()
    .expect("every audit row reads")
}

/// The fixture of version 11, migrated to the newest version.
fn migrated() -> (Harness, Connection) {
    let harness = Harness::new();
    harness.load_fixture(&fixture(11));
    drop(ready(harness.open()));
    let conn = harness.connect();
    conn.pragma_update(None, "foreign_keys", true)
        .expect("foreign keys can be turned on");
    (harness, conn)
}

// covers: AC-7 (spec 0035)
#[test]
fn a_version_11_registry_migrates_with_every_row_and_null_operation_fields() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(11));
    let before = row_counts(&harness.connect());
    assert!(
        before
            .iter()
            .any(|(table, count)| table == "audit_events" && *count > 0),
        "the version 11 fixture holds audit rows"
    );

    let (registry, summary) = ready(harness.open());

    assert_eq!(summary.migrated_from, Some(11));
    assert!(summary.snapshot.is_some(), "a migration takes a snapshot");
    let conn = harness.connect();
    let kept = |counts: Vec<(String, i64)>| -> Vec<(String, i64)> {
        counts
            .into_iter()
            .filter(|(table, _)| table != "schema_migrations" && table != "provider_imports")
            .collect()
    };
    assert_eq!(kept(row_counts(&conn)), kept(before), "every row stays");
    let columns = audit_columns(&conn);
    assert!(!columns.is_empty());
    assert!(
        columns.iter().all(|row| row.iter().all(Option::is_none)),
        "an old audit row has no instance, kind, revision, or client: {columns:?}"
    );

    let settings = registry.daemon_settings().unwrap().value;
    assert_eq!(settings.audit_retention, AuditRetentionDays::Days30);
    let page = registry
        .audit_events(AuditQuery::page(500, None))
        .unwrap()
        .value;
    assert_eq!(page.items.len(), columns.len(), "every old row reads");
    assert!(page.items.iter().all(|record| record.instance.is_none()
        && record.operation_kind.is_none()
        && record.config_revision.is_none()
        && record.client.is_none()));
}

// covers: AC-7 (spec 0035)
#[test]
fn migration_12_adds_the_partial_instance_index_and_keeps_audit_rows_append_only() {
    let (_harness, conn) = migrated();
    let index = schema_rows(&conn)
        .into_iter()
        .find(|(kind, name, _)| kind == "index" && name == "audit_events_instance")
        .expect("the instance index exists");
    let sql = index.2.expect("the index has its SQL");
    assert!(
        sql.contains("(instance_id, occurred_at, id)")
            && sql.contains("WHERE instance_id IS NOT NULL"),
        "{sql}"
    );

    let err = conn
        .execute(
            "UPDATE audit_events SET instance_id = '0199f000-0000-7000-8000-000000000030'",
            [],
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("audit events are append only"),
        "{err}"
    );
}

// covers: AC-7 (spec 0035)
#[test]
fn the_new_columns_refuse_what_the_data_model_forbids() {
    let (_harness, conn) = migrated();
    let insert = |id: &str, extra: &str, values: &str| {
        conn.execute(
            &format!(
                "INSERT INTO audit_events (id, actor, action, result, occurred_at{extra}) VALUES \
                 ('{id}', 'user', 'operation.finished', 'succeeded', \
                 '2026-11-10T00:00:00.000000000Z'{values})"
            ),
            [],
        )
    };
    let refused = |id: &str, extra: &str, values: &str| {
        let err = insert(id, extra, values).unwrap_err().to_string();
        assert!(err.contains("CHECK constraint failed"), "{extra}: {err}");
    };

    refused(
        "0199f000-0000-7000-8000-0000000000e0",
        ", config_revision",
        ", 2",
    );
    refused(
        "0199f000-0000-7000-8000-0000000000e1",
        ", instance_id, config_revision",
        ", '0199f000-0000-7000-8000-000000000030', 0",
    );
    refused(
        "0199f000-0000-7000-8000-0000000000e2",
        ", instance_id",
        ", 'not-a-uuid'",
    );
    refused(
        "0199f000-0000-7000-8000-0000000000e3",
        ", operation_kind",
        ", 'Start'",
    );
    refused(
        "0199f000-0000-7000-8000-0000000000e4",
        ", client",
        ", 'G-U-I'",
    );
    insert(
        "0199f000-0000-7000-8000-0000000000e5",
        ", instance_id, operation_kind, config_revision, client",
        ", '0199f000-0000-7000-8000-0000000000ff', 'start_instance', 4, 'gui'",
    )
    .expect("an instance with no row is allowed: there is no foreign key");

    for days in [0, 1, 14, 31, 365] {
        let err = conn
            .execute(
                "UPDATE daemon_settings SET audit_retention_days = ?1",
                [days],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "{days}: {err}"
        );
    }
    for days in [7, 30, 90] {
        conn.execute(
            "UPDATE daemon_settings SET audit_retention_days = ?1",
            [days],
        )
        .expect("7, 30, and 90 days are allowed");
    }
}

// covers: AC-7 (spec 0035)
#[test]
fn the_version_12_fixture_reads_its_operation_rows_and_retention() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(12));
    let (registry, summary) = ready(harness.open());
    assert_eq!(summary.migrated_from, Some(12));

    assert_eq!(
        registry.daemon_settings().unwrap().value.audit_retention,
        AuditRetentionDays::Days90
    );
    let rows = registry
        .audit_events(AuditQuery::page(500, None))
        .unwrap()
        .value
        .items;
    let create = rows
        .iter()
        .find(|record| record.id.to_string() == "0199f000-0000-7000-8000-0000000000a3")
        .expect("the create row reads");
    assert_eq!(create.operation_kind, Some(OperationKind::CreateInstance));
    assert_eq!(create.config_revision.map(|number| number.get()), Some(1));
    assert_eq!(
        create.client.as_ref().map(|client| client.as_str()),
        Some("cli")
    );
    assert_eq!(
        create.instance.map(|id| id.to_string()).as_deref(),
        Some("0199f000-0000-7000-8000-000000000030")
    );
    assert_eq!(
        create.resource.map(|resource| resource.kind_code()),
        Some("instance")
    );
    let install = rows
        .iter()
        .find(|record| record.id.to_string() == "0199f000-0000-7000-8000-0000000000a7")
        .expect("the install row reads");
    assert_eq!(
        install.resource.map(|resource| resource.kind_code()),
        Some("runtime")
    );
    assert_eq!(install.instance, None);

    let gone = "0199f000-0000-7000-8000-0000000000c0".parse().unwrap();
    let query = AuditQuery {
        instance: Some(gone),
        ..AuditQuery::page(50, None)
    };
    let page = registry.audit_events(query).unwrap().value;
    assert_eq!(page.items.len(), 1, "a removed instance keeps its history");
}
