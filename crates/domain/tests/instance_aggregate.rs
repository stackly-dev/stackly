//! The `Instance` aggregate rules of spec 0004 AC-16: how an instance is
//! born, the init guard, rebind limits, and the revision counter, plus the
//! needs secret blocker observation of spec 0011 AC-11.

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    DesiredState, FailureCode, Health, HealthState, Ignored, Instance, InstanceBlocker, InstanceId,
    InstanceLifecycle, InvariantDetail, LifecycleEvent, LineCompatibility, NeedsSecretItem,
    NeedsSecretReason, NewInstance, ObservationOutcome, OperationId, ProviderRevisionKey,
    Readiness, ReadinessState, ReasonCode, ResourceRevision, Rule, RuntimeId, SecretRef,
    StartupPolicy,
};

/// Returns `2026-01-01T00:00:00Z` plus `seconds`.
fn at(seconds: i64) -> Timestamp {
    let start: Timestamp = "2026-01-01T00:00:00Z"
        .parse()
        .expect("the start time is valid RFC 3339");
    start + SignedDuration::from_secs(seconds)
}

fn instance() -> Instance {
    Instance::new(NewInstance {
        id: InstanceId::generate(),
        name: "orders-db".parse().expect("orders-db is an instance name"),
        display_name: Some("Orders".parse().expect("Orders is a display name")),
        provider_revision: ProviderRevisionKey::new(
            "official/postgresql"
                .parse()
                .expect("official/postgresql is a provider name"),
            "1".parse().expect("1 is a revision label"),
            "ab".repeat(32).parse().expect("64 hex characters"),
        ),
        software: "postgresql".parse().expect("postgresql is a software name"),
        runtime_id: RuntimeId::generate(),
        data_line: "17".parse().expect("17 is a runtime line"),
        data_version: "17.6".parse().expect("17.6 is an exact version"),
        endpoints: Vec::new(),
        startup: StartupPolicy::Manual,
        created_at: at(0),
        created_by: OperationId::generate(),
    })
}

fn apply(instance: &mut Instance, events: Vec<LifecycleEvent>) {
    for (seconds, event) in (1..).zip(events) {
        instance
            .apply_lifecycle(event, at(seconds))
            .expect("the test path follows the lifecycle table");
    }
}

#[test]
fn a_new_instance_is_born_uninitialized_and_desired_stopped() {
    let instance = instance();

    assert_eq!(instance.lifecycle(), InstanceLifecycle::Uninitialized);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert_eq!(instance.data_initialized_at(), None);
    assert_eq!(instance.revision(), ResourceRevision::INITIAL);
    assert_eq!(instance.readiness().reason(), &ReasonCode::NEVER_OBSERVED);
    assert_eq!(instance.health().observed_at(), at(0));
    assert_eq!(instance.created_at(), at(0));
}

#[test]
fn an_instance_whose_init_failed_can_never_start() {
    let mut instance = instance();
    apply(
        &mut instance,
        vec![
            LifecycleEvent::InitBegin,
            LifecycleEvent::InitFailed {
                reason: FailureCode::INTERRUPTED,
            },
        ],
    );

    let err = instance
        .apply_lifecycle(LifecycleEvent::StartBegin, at(10))
        .unwrap_err();

    assert_eq!(err.rule(), Some(Rule::Br008));
    assert_eq!(instance.last_failure(), Some(&FailureCode::INTERRUPTED));
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Failed);
}

#[test]
fn rebind_while_running_is_refused() {
    let mut instance = instance();
    apply(
        &mut instance,
        vec![
            LifecycleEvent::InitBegin,
            LifecycleEvent::InitSucceeded,
            LifecycleEvent::StartBegin,
            LifecycleEvent::Spawned,
        ],
    );

    let err = instance
        .rebind(
            RuntimeId::generate(),
            &"postgresql".parse().unwrap(),
            &"17".parse().unwrap(),
            &LineCompatibility::default(),
        )
        .unwrap_err();

    assert_eq!(
        err.detail(),
        &InvariantDetail::RebindRequiresStopped {
            lifecycle: InstanceLifecycle::Running
        }
    );
}

#[test]
fn observations_keep_the_revision_and_every_other_mutation_adds_exactly_one() {
    let mut instance = instance();
    apply(
        &mut instance,
        vec![
            LifecycleEvent::InitBegin,
            LifecycleEvent::InitSucceeded,
            LifecycleEvent::StartBegin,
            LifecycleEvent::Spawned,
        ],
    );
    let revision = instance.revision();
    assert_eq!(revision.get(), 5);

    for seconds in 10..110 {
        let reason: ReasonCode = "probe_ok".parse().unwrap();
        if seconds % 2 == 0 {
            let readiness = Readiness::new(ReadinessState::Ready, at(seconds), reason, None);
            instance.observe_readiness(readiness).unwrap();
        } else {
            let health = Health::new(HealthState::Healthy, at(seconds), reason, None);
            instance.observe_health(health).unwrap();
        }
    }
    assert_eq!(instance.revision(), revision);

    instance.rename("sales-db".parse().unwrap());
    assert_eq!(instance.revision(), revision.next());
    instance.set_desired(DesiredState::Running);
    assert_eq!(instance.revision().get(), revision.get() + 2);
}

fn needs_secret(seconds: i64, reason: NeedsSecretReason) -> InstanceBlocker {
    let reference = SecretRef::parse("local:erp/postgres/admin").expect("a valid reference");
    InstanceBlocker::needs_secret([NeedsSecretItem::new(reference, reason, true)], at(seconds))
}

#[test]
fn a_needs_secret_blocker_changes_no_lifecycle_desired_state_or_revision() {
    let mut instance = instance();
    apply(
        &mut instance,
        vec![LifecycleEvent::InitBegin, LifecycleEvent::InitSucceeded],
    );
    let revision = instance.revision();
    assert_eq!(instance.blocker(), None);

    let blocker = needs_secret(10, NeedsSecretReason::StoreLocked);
    let outcome = instance.observe_blocker(blocker.clone());

    assert_eq!(outcome, ObservationOutcome::Applied);
    assert_eq!(instance.blocker(), Some(&blocker));
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert_eq!(instance.revision(), revision);

    assert_eq!(instance.clear_blocker(), Some(blocker));
    assert_eq!(instance.blocker(), None);
    assert_eq!(instance.revision(), revision);
}

#[test]
fn an_older_blocker_observation_is_ignored_and_a_newer_one_replaces_it() {
    let mut instance = instance();
    let newer = needs_secret(20, NeedsSecretReason::Missing);
    instance.observe_blocker(newer.clone());

    let stale = instance.observe_blocker(needs_secret(10, NeedsSecretReason::StoreLocked));
    assert_eq!(stale, ObservationOutcome::Ignored(Ignored::Stale));
    assert_eq!(instance.blocker(), Some(&newer));

    let latest = needs_secret(30, NeedsSecretReason::AccessDenied);
    assert_eq!(
        instance.observe_blocker(latest.clone()),
        ObservationOutcome::Applied
    );
    assert_eq!(instance.blocker(), Some(&latest));
}
