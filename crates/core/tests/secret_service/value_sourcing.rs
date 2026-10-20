//! Where `put` and `resolve_for_start` take the facts they record: the
//! interaction of the caller, the store key, the owner and purpose, the
//! clock, the warning threshold, the audit actor, and the references an
//! instance reads (spec 0011 AC-4, AC-5, AC-8, AC-9, AC-11).

use jiff::SignedDuration;
use stackly_core::secret::{PutRequest, PutTarget, PutWarning, SecretCaller, SecretInput};
use stackly_domain::{
    AuditActor, PlatformStoreKey, SecretOwner, SecretOwnerKind, SecretPurpose, SecretReferenceId,
    SecretReferenceRecord, SecretStatus, SecretUser, WriteOp,
};
use stackly_platform::{Clock, Interaction, SecretStore, WriteMode};
use stackly_testkit::SecretOp;

use crate::common::{Harness, instance_reference, reference, user, value};

fn upsert(target: &stackly_domain::SecretRef, text: &str) -> PutRequest {
    PutRequest::new(
        PutTarget::Existing(target.clone()),
        SecretInput::Value(value(text)),
        WriteMode::Upsert,
    )
}

fn client(interactive: bool) -> SecretCaller {
    SecretCaller::client("cli".parse().expect("a client code"), 501, interactive)
}

/// The interaction of every call that reached the store with one.
fn interactions(harness: &Harness, op: SecretOp) -> Vec<Option<Interaction>> {
    harness
        .store()
        .calls()
        .into_iter()
        .filter(|call| call.op == op)
        .map(|call| call.interaction)
        .collect()
}

#[test]
fn a_non_interactive_client_reaches_the_store_with_never_and_an_interactive_one_may_prompt() {
    let harness = Harness::new();
    let quiet = reference("local:erp/quiet");
    let loud = reference("local:erp/loud");

    harness
        .service
        .put(upsert(&quiet, "a-quiet-value-of-the-test"), &client(false))
        .unwrap();
    harness
        .service
        .put(upsert(&loud, "a-loud-value-of-the-test"), &client(true))
        .unwrap();

    let writes: Vec<_> = harness
        .store()
        .calls()
        .into_iter()
        .filter(|call| call.op == SecretOp::Write)
        .map(|call| (call.reference, call.interaction))
        .collect();
    assert_eq!(
        writes,
        [
            (quiet, Some(Interaction::Never)),
            (loud, Some(Interaction::AllowPrompt)),
        ]
    );
}

#[test]
fn the_store_key_of_the_row_is_the_key_the_store_computes_after_a_put_and_after_a_probe() {
    let harness = Harness::new();
    let target = reference("local:erp/keyed");

    let outcome = harness
        .put(upsert(&target, "a-keyed-value-of-the-test"))
        .unwrap();

    let computed = harness.store().store_key(&target);
    assert_eq!(
        outcome.reference.record.platform_store_key.as_str(),
        computed
    );
    assert_eq!(
        harness
            .view(&target)
            .unwrap()
            .record
            .platform_store_key
            .as_str(),
        computed
    );

    // A row written by another store root holds another key; a probe
    // records the computed one.
    let moved = reference("local:erp/moved");
    harness.set(&moved, "a-moved-value-of-the-test");
    let mut record: SecretReferenceRecord = harness.view(&moved).unwrap().record;
    record.id = SecretReferenceId::generate();
    record.reference = reference("local:erp/other");
    record.platform_store_key =
        PlatformStoreKey::new("macos-keychain:file:other-root/stackly-secret/erp/other".to_owned())
            .unwrap();
    harness.commit(vec![WriteOp::InsertSecretReference(record)]);
    let other = reference("local:erp/other");
    harness.store().insert(&other, "an-other-value-of-the-test");

    let probed = harness.service.probe_one(&other).unwrap();

    assert_eq!(probed.record.status, SecretStatus::Present);
    assert_eq!(
        probed.record.platform_store_key.as_str(),
        harness.store().store_key(&other)
    );
    assert_eq!(
        harness.view(&other).unwrap().record.platform_store_key,
        probed.record.platform_store_key
    );
}

#[test]
fn a_user_reference_has_no_owner_id_and_a_new_one_takes_owner_and_purpose_from_its_caller() {
    let harness = Harness::new();
    let user_target = reference("local:erp/by-user");

    let record = harness
        .put(upsert(&user_target, "a-user-value-of-the-test"))
        .unwrap()
        .reference
        .record;
    assert_eq!(record.owner, SecretOwner::User);
    assert_eq!(record.owner.kind(), SecretOwnerKind::User);
    assert_eq!(record.owner.id(), None);
    assert_eq!(
        SecretOwner::from_parts(record.owner.kind(), None),
        Ok(record.owner)
    );
    assert_eq!(record.purpose, SecretPurpose::user());

    let instance = harness.instance(false);
    let new_target = instance_reference(instance, "replica-password");
    let record = harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: new_target.clone(),
                owner: SecretOwner::Instance(instance),
                purpose: "replica-password".parse().unwrap(),
                field: None,
            },
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap()
        .reference
        .record;
    assert_eq!(record.owner, SecretOwner::Instance(instance));
    assert_eq!(record.owner.kind(), SecretOwnerKind::Instance);
    assert_eq!(
        SecretOwner::from_parts(record.owner.kind(), record.owner.id()),
        Ok(record.owner)
    );
    assert!(record.owner.id().is_some());
    assert_eq!(record.purpose.as_str(), "replica-password");
    assert_eq!(harness.view(&new_target).unwrap().record, record);

    let environment = harness.environment();
    let env_target = reference("local:env/token");
    let record = harness
        .put(PutRequest::new(
            PutTarget::New {
                reference: env_target,
                owner: SecretOwner::Environment(environment),
                purpose: "env-token".parse().unwrap(),
                field: None,
            },
            SecretInput::Generate,
            WriteMode::Create,
        ))
        .unwrap()
        .reference
        .record;
    assert_eq!(record.owner, SecretOwner::Environment(environment));
    assert_eq!(record.purpose.as_str(), "env-token");
}

