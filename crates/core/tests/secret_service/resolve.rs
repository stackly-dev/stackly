//! Reading every value an instance needs before it starts (spec 0011
//! AC-11), the needs secret blocker, and learning the values of running
//! instances.

use stackly_core::provider::{ApplyClass, SettingName};
use stackly_core::secret::{SecretCaller, SecretError};
use stackly_domain::{
    InstanceBlocker, InstanceLifecycle, NeedsSecretItem, NeedsSecretReason, Registry, SecretStatus,
    SecretUser,
};
use stackly_platform::{Clock, Interaction, SecretStoreError};
use stackly_testkit::SecretOp;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, ScriptedStore, reference, user};

fn setting(name: &str) -> SettingName {
    name.parse().expect("a valid test setting name")
}

fn needs(err: SecretError) -> Vec<NeedsSecretItem> {
    match err {
        SecretError::NeedsSecret { needs } => needs.references,
        other => panic!("expected needs secret, got {other:?}"),
    }
}

#[test]
fn a_locked_store_and_a_missing_value_give_needs_secret_in_reference_order() {
    let ev = evidence!(
        covers = ["AC-062", "VAL-018"],
        kind = Kind::Fixture,
        expected = "resolve_for_start on a locked fake store with one value missing returns \
                    needs secret listing both references in text order with reasons \
                    store_locked and missing, and never a stand in value",
    );
    let harness = Harness::new();
    let instance = harness.instance(false);
    let locked = reference("local:b/locked");
    let missing = reference("local:a/missing");
    harness.set(&locked, "the-value-behind-the-lock");
    harness.set(&missing, "the-value-that-goes-missing");
    harness.store().remove(&missing);
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &locked), ("replicaPassword", &missing)],
    );
    harness.store().lock();

    let err = harness
        .service
        .resolve_for_start(instance, &SecretCaller::background())
        .unwrap_err();

    assert_eq!(err.code().as_str(), "secret.needs_secret");
    let items = needs(err);
    let listed: Vec<_> = items
        .iter()
        .map(|item| (item.reference.clone(), item.reason, item.generate_allowed))
        .collect();
    assert_eq!(
        listed,
        [
            (missing.clone(), NeedsSecretReason::Missing, true),
            (locked, NeedsSecretReason::StoreLocked, true),
        ]
    );
    assert_eq!(
        harness.view(&missing).unwrap().record.status,
        SecretStatus::Missing,
        "a missing value updates the cache"
    );
    ev.pass();
}

#[test]
fn after_the_user_denies_one_dialog_the_other_references_are_not_read() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let first = reference("local:a/first");
    let second = reference("local:b/second");
    harness.set(&first, "the-first-value-here");
    harness.set(&second, "the-second-value-here");
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &first), ("replicaPassword", &second)],
    );
    harness.store().deny(&first);
    let reads = harness.store().count(SecretOp::Read);

    let err = harness
        .service
        .resolve_for_start(instance, &user())
        .unwrap_err();

    assert_eq!(harness.store().count(SecretOp::Read), reads + 1);
    let reasons: Vec<_> = needs(err).iter().map(|item| item.reason).collect();
    assert_eq!(
        reasons,
        [
            NeedsSecretReason::AccessDenied,
            NeedsSecretReason::AccessDenied
        ]
    );
    let interactions: Vec<_> = harness
        .store()
        .calls()
        .into_iter()
        .filter(|call| call.op == SecretOp::Read)
        .map(|call| call.interaction)
        .collect();
    assert_eq!(interactions.last(), Some(&Some(Interaction::AllowPrompt)));
}

#[test]
fn a_backend_error_wins_over_needs_secret() {
    let (harness, store) = ScriptedStore::harness();
    let instance = harness.instance(false);
    let missing = reference("local:a/missing");
    let broken = reference("local:b/broken");
    harness.set(&missing, "the-value-that-goes-missing");
    harness.set(&broken, "the-value-behind-the-backend");
    harness.store().remove(&missing);
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &missing), ("replicaPassword", &broken)],
    );
    *store.fail_reads_of.lock().unwrap() =
        Some((broken.clone(), SecretStoreError::Backend { code: -26276 }));

    let err = harness
        .service
        .resolve_for_start(instance, &SecretCaller::background())
        .unwrap_err();

    assert_eq!(
        err,
        SecretError::Store {
            reference: broken,
            error: SecretStoreError::Backend { code: -26276 }
        }
    );
    assert_eq!(err.code().as_str(), "secret.store_backend");
}

