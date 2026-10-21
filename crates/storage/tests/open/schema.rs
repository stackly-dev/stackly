//! The schema of the latest migration against the data models of spec 0005
//! (AC-1, AC-12, AC-13, AC-15) and of the operation journal of spec 0007
//! (AC-17).

use std::collections::BTreeSet;

use rusqlite::Connection;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::Harness;

const TS: &str = "2026-01-02T03:04:05.123456789Z";

/// One column: name, type, not null, and primary key position.
type ColumnSpec = (&'static str, &'static str, bool, u8);

/// Every table and its columns as `(name, type, not null, primary key
/// position)`, in declaration order, from the spec's data model.
const DATA_MODEL: &[(&str, &[ColumnSpec])] = &[
    (
        "registry_info",
        &[
            ("singleton", "INTEGER", true, 1),
            ("registry_id", "TEXT", true, 0),
            ("created_at", "TEXT", true, 0),
            ("created_by_version", "TEXT", true, 0),
            ("generation", "INTEGER", true, 0),
        ],
    ),
    (
        "schema_migrations",
        &[
            ("version", "INTEGER", true, 1),
            ("name", "TEXT", true, 0),
            ("sha256", "TEXT", true, 0),
            ("applied_at", "TEXT", true, 0),
            ("app_version", "TEXT", true, 0),
        ],
    ),
    (
        "provider_sources",
        &[
            ("id", "TEXT", true, 1),
            ("kind", "TEXT", true, 0),
            ("location", "TEXT", true, 0),
            ("trust_root", "TEXT", false, 0),
            ("last_sync_at", "TEXT", false, 0),
            ("status", "TEXT", true, 0),
            ("revision", "INTEGER", true, 0),
            ("created_at", "TEXT", true, 0),
            ("updated_at", "TEXT", true, 0),
        ],
    ),
    (
        "provider_revisions",
        &[
            ("pk", "INTEGER", true, 1),
            ("provider", "TEXT", true, 0),
            ("revision", "TEXT", true, 0),
            ("manifest_sha256", "TEXT", true, 0),
            ("software", "TEXT", true, 0),
            ("schema_version", "TEXT", false, 0),
            ("publisher", "TEXT", false, 0),
            ("capabilities", "TEXT", true, 0),
            ("line_compatibility", "TEXT", true, 0),
            ("source_id", "TEXT", false, 0),
            ("approval", "TEXT", false, 0),
            ("approved_at", "TEXT", false, 0),
            ("created_at", "TEXT", true, 0),
        ],
    ),
    (
        "artifacts",
        &[
            ("pk", "INTEGER", true, 1),
            ("sha256", "TEXT", true, 0),
            ("software", "TEXT", true, 0),
            ("version", "TEXT", true, 0),
            ("os", "TEXT", true, 0),
            ("arch", "TEXT", true, 0),
            ("minimum_os", "TEXT", false, 0),
            ("source", "TEXT", false, 0),
            ("signature_info", "TEXT", false, 0),
            ("dependency_manifest", "TEXT", false, 0),
            ("provider_revision_pk", "INTEGER", false, 0),
            ("created_at", "TEXT", true, 0),
            ("catalog_id", "TEXT", false, 0),
            ("build", "INTEGER", false, 0),
            ("format", "TEXT", false, 0),
            ("size_bytes", "INTEGER", false, 0),
        ],
    ),
    (
        "compatibility_records",
        &[
            ("artifact_sha256", "TEXT", true, 1),
            ("position", "INTEGER", true, 2),
            ("scope", "TEXT", true, 0),
            ("provider", "TEXT", false, 0),
            ("provider_revision", "TEXT", false, 0),
            ("manifest_sha256", "TEXT", false, 0),
            ("os", "TEXT", true, 0),
            ("os_version", "TEXT", true, 0),
            ("os_build", "TEXT", false, 0),
            ("arch", "TEXT", true, 0),
            ("result", "TEXT", true, 0),
            ("checks", "TEXT", true, 0),
            ("known_limitations", "TEXT", true, 0),
            ("summary", "TEXT", false, 0),
            ("tested_at", "TEXT", true, 0),
            ("evidence", "TEXT", true, 0),
        ],
    ),
    (
        "runtimes",
        &[
            ("id", "TEXT", true, 1),
            ("software", "TEXT", true, 0),
            ("version", "TEXT", true, 0),
            ("line", "TEXT", true, 0),
            ("os", "TEXT", true, 0),
            ("arch", "TEXT", true, 0),
            ("install_mode", "TEXT", true, 0),
            ("artifact_sha256", "TEXT", false, 0),
            ("external_root", "TEXT", false, 0),
            ("observed_sha256", "TEXT", false, 0),
            ("state", "TEXT", true, 0),
            ("installed_at", "TEXT", false, 0),
            ("last_verified_at", "TEXT", false, 0),
            ("revision", "INTEGER", true, 0),
            ("created_at", "TEXT", true, 0),
            ("updated_at", "TEXT", true, 0),
        ],
    ),
    (
        "runtime_dependencies",
        &[
            ("runtime_id", "TEXT", true, 1),
            ("dependency_runtime_id", "TEXT", true, 2),
            ("version_constraint", "TEXT", false, 0),
        ],
    ),
    (
        "instances",
        &[
            ("id", "TEXT", true, 1),
            ("name", "TEXT", true, 0),
            ("display_name", "TEXT", false, 0),
            ("provider_revision_pk", "INTEGER", true, 0),
            ("software", "TEXT", true, 0),
            ("runtime_id", "TEXT", true, 0),
            ("data_line", "TEXT", true, 0),
            ("data_version", "TEXT", true, 0),
            ("data_initialized_at", "TEXT", false, 0),
            ("desired", "TEXT", true, 0),
            ("startup", "TEXT", true, 0),
            ("lifecycle", "TEXT", true, 0),
            ("last_failure", "TEXT", false, 0),
            ("config_revision_id", "TEXT", false, 0),
            ("ownership", "TEXT", true, 0),
            ("retained_at", "TEXT", false, 0),
            ("retained_by_operation", "TEXT", false, 0),
            ("created_by_operation", "TEXT", true, 0),
            ("revision", "INTEGER", true, 0),
            ("created_at", "TEXT", true, 0),
            ("updated_at", "TEXT", true, 0),
        ],
    ),
    (
        "instance_paths",
        &[
            ("instance_id", "TEXT", true, 1),
            ("kind", "TEXT", true, 2),
            ("logical_root", "TEXT", true, 0),
            ("relative_path", "TEXT", true, 0),
            ("managed", "INTEGER", true, 0),
        ],
    ),
    (
        "instance_endpoints",
        &[
            ("instance_id", "TEXT", true, 1),
            ("position", "INTEGER", true, 0),
            ("name", "TEXT", true, 2),
            ("kind", "TEXT", true, 0),
            ("transport", "TEXT", false, 0),
            ("address", "TEXT", false, 0),
            ("port_request", "TEXT", false, 0),
            ("fixed_port", "INTEGER", false, 0),
            ("bound_port", "INTEGER", false, 0),
            ("socket_name", "TEXT", false, 0),
            ("enabled", "INTEGER", true, 0),
        ],
    ),
    (
        "config_revisions",
        &[
            ("id", "TEXT", true, 1),
            ("instance_id", "TEXT", true, 0),
            ("config_values", "TEXT", true, 0),
            ("rendered_sha256", "TEXT", false, 0),
            ("previous_id", "TEXT", false, 0),
            ("status", "TEXT", true, 0),
            ("created_by_operation", "TEXT", false, 0),
            ("created_at", "TEXT", true, 0),
        ],
    ),
    (
        "secret_references",
        &[
            ("id", "TEXT", true, 1),
            ("reference", "TEXT", true, 0),
            ("platform_store_key", "TEXT", true, 0),
            ("owner_kind", "TEXT", true, 0),
            ("owner_id", "TEXT", false, 0),
            ("purpose", "TEXT", true, 0),
            ("value_origin", "TEXT", false, 0),
            ("status", "TEXT", true, 0),
            ("status_observed_at", "TEXT", false, 0),
            ("value_updated_at", "TEXT", false, 0),
            ("revision", "INTEGER", true, 0),
            ("created_at", "TEXT", true, 0),
            ("updated_at", "TEXT", true, 0),
        ],
    ),
    (
        "secret_reference_uses",
        &[
            ("reference_id", "TEXT", true, 1),
            ("resource_kind", "TEXT", true, 2),
            ("resource_id", "TEXT", true, 3),
            ("field", "TEXT", true, 4),
        ],
    ),
    (
        "environments",
        &[
            ("id", "TEXT", true, 1),
            ("name", "TEXT", true, 0),
            ("manifest_location", "TEXT", false, 0),
            ("manifest_sha256", "TEXT", false, 0),
            ("lock_sha256", "TEXT", false, 0),
            ("last_operation_id", "TEXT", false, 0),
            ("revision", "INTEGER", true, 0),
            ("created_at", "TEXT", true, 0),
            ("updated_at", "TEXT", true, 0),
        ],
    ),
    (
        "environment_instances",
        &[
            ("environment_id", "TEXT", true, 1),
            ("position", "INTEGER", true, 0),
            ("alias", "TEXT", true, 2),
            ("instance_id", "TEXT", true, 0),
            ("ownership", "TEXT", true, 0),
            ("dependency_conditions", "TEXT", true, 0),
        ],
    ),
    (
        "instance_demands",
        &[
            ("id", "TEXT", true, 1),
            ("instance_id", "TEXT", true, 0),
            ("source_kind", "TEXT", true, 0),
            ("source_id", "TEXT", false, 0),
            ("session_id", "TEXT", false, 0),
            ("state", "TEXT", true, 0),
            ("state_changed_at", "TEXT", false, 0),
            ("acquired_at", "TEXT", true, 0),
        ],
    ),
    (
        "operations",
        &[
            ("id", "TEXT", true, 1),
            ("kind", "TEXT", true, 0),
            ("resource_kind", "TEXT", true, 0),
            ("resource_id", "TEXT", false, 0),
            ("idempotency_key", "TEXT", false, 0),
            ("state", "TEXT", true, 0),
            ("stage", "TEXT", false, 0),
            ("progress_done", "INTEGER", false, 0),
            ("progress_total", "INTEGER", false, 0),
            ("failure_code", "TEXT", false, 0),
            ("failure_retryable", "INTEGER", false, 0),
            ("recovery_hint", "TEXT", false, 0),
            ("started_at", "TEXT", false, 0),
            ("revision", "INTEGER", true, 0),
            ("created_at", "TEXT", true, 0),
            ("updated_at", "TEXT", true, 0),
        ],
    ),
    (
        "operation_journal",
        &[
            ("operation_id", "TEXT", true, 1),
            ("method", "TEXT", false, 0),
            ("request_fingerprint", "TEXT", false, 0),
            ("trigger_origin", "TEXT", true, 0),
            ("trigger_client", "TEXT", false, 0),
            ("trigger_interactive", "INTEGER", true, 0),
            ("resumable", "INTEGER", true, 0),
            ("input", "TEXT", true, 0),
            ("expected_revision", "INTEGER", false, 0),
            ("waiting_on", "TEXT", false, 0),
            ("point_of_no_return_at", "TEXT", false, 0),
            ("cancel_requested_at", "TEXT", false, 0),
            ("compensation_started_at", "TEXT", false, 0),
            ("recovery_cause", "TEXT", false, 0),
            ("failure_error_code", "TEXT", false, 0),
            ("failure_message", "TEXT", false, 0),
            ("failure_details", "TEXT", true, 0),
            ("result", "TEXT", false, 0),
            ("failure_items", "TEXT", true, 0),
            ("failure_rule", "TEXT", false, 0),
        ],
    ),
    (
        "operation_steps",
        &[
            ("operation_id", "TEXT", true, 1),
            ("seq", "INTEGER", true, 2),
            ("name", "TEXT", true, 0),
            ("repeatable", "INTEGER", true, 0),
            ("reversible", "INTEGER", true, 0),
            ("state", "TEXT", true, 0),
            ("attempts", "INTEGER", true, 0),
            ("checkpoint", "TEXT", false, 0),
            ("error_code", "TEXT", false, 0),
            ("started_at", "TEXT", true, 0),
            ("finished_at", "TEXT", false, 0),
        ],
    ),
    (
        "operation_created",
        &[
            ("operation_id", "TEXT", true, 1),
            ("seq", "INTEGER", true, 2),
            ("step_seq", "INTEGER", true, 0),
            ("item_kind", "TEXT", true, 0),
            ("item_ref", "TEXT", true, 0),
            ("payload", "TEXT", false, 0),
            ("state", "TEXT", true, 0),
            ("detail", "TEXT", false, 0),
            ("recorded_at", "TEXT", true, 0),
            ("settled_at", "TEXT", false, 0),
        ],
    ),
    (
        "operation_locks",
        &[
            ("operation_id", "TEXT", true, 1),
            ("lock_key", "TEXT", true, 2),
        ],
    ),
    (
        "process_observations",
        &[
            ("id", "INTEGER", true, 1),
            ("instance_id", "TEXT", true, 0),
            ("pid", "INTEGER", true, 0),
            ("start_identity", "TEXT", true, 0),
            ("executable_identity", "TEXT", true, 0),
            ("boot_id", "TEXT", false, 0),
            ("session_id", "TEXT", false, 0),
            ("observed_at", "TEXT", true, 0),
        ],
    ),
    (
        "backups",
        &[
            ("id", "TEXT", true, 1),
            ("instance_id", "TEXT", false, 0),
            ("runtime_id", "TEXT", false, 0),
            ("runtime_identity", "TEXT", true, 0),
            ("method", "TEXT", true, 0),
            ("scope", "TEXT", true, 0),
            ("consistency", "TEXT", true, 0),
            ("path", "TEXT", true, 0),
            ("checksum_manifest", "TEXT", false, 0),
            ("created_at", "TEXT", true, 0),
            ("verified_at", "TEXT", false, 0),
            ("restore_verified_at", "TEXT", false, 0),
        ],
    ),
    (
        "audit_events",
        &[
            ("id", "TEXT", true, 1),
            ("actor", "TEXT", true, 0),
            ("action", "TEXT", true, 0),
            ("resource_kind", "TEXT", false, 0),
            ("resource_id", "TEXT", false, 0),
            ("operation_id", "TEXT", false, 0),
            ("result", "TEXT", true, 0),
            ("safe_details", "TEXT", true, 0),
            ("occurred_at", "TEXT", true, 0),
        ],
    ),
    (
        "settings",
        &[
            ("key", "TEXT", true, 1),
            ("typed_value", "TEXT", true, 0),
            ("revision", "INTEGER", true, 0),
            ("updated_at", "TEXT", true, 0),
        ],
    ),
];

