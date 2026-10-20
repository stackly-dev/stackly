//! Migration 4, the stored error items and business rule of spec 0007
//! (AC-16, AC-17): migrating the frozen version 3 fixture gives the old
//! rows `'[]'` and NULL, the version 4 fixture reads back through the
//! registry with its items and rule, and the CHECKs refuse an item or a
//! rule on a row without an error, a rule outside `BR-NNN`, and items over
//! 65 536 bytes.

use rusqlite::Connection;
use stackly_domain::{FailureError, OperationId, Registry, Rule};

use crate::common::{Harness, ready, row_counts};
use crate::fixture::fixture;

const WITH_ITEMS: &str = "0199f000-0000-7000-8000-000000000056";
const WITHOUT_ITEMS: &str = "0199f000-0000-7000-8000-000000000054";
const NO_ERROR: &str = "0199f000-0000-7000-8000-000000000055";

fn read(registry: &impl Registry, id: &str) -> FailureError {
    registry
        .operation_journal(id.parse::<OperationId>().expect("the fixture ID parses"))
        .expect("the journal reads")
        .value
        .unwrap_or_else(|| panic!("the fixture has a journal for {id}"))
        .journal
        .failure_error()
        .cloned()
        .unwrap_or_else(|| panic!("{id} stores an error"))
}

fn failure_columns(conn: &Connection, id: &str) -> (String, Option<String>) {
    conn.query_row(
        "SELECT failure_items, failure_rule FROM operation_journal WHERE operation_id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("the journal row exists")
}

fn set(conn: &Connection, id: &str, items: &str, rule: Option<&str>) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE operation_journal SET failure_items = ?2, failure_rule = ?3 WHERE operation_id = ?1",
        (id, items, rule),
    )
}

#[test]
fn migrating_v3_gives_old_rows_no_items_and_no_rule_and_keeps_the_data() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(3));
    let kept = |table: &str| table != "schema_migrations";
    let mut before = row_counts(&harness.connect());
    before.retain(|(table, _)| kept(table));

    let (registry, summary) = ready(harness.open());

    assert_eq!(summary.migrated_from, Some(3));
    assert!(summary.snapshot.is_some(), "a migration takes a snapshot");
    let conn = harness.connect();
    let mut after = row_counts(&conn);
    after.retain(|(table, _)| kept(table));
    assert_eq!(after, before, "the version 3 data survives");
    let stored: i64 = conn
        .query_row(
            "SELECT count(*) FROM operation_journal \
             WHERE failure_items = '[]' AND failure_rule IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let total: i64 = conn
        .query_row("SELECT count(*) FROM operation_journal", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored, total, "every old row has the defaults");
    let error = read(&registry, WITHOUT_ITEMS);
    assert!(error.items().is_empty() && error.rule().is_none());
}

#[test]
fn the_v4_fixture_items_and_rule_read_back_through_the_registry() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(4));
    let (registry, summary) = ready(harness.open());
    assert_eq!(summary.migrated_from, None);

    let error = read(&registry, WITH_ITEMS);

    assert_eq!(error.code().as_str(), "provider.invalid_settings");
    assert_eq!(error.rule(), Some(Rule::Br002));
    assert_eq!(error.items().len(), 2);
    assert_eq!(error.items()[0].kind().as_str(), "issue");
    let fields: Vec<(&str, &str)> = error.items()[0]
        .fields()
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    assert_eq!(fields, [("path", "/port"), ("detail", "omitted")]);
    assert!(error.items()[1].fields().is_empty());

    let plain = read(&registry, WITHOUT_ITEMS);
    assert!(plain.items().is_empty() && plain.rule().is_none());
}

#[test]
fn the_checks_refuse_items_or_a_rule_without_an_error_and_bad_shapes() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(4));
    let _ = ready(harness.open());
    let conn = harness.connect();
    let item = r#"[{"kind":"issue","fields":[]}]"#;

    assert!(
        set(&conn, NO_ERROR, item, None).is_err(),
        "items without an error"
    );
    assert!(
        set(&conn, NO_ERROR, "[]", Some("BR-002")).is_err(),
        "a rule without an error"
    );
    for bad in ["BR-2", "BR-0021", "br-002", "VAL-007", "BR-00A"] {
        assert!(set(&conn, WITH_ITEMS, item, Some(bad)).is_err(), "{bad}");
    }
    assert!(
        set(&conn, WITH_ITEMS, "{}", None).is_err(),
        "an object is not an array"
    );
    assert!(set(&conn, WITH_ITEMS, "not json", None).is_err());
    let cell = r#"{"kind":"issue","fields":[{"key":"a","value":"x"}]}"#;
    let big = format!("[{}]", vec![cell; 1400].join(","));
    assert!(big.len() > 65_536);
    assert!(
        set(&conn, WITH_ITEMS, &big, None).is_err(),
        "over 65 536 bytes"
    );

    assert!(set(&conn, WITH_ITEMS, item, Some("BR-025")).is_ok());
    let fits = format!("[{}]", vec![cell; 1000].join(","));
    assert!(fits.len() < 65_536);
    assert!(set(&conn, WITH_ITEMS, &fits, None).is_ok());
    assert_eq!(failure_columns(&conn, WITH_ITEMS).1, None);
}

#[test]
fn a_rule_id_the_domain_does_not_know_is_a_corrupt_row() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(4));
    let _ = ready(harness.open());
    set(&harness.connect(), WITH_ITEMS, "[]", Some("BR-099")).unwrap();
    let (registry, _) = ready(harness.open());

    let err = registry
        .operation_journal(WITH_ITEMS.parse::<OperationId>().unwrap())
        .unwrap_err();

    assert!(
        matches!(err, stackly_domain::RegistryError::Corrupt { .. }),
        "{err:?}"
    );
}
