//! Reconciliation looks at the data and the runtime of every instance
//! (spec 0030 AC-4, AC-5): data that is gone is never replaced, a process
//! that still runs is adopted and reported, and a start fails before it
//! makes a folder or spawns.

use stackly_core::supervisor::{FindingKind, RegistryMode, StartRequest, codes};

use stackly_domain::{
    DesiredState, FailureCode, InstanceId, InstanceLifecycle, LifecycleEvent, OperationKind,
    OperationState, Registry, RuntimeState, WriteBatch, WriteOp,
};
use stackly_platform::InstancePaths;
use stackly_testkit::RuntimeBuilder;

use crate::common::{Plant, Rig, Spec};

/// How the data of an instance goes missing.
#[derive(Clone, Copy, Debug)]
enum Loss {
    /// The data folder is deleted.
    Folder,
    /// The data folder stays, with nothing in it.
    Emptied,
    /// Other files stay, but the data marker is gone.
    Marker,
}

impl Loss {
    const ALL: [Self; 3] = [Self::Folder, Self::Emptied, Self::Marker];

    fn apply(self, paths: &InstancePaths) {
        match self {
            Self::Folder => std::fs::remove_dir_all(&paths.data).expect("the data is deleted"),
            Self::Emptied => {
                std::fs::remove_file(paths.data.join("FAKE_VERSION")).expect("the marker goes");
            }
            Self::Marker => {
                std::fs::write(paths.data.join("base"), b"data").expect("another file is written");
                std::fs::remove_file(paths.data.join("FAKE_VERSION")).expect("the marker goes");
            }
        }
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

/// Turns a stopped instance into one that failed after its data was
/// initialized, as a crash loop leaves it.
fn make_failed(rig: &Rig, id: InstanceId) {
    for event in [
        LifecycleEvent::StartBegin,
        LifecycleEvent::StartFailed {
            reason: FailureCode::CRASH_LOOP,
        },
    ] {
        // One update per domain mutation, each from the revision just read.
        let mut instance = rig.instance_row(id);
        let expected = instance.revision();
        instance
            .apply_lifecycle(event, rig.fake_now())
            .expect("the failure is recorded");
        rig.registry
            .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
                instance,
                expected,
            }]))
            .expect("the instance is stored");
    }
}

fn run_dir_exists(rig: &Rig, id: InstanceId) -> bool {
    rig.layout
        .layout()
        .instance_run_dir(id.short_key())
        .exists()
}

fn runtime_executable(rig: &Rig) -> std::path::PathBuf {
    let stackly_domain::InstallMode::Managed { artifact_sha256 } = rig.runtime.install() else {
        unreachable!("the rig installs a managed runtime");
    };
    rig.layout
        .layout()
        .runtime_dir(
            rig.runtime.software(),
            rig.runtime.version(),
            rig.runtime.target(),
            artifact_sha256,
        )
        .join("stackly-fake-server")
}