/// The PRD section 17 fields that are not a column of the same name, and
/// the columns that hold them (the spec's "Peta field PRD").
const PRD_FIELD_MAP: &[(&str, &str, &[&str])] = &[
    ("provider_sources", "last_sync", &["last_sync_at"]),
    ("provider_revisions", "provider_id", &["provider", "pk"]),
    (
        "provider_revisions",
        "manifest_digest",
        &["manifest_sha256"],
    ),
    ("artifacts", "id", &["pk"]),
    ("artifacts", "provider_revision", &["provider_revision_pk"]),
    ("artifacts", "digest", &["sha256"]),
    ("runtimes", "artifact_id", &["artifact_sha256"]),
    ("runtimes", "resolved_path", &["external_root"]),
    ("runtimes", "last_verified", &["last_verified_at"]),
    (
        "runtime_dependencies",
        "constraint",
        &["version_constraint"],
    ),
    ("instances", "provider_revision", &["provider_revision_pk"]),
    ("instances", "config_revision", &["config_revision_id"]),
    ("instances", "desired_state", &["desired"]),
    ("instances", "startup_policy", &["startup"]),
    (
        "instance_paths",
        "resolved_path",
        &["logical_root", "relative_path"],
    ),
    ("instance_endpoints", "port", &["fixed_port", "bound_port"]),
    ("instance_endpoints", "requested_mode", &["port_request"]),
    ("config_revisions", "values", &["config_values"]),
    ("config_revisions", "rendered_digest", &["rendered_sha256"]),
    ("secret_references", "scope", &["owner_kind", "owner_id"]),
    ("environments", "manifest_digest", &["manifest_sha256"]),
    ("environments", "lock_digest", &["lock_sha256"]),
    ("environments", "last_operation", &["last_operation_id"]),
    ("instance_demands", "session_policy", &["session_id"]),
    ("operations", "type", &["kind"]),
    (
        "operations",
        "resource_id",
        &["resource_kind", "resource_id"],
    ),
    ("operations", "status", &["state"]),
    (
        "operations",
        "progress",
        &["progress_done", "progress_total"],
    ),
    (
        "operations",
        "error_code",
        &["failure_code", "failure_retryable"],
    ),
    (
        "process_observations",
        "boot/session_id",
        &["boot_id", "session_id"],
    ),
    (
        "audit_events",
        "resource",
        &["resource_kind", "resource_id"],
    ),
    ("audit_events", "timestamp", &["occurred_at"]),
];

