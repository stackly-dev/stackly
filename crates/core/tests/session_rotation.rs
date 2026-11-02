//! Login session rotation (spec 0031 AC-2, AC-3, AC-5, AC-14): the pure
//! plan over a table of states, and the guarded run over the fake registry.
#![cfg(unix)]

use std::sync::Arc;

use jiff::{SignedDuration, Timestamp};
use stackly_core::session::{RotationKind, plan_session_rotation, rotate_session};
use stackly_domain::{
    DaemonSession, DesiredState, Instance, InstanceId, InstanceLifecycle, Lease, LeaseFilter,
    LeaseHolder, LeaseId, LeaseState, Registry, SessionEndReason, StartupPolicy, WriteBatch,
    WriteOp,
};
use stackly_platform::{Clock, LoginSession};
use stackly_testkit::{
    EnvironmentBuilder, FakeClock, FakeRegistry, InstanceBuilder, LeaseBuilder,
    ProviderRevisionBuilder, RuntimeBuilder,
};

const BOOT: &str = "6f9619ff-8b86-4011-b42d-00c04fc964ff";

fn session(audit: u32) -> LoginSession {
    LoginSession::new(BOOT.parse().expect("test setup step succeeds"), audit)
        .expect("test setup step succeeds")
}

fn row(audit: u32, open: bool) -> DaemonSession {
    let current = session(audit);
    let mut row = DaemonSession::new(
        current.id,
        current.boot_id,
        audit,
        "2026-11-02T08:00:00Z"
            .parse()
            .expect("test setup step succeeds"),
    )
    .expect("test setup step succeeds");
    if !open {
        row.end(
            "2026-11-02T09:00:00Z"
                .parse()
                .expect("test setup step succeeds"),
            SessionEndReason::Clean,
        );
    }
    row
}

fn now() -> Timestamp {
    "2026-11-02T10:00:00Z"
        .parse()
        .expect("test setup step succeeds")
}

struct Fixture {
    provider: ProviderRevisionBuilder,
    runtime: RuntimeBuilder,
}

impl Fixture {
    fn new() -> Self {
        Self {
            provider: ProviderRevisionBuilder::new(),
            runtime: RuntimeBuilder::new(),
        }
    }

    /// An initialized instance in `stopped` with the given policy and
    /// desired state.
    fn instance(&self, name: &str, startup: StartupPolicy, desired: DesiredState) -> Instance {
        InstanceBuilder::new(&self.runtime.clone().build(), &self.provider.key())
            .name(name)
            .lifecycle(InstanceLifecycle::Stopped)
            .data_initialized_at(
                "2026-11-01T00:00:00Z"
                    .parse()
                    .expect("test setup step succeeds"),
            )
            .startup(startup)
            .desired(desired)
            .build()
    }
}

fn plan(
    current: &LoginSession,
    latest: Option<&DaemonSession>,
    instances: &[Instance],
    leases: &[Lease],
) -> stackly_core::session::SessionRotation {
    plan_session_rotation(
        current,
        latest,
        instances,
        leases,
        now(),
        &mut LeaseId::generate,
    )
}

fn desired_writes(
    rotation: &stackly_core::session::SessionRotation,
) -> Vec<(InstanceId, DesiredState)> {
    rotation
        .ops
        .iter()
        .filter_map(|op| match op {
            WriteOp::UpdateInstance { instance, .. } => Some((instance.id(), instance.desired())),
            _ => None,
        })
        .collect()
}

#[test]
fn without_a_row_the_rotation_is_first_and_only_inserts_the_row() {
    let fixture = Fixture::new();
    let running = fixture.instance("pg", StartupPolicy::AtLogin, DesiredState::Running);
    let rotation = plan(&session(7), None, &[running], &[]);

    assert_eq!(rotation.kind, RotationKind::First);
    assert!(matches!(
        rotation.ops.as_slice(),
        [WriteOp::InsertSession(_)]
    ));
    assert!(rotation.started.is_empty());
}