#[test]
fn a_cached_missing_status_never_skips_the_read() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let target = reference("local:erp/admin");
    harness.set(&target, "the-value-of-the-test");
    harness.store().remove(&target);
    harness.service.probe_one(&target).unwrap();
    assert_eq!(
        harness.view(&target).unwrap().record.status,
        SecretStatus::Missing
    );
    harness.store().insert(&target, "the-value-put-back");
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &target), ("monitorPassword", &target)],
    );

    let resolved = harness
        .service
        .resolve_for_start(instance, &SecretCaller::background())
        .unwrap();

    assert_eq!(
        resolved.fields().collect::<Vec<_>>(),
        [&setting("adminPassword"), &setting("monitorPassword")]
    );
    assert!(resolved.get(&setting("adminPassword")).is_some());
    assert_eq!(harness.redactor.redact("the-value-put-back"), "****");
}

#[test]
fn generate_is_not_allowed_for_a_field_fixed_after_its_data_is_initialized() {
    let harness = Harness::new();
    let initialized = harness.instance(true);
    let fresh = harness.instance(false);
    let fixed = reference("local:a/fixed");
    let open = reference("local:b/open");
    harness.set(&fixed, "the-fixed-value-here");
    harness.set(&open, "the-open-value-here");
    harness.store().remove(&fixed);
    harness.store().remove(&open);
    harness.uses(
        SecretUser::Instance(initialized),
        &[("adminPassword", &fixed)],
    );
    harness.uses(SecretUser::Instance(fresh), &[("adminPassword", &open)]);
    harness.rule(
        initialized,
        "adminPassword",
        8,
        Some(64),
        Some(ApplyClass::ImmutableAfterInit),
    );
    harness.rule(
        fresh,
        "adminPassword",
        8,
        Some(64),
        Some(ApplyClass::ImmutableAfterInit),
    );

    let fixed_needs = needs(
        harness
            .service
            .resolve_for_start(initialized, &SecretCaller::background())
            .unwrap_err(),
    );
    let open_needs = needs(
        harness
            .service
            .resolve_for_start(fresh, &SecretCaller::background())
            .unwrap_err(),
    );

    assert!(!fixed_needs[0].generate_allowed);
    assert!(open_needs[0].generate_allowed);
}

#[test]
fn a_needs_secret_blocker_keeps_the_lifecycle_and_revision_and_clears_once_every_reference_is_present()
 {
    let harness = Harness::new();
    let id = harness.instance(false);
    let target = reference("local:erp/admin");
    harness.set(&target, "the-value-of-the-test");
    harness.store().remove(&target);
    harness.uses(SecretUser::Instance(id), &[("adminPassword", &target)]);
    let mut instance = harness.registry.instance(id).unwrap().value.unwrap();
    let (lifecycle, revision) = (instance.lifecycle(), instance.revision());

    let items = needs(
        harness
            .service
            .resolve_for_start(id, &SecretCaller::background())
            .unwrap_err(),
    );
    let blocker = InstanceBlocker::needs_secret(items, harness.fake.clock().now());
    instance.observe_blocker(blocker.clone());

    assert_eq!(instance.blocker(), Some(&blocker));
    assert_eq!(
        (instance.lifecycle(), instance.revision()),
        (lifecycle, revision)
    );
    assert!(!harness.service.clears_blocker(&blocker).unwrap());

    harness.set(&target, "the-value-entered-again");
    let change = harness.changes.all().pop().unwrap();
    assert_eq!(change.reference, target);
    assert!(harness.service.clears_blocker(&blocker).unwrap());
    instance.clear_blocker();
    assert_eq!(instance.blocker(), None);
    harness
        .service
        .resolve_for_start(id, &SecretCaller::background())
        .unwrap();
}

#[test]
fn learning_the_running_instances_reads_without_a_dialog_and_counts_failures() {
    let harness = Harness::new();
    let running = harness.instance(true);
    let stopped = harness.instance(false);
    let known = reference("local:a/known");
    let gone = reference("local:b/gone");
    let other = reference("local:c/other");
    harness.set(&known, "the-value-to-learn-now");
    harness.set(&gone, "the-value-that-is-gone");
    harness.set(&other, "the-value-of-a-stopped-one");
    harness.store().remove(&gone);
    harness.uses(
        SecretUser::Instance(running),
        &[("adminPassword", &known), ("replicaPassword", &gone)],
    );
    harness.uses(SecretUser::Instance(stopped), &[("adminPassword", &other)]);
    // A fresh redactor, as after a daemon restart.
    let fresh = harness.restarted();

    let report = fresh
        .service
        .learn_running([
            (running, InstanceLifecycle::Running),
            (stopped, InstanceLifecycle::Stopped),
        ])
        .unwrap();

    assert_eq!(report.learned, 1);
    assert_eq!(report.failed.get("missing"), Some(&1));
    assert_eq!(fresh.redactor.redact("the-value-to-learn-now"), "****");
    assert_eq!(
        fresh.redactor.redact("the-value-of-a-stopped-one"),
        "the-value-of-a-stopped-one"
    );
    assert!(
        harness
            .store()
            .calls()
            .iter()
            .filter(|call| call.op == SecretOp::Read)
            .all(|call| call.interaction == Some(Interaction::Never))
    );
}
