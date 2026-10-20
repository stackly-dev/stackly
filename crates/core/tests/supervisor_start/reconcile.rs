//! Reconciliation after a daemon restart (spec 0010 AC-9, AC-10, AC-11,
//! AC-21, AC-22): the table over processes the fake planted, adoption of a
//! running fake, the recheck of `unknown` instances, and the shutdown that
//! signals nothing and commits nothing.

use std::time::Duration;

use stackly_core::supervisor::{ReconcilePhase, RegistryMode, codes};
use stackly_domain::{
    DesiredState, InstanceBlocker, InstanceId, InstanceLifecycle, OperationState, ReadinessState,
    RegistryError,
};
use stackly_platform::{ChildRole, ExitStatus, HoldsFile, ProviderSignal};
use stackly_testkit::{FakeProcessState, ProcessOp};

use crate::common::{Plant, Rig, Spec};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn main_spawns(rig: &Rig) -> usize {
    rig.processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count()
}

fn spec(lifecycle: InstanceLifecycle, desired: DesiredState) -> Spec {
    Spec {
        lifecycle,
        desired,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

/// Starts the instance and waits until it is `ready`, leaving the real
/// server of the fake process up.
async fn running(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

#[tokio::test]
async fn a_killed_daemon_leaves_the_process_and_the_next_daemon_adopts_it_without_a_spawn() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;
    let before = rig.instance_row(id);

    rig.sup.shutdown();
    let next = rig.reborn();
    let report = next.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(report.adopted, 1);
    assert_eq!(report.instances, 1);
    assert_eq!(report.unknown, 0);
    let status = next.status(id);
    let process = status.process.expect("the process is adopted");
    assert_eq!(process.pid, spawn.pid);
    assert!(process.adopted);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(
        rig.instance_row(id).revision(),
        before.revision(),
        "an adoption of a running instance writes nothing"
    );
    assert_eq!(main_spawns(&rig), 1, "no new spawn");
    assert_eq!(rig.processes().count(ProcessOp::Resume), 1);
    assert_eq!(rig.processes().signals(spawn.pid), []);
    wait_for("readiness returns at the stored port", || {
        let status = next.status(id);
        status.readiness.state() == ReadinessState::Ready && status.ports_final
    })
    .await;
    let phases = rig.events.reconciling.lock().unwrap().clone();
    assert!(matches!(phases[0], ReconcilePhase::Started));
    assert!(matches!(phases[1], ReconcilePhase::Finished(counts) if counts.adopted == 1));
}

#[tokio::test]
async fn the_log_of_an_adopted_process_goes_on_from_the_spool_with_a_resumed_batch() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;
    rig.processes()
        .write_output(
            spawn.pid,
            stackly_platform::OutputStream::Stdout,
            b"before\nhalf",
        )
        .unwrap();
    rig.sup.shutdown();

    let next = rig.reborn();
    next.reconcile(RegistryMode::Ready).await.unwrap();
    rig.processes()
        .write_output(
            spawn.pid,
            stackly_platform::OutputStream::Stdout,
            b" line\nafter\n",
        )
        .unwrap();
    wait_for("the log goes on", || {
        rig.logs.lines_of(id).iter().any(|line| line == "after")
    })
    .await;

    let batches = rig.logs.batches_of(id);
    assert!(
        batches.iter().any(|batch| batch.resumed),
        "the first batch after the restart is marked resumed"
    );
    assert!(!next.status(id).logs_unavailable);
}

#[tokio::test]
async fn a_process_held_back_before_its_first_instruction_is_continued_then_adopted() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Starting, DesiredState::Running));
    let identity = rig.plant(
        id,
        Plant {
            stopped: true,
            holds_lock: true,
        },
    );

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(report.adopted, 1);
    assert_eq!(rig.processes().count(ProcessOp::ContinueStopped), 1);
    assert_eq!(
        rig.processes().process_state(identity.pid),
        Some(FakeProcessState::Running)
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(rig.processes().signals(identity.pid), []);
}

