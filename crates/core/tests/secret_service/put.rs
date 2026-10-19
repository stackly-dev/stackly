//! `put`, the one use case that stores a value (spec 0011 AC-5, AC-6, and
//! the revision rules of AC-8).

use stackly_core::provider::ApplyClass;
use stackly_core::secret::{
    ChangeStatus, ConflictReason, EffectKind, FieldContext, PutRequest, PutTarget, PutWarning,
    SecretError, SecretFieldRule, SecretInput,
};
use stackly_domain::{
    AuditActor, AuditResult, RegistryError, ResourceRevision, SecretOrigin, SecretOwner,
    SecretPurpose, SecretStatus, SecretUser,
};
use stackly_platform::{SecretStoreError, WriteMode};
use stackly_testkit::SecretOp;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

use crate::common::{
    Harness, ScriptedStore, field_name, instance_reference, reference, user, value,
};

fn revision(number: u64) -> ResourceRevision {
    ResourceRevision::new(number).expect("a test revision is at least 1")
}

#[test]
fn secret_set_registers_a_user_reference_with_one_audit_row_and_no_value_anywhere() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "put on an unregistered user reference stores the value in the store, \
                    registers the row at revision 1 owned by the user with status present and \
                    origin entered, writes one secret.set audit row, reports one change, and \
                    neither the outcome, the row, the audit row, nor the change holds the value",
    );
    let harness = Harness::new();
    let credential = FixtureCredential::new();
    let target = reference("local:erp/postgres/admin");

    let outcome = harness
        .put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Value(value(credential.value())),
            WriteMode::Create,
        ))
        .unwrap();

    let record = &outcome.reference.record;
    assert_eq!(record.owner, SecretOwner::User);
    assert_eq!(record.purpose, SecretPurpose::user());
    assert_eq!(record.status, SecretStatus::Present);
    assert_eq!(record.value_origin, Some(SecretOrigin::Entered));
    assert_eq!(record.revision, revision(1));
    assert_eq!(
        record.platform_store_key.as_str(),
        "fake:stackly-secret/erp/postgres/admin"
    );
    assert!(outcome.warnings.is_empty() && outcome.effects.is_empty());
    assert_eq!(harness.view(&target).unwrap().record, *record);
    assert_eq!(
        harness.store().stored_value(&target).as_deref(),
        Some(credential.value())
    );
    let audit = harness.audit();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action.as_str(), "secret.set");
    assert_eq!(audit[0].actor, AuditActor::User);
    assert_eq!(audit[0].result, AuditResult::Succeeded);
    assert_eq!(
        audit[0].safe_details.get("reference"),
        Some(target.as_str())
    );
    assert_eq!(audit[0].safe_details.get("clientKind"), Some("cli"));
    assert_eq!(audit[0].safe_details.get("peerUid"), Some("501"));
    let changes = harness.changes.all();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].status, ChangeStatus::Present);
    assert_eq!(changes[0].revision, revision(1));
    for (what, text) in [
        ("the outcome", format!("{outcome:?}")),
        ("the row", format!("{:?}", harness.view(&target))),
        ("the audit rows", format!("{audit:?}")),
        ("the changes", format!("{changes:?}")),
    ] {
        credential.assert_absent(&text, what);
    }
    assert_eq!(
        harness
            .redactor
            .redact(&format!("x {} y", credential.value())),
        "x **** y"
    );
    ev.pass();
}

#[test]
fn a_replace_raises_the_revision_by_exactly_one_and_learns_the_old_value() {
    let harness = Harness::new();
    let target = reference("local:erp/admin");
    harness.set(&target, "the-first-value-of-the-test");

    let outcome = harness
        .put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Value(value("the-second-value-of-the-test")),
            WriteMode::Replace,
        ))
        .unwrap();

    assert_eq!(outcome.reference.record.revision, revision(2));
    assert_eq!(
        harness.store().stored_value(&target).as_deref(),
        Some("the-second-value-of-the-test")
    );
    assert_eq!(
        harness.redactor.redact("the-first-value-of-the-test"),
        "****"
    );
    assert_eq!(harness.audit().len(), 2);
}

