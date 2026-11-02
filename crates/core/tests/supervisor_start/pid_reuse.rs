//! A recorded PID that another executable now owns (spec 0030 AC-3):
//! reconciliation, the recheck of `unknown` instances, stop, force stop,
//! and start never signal that process and never take it for the
//! instance's.

use std::sync::Arc;
use std::time::Duration;

use stackly_core::operation::{BeginOutcome, BeginRequest, BusyPolicy, Origin};
use stackly_core::supervisor::{
    DaemonRestartRequest, RegistryMode, RestartHandover, codes, plan_digest,
};
use stackly_domain::{
    DesiredState, IdentityCheck, IdentityCheckName, InstanceBlocker, InstanceId, InstanceLifecycle,
    JournalPayload, LockKey, OperationKind, OperationResource, OperationState, PlanState, Registry,
    RestartPlanId,
};
use stackly_platform::ChildRole;
use stackly_testkit::{FakeProcessState, ProcessOp};
use uuid::Uuid;

use crate::common::{Plant, Rig, Spec};

fn spec(desired: DesiredState) -> Spec {
    Spec {
        lifecycle: InstanceLifecycle::Running,
        desired,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

/// A rig that rechecks often, and an instance whose recorded PID is now a
/// `sleep` of the test.
async fn reused(desired: DesiredState) -> (Rig, InstanceId, stackly_domain::Pid) {
    let rig = Rig::with_config(|config| {
        config.unknown_recheck_interval = Duration::from_millis(100);
    })
    .await;
    let id = rig.instance(&spec(desired));
    let identity = rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.processes().update_facts(identity.pid, |facts| {
        facts.executable = "/usr/bin/sleep".into();
    });
    (rig, id, identity.pid)
}

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn nothing_was_signalled(rig: &Rig, pid: stackly_domain::Pid) {
    assert_eq!(rig.processes().signals(pid), []);
    assert_eq!(rig.processes().count(ProcessOp::Signal), 0);
    assert_eq!(rig.processes().count(ProcessOp::ContinueStopped), 0);
    assert_eq!(
        rig.processes().process_state(pid),
        Some(FakeProcessState::Running),
        "the other process is untouched"
    );
}

#[tokio::test]
async fn a_pid_owned_by_another_executable_is_never_signalled_and_becomes_unknown_with_evidence() {
    // covers: spec 0030 AC-3 (reconciliation, recheck, stop, force stop, start)
    let (rig, id, pid) = reused(DesiredState::Running).await;
    // Someone still holds the data, as a child of the dead server would.
    let _holder = rig.take_lock(id);

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(report.unknown, 1);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);
    let blocker = rig.sup.status(id).blocker.expect("a blocker");
    let InstanceBlocker::DataInUse { evidence, .. } = blocker else {
        panic!("expected data in use, got {blocker:?}");
    };
    assert_eq!(evidence.pid(), Some(pid));
    assert_eq!(
        evidence.check(IdentityCheckName::ExecutableMatches),
        IdentityCheck::Failed,
        "the evidence says which check failed"
    );
    assert_eq!(
        evidence.check(IdentityCheckName::StartMatches),
        IdentityCheck::Passed
    );

    // The recheck looks again and again, and still leaves it alone.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);

    for op in [
        rig.stop(id).await,
        rig.force_stop(id).await,
        rig.start(id).await,
    ] {
        let detail = rig.wait_final(op).await;
        assert_eq!(detail.view.operation.state(), OperationState::Failed);
        assert_eq!(
            Rig::failure_code(&detail),
            codes::IDENTITY_UNPROVEN.to_string()
        );
    }
    rig.sup.start_reconciled();
    tokio::time::sleep(Duration::from_millis(100)).await;

    nothing_was_signalled(&rig, pid);
    assert!(
        rig.processes()
            .spawns()
            .iter()
            .all(|spawn| spawn.role != ChildRole::Main),
        "nothing started"
    );
}

#[tokio::test]
async fn a_pid_owned_by_another_executable_with_a_free_lock_is_gone_and_never_signalled() {
    // covers: spec 0030 AC-3
    let (rig, id, pid) = reused(DesiredState::Stopped).await;

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert!(rig.sup.status(id).process.is_none());
    nothing_was_signalled(&rig, pid);
}

#[tokio::test]
async fn the_recheck_never_signals_a_pid_that_another_executable_took_after_the_first_look() {
    // covers: spec 0030 AC-3
    let rig = Rig::with_config(|config| {
        config.unknown_recheck_interval = Duration::from_millis(100);
    })
    .await;
    let id = rig.instance(&spec(DesiredState::Running));
    let identity = rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);

    // The server is gone, and a `sleep` has its PID.
    rig.processes().update_facts(identity.pid, |facts| {
        facts.executable = "/usr/bin/sleep".into();
    });
    wait_for("the recheck confirms the instance stopped", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;
    rig.sup.start_reconciled();
    tokio::time::sleep(Duration::from_millis(200)).await;

    nothing_was_signalled(&rig, identity.pid);
    assert!(rig.processes().spawns().is_empty(), "no start of its own");
}

/// Ignores the call that tells the daemon it may exit.
struct HandedOver;

impl RestartHandover for HandedOver {
    fn handed_over(&self) {}
}

#[tokio::test]
async fn a_restart_plan_never_signals_a_pid_that_another_executable_took_after_the_adoption() {
    // covers: spec 0030 AC-3 (restart plan)
    let rig = Rig::with_config(|config| config.handover_proven.clear()).await;
    let id = rig.instance(&spec(DesiredState::Running));
    let identity = rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: true,
        },
    );
    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    assert_eq!(report.adopted, 1);
    // The server ended and a `sleep` took its PID before the plan stops it:
    // another executable, started later.
    rig.processes().update_facts(identity.pid, |facts| {
        facts.executable = "/usr/bin/sleep".into();
        facts.start = stackly_domain::ProcessStart::new(1_760_000_999, 1)
            .expect("a start time of the reused PID");
    });
    let entries = rig.sup.restart_plan_entries().await.unwrap();
    assert_eq!(entries.len(), 1);
    let (prepared, _written) = tokio::sync::oneshot::channel();
    let handover = Arc::new(HandedOver);
    let work = rig.sup.daemon_restart_work(DaemonRestartRequest::new(
        RestartPlanId::generate(),
        plan_digest(&entries),
        Uuid::now_v7(),
        prepared,
        handover,
    ));
    let outcome = rig
        .engine
        .begin(BeginRequest {
            kind: OperationKind::DaemonRestart,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: vec![LockKey::for_resource(OperationResource::Instance(id))],
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work,
            precheck: None,
        })
        .await
        .expect("the engine accepts the restart");
    let BeginOutcome::Created(view) = outcome else {
        panic!("expected a created operation, got {outcome:?}");
    };

    let detail = rig.wait_final(view.operation.id()).await;

    // The plan cannot prove the process is the server, so its stop does
    // not finish, and the plan aborts; the process that owns the PID now
    // is never signalled.
    assert_eq!(detail.view.operation.state(), OperationState::Failed);
    // The platform refuses a signal to a process with another start time,
    // so the other process received nothing and still runs.
    assert_eq!(rig.processes().signals(identity.pid), []);
    assert_eq!(rig.processes().count(ProcessOp::ForceKill), 0);
    assert_eq!(
        rig.processes().process_state(identity.pid),
        Some(FakeProcessState::Running)
    );
    let plan = rig
        .registry
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan was written");
    assert_eq!(plan.state(), PlanState::Aborted);
    assert!(plan.instance(id).expect("in the plan").stopped_at.is_none());
}
