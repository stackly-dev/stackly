//! The startup transaction on the fake provider (spec 0010 AC-2, AC-3,
//! AC-6, AC-8, AC-17, AC-18).

use std::net::TcpListener;
use std::time::Duration;

use stackly_core::supervisor::codes;
use stackly_domain::{DesiredState, InstanceLifecycle, OperationState, ReadinessState, Registry};
use stackly_platform::{
    ChildRole, ExitStatus, HoldsFile, LockOutcome, ProcessSupervision, ProviderSignal,
};
use stackly_testkit::{FakeProcessState, ProcessOp};

use crate::common::{LOOPBACK, Rig, Spec, config_file, replace};

const STAGES: [&str; 11] = [
    "validate",
    "lock",
    "verify",
    "endpoints",
    "journal",
    "render",
    "spawn",
    "identity",
    "readiness",
    "publish",
    "complete",
];

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_start_runs_every_stage_and_ends_ready_with_final_ports() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    let port = rig.serve(&spawn);
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    let stages: Vec<_> = detail
        .steps
        .iter()
        .map(|step| step.name().as_str())
        .collect();
    assert_eq!(stages, STAGES);
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(instance.desired(), DesiredState::Running);
    let status = rig.sup.status(id);
    assert_eq!(status.readiness.state(), ReadinessState::Ready);
    assert!(status.ports_final, "the port shows as final after ready");
    assert_eq!(status.process.as_ref().map(|p| p.pid), Some(spawn.pid));
    assert_eq!(
        status.process.as_ref().and_then(|p| p.adoptable),
        Some(true),
        "the process holds the instance lock"
    );
    // The auto port was stored when the process spawned.
    let bound = match &instance.endpoints()[0] {
        stackly_domain::Endpoint::Ip(ip) => ip.bound().map(|port| port.get()),
        stackly_domain::Endpoint::Unix(_) => None,
    };
    assert_eq!(bound, Some(port));
    // The identity is recorded before the process ran.
    let observation = rig
        .registry
        .latest_process_observation(id)
        .unwrap()
        .value
        .expect("an observation was recorded");
    assert_eq!(observation.identity.pid, spawn.pid);

    // The argv is exactly the plan's, the env is closed, the lock rides at
    // fd 3, and the output goes to the two spools.
    assert_eq!(spawn.args[0], "serve");
    assert_eq!(spawn.args[3], format!("--port={port}"));
    assert!(spawn.args.contains(&"--host=127.0.0.1".to_owned()));
    assert!(spawn.env.iter().any(|(name, _)| name == "PATH"));
    assert!(spawn.lock.is_some());
    assert!(spawn.spools.is_some());
    assert_eq!(spawn.stdin_len, None);
    assert_eq!(spawn.role, ChildRole::Main);
    assert!(
        config_file(&rig.paths(id)).is_file(),
        "the config was rendered"
    );
    assert_eq!(
        rig.processes().process_state(spawn.pid),
        Some(FakeProcessState::Running)
    );
    let discards = rig.processes().count(ProcessOp::Discard);
    assert_eq!(discards, 0);
    assert_eq!(
        rig.events.lifecycle_names(id),
        ["starting", "running"],
        "starting, then running"
    );
}

#[tokio::test]
async fn a_slow_server_reads_running_but_not_ready_until_its_probe_answers() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        settings: serde_json::json!({ "readyDelayMs": 700 }),
        ..Spec::default()
    });

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    wait_for("the process is running", || {
        rig.sup.status(id).process.is_some()
    })
    .await;
    let early = rig.sup.status(id);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(early.readiness.state(), ReadinessState::NotReady);
    assert!(!early.ports_final);

    rig.wait_state(op, OperationState::Succeeded).await;
    assert_eq!(rig.sup.status(id).readiness.state(), ReadinessState::Ready);
}

#[tokio::test]
async fn a_start_on_a_running_instance_changes_nothing_and_ten_starts_spawn_once() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(first, OperationState::Succeeded).await;

    let mut more = Vec::new();
    for _ in 0..10 {
        more.push(rig.start(id).await);
    }
    for op in more {
        rig.wait_state(op, OperationState::Succeeded).await;
    }

    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1, "one spawn for eleven starts");
}

