//! Stopping safely (spec 0010 AC-5): the provider's strategy only, never
//! `SIGKILL`, the identity checked again right before a signal, a timeout
//! that leaves the instance `stopping` and still watched, and the exit
//! handler that ends every stop.

use std::time::Duration;

use stackly_core::supervisor::codes;
use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, OperationKind, OperationState, Pid,
};
use stackly_platform::{ChildRole, ExitStatus, ProcessFacts, ProviderSignal};
use stackly_testkit::ProcessOp;
use stackly_testkit::evidence::{Kind, evidence};

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
    wait_for("the instance stops and its process is forgotten", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
            && rig.sup.status(id).process.is_none()
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
async fn a_stop_that_finds_its_process_already_gone_after_the_lock_ends_stopped() {
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
    // The second stop has read the instance `stopping` when it waits at the
    // door of its `lock` stage; the process exits and its exit is handled
    // to the end while it waits.
    let door = rig.registry.hold_stage(OperationKind::StopInstance, "lock");
    let second = rig.stop(id).await;
    wait_for("the second stop reaches the lock stage", || door.entered()).await;

    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    wait_for("the exit is handled to the end", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
            && rig.sup.status(id).process.is_none()
    })
    .await;
    door.release();

    rig.wait_state(second, OperationState::Succeeded).await;
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Stopped);
    assert_eq!(
        rig.processes().signals(spawn.pid),
        [ProviderSignal::Term],
        "the second stop signalled nothing"
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

/// provider.yaml of the fake with a stop command in place of the signal.
fn with_stop_command(text: &str, stop_seconds: u32) -> String {
    let text = replace(
        text,
        "  stop: { strategy: gracefulProcess, signal: term }\n",
        "  stop:\n    strategy: command\n    command:\n      executable: \"${runtime.executable}\"\n      args: [shutdown, \"--port=${endpoint.http.port}\"]\n    successExitCodes: [0]\n",
    );
    replace(
        &text,
        "stopSeconds: 10",
        &format!("stopSeconds: {stop_seconds}"),
    )
}

#[tokio::test]
async fn ac_5_a_stop_command_past_its_timeout_fails_with_stop_timeout_and_the_instance_stays_stopping()
 {
    let ev = evidence!(
        covers = ["AC-011", "REQ-011"],
        kind = Kind::Fixture,
        expected = "a provider stop command that hangs past stopSeconds gets TERM then KILL to its own group only, the operation fails with supervisor.stop_timeout, no signal reaches the main process, the instance stays stopping and watched, and the exit of the main process ends it stopped",
    );
    let rig = Rig::patched(|text| with_stop_command(text, 1)).await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    let op = rig.stop(id).await;
    // The command hangs: it gets TERM, then KILL after the grace, and only
    // then does its exit arrive.
    let command = rig.helper_spawn(ChildRole::StopCommand, 0).await;
    assert!(
        command.args.first().is_some_and(|arg| arg == "shutdown"),
        "the provider's stop command ran: {:?}",
        command.args
    );
    wait_for("the helper rule runs", || {
        rig.processes()
            .calls()
            .into_iter()
            .filter(|call| call.op == ProcessOp::TerminateHelperGroup)
            .map(|call| call.force)
            .collect::<Vec<_>>()
            == [Some(false), Some(true)]
    })
    .await;
    rig.processes()
        .exit(command.pid, Some(ExitStatus::Signaled(9)));
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), codes::STOP_TIMEOUT.to_string());
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Stopping
    );
    assert!(
        rig.processes().signals(spawn.pid).is_empty(),
        "a command strategy sends no signal to the main process"
    );
    assert!(
        rig.sup.status(id).process.is_some(),
        "the process is still watched"
    );

    // The exit of the main process ends the stop through the one handler.
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    wait_for("the instance stops and its process is forgotten", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
            && rig.sup.status(id).process.is_none()
    })
    .await;
    assert!(rig.sup.status(id).process.is_none());
    ev.pass();
}

#[tokio::test]
async fn ac_5_a_stop_command_that_succeeds_waits_for_the_exit_without_any_signal() {
    let rig = Rig::patched(|text| with_stop_command(text, 10)).await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    let op = rig.stop(id).await;
    let command = rig.helper_spawn(ChildRole::StopCommand, 0).await;
    rig.processes()
        .exit(command.pid, Some(ExitStatus::Exited(0)));
    // The command alone does not end the stop: the process must exit.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Stopping
    );
    assert!(
        !rig.engine
            .get(op)
            .await
            .unwrap()
            .unwrap()
            .view
            .operation
            .state()
            .is_final()
    );
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(op, OperationState::Succeeded).await;

    assert!(rig.processes().signals(spawn.pid).is_empty());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert!(
        rig.processes()
            .calls()
            .iter()
            .all(|call| call.force != Some(true)),
        "no SIGKILL to anything"
    );
}

#[tokio::test]
async fn ac_5_a_stop_command_that_exits_with_another_code_fails_and_signals_nothing() {
    let rig = Rig::patched(|text| with_stop_command(text, 10)).await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    let op = rig.stop(id).await;
    let command = rig.helper_spawn(ChildRole::StopCommand, 0).await;
    rig.processes()
        .exit(command.pid, Some(ExitStatus::Exited(2)));
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::STOP_COMMAND_FAILED.to_string()
    );
    assert!(rig.processes().signals(spawn.pid).is_empty());
    assert!(rig.sup.status(id).process.is_some());
    rig.kill_servers();
}
