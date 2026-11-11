//! `Registry::quick_check` and `Registry::scan_unrecognized` on a real
//! SQLite registry (spec 0034 build step 3, AC-8): the frozen fixture of
//! the newest schema, opened by the registry thread, with damage written
//! behind its back through a second connection, the way a crash, a newer
//! build, or a hand edit would leave it.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;
use stackly_domain::{
    AuditQuery, InstanceId, OperationId, ProviderRevisionKey, ProviderSourceKind, QuickCheck,
    Registry, RegistryError, UNRECOGNIZED_ROWS_PER_COLUMN, UNRECOGNIZED_VALUE_MAX_LEN,
    UnrecognizedValue,
};
use stackly_storage::{CODED_COLUMNS, CodedColumn, MigrationSet, SqliteRegistry};
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, ready};
use crate::fixture::fixture;

/// A code no open set of this build holds, and that passes the `code`
/// CHECK of every coded column.
const FOREIGN: &str = "zzz_not_known";

/// The columns of the schema with a `code` CHECK whose values no known set
/// limits, each with the reason. Every other such column must be in
/// [`CODED_COLUMNS`].
const FREE_CODE_COLUMNS: &[(&str, &str, &str)] = &[
    ("audit_events", "client", "a ClientCode takes any code"),
    ("backups", "method", "no mapper reads backups yet"),
    ("backups", "scope", "no mapper reads backups yet"),
    ("backups", "consistency", "no mapper reads backups yet"),
    (
        "daemon_restart_plan_instance",
        "failure_code",
        "a FailureCode takes any code",
    ),
    ("instances", "last_failure", "a FailureCode takes any code"),
    ("instances", "ownership", "no mapper reads it yet"),
    ("operation_created", "detail", "a ReasonCode takes any code"),
    (
        "operation_journal",
        "trigger_client",
        "a ClientCode takes any code",
    ),
    ("operation_steps", "name", "a StageName takes any code"),
    ("operations", "failure_code", "a FailureCode takes any code"),
    ("operations", "stage", "a StageName takes any code"),
    (
        "schema_migrations",
        "name",
        "the open checks migrations by name",
    ),
];

/// The registry of the newest frozen fixture, open, and a second
/// connection to write behind its back.
struct Setup {
    harness: Harness,
    registry: SqliteRegistry,
}

impl Setup {
    fn new() -> Self {
        let harness = Harness::new();
        harness.load_fixture(&fixture(MigrationSet::builtin().latest()));
        let (registry, _) = ready(harness.open());
        // The frozen fixture holds codes from before features 20 and 26
        // that this build no longer knows: the provider source's `builtin`
        // and `trusted`, the operation kinds `retain_instance` and
        // `doctor`, and the recovery hint `resume_retain`. The scan rightly
        // reports them, so the baseline takes the codes of this build.
        harness
            .connect()
            .execute_batch(
                "UPDATE provider_sources SET kind = 'bundled', status = 'active'; \
                 UPDATE operations SET kind = 'remove_instance' WHERE kind = 'retain_instance'; \
                 UPDATE operations SET kind = 'update_manager' WHERE kind = 'doctor'; \
                 UPDATE operations SET recovery_hint = 'abandon' \
                 WHERE recovery_hint = 'resume_retain';",
            )
            .expect("the fixture takes the codes of this build");
        Self { harness, registry }
    }

    /// A connection that writes what the CHECK constraints and the append
    /// only trigger of `audit_events` would refuse.
    fn side(&self) -> Connection {
        let conn = self.harness.connect();
        conn.execute_batch(
            "PRAGMA ignore_check_constraints = ON; \
             DROP TRIGGER IF EXISTS audit_events_append_only;",
        )
        .expect("the side connection can turn the guards off");
        conn
    }

    fn quick_check(&self) -> QuickCheck {
        self.registry
            .quick_check(running())
            .expect("quick_check runs")
    }

    fn scan(&self) -> Vec<UnrecognizedValue> {
        self.registry
            .scan_unrecognized(running())
            .expect("the scan runs")
    }

    fn generation(&self) -> u64 {
        self.registry
            .known_instance_ids()
            .expect("the registry reads")
            .generation
            .get()
    }
}