#[tokio::test]
async fn a_held_instance_lock_refuses_the_start_without_a_spawn() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    // Another process holds the lock, as a leftover child of an old daemon.
    let LockOutcome::Acquired(lock) = rig.processes().lock_instance(&rig.paths(id).lock).unwrap()
    else {
        panic!("the lock is free");
    };
    let other = stackly_platform::ProcessFacts {
        pid: stackly_domain::Pid::new(777_001).unwrap(),
        parent: None,
        group: stackly_domain::Pid::new(777_001).unwrap(),
        session: stackly_domain::Pid::new(777_001).unwrap(),
        uid: rig.processes().user(),
        start: "1.000000".parse().unwrap(),
        executable: "/bin/sleep".into(),
        stopped: false,
    };
    let pid = other.pid;
    rig.processes().insert(other);
    rig.processes().hand_lock(pid, lock);

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), codes::DATA_IN_USE.to_string());
    assert!(rig.processes().spawns().is_empty());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
}

#[tokio::test]
async fn a_live_process_of_the_last_observation_refuses_the_start_even_without_the_lock() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    // The user deletes the lock file, and the instance is made `stopped`
    // behind the supervisor's back, so another start looks possible.
    std::fs::remove_file(rig.paths(id).lock).unwrap();
    let mut changes = stackly_core::supervisor::InstanceChanges::new(rig.instance_row(id));
    for event in [
        stackly_domain::LifecycleEvent::StopBegin,
        stackly_domain::LifecycleEvent::Exited,
    ] {
        changes
            .apply(|instance| instance.apply_lifecycle(event, rig.fake_now()).map(|_| ()))
            .unwrap();
    }
    rig.registry
        .commit(stackly_domain::WriteBatch::new(changes.into_ops()))
        .unwrap();

    let second = rig.start(id).await;
    let detail = rig.wait_state(second, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), codes::DATA_IN_USE.to_string());
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1, "no second process on the same data");
    let _ = (ProviderSignal::Term, HoldsFile::Yes);
}

#[tokio::test]
async fn a_fixed_port_in_use_fails_before_any_spawn_and_leaves_the_other_listener_alone() {
    let rig = Rig::new().await;
    let listener = TcpListener::bind((LOOPBACK, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let id = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_IN_USE.to_string()
    );
    let error = detail.view.journal.failure_error().unwrap();
    let keys: Vec<_> = error
        .details()
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect();
    assert!(
        keys.contains(&("endpoint".to_owned(), "http".to_owned())),
        "{keys:?}"
    );
    assert!(
        keys.contains(&("port".to_owned(), port.to_string())),
        "{keys:?}"
    );
    assert!(rig.processes().spawns().is_empty());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Stopped);
    drop(listener);
    let _ = replace;
}

#[tokio::test]
async fn a_port_the_os_refuses_fails_before_the_journal_with_endpoint_unavailable() {
    let rig = Rig::new().await;
    let port = Rig::free_port();
    rig.ports
        .answer(port, stackly_core::supervisor::BindOutcome::Denied);
    let id = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_UNAVAILABLE.to_string()
    );
    let reason = detail
        .view
        .journal
        .failure_error()
        .unwrap()
        .details()
        .iter()
        .find(|(key, _)| key.as_str() == "reason")
        .map(|(_, value)| value.clone());
    assert_eq!(reason.as_deref(), Some("denied"));
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert!(rig.processes().spawns().is_empty());
}

