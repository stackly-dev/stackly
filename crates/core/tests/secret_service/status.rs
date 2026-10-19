//! List, status, and the probes that keep the cache true (spec 0011 AC-8).

use stackly_core::secret::{ChangeStatus, SecretError};
use stackly_domain::{
    PlatformStoreKey, Registry, SecretOwner, SecretOwnerKind, SecretPurpose, SecretReferenceFilter,
    SecretReferenceId, SecretReferenceRecord, SecretStatus, WriteOp,
};
use stackly_platform::{Clock, SecretBackend, SecretStoreError, StoreState};
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, reference};

#[test]
fn list_reads_the_cache_and_the_cached_store_state_without_a_store_call() {
    let harness = Harness::new();
    let a = reference("local:erp/a");
    let b = reference("local:erp/b");
    harness.set(&b, "the-value-behind-b");
    harness.set(&a, "the-value-behind-a");
    harness.store().remove(&a);
    harness.store().lock();
    let calls = harness.store().calls().len();

    let listing = harness
        .service
        .list(SecretReferenceFilter::default())
        .unwrap();

    assert_eq!(harness.store().calls().len(), calls);
    let listed: Vec<_> = listing
        .items
        .iter()
        .map(|view| (view.record.reference.clone(), view.record.status))
        .collect();
    assert_eq!(
        listed,
        [(a, SecretStatus::Present), (b, SecretStatus::Present)],
        "the list shows the cache, even for an item removed since"
    );
    assert_eq!(listing.store.backend, SecretBackend::Fake);
    assert_eq!(
        listing.store.state,
        StoreState::Available,
        "the state is the last one a call saw"
    );
    let users = harness
        .service
        .list(SecretReferenceFilter {
            owner_kind: Some(SecretOwnerKind::Instance),
            status: None,
        })
        .unwrap();
    assert!(users.items.is_empty());
}

#[test]
fn status_probes_again_and_an_item_removed_outside_reads_missing() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "secret.status probes the store again: an item removed outside Stackly \
                    reads missing, an item that cannot be checked reads unknown, the revision \
                    stays, and each status change is reported",
    );
    let harness = Harness::new();
    let target = reference("local:erp/admin");
    harness.set(&target, "the-value-of-the-test");
    harness.store().remove(&target);

    let missing = harness.service.status(&target).unwrap();
    assert_eq!(missing.items[0].record.status, SecretStatus::Missing);
    assert_eq!(missing.items[0].record.revision.get(), 1);
    assert_eq!(
        harness.view(&target).unwrap().record.status,
        SecretStatus::Missing
    );

    harness
        .store()
        .fail_next(SecretStoreError::Backend { code: -50 });
    let unknown = harness.service.status(&target).unwrap();
    assert_eq!(unknown.items[0].record.status, SecretStatus::Unknown);
    assert_eq!(harness.view(&target).unwrap().record.revision.get(), 1);

    let statuses: Vec<_> = harness
        .changes
        .all()
        .iter()
        .map(|change| (change.status, change.revision.get()))
        .collect();
    assert_eq!(
        statuses,
        [
            (ChangeStatus::Present, 1),
            (ChangeStatus::Missing, 1),
            (ChangeStatus::Unknown, 1)
        ]
    );
    let err = harness
        .service
        .status(&reference("local:not/registered"))
        .unwrap_err();
    assert!(matches!(err, SecretError::NotFound { .. }), "{err:?}");
    ev.pass();
}

#[test]
fn a_probe_of_every_reference_records_them_in_one_commit_without_raising_a_revision() {
    let harness = Harness::new();
    let present = reference("local:erp/present");
    let missing = reference("local:erp/missing");
    harness.set(&present, "the-present-value");
    harness.set(&missing, "the-missing-value");
    harness.store().remove(&missing);
    let before = harness.registry.known_instance_ids().unwrap().generation;

    let report = harness.service.probe_all().unwrap();

    assert_eq!((report.present, report.missing, report.unknown), (1, 1, 0));
    let after = harness.registry.known_instance_ids().unwrap().generation;
    assert_eq!(after, before.next(), "one commit for every observation");
    assert_eq!(
        harness.view(&missing).unwrap().record.status,
        SecretStatus::Missing
    );
    assert_eq!(harness.view(&missing).unwrap().record.revision.get(), 1);
    assert_eq!(harness.view(&present).unwrap().record.revision.get(), 1);

    harness.store().unsupported();
    let report = harness.service.probe_all().unwrap();
    assert_eq!((report.present, report.missing, report.unknown), (0, 0, 2));
    assert_eq!(
        harness.service.store_status().state,
        StoreState::Unsupported
    );
}

#[test]
fn a_row_with_another_store_key_is_probed_and_recorded_under_the_computed_key() {
    let harness = Harness::new();
    let target = reference("local:moved/admin");
    let now = harness.fake.clock().now();
    let mut record = SecretReferenceRecord::new(
        SecretReferenceId::generate(),
        target.clone(),
        PlatformStoreKey::new(
            "macos-keychain:file:other-root/stackly-secret/moved/admin".to_owned(),
        )
        .unwrap(),
        SecretOwner::User,
        SecretPurpose::user(),
        now,
    );
    record.status = SecretStatus::Present;
    harness.commit(vec![WriteOp::InsertSecretReference(record)]);

    let probed = harness.service.probe_one(&target).unwrap();

    assert_eq!(
        probed.record.platform_store_key.as_str(),
        "fake:stackly-secret/moved/admin"
    );
    assert_eq!(probed.record.status, SecretStatus::Missing);
    assert_eq!(
        harness.view(&target).unwrap().record.platform_store_key,
        probed.record.platform_store_key
    );
}
