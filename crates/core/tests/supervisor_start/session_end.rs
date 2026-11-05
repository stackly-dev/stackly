//! The end of a login session (spec 0031, *Akhir sesi*, AC-11 and AC-13):
//! every live instance stops through the provider's strategy and keeps its
//! desired state and its leases, a stop that is too slow leaves the row
//! `interrupted`, and the return in the same session reopens the row so the
//! start queue starts what the end stopped.

use std::time::Duration;

use stackly_core::session::{RotationKind, rotate_session};
use stackly_core::supervisor::{RegistryMode, StartRequest};
use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, Lease, LeaseFilter, LeaseHolder, LeaseId,
    OperationKind, OperationState, Registry, SessionEndReason, SessionId, WriteBatch, WriteOp,
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
    in_session(Rig::new().await)
}

/// Puts `rig` in the session and stores the row of that session.
fn in_session(rig: Rig) -> Rig {
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

fn main_spawns(rig: &Rig) -> usize {
    rig.processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == stackly_platform::ChildRole::Main)
        .count()
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

#[tokio::test]
async fn a_queued_start_that_waits_for_a_permit_never_begins_once_the_session_ends() {
    let rig = in_session(Rig::with_config(|config| config.reconcile_start_concurrency = 1).await);
    let a = rig.instance(&spec(DesiredState::Running));
    let b = rig.instance(&spec(DesiredState::Running));
    rig.sup
        .reconcile_in_session(RegistryMode::Ready, false)
        .await
        .expect("reconcile");
    let queue = rig.sup.queue_login_starts(false).await.expect("queue");
    assert_eq!(queue.added, 2);
    rig.sup.start_reconciled();
    // The first start holds the only permit: nothing serves its port, so it
    // waits for readiness while the second one waits for the permit.
    let first = rig.main_spawn(1).await;
    let mut begun = Vec::new();
    for id in [a, b] {
        if !rig
            .operations_of(id, OperationKind::StartInstance)
            .await
            .is_empty()
        {
            begun.push(id);
        }
    }
    assert_eq!(begun.len(), 1, "only one start holds the permit");
    let queued = if begun[0] == a { b } else { a };

    let deadline = Instant::now() + Duration::from_secs(10);
    let (report, ()) = tokio::join!(rig.sup.end_session(deadline), exit_on_term(&rig, &first));
    assert_eq!(report.reason, SessionEndReason::Clean);
    // The cancelled start gave its permit back; give the queue time to act
    // on it.
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        rig.operations_of(queued, OperationKind::StartInstance)
            .await
            .is_empty(),
        "no start begins after the session end took its snapshot"
    );
    assert_eq!(
        main_spawns(&rig),
        1,
        "nothing spawned after the session end"
    );
    let row = rig.instance_row(queued);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(
        row.desired(),
        DesiredState::Running,
        "the next start queue of the same session starts it"
    );
}

#[tokio::test]
async fn a_background_start_begun_as_the_session_ends_does_not_spawn() {
    let rig = rig_in_session().await;
    let id = rig.instance(&spec(DesiredState::Running));
    // Nothing is live, so the session end only raises its flag.
    let report = rig
        .sup
        .end_session(Instant::now() + Duration::from_secs(10))
        .await;
    assert_eq!(report.stops, 0);

    // A start the engine took just before the flag rose reads it under the
    // operation's lock and ends without a spawn.
    let work = rig.sup.start_work(StartRequest::background(id));
    let operation = rig.begin(OperationKind::StartInstance, id, work).await;
    rig.wait_state(operation, OperationState::Succeeded).await;

    assert_eq!(main_spawns(&rig), 0);
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.desired(), DesiredState::Running, "desired is untouched");
}

#[tokio::test]
async fn a_start_of_the_person_is_not_held_back_by_the_session_end_flag() {
    let rig = rig_in_session().await;
    let id = rig.instance(&spec(DesiredState::Stopped));
    rig.sup
        .end_session(Instant::now() + Duration::from_secs(10))
        .await;

    // The IPC server refuses every new request during a session end; the
    // flag itself only holds back what the daemon starts on its own.
    let operation = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(operation, OperationState::Succeeded).await;
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
}
