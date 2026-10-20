//! A bind with a field, store errors that carry their reference, and
//! `classify_create_failure` (spec 0011 AC-23).

use stackly_core::secret::{
    FieldContext, InvalidValueReason, PutRequest, PutTarget, SecretError, SecretFieldRule,
    SecretInput, classify_create_failure,
};
use stackly_domain::{
    AuditResult, InstanceId, NeedsSecretItem, NeedsSecretReason, Os, SecretOwner, SecretRef,
};
use stackly_platform::{SecretStoreError, WriteMode};
use stackly_testkit::SecretOp;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

use crate::common::{Harness, ScriptedStore, field_name, instance_reference, reference, user};

/// A password field of 8 to 64 characters that refuses control characters.
fn printable() -> SecretFieldRule {
    SecretFieldRule::new(8, Some(64), None).with_printable_only(true)
}

/// A request that registers the reference of `purpose` for `instance`,
/// for a field with `rule` when one is given.
fn new_target(
    instance: InstanceId,
    purpose: &str,
    rule: Option<SecretFieldRule>,
    input: SecretInput,
) -> PutRequest {
    PutRequest::new(
        PutTarget::New {
            reference: instance_reference(instance, purpose),
            owner: SecretOwner::Instance(instance),
            purpose: purpose.parse().expect("a valid test purpose"),
            field: rule.map(|rule| FieldContext::new(field_name("adminPassword"), rule)),
        },
        input,
        WriteMode::Create,
    )
}

/// The single item of a needs secret error.
fn needs_item(err: &SecretError) -> NeedsSecretItem {
    match err {
        SecretError::NeedsSecret { needs } => {
            assert_eq!(needs.references.len(), 1, "{needs:?}");
            needs.references[0].clone()
        }
        other => panic!("expected needs secret, got {other:?}"),
    }
}

fn store_error(reference: &SecretRef, error: SecretStoreError) -> SecretError {
    SecretError::Store {
        reference: reference.clone(),
        error,
    }
}

#[test]
fn a_bind_with_a_field_reads_the_bound_value_once_learns_it_and_writes_nothing() {
    let harness = Harness::new();
    let credential = FixtureCredential::new();
    let shared = reference("local:shared/postgres/admin");
    harness.set(&shared, credential.value());
    let instance = harness.instance(false);
    // A restarted daemon has not learned the value yet.
    let restarted = harness.restarted();
    let (reads, writes, audit) = (
        harness.store().count(SecretOp::Read),
        harness.store().count(SecretOp::Write),
        harness.audit().len(),
    );

    let bound = restarted
        .service
        .put(
            new_target(
                instance,
                "admin-password",
                Some(printable()),
                SecretInput::Bind(shared.clone()),
            ),
            &user(),
        )
        .unwrap();

    assert_eq!(bound.reference.record.reference, shared);
    assert_eq!(harness.store().count(SecretOp::Read), reads + 1);
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    assert_eq!(
        harness.store().calls().last().map(|call| &call.reference),
        Some(&shared)
    );
    assert_eq!(harness.audit().len(), audit, "no reveal or set is audited");
    assert_eq!(
        restarted
            .redactor
            .redact(&format!("login with {} failed", credential.value())),
        "login with **** failed"
    );
    credential.assert_absent(&format!("{bound:?}"), "the bind outcome");
    assert!(
        harness
            .view(&instance_reference(instance, "admin-password"))
            .is_none()
    );
}

#[test]
fn a_bound_value_that_breaks_the_field_fails_with_a_failed_audit_row_and_no_write() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let multiline = reference("local:shared/multiline");
    let short = reference("local:shared/short");
    harness.set(&multiline, "line-one\nline-two");
    harness.set(&short, "a\nb");
    let writes = harness.store().count(SecretOp::Write);

    for (bound, reason) in [
        (&multiline, InvalidValueReason::ControlCharacters),
        (&short, InvalidValueReason::Length),
    ] {
        let err = harness
            .put(new_target(
                instance,
                "admin-password",
                Some(printable()),
                SecretInput::Bind(bound.clone()),
            ))
            .unwrap_err();

        assert_eq!(
            err,
            SecretError::InvalidValue {
                field: field_name("adminPassword"),
                min_length: 8,
                max_length: Some(64),
                reason,
            }
        );
        let row = harness.audit().last().unwrap().clone();
        assert_eq!(row.action.as_str(), "secret.set");
        assert_eq!(row.result, AuditResult::Failed);
        assert_eq!(row.safe_details.get("reference"), Some(bound.as_str()));
        assert_eq!(
            row.safe_details.get("errorCode"),
            Some("secret.invalid_value")
        );
    }
    assert_eq!(harness.store().count(SecretOp::Write), writes);
}