#[tokio::test]
async fn an_auto_endpoint_takes_its_stored_port_again_when_it_is_free_and_a_new_one_when_not() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let first = rig.main_spawn(1).await;
    let port = rig.serve(&first);
    rig.wait_state(op, OperationState::Succeeded).await;
    let stop = rig.stop(id).await;
    let term_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while rig.processes().signals(first.pid).is_empty() {
        assert!(tokio::time::Instant::now() < term_deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(stop, OperationState::Succeeded).await;

    let op = rig.start(id).await;
    let second = rig.main_spawn(2).await;
    assert!(
        second.args.contains(&format!("--port={port}")),
        "the stored port is free, so it is used again"
    );
    rig.serve(&second);
    rig.wait_state(op, OperationState::Succeeded).await;
    let stop = rig.stop(id).await;
    let term_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while rig.processes().signals(second.pid).is_empty() {
        assert!(tokio::time::Instant::now() < term_deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    rig.kill_servers();
    rig.processes()
        .exit(second.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(stop, OperationState::Succeeded).await;

    // Somebody takes the stored port: a new one is picked.
    let _taken = TcpListener::bind((LOOPBACK, port)).unwrap();
    let op = rig.start(id).await;
    let third = rig.main_spawn(3).await;
    assert!(
        !third.args.contains(&format!("--port={port}")),
        "the stored port is taken: {:?}",
        third.args
    );
    rig.serve(&third);
    rig.wait_state(op, OperationState::Succeeded).await;
}

#[tokio::test]
async fn a_start_checks_the_package_the_folder_the_marker_and_the_runtime_first() {
    // The pinned package changed on disk: its digest no longer matches.
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let dir = rig.layout.layout().provider_revision_dir(rig.key());
    let manifest = dir.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, format!("{text}\n# changed\n")).unwrap();
    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        codes::PACKAGE_UNAVAILABLE.to_string()
    );

    // The data marker names another line than the instance records.
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        marker: "9.9\n",
        ..Spec::default()
    });
    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        codes::DATA_INCOMPATIBLE.to_string()
    );

    // The instance folder is open to others.
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(rig.paths(id).root, std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        codes::DATA_UNAVAILABLE.to_string()
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert!(rig.processes().spawns().is_empty());
}

#[tokio::test]
async fn a_missing_secret_blocks_the_start_without_any_lifecycle_change() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        token: None,
        ..Spec::default()
    });

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "secret.needs_secret");
    assert!(rig.sup.status(id).blocker.is_some());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Stopped);
    assert!(rig.processes().spawns().is_empty());
}

#[tokio::test]
async fn a_runtime_that_is_not_installed_refuses_the_start() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    // Replace the runtime row by one that is `failed`.
    let broken = stackly_testkit::RuntimeBuilder::new()
        .with_id(rig.runtime.id())
        .software("fake")
        .version("1.0.0", "1.0")
        .state(stackly_domain::RuntimeState::Failed)
        .revision(2)
        .install(rig.runtime.install().clone())
        .build();
    rig.registry
        .commit(stackly_domain::WriteBatch::new(vec![
            stackly_domain::WriteOp::UpdateRuntime {
                runtime: broken,
                expected: rig.runtime.revision(),
            },
        ]))
        .unwrap();

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::RUNTIME_UNAVAILABLE.to_string()
    );
}

#[tokio::test]
async fn a_crash_loop_spawns_six_times_then_fails_with_the_budget_spent() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    for attempt in 1..=6 {
        let spawn = rig.main_spawn(attempt).await;
        // Never two main processes at once: the earlier one is gone.
        if attempt > 1 {
            let earlier = rig.main_spawn(attempt - 1).await;
            assert_eq!(
                rig.processes().process_state(earlier.pid),
                Some(FakeProcessState::Exited)
            );
        }
        rig.processes()
            .exit(spawn.pid, Some(ExitStatus::Exited(70)));
    }
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::RETRY_BUDGET_EXHAUSTED.to_string()
    );
    let details: Vec<_> = detail
        .view
        .journal
        .failure_error()
        .unwrap()
        .details()
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect();
    assert!(
        details.contains(&("attempts".to_owned(), "6".to_owned())),
        "{details:?}"
    );
    assert!(
        details.contains(&("lastExitCode".to_owned(), "70".to_owned())),
        "{details:?}"
    );
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        instance.last_failure().map(|code| code.as_str()),
        Some("crash_loop")
    );
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 6, "six spawns for one crash loop");
    let restart = rig.sup.status(id).restart;
    assert!(restart.exhausted);
    assert_eq!(restart.attempts, 6);
    assert_eq!(rig.processes().count(ProcessOp::Discard), 0);
    // The lifecycle went through recovering between the spawns.
    let names = rig.events.lifecycle_names(id);
    assert_eq!(names.first(), Some(&"starting"));
    assert_eq!(names.last(), Some(&"failed"));
    assert!(names.contains(&"recovering"));
}