fn running() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn text(conn: &Connection, sql: &str, row_id: i64) -> String {
    conn.query_row(sql, [row_id], |row| row.get(0))
        .expect("the probe query finds its row")
}

/// The read that loads the row `row_id` of the entry's table, the way the
/// daemon would.
fn probe(setup: &Setup, side: &Connection, entry: &CodedColumn, row_id: i64) -> ProbeRead {
    let registry = &setup.registry;
    let result = match entry.table {
        "audit_events" => registry
            .audit_events(AuditQuery::page(AuditQuery::LIMIT_MAX, None))
            .map(drop),
        "config_revisions" => {
            let instance: InstanceId = text(
                side,
                "SELECT instance_id FROM config_revisions WHERE rowid = ?1",
                row_id,
            )
            .parse()
            .expect("the fixture stores instance IDs");
            registry.config_revisions(instance, 100).map(drop)
        }
        "operation_created" | "operation_journal" => {
            let operation: OperationId = text(
                side,
                &format!("SELECT operation_id FROM {} WHERE rowid = ?1", entry.table),
                row_id,
            )
            .parse()
            .expect("the fixture stores operation IDs");
            registry.operation_journal(operation).map(drop)
        }
        "operations" => {
            let operation: OperationId =
                text(side, "SELECT id FROM operations WHERE rowid = ?1", row_id)
                    .parse()
                    .expect("the fixture stores operation IDs");
            registry.operation(operation).map(drop)
        }
        "provider_imports" => registry.provider_imports().map(drop),
        "provider_revisions" => {
            let key: ProviderRevisionKey = format!(
                "{}@{}#{}",
                text(
                    side,
                    "SELECT provider FROM provider_revisions WHERE rowid = ?1",
                    row_id
                ),
                text(
                    side,
                    "SELECT revision FROM provider_revisions WHERE rowid = ?1",
                    row_id
                ),
                text(
                    side,
                    "SELECT manifest_sha256 FROM provider_revisions WHERE rowid = ?1",
                    row_id
                ),
            )
            .parse()
            .expect("the fixture stores provider revision keys");
            registry.provider_revision(&key).map(drop)
        }
        "provider_sources" => {
            // The read selects by kind, so it is the stored kind of the
            // fixture, not the column the entry changed.
            let location = text(
                side,
                "SELECT location FROM provider_sources WHERE rowid = ?1",
                row_id,
            );
            return match registry
                .provider_source_by_location(ProviderSourceKind::Bundled, &location)
            {
                Ok(found) if found.value.is_none() => ProbeRead::Hidden,
                Ok(_) => ProbeRead::Read(Ok(())),
                Err(err) => ProbeRead::Read(Err(err)),
            };
        }
        other => panic!("add a probe read for the new coded table {other}"),
    };
    ProbeRead::Read(result)
}

/// What the read of a changed row did.
#[derive(Debug)]
enum ProbeRead {
    /// The read ran and answered this.
    Read(Result<(), RegistryError>),
    /// The read selects by the changed column, so the row is gone from it.
    Hidden,
}

#[test]
fn a_healthy_registry_passes_both_checks_and_neither_writes() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "on the newest frozen fixture, quick_check passes and the scan finds no unrecognized value; the generation and the bytes of the registry file and its WAL stay the same",
    );
    let setup = Setup::new();
    let generation = setup.generation();
    let digests = setup.harness.digests();

    assert_eq!(setup.quick_check(), QuickCheck::Passed);
    assert_eq!(setup.scan(), []);

    assert_eq!(setup.generation(), generation);
    assert_eq!(setup.harness.digests(), digests);
    ev.pass();
}

#[test]
fn a_broken_check_constraint_fails_quick_check_and_is_not_an_open_set_value() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "an instance lifecycle of zzz written behind the registry (a closed set, CHECK IN) fails quick_check with a CHECK message for instances, the read of that instance fails Corrupt on lifecycle, and the scan of open sets stays empty",
    );
    let setup = Setup::new();
    let side = setup.side();
    let instance: InstanceId = side
        .query_row(
            "SELECT id FROM instances ORDER BY rowid LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("the fixture has an instance")
        .parse()
        .expect("the fixture stores instance IDs");
    side.execute(
        "UPDATE instances SET lifecycle = 'zzz' WHERE id = ?1",
        [instance.to_string()],
    )
    .expect("the side connection writes past the CHECK");

    let QuickCheck::Failed { messages } = setup.quick_check() else {
        panic!("a broken CHECK constraint fails quick_check");
    };

    assert!(
        messages
            .iter()
            .any(|message| message.contains("CHECK constraint failed in instances")),
        "{messages:?}"
    );
    assert_eq!(setup.scan(), [], "lifecycle is a closed set, not scanned");
    let err = setup.registry.instance(instance).unwrap_err();
    assert!(
        matches!(
            err,
            RegistryError::Corrupt {
                table: "instances",
                column: "lifecycle",
                ..
            }
        ),
        "{err:?}"
    );
    ev.pass();
}

