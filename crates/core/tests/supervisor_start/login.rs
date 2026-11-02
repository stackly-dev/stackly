//! The start queue of a launchd daemon and the new session reading of
//! reconciliation (spec 0031 AC-6, AC-7): level triggered starts with the
//! trigger `login`, a desired state that is read again under the lock, and
//! a process the logout ended that is not a crash.

use std::time::Duration;

use stackly_core::supervisor::{RegistryMode, StartRequest};
use stackly_domain::{
    DesiredState, InstanceLifecycle, OperationKind, OperationState, ReadinessState,
};
use stackly_testkit::ProcessOp;

use crate::common::{Plant, Rig, Spec};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn spec(lifecycle: InstanceLifecycle, desired: DesiredState) -> Spec {
    Spec {
        lifecycle,
        desired,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

fn main_spawns(rig: &Rig) -> usize {
    rig.processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == stackly_platform::ChildRole::Main)
        .count()
}

#[tokio::test]
async fn a_new_session_starts_a_wanted_stopped_instance_with_the_trigger_login() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Running));
    rig.sup
        .reconcile_in_session(RegistryMode::Ready, true)
        .await
        .expect("test setup step succeeds");

    let queue = rig
        .sup
        .queue_login_starts(true)
        .await
        .expect("test setup step succeeds");
    rig.sup.start_reconciled();
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    wait_for("the instance is ready", || {
        rig.sup.status(id).readiness.state() == ReadinessState::Ready
    })
    .await;

    assert_eq!(queue.added, 1);
    let starts = rig.operations_of(id, OperationKind::StartInstance).await;
    assert!(
        starts
            .iter()
            .any(|view| view.journal.input().as_str().contains("login")),
        "the start carries the trigger login"
    );
}

#[tokio::test]
async fn a_daemon_start_in_the_same_session_queues_with_the_trigger_reconcile() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Running));
    rig.sup
        .reconcile(RegistryMode::Ready)
        .await
        .expect("test setup step succeeds");

    rig.sup
        .queue_login_starts(false)
        .await
        .expect("test setup step succeeds");
    rig.sup.start_reconciled();
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    wait_for("the instance runs", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;

    let starts = rig.operations_of(id, OperationKind::StartInstance).await;
    assert!(
        starts
            .iter()
            .any(|view| view.journal.input().as_str().contains("reconcile"))
    );
}

#[tokio::test]
async fn an_instance_whose_desired_state_is_stopped_never_joins_the_queue() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    rig.sup
        .reconcile(RegistryMode::Ready)
        .await
        .expect("test setup step succeeds");

    let queue = rig
        .sup
        .queue_login_starts(true)
        .await
        .expect("test setup step succeeds");
    rig.sup.start_reconciled();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(queue.added, 0);
    assert_eq!(main_spawns(&rig), 0);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
}

#[tokio::test]
async fn a_failed_instance_joins_the_queue_only_for_a_failure_a_restart_can_fix() {
    let rig = Rig::new().await;
    rig.instance(&Spec {
        last_failure: Some("interrupted"),
        ..spec(InstanceLifecycle::Failed, DesiredState::Running)
    });
    rig.instance(&Spec {
        last_failure: Some("runtime_unavailable"),
        ..spec(InstanceLifecycle::Failed, DesiredState::Running)
    });
    rig.sup
        .reconcile(RegistryMode::Ready)
        .await
        .expect("test setup step succeeds");

    let queue = rig
        .sup
        .queue_login_starts(true)
        .await
        .expect("test setup step succeeds");

    assert_eq!(queue.added, 1, "only the interrupted start is queued");
}

#[tokio::test]
async fn a_system_start_reads_desired_under_the_lock_and_ends_without_a_start_when_stopped() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));

    let work = rig.sup.start_work(StartRequest::background(id));
    let operation = rig.begin(OperationKind::StartInstance, id, work).await;
    rig.wait_state(operation, OperationState::Succeeded).await;

    assert_eq!(main_spawns(&rig), 0);
    let row = rig.instance_row(id);
    assert_eq!(
        row.desired(),
        DesiredState::Stopped,
        "desired is not written"
    );
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
}

#[tokio::test]
async fn a_new_session_does_not_respawn_or_count_a_process_the_logout_ended() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);

    let report = rig
        .sup
        .reconcile_in_session(RegistryMode::Ready, true)
        .await
        .expect("test setup step succeeds");

    assert_eq!(report.restarting, 0, "nothing respawns");
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.restart_total(), 0, "a logout is not a crash");
    assert_eq!(main_spawns(&rig), 0);
}

#[tokio::test]
async fn a_proven_process_whose_desired_state_is_now_stopped_stops_with_the_trigger_session_rotation()
 {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Stopped));
    let identity = rig.plant(id, Plant::live());

    rig.sup
        .reconcile_in_session(RegistryMode::Ready, true)
        .await
        .expect("test setup step succeeds");
    rig.sup.start_reconciled();
    wait_for("the stop ran", || {
        rig.processes().count(ProcessOp::Signal) > 0
    })
    .await;

    let stops = rig.operations_of(id, OperationKind::StopInstance).await;
    assert!(
        stops
            .iter()
            .any(|view| view.journal.input().as_str().contains("session_rotation"))
    );
    let _ = identity;
}