#[tokio::test]
async fn a_port_taken_by_someone_else_after_the_exit_fails_without_using_the_budget() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    let port: u16 = spawn
        .args
        .iter()
        .find_map(|arg| arg.strip_prefix("--port="))
        .unwrap()
        .parse()
        .unwrap();
    // The fake dies and someone else binds the exact address.
    let _taker = TcpListener::bind((LOOPBACK, port)).unwrap();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(1)));
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_IN_USE.to_string()
    );
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1, "no respawn");
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("endpoint_in_use")
    );
}

#[tokio::test]
async fn the_readiness_deadline_fails_the_operation_but_the_process_and_its_probe_go_on() {
    let rig =
        Rig::patched(|text| replace(text, "readinessSeconds: 30", "readinessSeconds: 1")).await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::READINESS_TIMEOUT.to_string()
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(
        rig.processes().process_state(spawn.pid),
        Some(FakeProcessState::Running),
        "the process stays"
    );
    assert_eq!(
        rig.sup.status(id).readiness.state(),
        ReadinessState::NotReady
    );
    assert!(!rig.sup.status(id).ports_final);

    // The server comes up late: the probe that kept going sees it.
    rig.serve(&spawn);
    wait_for("readiness turns ready", || {
        rig.sup.status(id).readiness.state() == ReadinessState::Ready
    })
    .await;
    assert!(rig.sup.status(id).ports_final);
}

#[tokio::test]
async fn a_cancel_with_a_live_process_stops_it_gracefully_and_ends_cancelled() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    wait_for("the process runs", || rig.sup.status(id).process.is_some()).await;
    rig.engine.cancel(op).await.unwrap();
    wait_for("the stop signal arrives", || {
        !rig.processes().signals(spawn.pid).is_empty()
    })
    .await;
    assert_eq!(rig.processes().signals(spawn.pid), [ProviderSignal::Term]);
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    let detail = rig.wait_final(op).await;

    assert_eq!(detail.view.operation.state(), OperationState::Cancelled);
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
}

#[tokio::test]
async fn a_cancel_during_the_pause_between_respawns_ends_stopped_without_a_second_spawn() {
    let rig = Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_secs(30);
        config.restart_max_delay = Duration::from_secs(30);
    })
    .await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(1)));
    wait_for("the instance recovers", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;
    rig.engine.cancel(op).await.unwrap();
    let detail = rig.wait_final(op).await;

    assert_eq!(detail.view.operation.state(), OperationState::Cancelled);
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1);
}

#[tokio::test]
async fn a_failed_commit_of_the_identity_discards_the_suspended_child_before_it_runs() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    rig.registry.refuse_process_observations(true);

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        stackly_core::supervisor::codes::IDENTITY_CAPTURE_FAILED.to_string()
    );
    assert_eq!(rig.processes().count(ProcessOp::Discard), 1);
    assert_eq!(rig.processes().count(ProcessOp::Resume), 0, "it never ran");
    let spawn = rig.main_spawn(1).await;
    assert_eq!(
        rig.processes().process_state(spawn.pid),
        Some(FakeProcessState::Exited)
    );
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        instance.last_failure().map(|code| code.as_str()),
        Some("identity_capture_failed")
    );
}

#[tokio::test]
async fn a_spawn_the_os_refuses_fails_the_start_with_the_spawn_code() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    rig.fail_spawn();

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), codes::SPAWN_FAILED.to_string());
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        instance.last_failure().map(|code| code.as_str()),
        Some("spawn_failed")
    );
}

