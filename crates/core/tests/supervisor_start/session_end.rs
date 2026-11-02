//! The end of a login session (spec 0031, *Akhir sesi*, AC-11 and AC-13):
//! every live instance stops through the provider's strategy and keeps its
//! desired state and its leases, a stop that is too slow leaves the row
//! `interrupted`, and the return in the same session reopens the row so the
//! start queue starts what the end stopped.

use std::time::Duration;

use stackly_core::session::{RotationKind, rotate_session};
use stackly_core::supervisor::RegistryMode;
use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, Lease, LeaseFilter, LeaseHolder, LeaseId,
    OperationState, Registry, SessionEndReason, SessionId, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, ExitStatus, LoginSession, ProviderSignal};
use tokio::time::Instant;

use crate::common::{Rig, Spec};

const BOOT: &str = "6f9619ff-8b86-4011-b42d-00c04fc964ff";

fn session() -> SessionId {
    format!("{BOOT}/100014").parse().expect("a session ID")
}

fn login() -> LoginSession {
    LoginSession::new(BOOT.parse().expect("a boot ID"), 100_014).expect("a login session")
}

fn spec(desired: DesiredState) -> Spec {
    Spec {
        lifecycle: InstanceLifecycle::Stopped,
        desired,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

/// A rig in the session, whose registry holds the row of that session.
async fn rig_in_session() -> Rig {
    let rig = Rig::new().await;
    rig.sup.set_session(Some(session()));
    let outcome =
        rotate_session(&*rig.registry, &login(), rig.fake.clock().now()).expect("the first row");
    assert_eq!(outcome.kind, RotationKind::First);
    rig
}

fn startup_lease(rig: &Rig, id: InstanceId) {
    let lease = Lease::new(
        LeaseId::generate(),
        id,
        LeaseHolder::Startup,
        Some(session()),
        rig.fake.clock().now(),
    )
    .expect("a startup lease");
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertLease(lease)]))
        .expect("the lease is stored");
}

async fn running(rig: &Rig, id: InstanceId, nth: usize) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(nth).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

async fn exit_on_term(rig: &Rig, spawn: &stackly_testkit::FakeSpawn) {
    while rig.processes().signals(spawn.pid).is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
}

fn latest_row(rig: &Rig) -> stackly_domain::DaemonSession {
    rig.registry
        .recent_sessions(1)
        .expect("the rows are read")
        .value
        .remove(0)
}

#[tokio::test]
async fn a_session_end_stops_every_live_instance_and_changes_neither_desired_nor_leases() {
    let rig = rig_in_session().await;
    let a = rig.instance(&spec(DesiredState::Stopped));
    let b = rig.instance(&spec(DesiredState::Stopped));
    let first = running(&rig, a, 1).await;
    let second = running(&rig, b, 2).await;
    startup_lease(&rig, a);
    let before_a = rig
        .registry
        .leases(a, LeaseFilter::Unreleased)
        .expect("leases")
        .value;

    let deadline = Instant::now() + Duration::from_secs(10);
    let (report, (), ()) = tokio::join!(
        rig.sup.end_session(deadline),
        exit_on_term(&rig, &first),
        exit_on_term(&rig, &second),
    );

    assert_eq!(report.reason, SessionEndReason::Clean);
    assert_eq!(report.stops, 2);
    assert_eq!(report.unfinished, 0);
    for (id, spawn) in [(a, &first), (b, &second)] {
        let row = rig.instance_row(id);
        assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
        assert_eq!(row.desired(), DesiredState::Running, "desired is untouched");
        assert_eq!(
            rig.processes().signals(spawn.pid),
            vec![ProviderSignal::Term]
        );
    }
    let after_a = rig
        .registry
        .leases(a, LeaseFilter::Unreleased)
        .expect("leases")
        .value;
    assert_eq!(after_a, before_a, "no lease is released or suppressed");

    assert!(rig.sup.close_session(report.reason).await);
    let row = latest_row(&rig);
    assert_eq!(row.end_reason(), Some(SessionEndReason::Clean));
    assert!(
        !rig.sup.close_session(SessionEndReason::Interrupted).await,
        "the row closes once"
    );
}

