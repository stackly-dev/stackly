//! Migration 13 (spec 0037, step 1): the `provider_imports` table of
//! imported local provider packages and their trust decision.

use rusqlite::Connection;
use stackly_domain::{ApprovalState, ProviderRevisionKey, Registry, TrustDecision};

use crate::common::{Harness, ready, row_counts};
use crate::fixture::fixture;

const TS: &str = "2026-01-02T03:04:05.123456789Z";

// covers: AC-18 (spec 0037)
#[test]
fn a_version_12_registry_migrates_with_every_row_and_an_empty_import_table() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(12));
    let before = row_counts(&harness.connect());

    let (registry, summary) = ready(harness.open());

    assert_eq!(summary.migrated_from, Some(12));
    assert!(summary.snapshot.is_some(), "a migration takes a snapshot");
    let after = row_counts(&harness.connect());
    let kept = |counts: Vec<(String, i64)>| -> Vec<(String, i64)> {
        counts
            .into_iter()
            .filter(|(table, _)| table != "schema_migrations" && table != "provider_imports")
            .collect()
    };
    assert_eq!(kept(after.clone()), kept(before), "every row stays");
    assert!(after.contains(&("provider_imports".to_owned(), 0)));
    assert_eq!(registry.provider_imports().unwrap().value, Vec::new());
}

// covers: AC-18 (spec 0037)
#[test]
fn the_version_13_fixture_reads_its_imports_and_pinned_approval() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(13));
    let (registry, summary) = ready(harness.open());
    assert_eq!(summary.migrated_from, Some(13));

    let imports = registry.provider_imports().unwrap().value;
    let names: Vec<&str> = imports
        .iter()
        .map(|record| record.provider.as_str())
        .collect();
    assert_eq!(names, ["acme/demo", "acme/tool"], "ordered by provider");
    let demo = &imports[0];
    assert_eq!(demo.approval(), ApprovalState::Approved);
    assert_eq!(demo.quarantined, Some(true));
    assert_eq!(demo.quarantine_agent.as_deref(), Some("Safari"));
    assert_eq!(demo.package_bytes, 6144);
    assert_eq!(demo.revision.get(), 2);
    let tool = &imports[1];
    assert_eq!(
        tool.decision.as_ref().map(|decision| decision.decision),
        Some(TrustDecision::Revoked)
    );
    assert_eq!(
        tool.approval(),
        ApprovalState::Pending,
        "a decision on other bytes leaves the stored bytes pending"
    );
    assert_eq!(tool.quarantined, None);
    assert_eq!(
        registry
            .provider_import(&"acme/tool".parse().unwrap())
            .unwrap()
            .value
            .as_ref(),
        Some(tool)
    );

    let key: ProviderRevisionKey = format!("acme/demo@1.0.0#{}", "d".repeat(64))
        .parse()
        .unwrap();
    let pinned = registry.provider_revision(&key).unwrap().value.unwrap();
    assert_eq!(pinned.approval, Some(TrustDecision::Approved));
    assert_eq!(
        pinned.approved_at,
        Some("2026-01-02T03:08:00Z".parse().unwrap())
    );
    assert!(
        registry
            .provider_has_approved_revision(&"acme/demo".parse().unwrap())
            .unwrap()
            .value
    );
    assert!(
        !registry
            .provider_has_approved_revision(&"acme/tool".parse().unwrap())
            .unwrap()
            .value
    );
}

fn insert(conn: &Connection, values: &str) -> rusqlite::Result<usize> {
    conn.execute(
        &format!(
            "INSERT INTO provider_imports (provider, source_id, provider_revision, software, \
             manifest_sha256, versions_sha256, package_sha256, package_bytes, origin_path, \
             quarantined, quarantine_agent, imported_at, decision, decided_package_sha256, \
             decided_revision, decided_permissions, decided_at, revision, created_at, \
             updated_at) VALUES ({values})"
        ),
        [],
    )
}

fn row(provider: &str, package: char, tail: &str) -> String {
    let digest = package.to_string().repeat(64);
    format!(
        "'{provider}', '0199f000-0000-7000-8000-0000000000c0', '1.0.0', 'demo', '{digest}', \
         '{digest}', '{digest}', {tail}, 1, '{TS}', '{TS}'"
    )
}

// covers: AC-18 (spec 0037)
#[test]
fn the_import_table_refuses_what_the_data_model_forbids() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(13));
    drop(ready(harness.open()));
    let conn = harness.connect();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    let ok = format!("2048, '~/p', NULL, NULL, '{TS}', NULL, NULL, NULL, NULL, NULL");

    insert(&conn, &row("acme/fine", '5', &ok)).expect("a row without a decision is valid");
    let refused = [
        (
            "half a decision",
            row(
                "acme/half",
                '6',
                &format!("2048, '~/p', NULL, NULL, '{TS}', 'approved', NULL, NULL, NULL, NULL"),
            ),
        ),
        (
            "a package above 4 MiB",
            row(
                "acme/big",
                '7',
                &format!("4194305, '~/p', NULL, NULL, '{TS}', NULL, NULL, NULL, NULL, NULL"),
            ),
        ),
        (
            "an empty origin path",
            row(
                "acme/empty",
                '8',
                &format!("2048, '', NULL, NULL, '{TS}', NULL, NULL, NULL, NULL, NULL"),
            ),
        ),
        (
            "a quarantine flag that is not 0 or 1",
            row(
                "acme/flag",
                '9',
                &format!("2048, '~/p', 2, NULL, '{TS}', NULL, NULL, NULL, NULL, NULL"),
            ),
        ),
        (
            "decided permissions that are not an object",
            row(
                "acme/array",
                'a',
                &format!(
                    "2048, '~/p', NULL, NULL, '{TS}', 'approved', '{}', '1.0.0', '[]', '{TS}'",
                    "a".repeat(64)
                ),
            ),
        ),
        (
            "a package digest another row holds",
            row("acme/twin", '5', &ok),
        ),
        (
            "a source that does not exist",
            row("acme/orphan", 'b', &ok).replace(
                "0199f000-0000-7000-8000-0000000000c0",
                "0199f000-0000-7000-8000-0000000000c9",
            ),
        ),
        ("an invalid provider ID", row("Acme", 'c', &ok)),
    ];
    for (what, values) in refused {
        assert!(insert(&conn, &values).is_err(), "{what} is refused");
    }
}