#[test]
fn the_same_open_session_changes_nothing_and_a_closed_one_opens_again() {
    let open = plan(&session(7), Some(&row(7, true)), &[], &[]);
    assert_eq!(open.kind, RotationKind::Continued);
    assert!(open.ops.is_empty());

    let closed = row(7, false);
    let reopened = plan(&session(7), Some(&closed), &[], &[]);
    assert_eq!(reopened.kind, RotationKind::Continued);
    assert_eq!(
        reopened.ops,
        vec![WriteOp::ReopenSession {
            id: closed.id().clone()
        }]
    );
}

#[test]
fn a_new_session_starts_the_at_login_instances_and_stops_the_manual_ones() {
    let fixture = Fixture::new();
    let login = fixture.instance("a-login", StartupPolicy::AtLogin, DesiredState::Stopped);
    let manual = fixture.instance("b-manual", StartupPolicy::Manual, DesiredState::Running);
    let idle = fixture.instance("c-idle", StartupPolicy::Manual, DesiredState::Stopped);
    let latest = row(6, true);
    let rotation = plan(
        &session(7),
        Some(&latest),
        &[login.clone(), manual.clone(), idle],
        &[],
    );

    assert_eq!(rotation.kind, RotationKind::New);
    assert_eq!(rotation.started, vec![login.id()]);
    assert_eq!(
        desired_writes(&rotation),
        vec![
            (login.id(), DesiredState::Running),
            (manual.id(), DesiredState::Stopped),
        ],
        "an instance that already has its desired state is not rewritten"
    );
    assert!(matches!(
        rotation.ops.first(),
        Some(WriteOp::EndSession {
            reason: SessionEndReason::Unknown,
            ..
        })
    ));
    let leases: Vec<_> = rotation
        .ops
        .iter()
        .filter_map(|op| match op {
            WriteOp::InsertLease(lease) => Some(lease),
            _ => None,
        })
        .collect();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].instance_id(), login.id());
    assert_eq!(leases[0].holder(), LeaseHolder::Startup);
    assert_eq!(leases[0].session(), Some(&session(7).id));
    assert!(matches!(
        rotation.ops.last(),
        Some(WriteOp::PruneSessions { keep: 50 })
    ));
}

#[test]
fn a_closed_previous_row_is_not_ended_again() {
    let rotation = plan(&session(7), Some(&row(6, false)), &[], &[]);

    assert!(
        !rotation
            .ops
            .iter()
            .any(|op| matches!(op, WriteOp::EndSession { .. }))
    );
}

#[test]
fn instances_that_cannot_run_get_no_lease() {
    let fixture = Fixture::new();
    let mut uninitialized =
        InstanceBuilder::new(&fixture.runtime.clone().build(), &fixture.provider.key())
            .name("new-one")
            .startup(StartupPolicy::AtLogin)
            .build();
    assert_eq!(uninitialized.lifecycle(), InstanceLifecycle::Uninitialized);
    let unknown = InstanceBuilder::new(&fixture.runtime.clone().build(), &fixture.provider.key())
        .name("lost")
        .lifecycle(InstanceLifecycle::Unknown)
        .data_initialized_at(
            "2026-11-01T00:00:00Z"
                .parse()
                .expect("test setup step succeeds"),
        )
        .startup(StartupPolicy::AtLogin)
        .build();
    let rotation = plan(
        &session(7),
        Some(&row(6, true)),
        &[uninitialized.clone(), unknown],
        &[],
    );

    assert!(rotation.started.is_empty());
    assert!(
        !rotation
            .ops
            .iter()
            .any(|op| matches!(op, WriteOp::InsertLease(_)))
    );
    uninitialized.set_desired(DesiredState::Stopped);
}