#[tokio::test]
async fn a_vanished_process_on_the_same_boot_crashes_and_respawns_once_the_engine_runs() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(report.restarting, 1);
    assert_eq!(report.adopted, 0);
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Recovering);
    assert_eq!(
        row.last_failure().map(|code| code.as_str()),
        Some("exit_status_unknown")
    );
    assert_eq!(main_spawns(&rig), 0, "nothing runs before the engine does");

    rig.sup.start_reconciled();
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    wait_for("the respawn is ready", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
            && rig.sup.status(id).readiness.state() == ReadinessState::Ready
    })
    .await;
    assert_eq!(main_spawns(&rig), 1);
}

#[tokio::test]
async fn a_new_boot_confirms_stopped_and_never_starts_the_instance() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(id, Plant::live());
    rig.processes()
        .set_boot_id("0b1c2d3e-4f50-4617-8829-3a4b5c6d7e8f".parse().unwrap());
    rig.processes().exit(identity.pid, None);

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    rig.sup.start_reconciled();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(report.restarting, 0);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(main_spawns(&rig), 0);
    assert_eq!(
        rig.processes().count(ProcessOp::Inspect),
        0,
        "the facts of a process from another boot are never read"
    );
    assert_eq!(rig.processes().count(ProcessOp::HoldsFile), 0);
}

#[tokio::test]
async fn a_starting_instance_that_was_interrupted_fails_and_starts_again_when_wanted() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Starting, DesiredState::Running));
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        row.last_failure().map(|code| code.as_str()),
        Some("interrupted")
    );
    rig.sup.start_reconciled();
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    wait_for("the system start ends ready", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;
    let starts = rig
        .operations_of(id, stackly_domain::OperationKind::StartInstance)
        .await;
    assert!(
        starts
            .iter()
            .any(|view| view.journal.input().as_str().contains("reconcile")),
        "the start carries the trigger reconcile"
    );
}

#[tokio::test]
async fn an_initializing_instance_always_fails_as_interrupted() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(
        InstanceLifecycle::Initializing,
        DesiredState::Stopped,
    ));

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        row.last_failure().map(|code| code.as_str()),
        Some("interrupted")
    );
}

#[tokio::test]
async fn a_pid_reused_by_another_process_is_gone_and_never_signalled() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Stopped));
    let identity = rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.processes().update_facts(identity.pid, |facts| {
        facts.start = stackly_domain::ProcessStart::new(1_800_000_000, 5).unwrap();
    });

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(rig.processes().count(ProcessOp::Signal), 0);
    assert_eq!(rig.processes().count(ProcessOp::ContinueStopped), 0);
    assert_eq!(
        rig.processes().count(ProcessOp::HoldsFile),
        0,
        "the lock holders are not read when another check failed"
    );
    assert_eq!(
        rig.processes().process_state(identity.pid),
        Some(FakeProcessState::Running),
        "the other process is untouched"
    );
}

#[tokio::test]
async fn a_pid_reused_while_the_lock_is_held_is_unknown_with_data_in_use() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
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
    let _other = rig.take_lock(id);

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(report.unknown, 1);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);
    let blocker = rig.sup.status(id).blocker.expect("a blocker");
    let InstanceBlocker::DataInUse { evidence, .. } = blocker else {
        panic!("expected data in use, got {blocker:?}");
    };
    assert_eq!(evidence.pid(), Some(identity.pid));
    assert_eq!(rig.processes().count(ProcessOp::Signal), 0);
}

#[tokio::test]
async fn a_matching_process_that_does_not_hold_the_lock_is_unproven_and_left_alone() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(
        id,
        Plant {
            stopped: true,
            holds_lock: false,
        },
    );

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);
    let blocker = rig.sup.status(id).blocker.expect("a blocker");
    let InstanceBlocker::IdentityUnproven { evidence, .. } = blocker else {
        panic!("expected identity unproven, got {blocker:?}");
    };
    assert!(
        evidence.stopped(),
        "the evidence says the process is held back"
    );
    assert_eq!(rig.processes().count(ProcessOp::ContinueStopped), 0);
    assert_eq!(rig.processes().count(ProcessOp::Discard), 0);
    assert_eq!(
        rig.processes().process_state(identity.pid),
        Some(FakeProcessState::Suspended)
    );
}

