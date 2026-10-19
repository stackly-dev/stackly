//! Reveal with a fail closed audit (spec 0011 AC-9) and delete with row
//! restore (AC-10).

use stackly_core::secret::{
    ChangeStatus, ConflictReason, RevealPurpose, SecretCaller, SecretError,
};
use stackly_domain::{
    AuditActor, AuditResource, AuditResult, RegistryError, ResourceRevision, SecretUse, SecretUser,
};
use stackly_platform::{Interaction, SecretStoreError};
use stackly_testkit::SecretOp;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

use crate::common::{Harness, field_name, reference, user};

fn revision(number: u64) -> ResourceRevision {
    ResourceRevision::new(number).expect("a test revision is at least 1")
}

#[test]
fn a_reveal_returns_the_value_and_writes_one_audit_row_without_it() {
    let ev = evidence!(
        covers = ["AC-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "secret.reveal returns the stored value and writes one secret.reveal audit \
                    row with the actor, client, reference, purpose, and result succeeded, and \
                    the audit rows never hold the value",
    );
    let harness = Harness::new();
    let credential = FixtureCredential::new();
    let target = reference("local:erp/admin");
    harness.set(&target, credential.value());
    let id = harness.view(&target).unwrap().record.id;

    let revealed = harness
        .service
        .reveal(&target, RevealPurpose::Copy, &user())
        .unwrap();

    assert_eq!(revealed.expose(), credential.value());
    let audit = harness.audit();
    let row = audit.last().unwrap();
    assert_eq!(row.action.as_str(), "secret.reveal");
    assert_eq!(row.actor, AuditActor::User);
    assert_eq!(row.result, AuditResult::Succeeded);
    assert_eq!(row.resource, Some(AuditResource::SecretReference(id)));
    assert_eq!(row.safe_details.get("purpose"), Some("copy"));
    assert_eq!(row.safe_details.get("clientKind"), Some("cli"));
    assert_eq!(row.safe_details.get("peerUid"), Some("501"));
    assert_eq!(row.safe_details.get("errorCode"), None);
    credential.assert_absent(&format!("{audit:?}"), "the audit rows");
    ev.pass();
}

#[test]
fn a_reveal_that_fails_writes_a_failed_row_with_the_error_code() {
    let harness = Harness::new();
    let target = reference("local:erp/admin");
    harness.set(&target, "the-value-of-the-test");
    let failed_code = || {
        let audit = harness.audit();
        let row = audit.last().unwrap().clone();
        assert_eq!(row.result, AuditResult::Failed);
        row.safe_details.get("errorCode").map(str::to_owned)
    };

    harness.store().lock();
    let err = harness
        .service
        .reveal(&target, RevealPurpose::Show, &user())
        .unwrap_err();
    assert_eq!(err, SecretError::Store(SecretStoreError::Locked));
    assert_eq!(failed_code().as_deref(), Some("secret.store_locked"));
    harness.store().unlock();

    harness.store().deny(&target);
    let err = harness
        .service
        .reveal(&target, RevealPurpose::Show, &user())
        .unwrap_err();
    assert_eq!(
        err,
        SecretError::Store(SecretStoreError::AccessDenied { by_user: true })
    );
    assert_eq!(failed_code().as_deref(), Some("secret.store_access_denied"));

    let unknown = reference("local:not/registered");
    let err = harness
        .service
        .reveal(&unknown, RevealPurpose::Show, &user())
        .unwrap_err();
    assert_eq!(err, SecretError::NotFound { reference: unknown });
    assert_eq!(failed_code().as_deref(), Some("secret.not_found"));
    assert_eq!(harness.audit().last().unwrap().resource, None);
}

#[test]
fn a_reveal_whose_audit_row_cannot_be_written_fails_and_drops_the_value() {
    let harness = Harness::new();
    let credential = FixtureCredential::new();
    let target = reference("local:erp/admin");
    harness.set(&target, credential.value());
    let rows = harness.audit().len();

    harness.registry.fail_next_commit(RegistryError::NoSpace);
    let err = harness
        .service
        .reveal(&target, RevealPurpose::Show, &user())
        .unwrap_err();

    assert_eq!(err, SecretError::Registry(RegistryError::NoSpace));
    credential.assert_absent(&format!("{err:?} {err}"), "the reveal error");
    assert_eq!(harness.audit().len(), rows);
}

#[test]
fn a_used_reference_is_not_deleted_and_every_use_is_named() {
    let harness = Harness::new();
    let target = reference("local:shared/admin");
    harness.set(&target, "the-value-of-the-test");
    let instance = harness.instance(false);
    let environment = harness.environment();
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &target)],
    );
    harness.uses(
        SecretUser::Environment(environment),
        &[("db_password", &target)],
    );
    let deletes = harness.store().count(SecretOp::Delete);

    let err = harness.service.delete(&target, None, &user()).unwrap_err();

    assert_eq!(
        err,
        SecretError::Conflict {
            reference: target.clone(),
            reason: ConflictReason::InUse(vec![
                SecretUse::new(
                    target.clone(),
                    SecretUser::Environment(environment),
                    field_name("db_password")
                ),
                SecretUse::new(
                    target.clone(),
                    SecretUser::Instance(instance),
                    field_name("adminPassword")
                ),
            ]),
        }
    );
    assert_eq!(err.code().as_str(), "secret.conflict");
    assert_eq!(harness.store().count(SecretOp::Delete), deletes);
    assert!(harness.store().contains(&target));
    let audit = harness.audit();
    let row = audit.last().unwrap();
    assert_eq!(row.action.as_str(), "secret.delete");
    assert_eq!(row.result, AuditResult::Failed);
    assert_eq!(row.safe_details.get("errorCode"), Some("secret.conflict"));
}