#[test]
fn a_failed_quick_check_keeps_at_most_ten_messages() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected =
            "twelve rows that break a CHECK constraint fail quick_check with exactly ten messages",
    );
    let setup = Setup::new();
    setup
        .side()
        .execute_batch(
            "CREATE TABLE damaged (code TEXT NOT NULL CHECK (code IN ('a'))); \
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 12) \
             INSERT INTO damaged SELECT 'b' || i FROM n;",
        )
        .expect("the side connection writes the damaged rows");

    let QuickCheck::Failed { messages } = setup.quick_check() else {
        panic!("twelve broken rows fail quick_check");
    };

    assert_eq!(messages.len(), 10, "{messages:?}");
    ev.pass();
}

#[test]
fn every_coded_column_reports_a_foreign_code_that_fails_the_normal_read() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "for every CODED_COLUMNS entry, a foreign code written into one fixture row makes the read that loads the row fail Corrupt on that table and column (or hides the row from provider_source_by_location, which selects by kind), and the scan answers exactly that table, rowid, column, and value; restored, the scan is empty again",
    );
    let setup = Setup::new();
    let side = setup.side();

    for entry in CODED_COLUMNS {
        let row_id: i64 = side
            .query_row(
                &format!("SELECT min(rowid) FROM {}", entry.table),
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("the fixture has rows in {}: {err}", entry.table));
        let original: Option<String> = side
            .query_row(
                &format!(
                    "SELECT {} FROM {} WHERE rowid = ?1",
                    entry.column, entry.table
                ),
                [row_id],
                |row| row.get(0),
            )
            .unwrap();
        let set = format!(
            "UPDATE {} SET {} = ?1 WHERE rowid = ?2",
            entry.table, entry.column
        );
        side.execute(&set, rusqlite::params![FOREIGN, row_id])
            .unwrap();

        match probe(&setup, &side, entry, row_id) {
            ProbeRead::Read(Err(RegistryError::Corrupt { table, column, .. })) => {
                assert_eq!((table, column), (entry.table, entry.column));
            }
            ProbeRead::Hidden => assert_eq!(
                (entry.table, entry.column),
                ("provider_sources", "kind"),
                "only a read that selects by the column hides the row"
            ),
            other => panic!("{}.{} read as {other:?}", entry.table, entry.column),
        }
        assert_eq!(
            setup.scan(),
            [UnrecognizedValue::new(
                entry.table,
                row_id,
                entry.column,
                FOREIGN
            )],
            "{}.{}",
            entry.table,
            entry.column
        );

        side.execute(&set, rusqlite::params![original, row_id])
            .unwrap();
    }
    assert_eq!(setup.scan(), []);
    ev.pass();
}