#[test]
fn a_bind_without_a_field_never_touches_the_store() {
    let harness = Harness::new();
    let shared = reference("local:shared/token");
    harness.set(&shared, "line-one\nline-two");
    let instance = harness.instance(false);
    let calls = harness.store().calls().len();
    harness.store().lock();

    let bound = harness
        .put(new_target(
            instance,
            "token",
            None,
            SecretInput::Bind(shared.clone()),
        ))
        .unwrap();

    assert_eq!(bound.reference.record.reference, shared);
    assert_eq!(harness.store().calls().len(), calls);
}

#[test]
fn a_bound_source_the_store_cannot_read_fails_on_the_source_and_classifies_as_needs_secret() {
    let ev = evidence!(
        covers = ["AC-062"],
        kind = Kind::Fixture,
        expected = "a create put that binds a registered reference on a locked fake store \
                    fails with a store error naming the bound reference, which \
                    classify_create_failure maps to one needs secret item store_locked, and a \
                    bound reference the store lost maps to the item missing; neither writes",
    );
    let harness = Harness::new();
    let shared = reference("local:shared/postgres/admin");
    harness.set(&shared, "the-shared-admin-password");
    let instance = harness.instance(false);
    let target = instance_reference(instance, "admin-password");
    let writes = harness.store().count(SecretOp::Write);
    let bind = || {
        harness.put(new_target(
            instance,
            "admin-password",
            Some(printable()),
            SecretInput::Bind(shared.clone()),
        ))
    };

    harness.store().lock();
    let locked = bind().unwrap_err();
    harness.store().unlock();
    harness.store().remove(&shared);
    let missing = bind().unwrap_err();

    assert_eq!(locked, store_error(&shared, SecretStoreError::Locked));
    assert_eq!(missing, store_error(&shared, SecretStoreError::NotFound));
    assert_eq!(missing.code().as_str(), "secret.not_found");
    let audit = harness.audit();
    let codes: Vec<_> = audit[audit.len() - 2..]
        .iter()
        .map(|row| row.safe_details.get("errorCode").map(str::to_owned))
        .collect();
    assert_eq!(
        codes,
        [
            Some("secret.store_locked".to_owned()),
            Some("secret.not_found".to_owned())
        ]
    );
    for (err, reason) in [
        (locked, NeedsSecretReason::StoreLocked),
        (missing, NeedsSecretReason::Missing),
    ] {
        let classified = classify_create_failure(err, &target);
        assert_eq!(classified.code().as_str(), "secret.needs_secret");
        assert_eq!(
            needs_item(&classified),
            NeedsSecretItem::new(shared.clone(), reason, false)
        );
    }
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    ev.pass();
}

#[test]
fn a_copy_source_fails_as_needs_secret_and_a_target_write_as_its_store_code() {
    let (harness, store) = ScriptedStore::harness();
    let source = reference("local:shared/postgres/admin");
    harness.set(&source, "the-shared-admin-password");
    let instance = harness.instance(false);
    let copied = instance_reference(instance, "copied");
    *store.fail_reads_of.lock().unwrap() = Some((source.clone(), SecretStoreError::Locked));

    let from_source = harness
        .put(new_target(
            instance,
            "copied",
            None,
            SecretInput::CopyFrom(source.clone()),
        ))
        .unwrap_err();

    assert_eq!(from_source, store_error(&source, SecretStoreError::Locked));
    assert_eq!(
        needs_item(&classify_create_failure(from_source, &copied)),
        NeedsSecretItem::new(source, NeedsSecretReason::StoreLocked, false)
    );

    let generated = instance_reference(instance, "generated");
    harness.store().lock();
    let on_target = harness
        .put(new_target(
            instance,
            "generated",
            None,
            SecretInput::Generate,
        ))
        .unwrap_err();

    assert_eq!(on_target, store_error(&generated, SecretStoreError::Locked));
    let classified = classify_create_failure(on_target.clone(), &generated);
    assert_eq!(classified, on_target);
    assert_eq!(classified.code().as_str(), "secret.store_locked");
    assert!(harness.view(&generated).is_none());
}

