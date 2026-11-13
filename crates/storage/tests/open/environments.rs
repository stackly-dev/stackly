//! Migration 14 (spec 0044, step 1): the `services` snapshot of an
//! environment, a unique name, and a unique manifest location. Migration
//! 15 (spec 0046, steps 1 and 2): the last Up report and the coordinator of
//! a child operation.

use stackly_domain::{Ownership, Registry, UpPhase, UpStage, UpState};

use crate::common::{Harness, ready, row_counts};
use crate::fixture::fixture;

const TS: &str = "2026-01-02T03:04:05.123456789Z";
const ID_A: &str = "0199f000-0000-7000-8000-0000000000e1";
const ID_B: &str = "0199f000-0000-7000-8000-0000000000e2";

// covers: AC-11, AC-12 (spec 0044)
#[test]
fn a_version_13_registry_migrates_with_every_row_and_an_empty_snapshot() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(13));
    let before = row_counts(&harness.connect());

    let (registry, summary) = ready(harness.open());

    assert_eq!(summary.migrated_from, Some(13));
    let after = row_counts(&harness.connect());
    let kept = |counts: Vec<(String, i64)>| -> Vec<(String, i64)> {
        counts
            .into_iter()
            .filter(|(table, _)| table != "schema_migrations")
            .collect()
    };
    assert_eq!(kept(after), kept(before), "every row stays");
    let environments = registry.environments().unwrap().value;
    assert_eq!(environments.len(), 1);
    assert!(environments[0].declared().is_empty());
    assert_eq!(
        environments[0].manifest_location().map(ToString::to_string),
        Some("/Users/example/shop/stackly.yaml".to_owned())
    );
}

// covers: AC-11 (spec 0044)
#[test]
fn the_version_14_fixture_reads_its_services_snapshot() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(14));
    let (registry, summary) = ready(harness.open());
    assert_eq!(summary.migrated_from, Some(14));

    let environments = registry.environments().unwrap().value;
    let declared = environments[0].declared();
    let aliases: Vec<&str> = declared.iter().map(|s| s.alias().as_str()).collect();
    assert_eq!(aliases, ["db", "cache"], "manifest order");
    assert_eq!(declared[0].ownership(), Ownership::Dedicated);
    assert_eq!(
        declared[0].selector().map(ToString::to_string).as_deref(),
        Some("17")
    );
    assert_eq!(declared[0].depends_on()[0].service().as_str(), "cache");
    assert_eq!(declared[0].depends_on()[0].timeout_seconds(), Some(60));
    assert_eq!(declared[1].ownership(), Ownership::Shared);
    assert_eq!(
        declared[1]
            .instance_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("shared-valkey")
    );
}

// covers: AC-10, AC-24 (spec 0046)
#[test]
fn the_version_15_fixture_reads_its_last_up_and_a_child_journal() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(15));
    let (registry, summary) = ready(harness.open());
    assert_eq!(summary.migrated_from, None);

    let environments = registry.environments().unwrap().value;
    let report = environments[0]
        .last_up()
        .expect("the fixture has a last up");
    assert_eq!(report.state(), UpState::Failed);
    assert_eq!(
        environments[0].last_operation(),
        Some(report.operation()),
        "the last operation is the up"
    );
    let db = &report.services()[1];
    assert_eq!(
        (db.alias().as_str(), db.phase(), db.stage()),
        ("db", UpPhase::Failed, Some(UpStage::Start))
    );
    assert_eq!(
        db.code().map(ToString::to_string).as_deref(),
        Some("supervisor.readiness_timeout")
    );
    assert!(environments[0].updated_at().is_some());

    let child = registry
        .operation_journal("0199f000-0000-7000-8000-000000000051".parse().unwrap())
        .unwrap()
        .value
        .expect("the child journal reads back");
    assert_eq!(child.journal.parent(), Some(report.operation()));
}

// covers: AC-10 (spec 0046)
#[test]
fn a_last_up_that_is_not_a_json_object_is_refused_by_the_schema() {
    let harness = Harness::new();
    harness.create();
    let conn = harness.connect();
    insert(&conn, ID_A, "plain", None, "[]").unwrap();
    assert!(
        conn.execute(
            "UPDATE environments SET last_up = '[]' WHERE id = ?1",
            [ID_A]
        )
        .is_err()
    );
}

fn insert(
    conn: &rusqlite::Connection,
    id: &str,
    name: &str,
    location: Option<&str>,
    services: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO environments (id, name, manifest_location, services, revision, created_at, \
         updated_at) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
        rusqlite::params![id, name, location, services, TS],
    )
}

// covers: AC-11 (spec 0044)
#[test]
fn the_table_refuses_a_second_name_a_second_location_and_a_snapshot_that_is_not_an_array() {
    let harness = Harness::new();
    harness.create();
    let conn = harness.connect();
    insert(
        &conn,
        ID_A,
        "shop",
        Some("/Users/me/shop/stackly.yaml"),
        "[]",
    )
    .unwrap();

    assert!(
        insert(&conn, ID_B, "shop", None, "[]").is_err(),
        "name is unique"
    );
    assert!(
        insert(
            &conn,
            ID_B,
            "other",
            Some("/Users/me/shop/stackly.yaml"),
            "[]"
        )
        .is_err(),
        "a set location is unique"
    );
    assert!(
        insert(&conn, ID_B, "other", None, "{}").is_err(),
        "services is an array"
    );
    assert!(
        insert(&conn, ID_B, "other", None, "not json").is_err(),
        "services is JSON"
    );
    insert(&conn, ID_B, "other", None, "[]").unwrap();
    let third = "0199f000-0000-7000-8000-0000000000e3";
    insert(&conn, third, "third", None, "[]").unwrap();
}
