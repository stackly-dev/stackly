//! The operation `stop_all_instances` (spec 0031 AC-10): it stops every
//! instance of its list as a person's stop does, and a cancel lets the stop
//! that runs finish, begins no new one, and skips the rest.

use stackly_core::operation::{BeginOutcome, BeginRequest, BusyPolicy, OperationOutcome, Origin};
use stackly_core::supervisor::StopAllRequest;
use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, JournalPayload, LockKey, OperationId,
    OperationKind, OperationResource, OperationState, ResourceRef,
};
use stackly_platform::ExitStatus;

use crate::common::{Rig, Spec};

fn spec() -> Spec {
    Spec {
        lifecycle: InstanceLifecycle::Stopped,
        desired: DesiredState::Stopped,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

async fn running(rig: &Rig, id: InstanceId, nth: usize) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(nth).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

async fn begin_stop_all(rig: &Rig, steps: &[InstanceId]) -> OperationId {
    let locks = steps
        .iter()
        .map(|id| LockKey::for_resource(OperationResource::Instance(*id)))
        .collect();
    let outcome = rig
        .engine
        .begin(BeginRequest {
            kind: OperationKind::StopAllInstances,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: locks,
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work: rig.sup.stop_all_work(StopAllRequest::new(steps.to_vec())),
            precheck: None,
        })
        .await
        .expect("the engine accepts the operation");
    match outcome {
        BeginOutcome::Created(view) => view.operation.id(),
        other => panic!("expected a created operation, got {other:?}"),
    }
}

async fn end_on_term(rig: &Rig, spawn: &stackly_testkit::FakeSpawn) {
    while rig.processes().signals(spawn.pid).is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
}

#[tokio::test]
async fn the_operation_stops_every_instance_of_its_list_and_writes_desired_stopped() {
    let rig = Rig::new().await;
    let a = rig.instance(&spec());
    let b = rig.instance(&spec());
    let first = running(&rig, a, 1).await;
    let second = running(&rig, b, 2).await;

    let op = begin_stop_all(&rig, &[a, b]).await;
    end_on_term(&rig, &first).await;
    end_on_term(&rig, &second).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    let outcome = OperationOutcome::from_payload(detail.view.journal.result().expect("a result"))
        .expect("an outcome");
    assert!(outcome.resources.contains(&ResourceRef::Instance(a)));
    assert!(outcome.resources.contains(&ResourceRef::Instance(b)));
    assert!(outcome.partial.is_empty());
    for id in [a, b] {
        let row = rig.instance_row(id);
        assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
        assert_eq!(row.desired(), DesiredState::Stopped);
    }
}

#[tokio::test]
async fn a_cancel_lets_the_running_stop_finish_and_skips_the_rest() {
    let rig = Rig::with_config(|config| config.reconcile_start_concurrency = 1).await;
    let a = rig.instance(&spec());
    let b = rig.instance(&spec());
    let first = running(&rig, a, 1).await;
    let _second = running(&rig, b, 2).await;

    let op = begin_stop_all(&rig, &[a, b]).await;
    while rig.processes().signals(first.pid).is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    rig.engine.cancel(op).await.expect("the cancel is accepted");
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    assert!(detail.view.journal.cancel_requested_at().is_some());
    let outcome = OperationOutcome::from_payload(detail.view.journal.result().expect("a result"))
        .expect("an outcome");
    assert_eq!(outcome.resources, vec![ResourceRef::Instance(a)]);
    assert_eq!(outcome.partial.len(), 1);
    assert_eq!(
        outcome.partial[0].code.as_deref(),
        Some("operation.cancelled")
    );
    assert_eq!(rig.instance_row(b).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(rig.instance_row(b).desired(), DesiredState::Running);
}