#[tokio::test]
async fn missing_data_fails_the_instance_without_init_start_or_new_folders() {
    // covers: spec 0030 AC-4
    for loss in Loss::ALL {
        for (lifecycle, desired) in [
            (InstanceLifecycle::Stopped, DesiredState::Stopped),
            (InstanceLifecycle::Stopped, DesiredState::Running),
            (InstanceLifecycle::Failed, DesiredState::Running),
        ] {
            let rig = Rig::new().await;
            let id = rig.instance(&spec(InstanceLifecycle::Stopped, desired));
            if lifecycle == InstanceLifecycle::Failed {
                make_failed(&rig, id);
            }
            let paths = rig.paths(id);
            loss.apply(&paths);
            let run_dir_before = run_dir_exists(&rig, id);
            let label = format!("{loss:?} {lifecycle:?} {desired:?}");

            let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
            rig.sup.start_reconciled();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let row = rig.instance_row(id);
            assert_eq!(row.lifecycle(), InstanceLifecycle::Failed, "{label}");
            assert_eq!(
                row.last_failure().map(|code| code.as_str()),
                Some("data_missing"),
                "{label}"
            );
            assert_eq!(row.desired(), desired, "the desired state is kept: {label}");
            assert_eq!(report.restarting, 0, "{label}");
            assert!(
                report.findings.is_empty(),
                "no process, no finding: {label}"
            );
            assert!(rig.processes().spawns().is_empty(), "{label}");
            assert_eq!(
                paths.data.exists(),
                !matches!(loss, Loss::Folder),
                "no data folder is made: {label}"
            );
            assert_eq!(run_dir_exists(&rig, id), run_dir_before, "{label}");

            // A start is refused before it makes a folder, with the code of
            // data that is not there, and the instance stays failed.
            let op = rig.start(id).await;
            let detail = rig.wait_state(op, OperationState::Failed).await;
            assert_eq!(
                Rig::failure_code(&detail),
                codes::DATA_UNAVAILABLE.to_string(),
                "{label}"
            );
            assert!(rig.processes().spawns().is_empty(), "{label}");
            assert_eq!(run_dir_exists(&rig, id), run_dir_before, "{label}");
            assert_eq!(
                paths.data.exists(),
                !matches!(loss, Loss::Folder),
                "{label}"
            );
            assert_eq!(
                rig.instance_row(id)
                    .last_failure()
                    .map(|code| code.as_str()),
                Some("data_missing"),
                "{label}"
            );
        }
    }
}

#[tokio::test]
async fn a_second_reconciliation_does_not_flag_the_same_missing_data_again() {
    // covers: spec 0030 AC-4
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    Loss::Folder.apply(&rig.paths(id));
    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    let flagged = rig.instance_row(id).revision();

    rig.reborn().reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(rig.instance_row(id).revision(), flagged);
}

#[tokio::test]
async fn a_vanished_process_on_missing_data_ends_failed_instead_of_respawning() {
    // covers: spec 0030 AC-4
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);
    Loss::Folder.apply(&rig.paths(id));

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    rig.sup.start_reconciled();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    assert_eq!(report.restarting, 0);
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        row.last_failure().map(|code| code.as_str()),
        Some("data_missing")
    );
    assert!(rig.processes().spawns().is_empty());
    assert!(!rig.paths(id).data.exists());
}

#[tokio::test]
async fn a_proven_process_on_missing_data_is_adopted_and_reported() {
    // covers: spec 0030 AC-4
    for loss in Loss::ALL {
        let rig = Rig::new().await;
        let id = rig.instance(&Spec::default());
        let op = rig.start(id).await;
        let spawn = rig.main_spawn(1).await;
        rig.serve(&spawn);
        rig.wait_state(op, OperationState::Succeeded).await;
        rig.sup.shutdown();
        loss.apply(&rig.paths(id));

        let next = rig.reborn();
        let report = next.reconcile(RegistryMode::Ready).await.unwrap();

        assert_eq!(report.adopted, 1, "{loss:?}");
        assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
        assert_eq!(report.findings.len(), 1, "{loss:?}");
        let finding = &report.findings[0];
        assert_eq!(finding.instance_id, id);
        assert_eq!(finding.kind, FindingKind::DataMissing);
        assert_eq!(rig.processes().signals(spawn.pid), []);
    }
}

