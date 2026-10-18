//! The state machine tables of spec 0004, checked pair by pair: every
//! (state, event) pair, with every payload value, either gives the state the
//! table names or a `TransitionError` that names the machine, the state, and
//! the event. Every state also round trips through its stable code.

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    DesiredState, FailureCode, Health, HealthState, Ignored, InstallKind, Instance, InstanceId,
    InstanceLifecycle, LifecycleEvent, Machine, NewInstance, ObservationOutcome, OperationEvent,
    OperationFailure, OperationId, OperationState, Outcome, ProviderRevisionKey, Readiness,
    ReadinessState, ReasonCode, Rule, RuntimeEvent, RuntimeId, RuntimeState, StartupPolicy,
    TransitionError,
};
use stackly_evidence::{Kind, evidence};

/// A table row: `(from, event, payload, to)`, all as codes. A `None`
/// payload matches every payload value.
type Row = (
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
);

/// An operation table row: `(from, event, payload, started, to)`. A `None`
/// payload or `started` matches every value.
type OperationRow = (
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<bool>,
    &'static str,
);

/// The runtime table. The payload is the install mode where it matters.
const RUNTIME_TABLE: &[Row] = &[
    ("absent", "resolve", None, "resolving"),
    ("resolving", "resolved", Some("managed"), "downloading"),
    ("resolving", "resolved", Some("external"), "verifying"),
    ("downloading", "downloaded", None, "verifying"),
    ("verifying", "verified", None, "installing"),
    ("installing", "committed", None, "installed"),
    ("resolving", "failed", None, "failed"),
    ("downloading", "failed", None, "failed"),
    ("verifying", "failed", None, "failed"),
    ("installing", "failed", None, "failed"),
    ("failed", "retry", None, "resolving"),
    ("installed", "observed_missing", None, "missing"),
    ("installed", "observed_drift", None, "drifted"),
    ("missing", "reverified", None, "installed"),
    ("drifted", "reverified", None, "installed"),
    ("missing", "retry", None, "resolving"),
    ("drifted", "retry", None, "resolving"),
    ("failed", "removed", None, "absent"),
    ("installed", "removed", None, "absent"),
    ("missing", "removed", None, "absent"),
    ("drifted", "removed", None, "absent"),
];

/// The lifecycle table. The payload is `will_retry` of `crashed`.
const LIFECYCLE_TABLE: &[Row] = &[
    ("uninitialized", "init_begin", None, "initializing"),
    ("initializing", "init_succeeded", None, "stopped"),
    ("initializing", "init_failed", None, "failed"),
    ("stopped", "start_begin", None, "starting"),
    ("failed", "start_begin", None, "starting"),
    ("starting", "spawned", None, "running"),
    ("recovering", "spawned", None, "running"),
    ("starting", "start_failed", None, "failed"),
    ("running", "crashed", Some("true"), "recovering"),
    ("recovering", "crashed", Some("true"), "recovering"),
    ("running", "crashed", Some("false"), "failed"),
    ("recovering", "crashed", Some("false"), "failed"),
    ("starting", "stop_begin", None, "stopping"),
    ("running", "stop_begin", None, "stopping"),
    ("recovering", "stop_begin", None, "stopping"),
    ("stopping", "exited", None, "stopped"),
    ("starting", "identity_lost", None, "unknown"),
    ("running", "identity_lost", None, "unknown"),
    ("stopping", "identity_lost", None, "unknown"),
    ("recovering", "identity_lost", None, "unknown"),
    ("unknown", "adopted", None, "running"),
    ("unknown", "confirmed_stopped", None, "stopped"),
    ("uninitialized", "retain", None, "retained"),
    ("stopped", "retain", None, "retained"),
    ("failed", "retain", None, "retained"),
];

/// The operation table. The payload is `resumable` of `interrupted` or the
/// outcome of `resolve`; `started` says whether `started_at` is set, where
/// it matters.
const OPERATION_TABLE: &[OperationRow] = &[
    ("queued", "start", None, None, "running"),
    ("queued", "fail", None, None, "failed"),
    ("queued", "request_cancel", None, Some(false), "cancelled"),
    (
        "queued",
        "request_cancel",
        None,
        Some(true),
        "cancel_requested",
    ),
    ("running", "succeed", None, None, "succeeded"),
    ("running", "fail", None, None, "failed"),
    ("running", "request_cancel", None, None, "cancel_requested"),
    ("running", "interrupted", Some("true"), None, "queued"),
    (
        "running",
        "interrupted",
        Some("false"),
        None,
        "needs_recovery",
    ),
    ("cancel_requested", "cancelled", None, None, "cancelled"),
    ("cancel_requested", "succeed", None, None, "succeeded"),
    ("cancel_requested", "fail", None, None, "failed"),
    (
        "cancel_requested",
        "interrupted",
        None,
        None,
        "needs_recovery",
    ),
    (
        "running",
        "compensation_failed",
        None,
        None,
        "needs_recovery",
    ),
    (
        "cancel_requested",
        "compensation_failed",
        None,
        None,
        "needs_recovery",
    ),
    ("needs_recovery", "resume", None, None, "queued"),
    (
        "needs_recovery",
        "resolve",
        Some("succeeded"),
        None,
        "succeeded",
    ),
    ("needs_recovery", "resolve", Some("failed"), None, "failed"),
    (
        "needs_recovery",
        "resolve",
        Some("cancelled"),
        None,
        "cancelled",
    ),
];