#[test]
fn a_store_that_fails_to_delete_gets_the_row_back_with_its_revision_up_by_one() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "when the store fails to delete the item after the row was deleted, the row \
                    comes back exactly as it was with its revision up by 1, the store error is \
                    returned, and a failed secret.delete audit row is written",
    );
    let harness = Harness::new();
    let target = reference("local:erp/admin");
    harness.set(&target, "the-value-of-the-test");
    let before = harness.view(&target).unwrap();
    harness
        .store()
        .fail_next(SecretStoreError::Backend { code: -25291 });

    let err = harness.service.delete(&target, None, &user()).unwrap_err();

    assert_eq!(
        err,
        SecretError::Store(SecretStoreError::Backend { code: -25291 })
    );
    let after = harness.view(&target).unwrap();
    let mut expected = before.record.clone();
    expected.revision = revision(2);
    assert_eq!(after.record, expected);
    assert!(harness.store().contains(&target));
    let audit = harness.audit();
    let row = audit.last().unwrap();
    assert_eq!(row.result, AuditResult::Failed);
    assert_eq!(
        row.safe_details.get("errorCode"),
        Some("secret.store_backend")
    );
    let last = harness.changes.all().pop().unwrap();
    assert_eq!(
        (last.status, last.revision),
        (ChangeStatus::Present, revision(2))
    );
    ev.pass();
}

#[test]
fn an_item_already_gone_or_a_locked_store_still_deletes_the_row() {
    let harness = Harness::new();
    let gone = reference("local:erp/gone");
    let locked = reference("local:erp/locked");
    harness.set(&gone, "the-value-of-gone");
    harness.set(&locked, "the-value-of-locked");
    harness.store().remove(&gone);

    harness
        .service
        .delete(&gone, Some(revision(1)), &user())
        .unwrap();
    harness.store().lock();
    harness
        .service
        .delete(&locked, None, &SecretCaller::background())
        .unwrap();

    assert!(harness.view(&gone).is_none() && harness.view(&locked).is_none());
    assert!(!harness.store().contains(&locked));
    let audit = harness.audit();
    let deletes: Vec<_> = audit
        .iter()
        .filter(|row| row.action.as_str() == "secret.delete")
        .map(|row| (row.result, row.actor, row.resource))
        .collect();
    assert_eq!(
        deletes,
        [
            (AuditResult::Succeeded, AuditActor::User, None),
            (AuditResult::Succeeded, AuditActor::Daemon, None),
        ]
    );
    let last = harness.changes.all().pop().unwrap();
    assert_eq!(last.status, ChangeStatus::Deleted);
    assert_eq!(SecretCaller::background().interaction, Interaction::Never);
}

#[test]
fn a_delete_at_a_stale_revision_touches_nothing() {
    let harness = Harness::new();
    let target = reference("local:erp/admin");
    harness.set(&target, "the-value-of-the-test");
    let calls = harness.store().calls().len();

    let err = harness
        .service
        .delete(&target, Some(revision(4)), &user())
        .unwrap_err();

    assert_eq!(
        err,
        SecretError::RevisionMismatch {
            reference: target.clone(),
            expected: revision(4),
            actual: revision(1),
        }
    );
    assert_eq!(harness.store().calls().len(), calls);
    assert!(harness.view(&target).is_some());
    let missing = harness
        .service
        .delete(&reference("local:not/registered"), None, &user())
        .unwrap_err();
    assert!(
        matches!(missing, SecretError::NotFound { .. }),
        "{missing:?}"
    );
}