type Column = (String, String, bool, u8, Option<String>);

fn columns(conn: &Connection, table: &str) -> Vec<Column> {
    conn.prepare("SELECT name, type, \"notnull\", pk, dflt_value FROM pragma_table_info(?1)")
        .expect("test setup step succeeds")
        .query_map([table], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .expect("test setup step succeeds")
        .collect::<rusqlite::Result<_>>()
        .expect("test setup step succeeds")
}

/// A write connection like the registry's: foreign keys on and an
/// untrusted schema, so CHECKs and triggers run as they do in the daemon.
fn writer(harness: &Harness) -> Connection {
    let conn = harness.connect();
    conn.pragma_update(None, "foreign_keys", true)
        .expect("test setup step succeeds");
    conn.pragma_update(None, "trusted_schema", false)
        .expect("test setup step succeeds");
    conn
}

fn insert_provider_revision(conn: &Connection) {
    conn.execute(
        "INSERT INTO provider_revisions (pk, provider, revision, manifest_sha256, software, \
         capabilities, line_compatibility, created_at) VALUES (1, 'official/postgresql', \
         '2026.10.1', ?1, 'postgresql', '[\"start\"]', '{\"17\":[\"17\"]}', ?2)",
        (&"a".repeat(64), TS),
    )
    .expect("test setup step succeeds");
}

#[test]
fn every_table_and_column_matches_the_data_model() {
    let harness = Harness::new();
    harness.create();
    let conn = harness.connect();

    let tables: BTreeSet<String> = conn
        .prepare(
            "SELECT name FROM pragma_table_list WHERE schema = 'main' AND name NOT LIKE 'sqlite_%'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let expected: BTreeSet<String> = DATA_MODEL.iter().map(|(t, _)| (*t).to_owned()).collect();
    assert_eq!(tables, expected);
    assert_eq!(tables.len(), 26);
    for (table, expected_columns) in DATA_MODEL {
        let actual: Vec<_> = columns(&conn, table)
            .into_iter()
            .map(|(name, kind, notnull, pk, _)| (name, kind, notnull, pk))
            .collect();
        let expected: Vec<_> = expected_columns
            .iter()
            .map(|(name, kind, notnull, pk)| {
                ((*name).to_owned(), (*kind).to_owned(), *notnull, *pk)
            })
            .collect();
        assert_eq!(actual, expected, "columns of {table}");
        let (strict, without_rowid): (bool, bool) = conn
            .query_row(
                "SELECT strict, wr FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
                [table],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(strict, "{table} is STRICT");
        assert_eq!(
            without_rowid,
            *table == "instance_paths",
            "{table} WITHOUT ROWID"
        );
    }
}

#[test]
fn prd_fields_map_to_existing_columns() {
    let harness = Harness::new();
    harness.create();
    let conn = harness.connect();

    for (table, field, mapped) in PRD_FIELD_MAP {
        let names: Vec<String> = columns(&conn, table).into_iter().map(|c| c.0).collect();
        for column in *mapped {
            assert!(
                names.iter().any(|name| name == column),
                "PRD field {table}.{field} maps to missing column {column}"
            );
        }
    }
}

#[test]
fn every_foreign_key_has_an_index_led_by_its_child_columns() {
    let harness = Harness::new();
    harness.create();
    let conn = harness.connect();

    for (table, _) in DATA_MODEL {
        let mut foreign_keys: Vec<(i64, i64, String)> = conn
            .prepare("SELECT id, seq, \"from\" FROM pragma_foreign_key_list(?1)")
            .unwrap()
            .query_map([table], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        foreign_keys.sort();
        let mut by_id: Vec<(i64, Vec<String>)> = Vec::new();
        for (id, _, column) in foreign_keys {
            match by_id.last_mut() {
                Some((last, columns)) if *last == id => columns.push(column),
                _ => by_id.push((id, vec![column])),
            }
        }
        let indexes: Vec<(String, bool, bool)> = conn
            .prepare("SELECT name, \"unique\", partial FROM pragma_index_list(?1)")
            .unwrap()
            .query_map([table], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let index_columns: Vec<(Vec<String>, bool)> = indexes
            .into_iter()
            .filter(|(_, _, partial)| !partial)
            .map(|(name, unique, _)| {
                let columns: Vec<String> = conn
                    .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                    .unwrap()
                    .query_map([&name], |row| row.get::<_, Option<String>>(0))
                    .unwrap()
                    .map(|name| name.unwrap().unwrap_or_default())
                    .collect();
                (columns, unique)
            })
            .collect();
        for (_, children) in by_id {
            // An index whose leading columns are the child columns, or a
            // unique index on a leading part of them (equally selective).
            let covered = index_columns.iter().any(|(columns, unique)| {
                columns.starts_with(&children)
                    || (*unique && !columns.is_empty() && children.starts_with(columns))
            });
            assert!(
                covered,
                "foreign key {table}({}) has no index",
                children.join(", ")
            );
        }
    }
}

#[test]
fn a_new_registry_passes_the_foreign_key_check_and_reads_no_time_from_sqlite() {
    let harness = Harness::new();
    harness.create();
    let conn = harness.connect();

    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    for (table, _) in DATA_MODEL {
        for (name, _, _, _, default) in columns(&conn, table) {
            let default = default.unwrap_or_default().to_uppercase();
            assert!(
                !default.contains("CURRENT_") && !default.contains("NOW"),
                "{table}.{name} defaults to a SQLite time"
            );
        }
    }
}

#[test]
fn time_columns_accept_only_the_30_character_utc_form_and_round_trip() {
    let harness = Harness::new();
    harness.create();
    let conn = writer(&harness);
    let insert = |key: &str, at: &str| {
        conn.execute(
            "INSERT INTO settings (key, typed_value, revision, updated_at) VALUES (?1, '1', 1, ?2)",
            (key, at),
        )
    };

    for (index, bad) in [
        "2026-10-18T05:42:00+07:00",
        "2026-10-18T05:42:00.000Z",
        "2026-10-18T05:42:00Z",
        "2026-10-18 05:42:00.000000000Z",
    ]
    .into_iter()
    .enumerate()
    {
        let err = insert(&format!("bad.{index}"), bad).unwrap_err();
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "{bad}: {err}"
        );
    }
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    for index in 0..1000 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let seconds = i64::try_from(state % 253_402_300_799).unwrap();
        let nanos = i32::try_from(state % 1_000_000_000).unwrap();
        let ts = jiff::Timestamp::new(seconds, nanos).unwrap();
        let text = format!("{ts:.9}");
        assert_eq!(text.len(), 30, "{text}");
        insert(&format!("ts.{index}"), &text).unwrap();
        let back: String = conn
            .query_row(
                "SELECT updated_at FROM settings WHERE key = ?1",
                [format!("ts.{index}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(back.parse::<jiff::Timestamp>().unwrap(), ts);
    }
}

#[test]
fn provider_revision_manifest_fields_are_immutable() {
    let ev = evidence!(
        covers = ["BR-017"],
        kind = Kind::Fixture,
        expected = "an UPDATE of a provider revision's manifest fields is rejected by a trigger, \
                    while approval and approved_at may change",
    );
    let harness = Harness::new();
    harness.create();
    let conn = writer(&harness);
    insert_provider_revision(&conn);

    for update in [
        "UPDATE provider_revisions SET capabilities = '[]'",
        "UPDATE provider_revisions SET manifest_sha256 = 'b' || substr(manifest_sha256, 2)",
        "UPDATE provider_revisions SET line_compatibility = '{}'",
        "UPDATE provider_revisions SET software = 'mysql'",
    ] {
        let err = conn.execute(update, []).unwrap_err();
        assert!(
            err.to_string().contains("provider revision is immutable"),
            "{update}: {err}"
        );
    }
    conn.execute(
        "UPDATE provider_revisions SET approval = 'approved', approved_at = ?1",
        [TS],
    )
    .unwrap();
    ev.pass();
}

#[test]
fn audit_events_are_append_only_and_closed_code_sets_reject_other_codes() {
    let harness = Harness::new();
    harness.create();
    let conn = writer(&harness);
    conn.execute(
        "INSERT INTO audit_events (id, actor, action, result, occurred_at) VALUES \
         ('0199f000-0000-7000-8000-0000000000a0', 'cli', 'instance.create', 'succeeded', ?1)",
        [TS],
    )
    .unwrap();

    let err = conn
        .execute("UPDATE audit_events SET result = 'failed'", [])
        .unwrap_err();
    assert!(
        err.to_string().contains("audit events are append only"),
        "{err}"
    );
    let err = conn
        .execute(
            "INSERT INTO settings (key, typed_value, revision, updated_at) VALUES ('x', 'not json', 1, ?1)",
            [TS],
        )
        .unwrap_err();
    assert!(err.to_string().contains("CHECK constraint failed"), "{err}");
    insert_provider_revision(&conn);
    conn.execute(
        "INSERT INTO artifacts (pk, sha256, software, version, os, arch, created_at) VALUES \
         (1, ?1, 'postgresql', '17.6', 'macos', 'aarch64', ?2)",
        (&"c".repeat(64), TS),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runtimes (id, software, version, line, os, arch, install_mode, \
         artifact_sha256, state, revision, created_at, updated_at) VALUES \
         ('0199f000-0000-7000-8000-000000000020', 'postgresql', '17.6', '17', 'macos', \
         'aarch64', 'managed', ?1, 'installed', 1, ?2, ?2)",
        (&"c".repeat(64), TS),
    )
    .unwrap();
    let insert_instance = |lifecycle: &str| {
        conn.execute(
            "INSERT INTO instances (id, name, provider_revision_pk, software, runtime_id, \
             data_line, data_version, data_initialized_at, desired, startup, lifecycle, \
             ownership, created_by_operation, revision, created_at, updated_at) VALUES \
             ('0199f000-0000-7000-8000-000000000030', 'orders', 1, 'postgresql', \
             '0199f000-0000-7000-8000-000000000020', '17', '17.6', ?2, 'stopped', 'manual', ?1, \
             'created', '0199f000-0000-7000-8000-000000000050', 1, ?2, ?2)",
            (lifecycle, TS),
        )
    };
    let err = insert_instance("paused").unwrap_err();
    assert!(err.to_string().contains("CHECK constraint failed"), "{err}");
    insert_instance("stopped").unwrap();
    let err = conn.execute("DELETE FROM runtimes", []).unwrap_err();
    assert!(
        err.to_string().contains("FOREIGN KEY constraint failed"),
        "{err}"
    );
}

#[test]
fn secret_references_hold_only_reference_columns_and_no_table_has_a_secret_value_column() {
    let harness = Harness::new();
    harness.create();
    let conn = harness.connect();

    let names =
        |table: &str| -> Vec<String> { columns(&conn, table).into_iter().map(|c| c.0).collect() };
    assert_eq!(
        names("secret_references"),
        [
            "id",
            "reference",
            "platform_store_key",
            "owner_kind",
            "owner_id",
            "purpose",
            "value_origin",
            "status",
            "status_observed_at",
            "value_updated_at",
            "revision",
            "created_at",
            "updated_at"
        ]
    );
    assert_eq!(
        names("secret_reference_uses"),
        ["reference_id", "resource_kind", "resource_id", "field"]
    );
    for (table, columns) in DATA_MODEL {
        for (column, _, _, _) in *columns {
            for word in ["secret", "password", "token", "credential", "plaintext"] {
                assert!(
                    !column.contains(word),
                    "{table}.{column} looks like it holds a secret value"
                );
            }
        }
    }
}

const REFERENCE_ID: &str = "0199f000-0000-7000-8000-000000000060";
const OWNER_ID: &str = "0199f000-0000-7000-8000-000000000030";

fn insert_reference(conn: &Connection, owner_kind: &str, owner_id: Option<&str>) {
    conn.execute(
        "INSERT INTO secret_references (id, reference, platform_store_key, owner_kind, owner_id, \
         purpose, status, revision, created_at, updated_at) VALUES (?1, 'local:erp/admin', \
         'fake:stackly-secret/erp/admin', ?2, ?3, 'admin', 'missing', 1, ?4, ?4)",
        (REFERENCE_ID, owner_kind, owner_id, TS),
    )
    .map(drop)
    .unwrap_or_else(|err| panic!("insert a {owner_kind} owned reference: {err}"));
}

#[test]
fn a_secret_reference_owner_is_fixed_except_the_move_to_the_user() {
    let ev = evidence!(
        covers = ["BR-018"],
        kind = Kind::Fixture,
        expected = "the secret_references_owner_fixed trigger rejects an owner change other \
                    than the move to the user, the owner check ties owner_id to the owner kind, \
                    and a used reference cannot be deleted raw",
    );
    let harness = Harness::new();
    harness.create();
    let conn = writer(&harness);
    for (kind, id) in [("user", Some(OWNER_ID)), ("instance", None)] {
        let err = conn
            .execute(
                "INSERT INTO secret_references (id, reference, platform_store_key, owner_kind, \
                 owner_id, purpose, status, revision, created_at, updated_at) VALUES \
                 ('0199f000-0000-7000-8000-000000000061', 'local:erp/other', 'k', ?1, ?2, \
                 'user', 'missing', 1, ?3, ?3)",
                (kind, id, TS),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "{kind}: {err}"
        );
    }
    insert_reference(&conn, "instance", Some(OWNER_ID));

    for update in [
        "UPDATE secret_references SET owner_kind = 'environment'",
        "UPDATE secret_references SET owner_id = '0199f000-0000-7000-8000-000000000031'",
        "UPDATE secret_references SET owner_kind = 'user'",
    ] {
        let err = conn.execute(update, []).unwrap_err();
        assert!(
            err.to_string()
                .contains("the owner of a secret reference is fixed")
                || err.to_string().contains("CHECK constraint failed"),
            "{update}: {err}"
        );
    }
    conn.execute(
        "UPDATE secret_references SET status = 'present', revision = 2",
        [],
    )
    .expect("the value columns may change");
    conn.execute(
        "UPDATE secret_references SET owner_kind = 'user', owner_id = NULL",
        [],
    )
    .expect("the owner may move to the user");
    let err = conn
        .execute(
            "UPDATE secret_references SET owner_kind = 'instance', owner_id = ?1",
            [OWNER_ID],
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("the owner of a secret reference is fixed"),
        "{err}"
    );

    conn.execute(
        "INSERT INTO secret_reference_uses (reference_id, resource_kind, resource_id, field) \
         VALUES (?1, 'instance', ?2, 'adminPassword')",
        (REFERENCE_ID, OWNER_ID),
    )
    .unwrap();
    for (kind, field) in [("runtime", "adminPassword"), ("instance", "Admin")] {
        let err = conn
            .execute(
                "INSERT INTO secret_reference_uses (reference_id, resource_kind, resource_id, \
                 field) VALUES (?1, ?2, ?3, ?4)",
                (REFERENCE_ID, kind, OWNER_ID, field),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "{kind} {field}: {err}"
        );
    }
    let err = conn
        .execute("DELETE FROM secret_references", [])
        .unwrap_err();
    assert!(
        err.to_string().contains("FOREIGN KEY constraint failed"),
        "{err}"
    );
    ev.pass();
}