#[tokio::test]
async fn an_executable_that_leaves_the_runtime_is_refused_before_the_spawn() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let stackly_domain::InstallMode::Managed { artifact_sha256 } = rig.runtime.install() else {
        unreachable!();
    };
    let dir = rig.layout.layout().runtime_dir(
        rig.runtime.software(),
        rig.runtime.version(),
        rig.runtime.target(),
        artifact_sha256,
    );
    let executable = dir.join("stackly-fake-server");
    std::fs::remove_file(&executable).unwrap();
    let elsewhere = rig.root.path().join("elsewhere");
    std::fs::write(&elsewhere, b"#!/bin/sh\n").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &executable).unwrap();

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::EXECUTABLE_INVALID.to_string()
    );
    assert!(rig.processes().spawns().is_empty());
    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("executable_invalid")
    );
}

#[tokio::test]
async fn only_the_first_attempt_may_show_a_keychain_dialog_and_every_respawn_never_does() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let before = rig.fake.secrets().calls().len();

    let op = rig.start(id).await;
    let first = rig.main_spawn(1).await;
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(1)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(op, OperationState::Succeeded).await;

    let reads: Vec<_> = rig
        .fake
        .secrets()
        .calls()
        .into_iter()
        .skip(before)
        .filter(|call| call.op == stackly_testkit::SecretOp::Read)
        .map(|call| call.interaction)
        .collect();
    assert_eq!(
        reads,
        [
            Some(stackly_platform::Interaction::AllowPrompt),
            Some(stackly_platform::Interaction::Never)
        ],
        "the user's interaction applies to the first attempt only"
    );
}

#[tokio::test]
async fn ac_8_a_cancel_before_the_journal_leaves_no_side_effect() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let before = rig.instance_row(id).revision();
    // The `endpoints` stage waits at the registry read, before `journal`.
    let hold = rig.registry.hold_instances();

    let op = rig.start(id).await;
    wait_for("the start reaches the endpoints stage", || hold.entered()).await;
    rig.engine.cancel(op).await.unwrap();
    hold.release();
    let detail = rig.wait_final(op).await;

    assert_eq!(detail.view.operation.state(), OperationState::Cancelled);
    let stages: Vec<_> = detail
        .steps
        .iter()
        .map(|step| step.name().as_str())
        .collect();
    assert!(
        !stages.contains(&"journal"),
        "the cancel came before the journal: {stages:?}"
    );
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.desired(), DesiredState::Stopped);
    assert_eq!(
        instance.revision(),
        before,
        "the registry row never changed"
    );
    assert!(rig.processes().spawns().is_empty(), "nothing was spawned");
    assert!(
        rig.processes().count(ProcessOp::Resume) == 0
            && rig.processes().count(ProcessOp::Discard) == 0
    );
    assert_eq!(
        rig.processes()
            .probe_lock(&rig.paths(id).lock)
            .expect("the lock can be probed"),
        stackly_platform::LockState::Free,
        "the instance lock was released"
    );
}

#[tokio::test]
async fn ac_3_a_secret_value_in_an_argument_is_refused_before_the_journal_and_nothing_spawns() {
    // The label setting reaches argv as `--label=<value>`; here it holds the
    // secret's value, so `ensure_not_in_argv` must refuse the dry run.
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        settings: serde_json::json!({ "label": crate::common::TOKEN }),
        ..Spec::default()
    });
    let before = rig.instance_row(id).revision();

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        stackly_core::secret::codes::DELIVERY_REJECTED.to_string()
    );
    let stages: Vec<_> = detail
        .steps
        .iter()
        .map(|step| step.name().as_str())
        .collect();
    assert!(
        !stages.contains(&"journal"),
        "the dry run fails before the journal: {stages:?}"
    );
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.revision(), before);
    assert!(rig.processes().spawns().is_empty(), "no spawn happened");
    let failure = format!("{:?}", detail.view.journal.failure_error());
    assert!(
        !failure.contains(crate::common::TOKEN),
        "the refusal never repeats the secret value"
    );
}
