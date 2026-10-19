//! Migration 3, the secret references of spec 0011 (spec 0005 "Tambahan
//! fitur 11", AC-1, AC-4, AC-12): migrating the version 2 fixture drops the
//! old secret reference rows and keeps everything else, and the secret and
//! audit rows of the frozen version 3 fixture read back through the
//! registry.

use stackly_domain::{
    AuditActor, AuditQuery, AuditResource, AuditResult, EnvironmentId, InstanceId, Registry,
    SecretOrigin, SecretOwner, SecretRef, SecretReferenceFilter, SecretStatus, SecretUse,
    SecretUser,
};
use stackly_storage::MigrationSet;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, ready, row_counts};
use crate::fixture::fixture;

const INSTANCE: &str = "0199f000-0000-7000-8000-000000000030";
const ENVIRONMENT: &str = "0199f000-0000-7000-8000-000000000070";

fn reference(text: &str) -> SecretRef {
    SecretRef::parse(text).expect("the test reference is valid")
}

#[test]
fn migrating_v2_replaces_the_old_secret_references_and_keeps_every_other_row() {
    let ev = evidence!(
        covers = ["BR-018"],
        kind = Kind::Fixture,
        expected = "migrating the version 2 fixture to version 3 takes a verified snapshot, \
                    drops the old secret_references rows, creates secret_reference_uses empty, \
                    and keeps the row count of every other table",
    );
    let harness = Harness::new();
    harness.load_fixture(&fixture(2));
    let kept = |table: &str| table != "schema_migrations" && !table.starts_with("secret_");
    let mut before = row_counts(&harness.connect());
    before.retain(|(table, _)| kept(table));

    let (registry, summary) = ready(harness.open());

    assert_eq!(summary.migrated_from, Some(2));
    assert_eq!(summary.schema_version, MigrationSet::builtin().latest());
    assert!(summary.snapshot.is_some(), "a migration takes a snapshot");
    let counts = row_counts(&harness.connect());
    let count_of = |table: &str| {
        counts
            .iter()
            .find(|(name, _)| name == table)
            .map(|(_, count)| *count)
    };
    assert_eq!(count_of("secret_references"), Some(0));
    assert_eq!(count_of("secret_reference_uses"), Some(0));
    let mut after = counts.clone();
    after.retain(|(table, _)| kept(table));
    assert_eq!(after, before, "the version 2 data survives");
    assert!(
        registry
            .secret_references(SecretReferenceFilter::default())
            .unwrap()
            .value
            .is_empty()
    );
    ev.pass();
}

#[test]
fn the_v3_fixture_secret_and_audit_rows_read_back_through_the_registry() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(3));
    let (registry, _) = ready(harness.open());
    let instance = SecretUser::Instance(INSTANCE.parse::<InstanceId>().unwrap());
    let environment = SecretUser::Environment(ENVIRONMENT.parse::<EnvironmentId>().unwrap());

    let listed = registry
        .secret_references(SecretReferenceFilter::default())
        .unwrap()
        .value;
    let texts: Vec<&str> = listed
        .iter()
        .map(|view| view.record.reference.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "local:erp/postgres/admin",
            "local:instance/0199f000-0000-7000-8000-000000000030/admin-password",
            "local:shop/api-token",
        ]
    );

    let admin = &listed[1];
    assert_eq!(
        admin.record.owner,
        SecretOwner::Instance(INSTANCE.parse().unwrap())
    );
    assert_eq!(admin.record.purpose.as_str(), "admin-password");
    assert_eq!(admin.record.value_origin, Some(SecretOrigin::Generated));
    assert_eq!(admin.record.status, SecretStatus::Present);
    assert_eq!(admin.record.revision.get(), 1);
    assert_eq!(
        admin.used_by,
        [SecretUse::new(
            admin.record.reference.clone(),
            instance,
            "adminPassword".parse().unwrap()
        )]
    );
    let user = &listed[0];
    assert_eq!(user.record.owner, SecretOwner::User);
    assert_eq!(user.record.status, SecretStatus::Unknown);
    assert_eq!(user.record.revision.get(), 2);
    let token = &listed[2];
    assert_eq!(
        token.record.owner,
        SecretOwner::Environment(ENVIRONMENT.parse().unwrap())
    );
    assert_eq!(token.record.value_origin, None);
    assert_eq!(token.record.status_observed_at, None);

    let environment_uses = registry.secret_uses_by(environment).unwrap().value;
    let uses: Vec<(&str, &str)> = environment_uses
        .iter()
        .map(|used| (used.field.as_str(), used.reference.as_str()))
        .collect();
    assert_eq!(
        uses,
        [
            ("api_token", "local:shop/api-token"),
            ("erp_admin", "local:erp/postgres/admin"),
        ]
    );
    let present = registry
        .secret_references(SecretReferenceFilter {
            owner_kind: None,
            status: Some(SecretStatus::Present),
        })
        .unwrap()
        .value;
    assert_eq!(present.len(), 1);
    assert!(
        registry
            .secret_reference(&reference("local:gone/admin"))
            .unwrap()
            .value
            .is_none()
    );

    let page = registry
        .audit_events(AuditQuery {
            limit: 2,
            before: None,
        })
        .unwrap()
        .value;
    let ids: Vec<String> = page.items.iter().map(|row| row.id.to_string()).collect();
    assert_eq!(
        ids,
        [
            "0199f000-0000-7000-8000-0000000000a2",
            "0199f000-0000-7000-8000-0000000000a0",
        ]
    );
    let reveal = &page.items[0];
    assert_eq!(reveal.actor, AuditActor::User);
    assert_eq!(reveal.action.as_str(), "secret.reveal");
    assert_eq!(reveal.result, AuditResult::Failed);
    assert_eq!(reveal.resource, None);
    assert_eq!(
        reveal.safe_details.get("errorCode"),
        Some("secret.not_found")
    );
    let set = &page.items[1];
    assert_eq!(
        set.resource,
        Some(AuditResource::SecretReference(
            "0199f000-0000-7000-8000-000000000061".parse().unwrap()
        ))
    );
    assert_eq!(set.safe_details.get("peerUid"), Some("501"));
    let rest = registry
        .audit_events(AuditQuery {
            limit: 500,
            before: page.next,
        })
        .unwrap()
        .value;
    assert_eq!(rest.items.len(), 1);
    assert_eq!(rest.items[0].actor, AuditActor::Daemon);
    assert_eq!(
        rest.items[0].operation.map(|id| id.to_string()).as_deref(),
        Some("0199f000-0000-7000-8000-000000000050")
    );
    assert_eq!(rest.next, None);
}
