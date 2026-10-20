//! What a new value changes for each use of its reference (spec 0011 AC-5,
//! `effects`): the apply class of the field and the data of the instance
//! decide between `stored_only`, `next_reload`, and `next_start`, and only
//! `stored_only` asks for a confirmation.

use stackly_core::provider::ApplyClass;
use stackly_core::secret::{EffectKind, PutRequest, PutTarget, SecretInput};
use stackly_domain::{SecretOwner, SecretUser};
use stackly_platform::WriteMode;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, field_name, instance_reference, value};

/// One instance with a generated secret, a rule for its field (or none),
/// and its data initialized or not; returns the effect of a confirmed
/// replace.
fn effect_of_a_replace(initialized: bool, apply: Option<ApplyClass>, has_rule: bool) -> EffectKind {
    let harness = Harness::new();
    let instance = harness.instance(initialized);
    let target = instance_reference(instance, "admin-password");
    harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: target.clone(),
                owner: SecretOwner::Instance(instance),
                purpose: "admin-password".parse().expect("a purpose segment"),
                field: None,
            },
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .expect("the generated secret is stored");
    harness.uses(
        SecretUser::Instance(instance),
        &[("adminPassword", &target)],
    );
    if has_rule {
        harness.rule(instance, "adminPassword", 8, Some(64), apply);
    }

    let mut request = PutRequest::new(
        PutTarget::Existing(target),
        SecretInput::Value(value("a-new-admin-password")),
        WriteMode::Replace,
    );
    request.confirm_stored_only = true;
    let outcome = harness.put(request).expect("a confirmed replace is stored");

    assert_eq!(outcome.effects.len(), 1, "{:?}", outcome.effects);
    assert_eq!(outcome.effects[0].field, field_name("adminPassword"));
    outcome.effects[0].effect
}

#[test]
fn a_reloadable_field_reports_next_reload_and_a_restart_field_next_start() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "AC-5: a replace on a field that a reload applies reports next_reload, one that a restart applies reports next_start, and the data being initialized changes neither",
    );

    for initialized in [false, true] {
        assert_eq!(
            effect_of_a_replace(initialized, Some(ApplyClass::Reload), true),
            EffectKind::NextReload,
            "reload, initialized {initialized}"
        );
        assert_eq!(
            effect_of_a_replace(initialized, Some(ApplyClass::Restart), true),
            EffectKind::NextStart,
            "restart, initialized {initialized}"
        );
    }
    ev.pass();
}

#[test]
fn a_field_fixed_after_init_reports_stored_only_only_once_the_data_is_initialized() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "AC-5: a field that is immutable after init reports next_start while the instance data is not initialized and stored_only once it is",
    );

    assert_eq!(
        effect_of_a_replace(false, Some(ApplyClass::ImmutableAfterInit), true),
        EffectKind::NextStart
    );
    assert_eq!(
        effect_of_a_replace(true, Some(ApplyClass::ImmutableAfterInit), true),
        EffectKind::StoredOnly
    );
    ev.pass();
}

#[test]
fn a_field_without_known_rules_is_stored_only_on_initialized_data_and_next_start_before() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "AC-5: with no rule known for the field, nothing proves the value inside the data follows, so initialized data reports stored_only and uninitialized data next_start",
    );

    assert_eq!(
        effect_of_a_replace(false, None, false),
        EffectKind::NextStart
    );
    assert_eq!(
        effect_of_a_replace(true, None, false),
        EffectKind::StoredOnly
    );
    ev.pass();
}

#[test]
fn a_replace_that_is_not_stored_only_needs_no_confirmation() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "AC-5: a replace with a next_reload effect is written without confirmStoredOnly",
    );
    let harness = Harness::new();
    let instance = harness.instance(true);
    let target = instance_reference(instance, "api-token");
    harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: target.clone(),
                owner: SecretOwner::Instance(instance),
                purpose: "api-token".parse().expect("a purpose segment"),
                field: None,
            },
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .expect("the generated secret is stored");
    harness.uses(SecretUser::Instance(instance), &[("apiToken", &target)]);
    harness.rule(instance, "apiToken", 8, Some(64), Some(ApplyClass::Reload));

    let outcome = harness
        .put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Value(value("a-new-api-token-value")),
            WriteMode::Replace,
        ))
        .expect("a reloadable field needs no confirmation");

    assert_eq!(outcome.effects[0].effect, EffectKind::NextReload);
    assert_eq!(
        harness.store().stored_value(&target).as_deref(),
        Some("a-new-api-token-value")
    );
    ev.pass();
}

#[test]
fn an_environment_use_reports_next_start() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "AC-5: an environment has no field rules, so its use reports next_start and needs no confirmation",
    );
    let harness = Harness::new();
    let environment = harness.environment();
    let target = crate::common::reference("local:shared/db-password");
    harness.set(&target, "the-first-db-password");
    harness.uses(
        SecretUser::Environment(environment),
        &[("db_password", &target)],
    );

    let outcome = harness
        .put(PutRequest::new(
            PutTarget::Existing(target),
            SecretInput::Value(value("the-second-db-password")),
            WriteMode::Replace,
        ))
        .expect("an environment use needs no confirmation");

    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].effect, EffectKind::NextStart);
    ev.pass();
}