#[tokio::test]
async fn an_unknown_instance_refuses_a_start_and_a_stop() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    let start = rig.start(id).await;
    let stop = rig.stop(id).await;
    let started = rig.wait_final(start).await;
    let stopped = rig.wait_final(stop).await;

    for detail in [started, stopped] {
        assert_eq!(detail.view.operation.state(), OperationState::Failed);
        assert_eq!(
            Rig::failure_code(&detail),
            codes::IDENTITY_UNPROVEN.to_string()
        );
    }
    assert_eq!(rig.processes().count(ProcessOp::Signal), 0);
}

#[tokio::test]
async fn an_error_while_gathering_the_evidence_is_unproven_and_never_gone() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    rig.plant(id, Plant::live());
    rig.processes().fail_next(
        ProcessOp::Inspect,
        stackly_platform::ProcessError::PermissionDenied {
            pid: stackly_domain::Pid::new(1).unwrap(),
        },
    );

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);
    assert!(matches!(
        rig.sup.status(id).blocker,
        Some(InstanceBlocker::IdentityUnproven { .. })
    ));
}

#[tokio::test]
async fn a_proven_process_on_a_stopped_instance_changes_nothing_but_the_blocker() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    let identity = rig.plant(id, Plant::live());
    let before = rig.instance_row(id);

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(rig.instance_row(id), before);
    assert!(matches!(
        rig.sup.status(id).blocker,
        Some(InstanceBlocker::DataInUse { .. })
    ));
    assert!(rig.sup.status(id).process.is_none(), "it is not adopted");
    assert_eq!(rig.processes().signals(identity.pid), []);
}

#[tokio::test]
async fn a_stopping_instance_is_stopped_again_when_its_process_is_still_there() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopping, DesiredState::Stopped));
    let identity = rig.plant(id, Plant::live());

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    assert_eq!(report.adopted, 1);
    rig.sup.start_reconciled();
    wait_for("the stop signal is sent again", || {
        !rig.processes().signals(identity.pid).is_empty()
    })
    .await;
    assert_eq!(
        rig.processes().signals(identity.pid),
        [ProviderSignal::Term]
    );
    rig.processes().exit(identity.pid, None);

    wait_for("the instance stops", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;
}

#[tokio::test]
async fn a_stopping_instance_whose_process_is_gone_ends_stopped() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopping, DesiredState::Stopped));
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
}

#[tokio::test]
async fn a_running_instance_that_should_be_stopped_gets_a_system_stop() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Stopped));
    let identity = rig.plant(id, Plant::live());

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    rig.sup.start_reconciled();
    wait_for("the stop signal arrives", || {
        !rig.processes().signals(identity.pid).is_empty()
    })
    .await;
    rig.processes().exit(identity.pid, None);

    wait_for("the instance stops", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;
    let stops = rig
        .operations_of(id, stackly_domain::OperationKind::StopInstance)
        .await;
    assert!(
        stops
            .iter()
            .any(|view| view.journal.input().as_str().contains("reconcile"))
    );
}