#[tokio::test]
async fn a_missing_runtime_executable_keeps_the_adopted_process_and_blocks_the_next_start() {
    // covers: spec 0030 AC-5
    let rig = Rig::new().await;
    let running = rig.instance(&Spec::default());
    let op = rig.start(running).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    let stopped = rig.instance(&Spec::default());
    rig.sup.shutdown();
    std::fs::remove_file(runtime_executable(&rig)).expect("the executable is deleted");

    let next = rig.reborn();
    let report = next.reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(report.adopted, 1);
    assert_eq!(
        rig.instance_row(running).lifecycle(),
        InstanceLifecycle::Running
    );
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(report.findings[0].instance_id, running);
    assert_eq!(report.findings[0].kind, FindingKind::RuntimeMissing);
    assert_eq!(rig.processes().signals(spawn.pid), []);
    assert_eq!(
        rig.instance_row(stopped).lifecycle(),
        InstanceLifecycle::Stopped,
        "an instance without a process is not changed"
    );

    let work = next.start_work(StartRequest::user(stopped, Rig::user()));
    let op = rig.begin(OperationKind::StartInstance, stopped, work).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        codes::EXECUTABLE_INVALID.to_string()
    );
    assert_eq!(
        rig.instance_row(stopped)
            .last_failure()
            .map(|code| code.as_str()),
        Some("executable_invalid")
    );
    assert_eq!(next.status(stopped).blocker, None, "no new blocker");
    assert_eq!(
        rig.processes().spawns().len(),
        1,
        "only the first start spawned"
    );
}

#[tokio::test]
async fn a_respawn_on_a_runtime_that_is_not_installed_fails_as_runtime_unavailable() {
    // covers: spec 0030 AC-5
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);
    let broken = RuntimeBuilder::new()
        .with_id(rig.runtime.id())
        .software("fake")
        .version("1.0.0", "1.0")
        .state(RuntimeState::Failed)
        .revision(2)
        .install(rig.runtime.install().clone())
        .build();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateRuntime {
            runtime: broken,
            expected: rig.runtime.revision(),
        }]))
        .unwrap();

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    rig.sup.start_reconciled();
    wait_for_failed(&rig, id).await;

    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("runtime_unavailable")
    );
    assert!(rig.processes().spawns().is_empty());
}

#[tokio::test]
async fn a_respawn_without_the_runtime_executable_fails_as_executable_invalid() {
    // covers: spec 0030 AC-5
    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Running, DesiredState::Running));
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);
    std::fs::remove_file(runtime_executable(&rig)).expect("the executable is deleted");

    let report = rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    rig.sup.start_reconciled();
    wait_for_failed(&rig, id).await;

    assert!(report.findings.is_empty(), "no process, no finding");
    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("executable_invalid")
    );
    assert!(rig.processes().spawns().is_empty());
}

async fn wait_for_failed(rig: &Rig, id: InstanceId) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    while rig.instance_row(id).lifecycle() != InstanceLifecycle::Failed {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the respawn never failed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn intact_instances_produce_no_findings() {
    // covers: spec 0030 AC-4, AC-5
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    rig.instance(&Spec::default());
    rig.sup.shutdown();

    let report = rig.reborn().reconcile(RegistryMode::Ready).await.unwrap();

    assert_eq!(report.adopted, 1);
    assert!(report.findings.is_empty());
}

#[tokio::test]
async fn a_start_checks_the_locations_the_instance_lives_in_before_its_data() {
    // covers: spec 0032 AC-20
    use std::os::unix::fs::PermissionsExt;

    use stackly_domain::{LocationKind, StorageLocationId};

    let rig = Rig::new().await;
    let id = rig.instance(&spec(InstanceLifecycle::Stopped, DesiredState::Stopped));
    let location = StorageLocationId::generate();
    let base = rig.root.path().join("unplugged/Stackly");
    rig.layout
        .locations()
        .add_location(location, LocationKind::Data, base.clone());
    rig.layout
        .locations()
        .note_instance(id, location, LocationKind::Logs.built_in_id());

    // The folder of the location is gone.
    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        "instance.storage_location_missing"
    );
    assert!(rig.processes().spawns().is_empty());

    // The folder is there and the daemon cannot write to it.
    std::fs::create_dir_all(base.join("instances")).expect("the folders are made");
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o500))
        .expect("the folder becomes read only");
    let can_write = std::fs::File::create(base.join("probe")).is_ok();
    if !can_write {
        let op = rig.start(id).await;
        let detail = rig.wait_state(op, OperationState::Failed).await;
        assert_eq!(Rig::failure_code(&detail), "storage.location_invalid");
        assert!(rig.processes().spawns().is_empty());
    }
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))
        .expect("the folder opens again");
}