#[test]
fn an_unsupported_store_is_store_unsupported_for_a_source_and_for_a_target() {
    let harness = Harness::new();
    let source = reference("local:shared/postgres/admin");
    harness.set(&source, "the-shared-admin-password");
    let instance = harness.instance(false);
    harness.store().unsupported();

    for (purpose, input) in [
        ("copied", SecretInput::CopyFrom(source.clone())),
        ("generated", SecretInput::Generate),
    ] {
        let target = instance_reference(instance, purpose);
        let err = harness
            .put(new_target(instance, purpose, None, input))
            .unwrap_err();

        assert!(
            matches!(
                err,
                SecretError::Store {
                    error: SecretStoreError::NotSupported { .. },
                    ..
                }
            ),
            "{err:?}"
        );
        let classified = classify_create_failure(err.clone(), &target);
        assert_eq!(classified, err);
        assert_eq!(classified.code().as_str(), "secret.store_unsupported");
    }
}

#[test]
fn classify_create_failure_follows_every_row_of_the_table() {
    let target = reference("local:instance/0199f000-0000-7000-8000-000000000040/admin-password");
    let source = reference("local:shared/postgres/admin");
    let unsupported = SecretStoreError::NotSupported { os: Os::Windows };

    let to_items = [
        (SecretStoreError::Locked, NeedsSecretReason::StoreLocked),
        (
            SecretStoreError::AccessDenied { by_user: true },
            NeedsSecretReason::AccessDenied,
        ),
        (
            SecretStoreError::AccessDenied { by_user: false },
            NeedsSecretReason::AccessDenied,
        ),
        (SecretStoreError::Busy, NeedsSecretReason::StoreBusy),
        (SecretStoreError::NotFound, NeedsSecretReason::Missing),
    ];
    for (error, reason) in to_items {
        let classified = classify_create_failure(store_error(&source, error), &target);
        assert_eq!(classified.code().as_str(), "secret.needs_secret", "{error}");
        assert_eq!(
            needs_item(&classified),
            NeedsSecretItem::new(source.clone(), reason, false)
        );
    }

    let kept = [
        (&source, unsupported, "secret.store_unsupported"),
        (
            &source,
            SecretStoreError::OutcomeUnknown,
            "secret.store_outcome_unknown",
        ),
        (
            &source,
            SecretStoreError::Backend { code: -26276 },
            "secret.store_backend",
        ),
        (&target, SecretStoreError::Locked, "secret.store_locked"),
        (
            &target,
            SecretStoreError::AccessDenied { by_user: true },
            "secret.store_access_denied",
        ),
        (&target, SecretStoreError::Busy, "secret.store_busy"),
        (&target, unsupported, "secret.store_unsupported"),
        (
            &target,
            SecretStoreError::OutcomeUnknown,
            "secret.store_outcome_unknown",
        ),
        (
            &target,
            SecretStoreError::Backend { code: -25291 },
            "secret.store_backend",
        ),
    ];
    for (reference, error, code) in kept {
        let err = store_error(reference, error);
        let classified = classify_create_failure(err.clone(), &target);
        assert_eq!(classified, err);
        assert_eq!(classified.code().as_str(), code);
    }

    for other in [
        SecretError::NotFound {
            reference: source.clone(),
        },
        SecretError::InvalidValue {
            field: field_name("adminPassword"),
            min_length: 8,
            max_length: Some(64),
            reason: InvalidValueReason::ControlCharacters,
        },
    ] {
        assert_eq!(classify_create_failure(other.clone(), &target), other);
    }
    assert_eq!(
        classify_create_failure(
            SecretError::NotFound {
                reference: source.clone()
            },
            &target
        )
        .code()
        .as_str(),
        "secret.not_found",
        "a source that is not registered stays not found"
    );
    // The store error message names the reference, never a value.
    let message = store_error(&source, SecretStoreError::Locked).to_string();
    assert_eq!(
        message,
        "the secret store is locked (local:shared/postgres/admin)"
    );
}