#[test]
fn leases_of_other_sessions_are_released_and_environment_leases_hold_manual_instances() {
    let fixture = Fixture::new();
    let held = fixture.instance("held", StartupPolicy::Manual, DesiredState::Running);
    let environment = EnvironmentBuilder::new().shared(held.id()).build();
    let env_lease = LeaseBuilder::new(held.id())
        .environment(environment.id())
        .build();
    let old_manual = LeaseBuilder::new(held.id())
        .manual(Some(&row(6, true).id().to_string()))
        .build();
    let old_startup = LeaseBuilder::new(held.id())
        .startup(&row(6, true).id().to_string())
        .build();
    let sessionless = LeaseBuilder::new(held.id()).manual(None).build();
    let rotation = plan(
        &session(7),
        Some(&row(6, true)),
        std::slice::from_ref(&held),
        &[
            env_lease.clone(),
            old_manual.clone(),
            old_startup.clone(),
            sessionless.clone(),
        ],
    );

    let released: Vec<LeaseId> = rotation
        .ops
        .iter()
        .filter_map(|op| match op {
            WriteOp::UpdateLease(lease) if matches!(lease.state(), LeaseState::Released { .. }) => {
                Some(lease.id())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        released,
        vec![old_manual.id(), old_startup.id(), sessionless.id()]
    );
    assert!(
        desired_writes(&rotation).is_empty(),
        "an environment lease keeps a manual instance wanted"
    );
}

/// Commits the provider, runtime, and `instances` to a fresh registry.
fn registry_with(fixture: &Fixture, instances: &[Instance]) -> FakeRegistry {
    let clock = FakeClock::new();
    let registry = FakeRegistry::new(Arc::new(clock.clone()), clock.now());
    let mut ops = vec![
        WriteOp::InsertProviderRevision(fixture.provider.clone().record()),
        WriteOp::InsertArtifact(fixture.runtime.artifact()),
        WriteOp::InsertRuntime(fixture.runtime.clone().build()),
    ];
    ops.extend(instances.iter().cloned().map(WriteOp::InsertInstance));
    registry
        .commit(WriteBatch {
            expected_generation: None,
            ops,
        })
        .expect("the fixture commits");
    registry
}

fn startup_leases(registry: &FakeRegistry, instance: InstanceId) -> usize {
    registry
        .leases(instance, LeaseFilter::Unreleased)
        .expect("test setup step succeeds")
        .value
        .iter()
        .filter(|lease| lease.holder() == LeaseHolder::Startup)
        .count()
}

#[test]
fn a_daemon_that_restarts_in_the_same_session_never_makes_a_second_lease() {
    let fixture = Fixture::new();
    let login = fixture.instance("pg", StartupPolicy::AtLogin, DesiredState::Stopped);
    let registry = registry_with(&fixture, std::slice::from_ref(&login));

    let first = rotate_session(&registry, &session(6), now()).expect("test setup step succeeds");
    assert_eq!(first.kind, RotationKind::First);
    assert!(
        first.started.is_empty(),
        "the first rotation starts nothing"
    );
    let second = rotate_session(&registry, &session(7), now()).expect("test setup step succeeds");
    assert_eq!(second.kind, RotationKind::New);
    assert_eq!(second.started, vec![login.id()]);
    assert_eq!(second.previous_end, Some(SessionEndReason::Unknown));
    let again = rotate_session(&registry, &session(7), now()).expect("test setup step succeeds");
    assert_eq!(again.kind, RotationKind::Continued);
    assert!(again.started.is_empty());

    assert_eq!(startup_leases(&registry, login.id()), 1);
    let stored = registry
        .instance(login.id())
        .expect("test setup step succeeds")
        .value
        .expect("test setup step succeeds");
    assert_eq!(stored.desired(), DesiredState::Running);
}

#[test]
fn rotation_keeps_the_newest_fifty_session_rows() {
    let fixture = Fixture::new();
    let registry = registry_with(&fixture, &[]);

    for audit in 1..=55 {
        rotate_session(
            &registry,
            &session(audit),
            now() + SignedDuration::from_secs(i64::from(audit)),
        )
        .expect("test setup step succeeds");
    }

    let rows = registry
        .recent_sessions(100)
        .expect("test setup step succeeds")
        .value;
    assert_eq!(rows.len(), 50);
    assert_eq!(rows[0].id(), &session(55).id);
}