#[test]
fn the_recorded_times_are_the_clock_of_the_platform() {
    let harness = Harness::new();
    let target = reference("local:erp/timed");
    let first = harness.fake.clock().now();

    let record = harness
        .put(upsert(&target, "the-first-timed-value-of-the-test"))
        .unwrap()
        .reference
        .record;
    assert_eq!(record.value_updated_at, Some(first));
    assert_eq!(record.status_observed_at, Some(first));

    harness.fake.clock().advance(SignedDuration::from_secs(90));
    let second = harness.fake.clock().now();
    assert_ne!(first, second);
    let record = harness
        .put(PutRequest::new(
            PutTarget::Existing(target.clone()),
            SecretInput::Value(value("the-second-timed-value-of-the-test")),
            WriteMode::Replace,
        ))
        .unwrap()
        .reference
        .record;
    assert_eq!(record.value_updated_at, Some(second));
    assert_eq!(record.status_observed_at, Some(second));

    harness.fake.clock().advance(SignedDuration::from_secs(30));
    let third = harness.fake.clock().now();
    let probed = harness.service.probe_one(&target).unwrap().record;
    assert_eq!(probed.status_observed_at, Some(third));
    assert_eq!(
        probed.value_updated_at,
        Some(second),
        "a probe never moves the time of the value"
    );
}

#[test]
fn a_value_of_exactly_8_bytes_is_stored_without_a_warning() {
    let harness = Harness::new();

    let short = harness.set(&reference("local:erp/seven"), "1234567");
    let exact = harness.set(&reference("local:erp/eight"), "12345678");

    assert_eq!(short.warnings, [PutWarning::ShortValueNotScrubbed]);
    assert!(exact.warnings.is_empty(), "{:?}", exact.warnings);
    // Seven two-byte characters are 14 bytes: the threshold counts bytes.
    let wide = harness.set(&reference("local:erp/wide"), "ééééééé");
    assert!(wide.warnings.is_empty(), "{:?}", wide.warnings);
}

#[test]
fn the_audit_actor_is_the_user_for_a_client_and_the_daemon_for_a_background_trigger() {
    let harness = Harness::new();
    let by_client = reference("local:erp/by-client");
    let by_daemon = reference("local:erp/by-daemon");

    harness
        .service
        .put(upsert(&by_client, "a-client-value-of-the-test"), &user())
        .unwrap();
    harness
        .service
        .put(
            upsert(&by_daemon, "a-daemon-value-of-the-test"),
            &SecretCaller::background(),
        )
        .unwrap();

    let audit = harness.audit();
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].actor, AuditActor::User);
    assert_eq!(audit[0].safe_details.get("clientKind"), Some("cli"));
    assert_eq!(audit[0].safe_details.get("peerUid"), Some("501"));
    assert_eq!(audit[1].actor, AuditActor::Daemon);
    assert_eq!(
        audit[1].safe_details.get("reference"),
        Some(by_daemon.as_str())
    );
    assert_eq!(audit[1].safe_details.get("clientKind"), None);
    assert_eq!(audit[1].safe_details.get("peerUid"), None);
    assert!(interactions(&harness, SecretOp::Write).contains(&Some(Interaction::Never)));
}

#[test]
fn resolving_an_instance_reads_only_the_references_its_own_uses_name() {
    let harness = Harness::new();
    let mine = harness.instance(false);
    let other = harness.instance(false);
    let own = reference("local:mine/admin");
    let shared = reference("local:shared/token");
    let foreign = reference("local:other/admin");
    let unused = reference("local:unused/token");
    for (target, text) in [
        (&own, "the-own-value-of-the-test"),
        (&shared, "the-shared-value-of-the-test"),
        (&foreign, "the-foreign-value-of-the-test"),
        (&unused, "the-unused-value-of-the-test"),
    ] {
        harness.set(target, text);
    }
    harness.uses(
        SecretUser::Instance(mine),
        &[("adminPassword", &own), ("apiToken", &shared)],
    );
    harness.uses(
        SecretUser::Instance(other),
        &[("adminPassword", &foreign), ("apiToken", &shared)],
    );
    let before = harness.store().calls().len();

    let resolved = harness
        .service
        .resolve_for_start(mine, &SecretCaller::background())
        .unwrap();

    let read: Vec<_> = harness
        .store()
        .calls()
        .into_iter()
        .skip(before)
        .map(|call| (call.op, call.reference))
        .collect();
    assert_eq!(read, [(SecretOp::Read, own), (SecretOp::Read, shared)]);
    let fields: Vec<_> = resolved.fields().map(ToString::to_string).collect();
    assert_eq!(fields, ["adminPassword", "apiToken"]);
}