/// Returns `2026-01-01T00:00:00Z` plus `seconds`.
fn at(seconds: i64) -> Timestamp {
    let start: Timestamp = "2026-01-01T00:00:00Z"
        .parse()
        .expect("the start time is valid RFC 3339");
    start + SignedDuration::from_secs(seconds)
}

fn reason() -> FailureCode {
    "boom".parse().expect("boom is a failure code")
}

fn lifecycle_events() -> Vec<(LifecycleEvent, Option<&'static str>)> {
    vec![
        (LifecycleEvent::InitBegin, None),
        (LifecycleEvent::InitSucceeded, None),
        (LifecycleEvent::InitFailed { reason: reason() }, None),
        (LifecycleEvent::StartBegin, None),
        (LifecycleEvent::Spawned, None),
        (LifecycleEvent::StartFailed { reason: reason() }, None),
        (
            LifecycleEvent::Crashed {
                will_retry: true,
                reason: reason(),
            },
            Some("true"),
        ),
        (
            LifecycleEvent::Crashed {
                will_retry: false,
                reason: reason(),
            },
            Some("false"),
        ),
        (LifecycleEvent::StopBegin, None),
        (LifecycleEvent::Exited, None),
        (LifecycleEvent::IdentityLost, None),
        (LifecycleEvent::Adopted, None),
        (LifecycleEvent::ConfirmedStopped, None),
        (
            LifecycleEvent::Retain {
                by: OperationId::generate(),
            },
            None,
        ),
    ]
}

fn operation_events() -> Vec<(OperationEvent, Option<&'static str>)> {
    vec![
        (OperationEvent::Start, None),
        (OperationEvent::Succeed, None),
        (
            OperationEvent::Fail(OperationFailure::new(reason(), false)),
            None,
        ),
        (OperationEvent::RequestCancel, None),
        (OperationEvent::Cancelled, None),
        (
            OperationEvent::Interrupted { resumable: true },
            Some("true"),
        ),
        (
            OperationEvent::Interrupted { resumable: false },
            Some("false"),
        ),
        (OperationEvent::CompensationFailed, None),
        (OperationEvent::Resume, None),
        (
            OperationEvent::Resolve(Outcome::Succeeded),
            Some("succeeded"),
        ),
        (OperationEvent::Resolve(Outcome::Failed), Some("failed")),
        (
            OperationEvent::Resolve(Outcome::Cancelled),
            Some("cancelled"),
        ),
    ]
}

/// Checks one pair against the table's answer.
fn check<S: Copy + PartialEq + std::fmt::Debug>(
    result: Result<S, TransitionError>,
    expected: Option<S>,
    machine: Machine,
    from: &str,
    event: &str,
) {
    match expected {
        Some(to) => assert_eq!(result, Ok(to), "{from} + {event}"),
        None => match result {
            Ok(to) => panic!("{from} + {event} gave {to:?}, but the table has no such row"),
            Err(err) => {
                assert_eq!(err.machine(), machine, "{from} + {event}");
                assert_eq!((err.from(), err.event()), (from, event));
                assert_eq!(err.rule(), None);
            }
        },
    }
}

fn check_runtime_table() {
    for state in RuntimeState::ALL {
        assert_eq!(state.as_code().parse::<RuntimeState>(), Ok(*state));
        for event in RuntimeEvent::ALL {
            for install in InstallKind::ALL {
                let expected = RUNTIME_TABLE
                    .iter()
                    .find(|(from, ev, payload, _)| {
                        *from == state.as_code()
                            && *ev == event.as_code()
                            && payload.is_none_or(|kind| kind == install.as_code())
                    })
                    .map(|(.., to)| to.parse().expect("table states are runtime states"));
                let result = state.apply(event, install);
                check(
                    result,
                    expected,
                    Machine::Runtime,
                    state.as_code(),
                    event.as_code(),
                );
            }
        }
    }
}

fn check_lifecycle_table() {
    for state in InstanceLifecycle::ALL {
        assert_eq!(state.as_code().parse::<InstanceLifecycle>(), Ok(*state));
        for (event, payload) in lifecycle_events() {
            let expected = LIFECYCLE_TABLE
                .iter()
                .find(|(from, ev, row_payload, _)| {
                    *from == state.as_code()
                        && *ev == event.as_code()
                        && (row_payload.is_none() || *row_payload == payload)
                })
                .map(|(.., to)| to.parse().expect("table states are lifecycle states"));
            let result = state.apply(&event);
            check(
                result,
                expected,
                Machine::InstanceLifecycle,
                state.as_code(),
                event.as_code(),
            );
        }
    }
}