/// Every input against every mode, on a missing and on an existing item.
#[test]
fn each_input_meets_each_mode_as_the_store_requires() {
    for mode in [WriteMode::Create, WriteMode::Replace, WriteMode::Upsert] {
        for present in [false, true] {
            for input in ["value", "generate", "copy_from"] {
                let harness = Harness::new();
                let source = reference("local:copy/source");
                harness.set(&source, "a-value-to-copy-from");
                let target = reference("local:copy/target");
                if present {
                    harness.store().insert(&target, "an-existing-value");
                }
                let input_value = match input {
                    "value" => SecretInput::Value(value("an-entered-value")),
                    "generate" => SecretInput::Generate,
                    _ => SecretInput::CopyFrom(source.clone()),
                };
                let result = harness.put(PutRequest::new(
                    PutTarget::Existing(target.clone()),
                    input_value,
                    mode,
                ));
                let case = format!("{input} with {mode:?} on a present={present} item");
                match (mode, present) {
                    (WriteMode::Create, true) => assert!(
                        matches!(
                            result,
                            Err(SecretError::Conflict {
                                reason: ConflictReason::AlreadyExists,
                                ..
                            })
                        ),
                        "{case}: {result:?}"
                    ),
                    (WriteMode::Replace, false) => assert!(
                        matches!(result, Err(SecretError::NotFound { .. })),
                        "{case}: {result:?}"
                    ),
                    _ => {
                        let outcome = result.unwrap_or_else(|err| panic!("{case}: {err}"));
                        let origin = outcome.reference.record.value_origin;
                        let stored = harness.store().stored_value(&target).unwrap();
                        match input {
                            "value" => {
                                assert_eq!(origin, Some(SecretOrigin::Entered), "{case}");
                                assert_eq!(stored, "an-entered-value", "{case}");
                            }
                            "generate" => {
                                assert_eq!(origin, Some(SecretOrigin::Generated), "{case}");
                                assert_eq!(stored.len(), 32, "{case}");
                                assert!(stored.bytes().all(|b| b.is_ascii_alphanumeric()));
                            }
                            _ => {
                                assert_eq!(origin, Some(SecretOrigin::Copied), "{case}");
                                assert_eq!(stored, "a-value-to-copy-from", "{case}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_wrong_expected_revision_fails_without_any_store_call() {
    let harness = Harness::new();
    let target = reference("local:erp/admin");
    harness.set(&target, "the-first-value-of-the-test");
    let calls = harness.store().calls().len();

    let mut request = PutRequest::new(
        PutTarget::Existing(target.clone()),
        SecretInput::Value(value("another-value-entirely")),
        WriteMode::Replace,
    );
    request.expected_revision = Some(revision(7));
    let err = harness.put(request).unwrap_err();

    assert_eq!(
        err,
        SecretError::RevisionMismatch {
            reference: target.clone(),
            expected: revision(7),
            actual: revision(1),
        }
    );
    assert_eq!(err.code().as_str(), "resource.revision_mismatch");
    assert_eq!(harness.store().calls().len(), calls);
    let audit = harness.audit();
    assert_eq!(audit.last().unwrap().result, AuditResult::Failed);
    assert_eq!(
        audit.last().unwrap().safe_details.get("errorCode"),
        Some("resource.revision_mismatch")
    );
}

#[test]
fn create_conflicts_when_the_registry_says_missing_but_the_store_holds_an_item() {
    let harness = Harness::new();
    let environment = harness.environment();
    let target = reference("local:shop/api-token");
    // Env up registers a reference of its own without a value.
    harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: target.clone(),
                owner: SecretOwner::Environment(environment),
                purpose: "api-token".parse().unwrap(),
                field: None,
            },
            SecretInput::Value(value("a-value-for-the-env")),
            WriteMode::Create,
        ))
        .unwrap();
    harness.service.probe_one(&target).unwrap();
    harness.store().remove(&target);
    harness.service.probe_one(&target).unwrap();
    assert_eq!(
        harness.view(&target).unwrap().record.status,
        SecretStatus::Missing
    );
    harness.store().insert(&target, "planted-behind-the-cache");
    let writes = harness.store().count(SecretOp::Write);

    let err = harness
        .put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap_err();

    assert!(
        matches!(
            err,
            SecretError::Conflict {
                reason: ConflictReason::AlreadyExists,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.code().as_str(), "secret.conflict");
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    assert_eq!(
        harness.store().stored_value(&target).as_deref(),
        Some("planted-behind-the-cache")
    );
}

#[test]
fn a_failed_registry_commit_puts_the_old_value_back_or_deletes_the_new_item() {
    let harness = Harness::new();
    let replaced = reference("local:erp/replaced");
    harness.set(&replaced, "the-value-before-the-replace");

    harness.registry.fail_next_commit(RegistryError::Busy);
    let err = harness
        .put(PutRequest::new(
            PutTarget::Existing(replaced.clone()),
            SecretInput::Value(value("the-value-that-did-not-commit")),
            WriteMode::Replace,
        ))
        .unwrap_err();
    assert_eq!(err, SecretError::Registry(RegistryError::Busy));
    assert_eq!(
        harness.store().stored_value(&replaced).as_deref(),
        Some("the-value-before-the-replace")
    );
    assert_eq!(
        harness.view(&replaced).unwrap().record.revision,
        revision(1)
    );

    let created = reference("local:erp/created");
    harness.registry.fail_next_commit(RegistryError::Busy);
    let err = harness
        .put(PutRequest::new(
            PutTarget::Existing(created.clone()),
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap_err();
    assert_eq!(err, SecretError::Registry(RegistryError::Busy));
    assert!(!harness.store().contains(&created));
    assert!(harness.view(&created).is_none());
    let audit = harness.audit();
    assert_eq!(audit.last().unwrap().result, AuditResult::Failed);
    assert_eq!(
        audit.last().unwrap().safe_details.get("errorCode"),
        Some("registry.busy")
    );
}

#[test]
fn a_write_back_that_fails_too_is_outcome_unknown() {
    let (harness, store) = ScriptedStore::harness();
    let target = reference("local:erp/admin");
    harness.set(&target, "the-value-before-the-replace");
    // The replace writes; the write back after the failed commit does not.
    *store.writes_ok.lock().unwrap() = Some(1);
    harness.registry.fail_next_commit(RegistryError::Busy);

    let err = harness
        .put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Value(value("the-value-that-did-not-commit")),
            WriteMode::Replace,
        ))
        .unwrap_err();

    assert_eq!(
        err,
        SecretError::WriteBackFailed {
            registry: RegistryError::Busy
        }
    );
    assert_eq!(err.code().as_str(), "secret.store_outcome_unknown");
}

#[test]
fn a_stored_only_replace_needs_a_confirmation_and_writes_nothing_without_it() {
    let harness = Harness::new();
    let instance = harness.instance(true);
    let target = instance_reference(instance, "admin-password");
    harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: target.clone(),
                owner: SecretOwner::Instance(instance),
                purpose: "admin-password".parse().unwrap(),
                field: None,
            },
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap();
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &target)],
    );
    harness.rule(
        instance,
        "adminPassword",
        8,
        Some(64),
        Some(ApplyClass::ImmutableAfterInit),
    );
    let before = harness.store().stored_value(&target);
    let writes = harness.store().count(SecretOp::Write);
    let replace = |confirm: bool| {
        let mut request = PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Value(value("a-new-admin-password")),
            WriteMode::Replace,
        );
        request.confirm_stored_only = confirm;
        harness.put(request)
    };

    let err = replace(false).unwrap_err();
    let SecretError::ConfirmationRequired { effects, .. } = &err else {
        panic!("expected a confirmation, got {err:?}");
    };
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].effect, EffectKind::StoredOnly);
    assert_eq!(effects[0].field, field_name("adminPassword"));
    assert_eq!(err.code().as_str(), "secret.confirmation_required");
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    assert_eq!(harness.store().stored_value(&target), before);

    let outcome = replace(true).unwrap();
    assert_eq!(outcome.effects[0].effect, EffectKind::StoredOnly);
    assert_eq!(
        harness.store().stored_value(&target).as_deref(),
        Some("a-new-admin-password")
    );
}

