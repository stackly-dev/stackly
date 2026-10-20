//! Stopping safely (spec 0010 AC-5): the provider's strategy only, never
//! `SIGKILL`, the identity checked again right before a signal, a timeout
//! that leaves the instance `stopping` and still watched, and the exit
//! handler that ends every stop.

use std::time::Duration;

use stackly_core::supervisor::codes;
use stackly_domain::{DesiredState, InstanceId, InstanceLifecycle, OperationState, Pid};
use stackly_platform::{ExitStatus, ProcessFacts, ProviderSignal};
use stackly_testkit::ProcessOp;

use crate::common::{Rig, Spec, replace};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Starts the instance on the dummy server and returns its fake PID.
async fn running(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

#[tokio::test]
async fn a_stop_sends_term_waits_for_the_exit_and_never_kills() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;
    // Material a start rendered into the secrets folder is removed on a
    // clean stop.
    let secrets = rig.layout.layout().instance_secrets_dir(id.short_key());
    std::fs::create_dir_all(&secrets).unwrap();
    std::fs::write(secrets.join("token"), b"x").unwrap();

    let op = rig.stop(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(spawn.pid).is_empty()
    })
    .await;
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Stopping
    );
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    let stages: Vec<_> = detail
        .steps
        .iter()
        .map(|step| step.name().as_str())
        .collect();
    assert_eq!(stages, ["validate", "lock", "journal", "stop", "complete"]);
    assert_eq!(rig.processes().signals(spawn.pid), [ProviderSignal::Term]);
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert!(
        !secrets.exists(),
        "the secret material is removed after the exit"
    );
    let status = rig.sup.status(id);
    assert!(status.process.is_none());
    assert_eq!(rig.processes().count(ProcessOp::Discard), 0);
    assert!(
        rig.processes()
            .calls()
            .iter()
            .all(|call| call.force != Some(true)),
        "no SIGKILL to anything"
    );
}

#[tokio::test]
async fn a_stop_past_its_budget_fails_keeps_stopping_and_the_exit_ends_it_later() {
    let rig = Rig::patched(|text| replace(text, "stopSeconds: 10", "stopSeconds: 1")).await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    let first = rig.stop(id).await;
    let detail = rig.wait_state(first, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), codes::STOP_TIMEOUT.to_string());
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Stopping
    );
    assert_eq!(rig.processes().signals(spawn.pid), [ProviderSignal::Term]);

    // A second stop sends the strategy again with a new budget.
    let second = rig.stop(id).await;
    let detail = rig.wait_state(second, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), codes::STOP_TIMEOUT.to_string());
    assert_eq!(
        rig.processes().signals(spawn.pid),
        [ProviderSignal::Term, ProviderSignal::Term]
    );

    // The process finally exits on its own: the same handler stops it.
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    wait_for("the instance stops", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;
    assert!(rig.sup.status(id).process.is_none());
    assert!(
        rig.processes()
            .calls()
            .iter()
            .all(|call| call.force != Some(true))
    );
}

#[tokio::test]
async fn a_changed_identity_refuses_the_signal_and_the_instance_becomes_unknown() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;
    // The PID now belongs to a process that started later.
    rig.processes().update_facts(spawn.pid, |facts| {
        facts.start = "9999999999.000001".parse().unwrap();
    });

    let op = rig.stop(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::SIGNAL_REFUSED.to_string()
    );
    assert!(
        rig.processes().signals(spawn.pid).is_empty(),
        "no signal reached the changed identity"
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);
    assert!(rig.sup.status(id).blocker.is_some());

    // An unknown instance refuses stop and start.
    let again = rig.stop(id).await;
    let detail = rig.wait_state(again, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        codes::IDENTITY_UNPROVEN.to_string()
    );
    let start = rig.start(id).await;
    let detail = rig.wait_state(start, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        codes::IDENTITY_UNPROVEN.to_string()
    );
}

#[tokio::test]
async fn members_left_in_the_group_after_the_exit_are_reported_and_not_signalled() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;
    let child = Pid::new(spawn.pid.get() + 1000).unwrap();
    let main = rig
        .processes()
        .spawns()
        .into_iter()
        .find(|candidate| candidate.pid == spawn.pid)
        .unwrap();
    let _ = main;
    rig.processes().insert(ProcessFacts {
        pid: child,
        parent: Some(spawn.pid),
        group: spawn.pid,
        session: spawn.pid,
        uid: rig.processes().user(),
        start: "5.000000".parse().unwrap(),
        executable: "/bin/sleep".into(),
        stopped: false,
    });

    let op = rig.stop(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(spawn.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.sup.status(id).leftover, [child]);
    assert!(
        rig.processes().signals(child).is_empty(),
        "a leftover member is reported, never signalled"
    );
}

#[tokio::test]
async fn a_stop_of_a_stopped_instance_only_stores_the_desired_state() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        desired: DesiredState::Running,
        ..Spec::default()
    });

    let op = rig.stop(id).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert!(rig.processes().calls().is_empty(), "nothing was signalled");
}