#[tokio::test]
async fn a_stop_that_outlasts_the_deadline_leaves_the_instance_stopping_and_the_row_interrupted() {
    let rig = rig_in_session().await;
    let slow = rig.instance(&spec(DesiredState::Stopped));
    let quick = rig.instance(&spec(DesiredState::Stopped));
    let stuck = running(&rig, slow, 1).await;
    let done = running(&rig, quick, 2).await;

    let deadline = Instant::now() + Duration::from_millis(600);
    let (report, ()) = tokio::join!(rig.sup.end_session(deadline), exit_on_term(&rig, &done));

    assert_eq!(report.reason, SessionEndReason::Interrupted);
    assert_eq!(report.stops, 2);
    assert_eq!(report.unfinished, 1);
    assert_eq!(
        rig.instance_row(slow).lifecycle(),
        InstanceLifecycle::Stopping
    );
    assert_eq!(rig.instance_row(slow).desired(), DesiredState::Running);
    assert_eq!(
        rig.instance_row(quick).lifecycle(),
        InstanceLifecycle::Stopped
    );
    assert_eq!(
        rig.processes().signals(stuck.pid),
        vec![ProviderSignal::Term]
    );

    assert!(rig.sup.close_session(report.reason).await);
    assert_eq!(
        latest_row(&rig).end_reason(),
        Some(SessionEndReason::Interrupted)
    );
}

#[tokio::test]
async fn a_start_that_is_still_running_is_cancelled_and_its_process_stops() {
    let rig = rig_in_session().await;
    let id = rig.instance(&spec(DesiredState::Stopped));
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    // Nothing serves the port, so the start waits for readiness.

    let deadline = Instant::now() + Duration::from_secs(10);
    let (report, ()) = tokio::join!(rig.sup.end_session(deadline), exit_on_term(&rig, &spawn));

    assert_eq!(report.reason, SessionEndReason::Clean);
    let detail = rig.wait_final(op).await;
    assert_eq!(detail.view.operation.state(), OperationState::Cancelled);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(
        rig.instance_row(id).desired(),
        DesiredState::Running,
        "the cancel of a session end is not a choice of the person"
    );
}

#[tokio::test]
async fn coming_back_in_the_same_session_reopens_the_row_and_starts_only_what_the_end_stopped() {
    let rig = rig_in_session().await;
    let ended = rig.instance(&spec(DesiredState::Stopped));
    let by_user = rig.instance(&spec(DesiredState::Stopped));
    let spawn = running(&rig, ended, 1).await;
    let user_spawn = running(&rig, by_user, 2).await;
    let stop = rig.stop(by_user).await;
    exit_on_term(&rig, &user_spawn).await;
    rig.wait_state(stop, OperationState::Succeeded).await;
    assert_eq!(rig.instance_row(by_user).desired(), DesiredState::Stopped);

    let deadline = Instant::now() + Duration::from_secs(10);
    let (report, ()) = tokio::join!(rig.sup.end_session(deadline), exit_on_term(&rig, &spawn));
    rig.sup.close_session(report.reason).await;
    assert_eq!(latest_row(&rig).end_reason(), Some(SessionEndReason::Clean));

    let rotation =
        rotate_session(&*rig.registry, &login(), rig.fake.clock().now()).expect("the rotation");
    assert_eq!(rotation.kind, RotationKind::Continued);
    let row = latest_row(&rig);
    assert!(row.is_open(), "the row is open again");
    assert_eq!(row.end_reason(), None);

    let reborn = rig.reborn();
    reborn.set_session(Some(session()));
    reborn
        .reconcile_in_session(RegistryMode::Ready, false)
        .await
        .expect("reconcile");
    reborn.queue_login_starts(false).await.expect("queue");
    reborn.start_reconciled();
    let again = rig.main_spawn(3).await;
    rig.serve(&again);
    while rig.instance_row(ended).lifecycle() != InstanceLifecycle::Running {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(rig.instance_row(ended).desired(), DesiredState::Running);
    assert_eq!(
        rig.instance_row(by_user).lifecycle(),
        InstanceLifecycle::Stopped,
        "an instance whose desired state is stopped does not start"
    );
}
