//! The leases a user start and a user stop write (spec 0031 AC-8, AC-9): a
//! manual lease with the start, and with the stop the manual leases released
//! and the startup lease of the session suppressed, in the commit that writes
//! desired.

use stackly_core::supervisor::RegistryMode;
use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, Lease, LeaseFilter, LeaseHolder, LeaseId,
    LeaseState, OperationKind, OperationState, Registry, SessionId, WriteBatch, WriteOp,
};
use stackly_platform::Clock;

use crate::common::{Rig, Spec};

fn session() -> SessionId {
    "6f9619ff-8b86-4011-b42d-00c04fc964ff/100014"
        .parse()
        .expect("a session ID")
}

fn spec(lifecycle: InstanceLifecycle, desired: DesiredState) -> Spec {
    Spec {
        lifecycle,
        desired,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

fn leases(rig: &Rig, id: InstanceId, filter: LeaseFilter) -> Vec<Lease> {
    rig.registry.leases(id, filter).expect("leases read").value
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

#[tokio::test]
async fn a_user_start_writes_one_manual_lease_for_the_session_and_a_second_start_none() {
    let rig = Rig::new().await;
    rig.sup.set_session(Some(session()));
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;

    let held = leases(&rig, id, LeaseFilter::Unreleased);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].holder(), LeaseHolder::Manual);
    assert_eq!(held[0].session(), Some(&session()));
    assert!(held[0].is_active());
}

#[tokio::test]
async fn a_start_the_daemon_makes_writes_no_manual_lease() {
    let rig = Rig::new().await;
    rig.sup.set_session(Some(session()));
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Running));
    rig.sup
        .reconcile_in_session(RegistryMode::Ready, true)
        .await
        .expect("reconcile");
    rig.sup.queue_login_starts(true).await.expect("queue");
    rig.sup.start_reconciled();
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    while rig.instance_row(id).lifecycle() != InstanceLifecycle::Running {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(leases(&rig, id, LeaseFilter::All).is_empty());
}

#[tokio::test]
async fn a_user_start_without_a_session_writes_a_manual_lease_without_one() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;

    let held = leases(&rig, id, LeaseFilter::Unreleased);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].session(), None);
}

#[tokio::test]
async fn a_user_stop_releases_the_manual_lease_and_suppresses_the_startup_lease() {
    let rig = Rig::new().await;
    rig.sup.set_session(Some(session()));
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    startup_lease(&rig, id);
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;

    let stop = rig.stop(id).await;
    while rig.processes().signals(spawn.pid).is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    rig.kill_servers();
    rig.processes()
        .exit(spawn.pid, Some(stackly_platform::ExitStatus::Exited(0)));
    rig.wait_state(stop, OperationState::Succeeded).await;

    let all = leases(&rig, id, LeaseFilter::All);
    let manual = all
        .iter()
        .find(|lease| lease.holder() == LeaseHolder::Manual)
        .expect("the manual lease");
    assert!(matches!(manual.state(), LeaseState::Released { .. }));
    let startup = all
        .iter()
        .find(|lease| lease.holder() == LeaseHolder::Startup)
        .expect("the startup lease");
    assert!(matches!(startup.state(), LeaseState::Suppressed { .. }));
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Stopped);
}

#[tokio::test]
async fn a_stop_of_an_instance_that_is_wanted_but_does_not_run_is_a_registry_change_with_the_same_leases()
 {
    let rig = Rig::new().await;
    rig.sup.set_session(Some(session()));
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Running));
    startup_lease(&rig, id);

    let stop = rig.stop(id).await;
    let done = rig.wait_state(stop, OperationState::Succeeded).await;

    assert_eq!(done.view.operation.kind(), OperationKind::StopInstance);
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Stopped);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    let startup = leases(&rig, id, LeaseFilter::All);
    assert!(
        startup
            .iter()
            .any(|lease| matches!(lease.state(), LeaseState::Suppressed { .. }))
    );
}

/// Begins a start of `id` as an environment Up starts it (spec 0046).
async fn environment_start(rig: &Rig, id: InstanceId) -> stackly_domain::OperationId {
    let work = rig
        .sup
        .start_work(stackly_core::supervisor::StartRequest::environment(
            id,
            Rig::user(),
        ));
    rig.begin(OperationKind::StartInstance, id, work).await
}

// covers: AC-13 (spec 0046)
#[tokio::test]
async fn an_environment_start_writes_desired_running_and_no_manual_lease() {
    let rig = Rig::new().await;
    rig.sup.set_session(Some(session()));
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));

    let op = environment_start(&rig, id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.instance_row(id).desired(), DesiredState::Running);
    assert!(leases(&rig, id, LeaseFilter::All).is_empty());
}

// covers: AC-13 (spec 0046)
#[tokio::test]
async fn an_environment_start_of_a_running_instance_wanted_stopped_writes_only_desired() {
    let rig = Rig::new().await;
    rig.sup.set_session(Some(session()));
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    let op = environment_start(&rig, id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    let running = rig.instance_row(id);
    let mut changes = stackly_core::supervisor::InstanceChanges::new(running.clone());
    changes
        .apply(|instance| {
            instance.set_desired(DesiredState::Stopped);
            Ok::<_, stackly_core::supervisor::SupervisorError>(())
        })
        .expect("desired can change");
    rig.registry
        .commit(WriteBatch::new(changes.into_ops()))
        .expect("desired stopped is stored");

    let again = environment_start(&rig, id).await;
    rig.wait_state(again, OperationState::Succeeded).await;

    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(row.desired(), DesiredState::Running);
    assert!(leases(&rig, id, LeaseFilter::All).is_empty());
}