#[test]
fn runtime_and_lifecycle_tables_are_exhaustive() {
    let ev = evidence!(
        covers = ["REQ-012"],
        kind = Kind::Fixture,
        expected = "every (state, event, payload) pair of the runtime and instance lifecycle machines gives the state spec 0004's tables name, every other pair returns a TransitionError naming machine, state, and event, and every state round trips through its code",
    );
    check_runtime_table();
    check_lifecycle_table();
    ev.pass();
}

#[test]
fn operation_table_is_exhaustive() {
    let ev = evidence!(
        covers = ["REQ-054"],
        kind = Kind::Fixture,
        expected = "every (state, event, payload, started) case of the seven operation states gives the state spec 0004's table names, succeeded, failed, and cancelled accept no event, and every state round trips through its code",
    );
    for state in OperationState::ALL {
        assert_eq!(state.as_code().parse::<OperationState>(), Ok(*state));
        for (event, payload) in operation_events() {
            for started in [false, true] {
                let expected = OPERATION_TABLE
                    .iter()
                    .find(|(from, ev, row_payload, row_started, _)| {
                        *from == state.as_code()
                            && *ev == event.as_code()
                            && (row_payload.is_none() || *row_payload == payload)
                            && row_started.is_none_or(|value| value == started)
                    })
                    .map(|(.., to)| to.parse().expect("table states are operation states"));
                let result = state.apply(&event, started);
                check(
                    result,
                    expected,
                    Machine::Operation,
                    state.as_code(),
                    event.as_code(),
                );
                if state.is_final() {
                    assert!(result.is_err(), "{state} is final");
                }
            }
        }
    }
    assert_eq!(OperationState::ALL.len(), 7);
    ev.pass();
}

fn instance() -> Instance {
    Instance::new(NewInstance {
        id: InstanceId::generate(),
        name: "orders-db".parse().expect("orders-db is an instance name"),
        display_name: None,
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

fn probe() -> ReasonCode {
    "probe_ok".parse().expect("probe_ok is a reason code")
}

#[test]
fn readiness_and_health_follow_the_lifecycle() {
    let ev = evidence!(
        covers = ["REQ-012"],
        kind = Kind::Fixture,
        expected = "entering running sets readiness not_ready and health unknown with reason awaiting_probe, leaving running sets both unknown with reason not_running, ready and known health are refused outside running, older observations are ignored, and observations never change lifecycle, desired state, or revision",
    );
    let mut instance = instance();
    for (seconds, event) in [
        (1, LifecycleEvent::InitBegin),
        (2, LifecycleEvent::InitSucceeded),
    ] {
        instance.apply_lifecycle(event, at(seconds)).unwrap();
    }
    let healthy = |seconds| Health::new(HealthState::Healthy, at(seconds), probe(), None);
    let ready = |seconds| Readiness::new(ReadinessState::Ready, at(seconds), probe(), None);

    assert!(instance.observe_health(healthy(3)).is_err());
    assert_eq!(
        instance.observe_health(healthy(3)).unwrap_err().rule(),
        None
    );

    instance.set_desired(DesiredState::Running);
    instance
        .apply_lifecycle(LifecycleEvent::StartBegin, at(4))
        .unwrap();
    instance
        .apply_lifecycle(LifecycleEvent::Spawned, at(5))
        .unwrap();
    assert_eq!(instance.readiness().state(), ReadinessState::NotReady);
    assert_eq!(instance.readiness().reason(), &ReasonCode::AWAITING_PROBE);
    assert_eq!(instance.health().state(), HealthState::Unknown);
    assert_eq!(instance.health().reason(), &ReasonCode::AWAITING_PROBE);

    let before = instance.clone();
    for seconds in 6..106 {
        instance.observe_readiness(ready(seconds)).unwrap();
        instance.observe_health(healthy(seconds)).unwrap();
    }
    assert_eq!(instance.lifecycle(), before.lifecycle());
    assert_eq!(instance.desired(), before.desired());
    assert_eq!(instance.revision(), before.revision());
    assert_eq!(
        instance.observe_readiness(Readiness::new(
            ReadinessState::NotReady,
            at(50),
            probe(),
            None
        )),
        Ok(ObservationOutcome::Ignored(Ignored::Stale))
    );
    assert_eq!(instance.readiness().state(), ReadinessState::Ready);

    instance
        .apply_lifecycle(
            LifecycleEvent::Crashed {
                will_retry: true,
                reason: reason(),
            },
            at(200),
        )
        .unwrap();
    assert_eq!(instance.readiness().state(), ReadinessState::Unknown);
    assert_eq!(instance.readiness().reason(), &ReasonCode::NOT_RUNNING);
    assert_eq!(instance.health().state(), HealthState::Unknown);
    assert_eq!(instance.health().reason(), &ReasonCode::NOT_RUNNING);
    assert_eq!(instance.desired(), DesiredState::Running);
    assert_eq!(
        instance.observe_readiness(ready(150)),
        Ok(ObservationOutcome::Ignored(Ignored::Stale))
    );
    assert_eq!(
        instance.observe_readiness(ready(201)).unwrap_err().rule(),
        Some(Rule::Br010)
    );
    ev.pass();
}