#[tokio::test]
async fn the_registry_in_recovery_mode_is_not_touched_at_all() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    rig.plant(id, Plant::live());
    let calls = rig.processes().calls().len();

    let report = rig.sup.reconcile(RegistryMode::Recovery).await.unwrap();

    assert!(report.skipped);
    assert_eq!(
        rig.processes().calls().len(),
        calls,
        "no process was looked at"
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert!(rig.events.reconciling.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reconciliation_sweeps_the_secret_files_of_stopped_instances_and_learns_the_running_ones() {
    let rig = Rig::new().await;
    let stopped = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    let live = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    rig.plant(live, Plant::live());
    let secrets_dir = |id: InstanceId| {
        stackly_core::ensure_instance_run_dir(&rig.platform, &rig.layout, id.short_key())
            .unwrap()
            .join("secrets")
    };
    for id in [stopped, live] {
        std::fs::create_dir_all(secrets_dir(id)).unwrap();
        std::fs::write(secrets_dir(id).join("authToken"), "x").unwrap();
    }
    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();

    assert!(!secrets_dir(stopped).exists(), "swept");
    assert!(secrets_dir(live).exists(), "kept for a running instance");
}

#[tokio::test]
async fn an_unknown_instance_is_adopted_by_the_recheck_once_its_process_is_proven() {
    let rig = Rig::with_config(|config| {
        config.unknown_recheck_interval = Duration::from_millis(100);
    })
    .await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);

    rig.processes().set_holds(identity.pid, HoldsFile::Yes);
    wait_for("the recheck adopts the process", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;

    assert!(rig.sup.status(id).process.is_some_and(|p| p.adopted));
    assert!(rig.sup.status(id).blocker.is_none());
    assert_eq!(rig.processes().signals(identity.pid), []);
}

#[tokio::test]
async fn an_unknown_instance_whose_process_vanished_is_confirmed_stopped_and_not_started() {
    let rig = Rig::with_config(|config| {
        config.unknown_recheck_interval = Duration::from_millis(100);
    })
    .await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Unknown);

    rig.processes().exit(identity.pid, None);
    wait_for("the recheck confirms it stopped", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Stopped
    })
    .await;

    rig.sup.start_reconciled();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(main_spawns(&rig), 0, "no start of its own");
}

#[tokio::test]
async fn a_data_in_use_blocker_goes_when_the_lock_is_free_again() {
    let rig = Rig::with_config(|config| {
        config.unknown_recheck_interval = Duration::from_millis(100);
    })
    .await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    let held = rig.take_lock(id);

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    assert!(matches!(
        rig.sup.status(id).blocker,
        Some(InstanceBlocker::DataInUse { .. })
    ));
    drop(held);

    wait_for("the blocker goes", || rig.sup.status(id).blocker.is_none()).await;
}

#[tokio::test]
async fn a_shutdown_signals_nothing_commits_nothing_and_the_next_daemon_sees_the_exit() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    wait_for("the process runs", || rig.sup.status(id).process.is_some()).await;
    let before = rig.instance_row(id);

    rig.sup.shutdown();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(9)));
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(rig.processes().signals(spawn.pid), []);
    assert_eq!(
        rig.instance_row(id),
        before,
        "the exit of a shut down daemon commits nothing"
    );
    let _ = op;
    let next = rig.reborn();
    next.reconcile(RegistryMode::Ready).await.unwrap();
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Recovering,
        "the next daemon finds the process gone"
    );
}

#[tokio::test]
async fn a_commit_the_registry_refuses_is_retried_and_nothing_respawns_meanwhile() {
    let rig = Rig::with_config(|config| {
        config.registry_retry_interval = Duration::from_millis(150);
        config.restart_base_delay = Duration::from_millis(20);
    })
    .await;
    let id = rig.instance(&Spec::default());
    let first = running(&rig, id).await;
    rig.kill_servers();

    rig.registry.fail_commits(vec![
        RegistryError::RevisionConflict {
            resource: stackly_domain::ResourceRef::Instance(id),
            expected: stackly_domain::ResourceRevision::new(1).unwrap(),
            actual: stackly_domain::ResourceRevision::new(9).unwrap(),
        },
        RegistryError::NoSpace,
        RegistryError::NoSpace,
    ]);
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));

    wait_for("the subsystem degrades", || {
        rig.events.degraded.lock().unwrap().contains(&true)
    })
    .await;
    assert!(
        rig.sup.status(id).pending_commit,
        "the state in memory is shown while the commit waits"
    );
    assert_eq!(main_spawns(&rig), 1, "no spawn while the commit is pending");
    assert_eq!(rig.processes().count(ProcessOp::Signal), 0);

    let second = rig.main_spawn(2).await;
    assert_ne!(second.pid, first.pid);
    assert_eq!(
        rig.events.degraded.lock().unwrap().last(),
        Some(&false),
        "the subsystem recovers once the registry takes the commit"
    );
    assert!(!rig.sup.status(id).pending_commit);
}