#[test]
fn bind_writes_no_value_and_copy_from_never_returns_one() {
    let harness = Harness::new();
    let credential = FixtureCredential::new();
    let shared = reference("local:shared/token");
    harness.set(&shared, credential.value());
    let writes = harness.store().count(SecretOp::Write);
    let instance = harness.instance(false);

    let bound = harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: instance_reference(instance, "token"),
                owner: SecretOwner::Instance(instance),
                purpose: "token".parse().unwrap(),
                field: None,
            },
            SecretInput::Bind(shared.clone()),
            WriteMode::Create,
        ))
        .unwrap();
    assert_eq!(bound.reference.record.reference, shared);
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    assert!(
        harness
            .view(&instance_reference(instance, "token"))
            .is_none()
    );
    let unknown = harness.put(PutRequest::new(
        PutTarget::New {
            reference: instance_reference(instance, "other"),
            owner: SecretOwner::Instance(instance),
            purpose: "other".parse().unwrap(),
            field: None,
        },
        SecretInput::Bind(reference("local:not/registered")),
        WriteMode::Create,
    ));
    assert!(
        matches!(unknown, Err(SecretError::NotFound { .. })),
        "{unknown:?}"
    );
    let existing = harness.put(PutRequest::new(
        PutTarget::Existing(reference("local:erp/x")),
        SecretInput::Bind(shared.clone()),
        WriteMode::Create,
    ));
    assert!(
        matches!(existing, Err(SecretError::Internal { .. })),
        "{existing:?}"
    );

    let copy = reference("local:copied/token");
    let copied = harness
        .put(PutRequest::new(
            PutTarget::Existing(copy.clone()),
            SecretInput::CopyFrom(shared.clone()),
            WriteMode::Create,
        ))
        .unwrap();
    credential.assert_absent(&format!("{copied:?}"), "the copy outcome");
    assert_eq!(
        harness.store().stored_value(&copy).as_deref(),
        Some(credential.value())
    );
    let missing = harness.put(PutRequest::new(
        PutTarget::Existing(reference("local:copied/again")),
        SecretInput::CopyFrom(reference("local:not/registered")),
        WriteMode::Create,
    ));
    assert_eq!(
        missing.unwrap_err(),
        SecretError::NotFound {
            reference: reference("local:not/registered")
        }
    );
}