#[test]
fn the_scan_keeps_64_rows_per_column_and_cuts_a_long_value() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "seventy audit rows with a foreign actor report 64 rows for audit_events.actor in rowid order, and a 100 character kind is reported cut to 64 bytes",
    );
    let setup = Setup::new();
    let side = setup.side();
    side.execute_batch(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 70) \
         INSERT INTO audit_events (id, actor, action, resource_kind, resource_id, operation_id, \
         result, safe_details, occurred_at) \
         SELECT printf('0199f000-0000-7000-8000-%012x', 4096 + i), 'zzz_robot', 'secret.set', \
         NULL, NULL, NULL, 'succeeded', '{}', '2026-01-02T03:04:05.123456789Z' FROM n;",
    )
    .unwrap();
    let operation: i64 = side
        .query_row("SELECT min(rowid) FROM operations", [], |row| row.get(0))
        .unwrap();
    side.execute(
        "UPDATE operations SET kind = ?1 WHERE rowid = ?2",
        rusqlite::params!["k".repeat(100), operation],
    )
    .unwrap();

    let found = setup.scan();

    let actors: Vec<&UnrecognizedValue> = found
        .iter()
        .filter(|value| (value.table, value.column) == ("audit_events", "actor"))
        .collect();
    assert_eq!(actors.len(), UNRECOGNIZED_ROWS_PER_COLUMN);
    assert!(actors.iter().all(|value| value.value == "zzz_robot"));
    assert!(
        actors
            .windows(2)
            .all(|pair| pair[0].row_id < pair[1].row_id),
        "rowid order"
    );
    let kinds: Vec<&UnrecognizedValue> = found
        .iter()
        .filter(|value| (value.table, value.column) == ("operations", "kind"))
        .collect();
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0].row_id, operation);
    assert_eq!(kinds[0].value, "k".repeat(UNRECOGNIZED_VALUE_MAX_LEN));
    ev.pass();
}

#[test]
fn a_set_stop_flag_interrupts_both_checks_and_the_registry_stays_usable() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "with its stop flag already set, the PRAGMA quick_check statement is cut by the progress handler (no check runs before it) and the scan stops before its first column, both answering Io; afterwards the handler is gone: quick_check passes, the scan is empty, and the generation is unchanged",
    );
    let setup = Setup::new();
    let generation = setup.generation();
    let stopped = Arc::new(AtomicBool::new(true));

    let quick = setup.registry.quick_check(Arc::clone(&stopped));
    let scan = setup.registry.scan_unrecognized(stopped);

    assert!(matches!(quick, Err(RegistryError::Io { .. })), "{quick:?}");
    assert!(matches!(scan, Err(RegistryError::Io { .. })), "{scan:?}");
    assert_eq!(setup.quick_check(), QuickCheck::Passed);
    assert_eq!(setup.scan(), []);
    assert_eq!(setup.generation(), generation);
    ev.pass();
}

#[test]
fn every_open_set_column_of_the_schema_is_coded_or_named_free() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "the columns of the newest schema with a code CHECK (NOT GLOB '*[^a-z0-9_]*') are exactly the CODED_COLUMNS entries plus the free code columns this test names with a reason, so a new open set column cannot be left out of the scan",
    );
    let setup = Setup::new();
    let side = setup.harness.connect();
    let mut schema = BTreeSet::new();
    let mut statement = side
        .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table' AND sql IS NOT NULL")
        .unwrap();
    let tables = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap();
    for table in tables {
        let (name, sql) = table.unwrap();
        for column in code_columns(&sql) {
            schema.insert((name.clone(), column));
        }
    }

    let coded: BTreeSet<(String, String)> = CODED_COLUMNS
        .iter()
        .map(|entry| (entry.table.to_owned(), entry.column.to_owned()))
        .collect();
    let free: BTreeSet<(String, String)> = FREE_CODE_COLUMNS
        .iter()
        .map(|(table, column, _)| ((*table).to_owned(), (*column).to_owned()))
        .collect();
    assert!(coded.is_disjoint(&free), "{:?}", coded.intersection(&free));
    let listed: BTreeSet<_> = coded.union(&free).cloned().collect();
    assert_eq!(
        schema.difference(&listed).collect::<Vec<_>>(),
        Vec::<&(String, String)>::new(),
        "add each to CODED_COLUMNS or to FREE_CODE_COLUMNS with a reason"
    );
    assert_eq!(
        listed.difference(&schema).collect::<Vec<_>>(),
        Vec::<&(String, String)>::new(),
        "a listed column has no code CHECK in the schema"
    );
    ev.pass();
}

/// The columns of a `CREATE TABLE` whose CHECK limits them to the code
/// alphabet: `<column> NOT GLOB '*[^a-z0-9_]*'`.
fn code_columns(sql: &str) -> Vec<String> {
    const MARK: &str = " NOT GLOB '*[^a-z0-9_]*'";
    let mut columns = Vec::new();
    let mut rest = sql;
    while let Some(at) = rest.find(MARK) {
        let before = &rest[..at];
        let start = before
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |index| index + 1);
        columns.push(before[start..].to_owned());
        rest = &rest[at + MARK.len()..];
    }
    columns.sort();
    columns.dedup();
    columns
}