#[test]
fn a_new_row_starts_at_revision_1_and_a_probe_keeps_it() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let target = instance_reference(instance, "admin-password");

    let outcome = harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: target.clone(),
                owner: SecretOwner::Instance(instance),
                purpose: "admin-password".parse().unwrap(),
                field: None,
            },
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap();
    assert_eq!(outcome.reference.record.revision, revision(1));
    assert_eq!(
        outcome.reference.record.owner,
        SecretOwner::Instance(instance)
    );

    let probed = harness.service.probe_one(&target).unwrap();
    assert_eq!(probed.record.revision, revision(1));
    harness.service.probe_all().unwrap();
    assert_eq!(harness.view(&target).unwrap().record.revision, revision(1));

    let again = harness.put(PutRequest::new(
        PutTarget::New {
            reference: target.clone(),
            owner: SecretOwner::Instance(instance),
            purpose: "admin-password".parse().unwrap(),
            field: None,
        },
        SecretInput::Generate,
        WriteMode::Create,
    ));
    assert!(
        matches!(
            again,
            Err(SecretError::Conflict {
                reason: ConflictReason::AlreadyExists,
                ..
            })
        ),
        "a new resource never takes a registered reference: {again:?}"
    );
}

#[test]
fn a_generated_value_is_32_characters_clamped_to_its_field_bounds() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "generate stores 32 characters of [A-Za-z0-9] without a field, clamps the \
                    length to minLength 40 and to maxLength 12, and clamps an existing \
                    reference to the bounds every using field shares",
    );
    let harness = Harness::new();
    let instance = harness.instance(false);
    let new = |purpose: &str, rule: Option<SecretFieldRule>| {
        let target = instance_reference(instance, purpose);
        harness
            .put(PutRequest::new(
                PutTarget::New {
                    reference: target.clone(),
                    owner: SecretOwner::Instance(instance),
                    purpose: purpose.parse().unwrap(),
                    field: rule.map(|rule| FieldContext::new(field_name("adminPassword"), rule)),
                },
                SecretInput::Generate,
                WriteMode::Create,
            ))
            .unwrap();
        harness.store().stored_value(&target).unwrap()
    };

    assert_eq!(new("plain", None).len(), 32);
    assert_eq!(
        new("long", Some(SecretFieldRule::new(40, Some(64), None))).len(),
        40
    );
    let short = new("short", Some(SecretFieldRule::new(0, Some(12), None)));
    assert_eq!(short.len(), 12);
    assert!(short.bytes().all(|b| b.is_ascii_alphanumeric()));

    let shared = reference("local:shared/password");
    harness.set(&shared, "the-first-shared-value");
    let other = harness.instance(false);
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &shared)],
    );
    harness.uses(SecretUser::Instance(other), &[("rootPassword", &shared)]);
    harness.rule(instance, "adminPassword", 20, Some(48), None);
    harness.rule(other, "rootPassword", 0, Some(24), None);
    harness
        .put(PutRequest::new(
            PutTarget::Existing(shared.clone()),
            SecretInput::Generate,
            WriteMode::Replace,
        ))
        .unwrap();
    assert_eq!(harness.store().stored_value(&shared).unwrap().len(), 24);
    ev.pass();
}

#[test]
fn a_value_outside_its_field_bounds_fails_before_the_store_without_naming_the_value() {
    let harness = Harness::new();
    let instance = harness.instance(false);
    let target = reference("local:erp/bounded");
    harness.set(&target, "a-long-enough-value");
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &target)],
    );
    harness.rule(instance, "adminPassword", 8, Some(10), None);
    let writes = harness.store().count(SecretOp::Write);
    let replace = |text: &str| {
        harness.put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Value(value(text)),
            WriteMode::Replace,
        ))
    };

    let err = replace("short").unwrap_err();
    assert_eq!(
        err,
        SecretError::InvalidValue {
            field: field_name("adminPassword"),
            min_length: 8,
            max_length: Some(10),
        }
    );
    assert_eq!(err.code().as_str(), "secret.invalid_value");
    assert!(!err.to_string().contains("short"));
    assert!(replace("much-too-long-for-the-field").is_err());
    assert_eq!(harness.store().count(SecretOp::Write), writes);
    // The bounds count characters: nine of them in 18 bytes fit 8 to 10.
    replace("ééééééééé").unwrap();
}

#[test]
fn unregistered_instance_and_reserved_references_are_refused_without_an_audit_row() {
    let harness = Harness::new();
    let instance = reference("local:instance/0199f000-0000-7000-8000-000000000030/admin");
    let reserved = reference("local:stackly/internal");

    let err = harness
        .put(PutRequest::new(
            PutTarget::Existing(instance.clone()),
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap_err();
    assert_eq!(
        err,
        SecretError::NotFound {
            reference: instance
        }
    );
    let err = harness
        .put(PutRequest::new(
            PutTarget::Existing(reserved.clone()),
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap_err();
    assert!(
        matches!(err, SecretError::InvalidReference { .. }),
        "{err:?}"
    );
    assert_eq!(err.code().as_str(), "secret.invalid_reference");
    assert!(harness.audit().is_empty());
    assert!(harness.store().calls().is_empty());
}

#[test]
fn a_value_under_8_bytes_is_stored_with_a_warning() {
    let harness = Harness::new();

    let outcome = harness.set(&reference("local:erp/pin"), "1234567");

    assert_eq!(outcome.warnings, [PutWarning::ShortValueNotScrubbed]);
    assert_eq!(
        PutWarning::ShortValueNotScrubbed.as_code(),
        "short_value_not_scrubbed"
    );
}

#[test]
fn a_store_failure_stops_the_put_and_is_audited() {
    let harness = Harness::new();
    let target = reference("local:erp/admin");
    harness.store().lock();

    let err = harness
        .put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap_err();

    assert_eq!(err, SecretError::Store(SecretStoreError::Locked));
    assert_eq!(err.code().as_str(), "secret.store_locked");
    assert!(harness.view(&target).is_none());
    let audit = harness.audit();
    assert_eq!(audit.len(), 1);
    assert_eq!(
        audit[0].safe_details.get("errorCode"),
        Some("secret.store_locked")
    );
    assert_eq!(
        user().interaction,
        stackly_platform::Interaction::AllowPrompt
    );
}
