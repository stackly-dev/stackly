//! Changing an instance that exists (spec 0029, milestone 1): the history
//! copies and the drift they make detectable (AC-10, AC-11), the revision
//! that proved itself ready (AC-7), and the operations `apply_config`,
//! `change_endpoints`, `rename_instance`, and `resolve_config_drift`
//! (AC-3 to AC-9, AC-12 to AC-14), over the fake provider package.

use std::time::Duration;

use serde_json::json;
use stackly_core::instance::config::{ConfigChange, Planned, Rejected};
use stackly_core::supervisor::{RegistryMode, codes};
use stackly_domain::{
    ConfigRevisionId, ConfigRevisionOrigin, ConfigRevisionRecord, ConfigRevisionStatus,
    InstanceBlocker, InstanceId, InstanceLifecycle, OperationKind, OperationState, Port,
    PortRequest, ReadinessState, Registry, WriteBatch, WriteOp,
};
use stackly_platform::{ChildRole, ExitStatus, ProviderSignal};
use stackly_testkit::FakeSpawn;

use crate::common::{Rig, Spec};

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

/// Starts the instance and waits until it is `ready`, leaving the real
/// server of the fake process up.
async fn running(rig: &Rig, id: InstanceId) -> (FakeSpawn, u16) {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(main_spawns(rig) + 1).await;
    let port = rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    (spawn, port)
}

/// Stops the running instance the way its provider asks and waits for it.
async fn stopped(rig: &Rig, id: InstanceId, spawn: &FakeSpawn) {
    let stop = rig.stop(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(spawn.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(stop, OperationState::Succeeded).await;
}

fn conf(rig: &Rig, id: InstanceId) -> std::path::PathBuf {
    rig.paths(id).config.join("fake.conf")
}

fn copy_of_conf(rig: &Rig, id: InstanceId) -> std::path::PathBuf {
    rig.paths(id)
        .config_history
        .join(rig.active(id).id.to_string())
        .join("fake.conf")
}

fn blocker(rig: &Rig, id: InstanceId) -> Option<InstanceBlocker> {
    rig.sup.status(id).blocker
}

async fn ready_at_is_set(rig: &Rig, id: InstanceId) {
    wait_for("the active revision is marked ready", || {
        rig.active(id).ready_at.is_some()
    })
    .await;
}

// Drift (AC-10, AC-11)

#[tokio::test]
async fn a_render_keeps_a_copy_of_each_plain_config_file_and_none_of_the_sensitive_one() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let _running = running(&rig, id).await;

    let copy = std::fs::read(copy_of_conf(&rig, id)).expect("fake.conf has a copy");
    assert_eq!(copy, std::fs::read(conf(&rig, id)).unwrap());
    let history = rig
        .paths(id)
        .config_history
        .join(rig.active(id).id.to_string());
    let names: Vec<_> = std::fs::read_dir(history)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        names,
        ["fake.conf", "notes.txt"],
        "fake.json is sensitive and has no copy"
    );
}

#[tokio::test]
async fn a_file_changed_outside_stackly_blocks_start_until_the_revision_is_kept() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, _) = running(&rig, id).await;
    stopped(&rig, id, &first).await;
    let original = std::fs::read(conf(&rig, id)).unwrap();
    std::fs::write(conf(&rig, id), b"{\"greeting\": \"edited by hand\"}").unwrap();

    let start = rig.start(id).await;
    let detail = rig.wait_state(start, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "config.drift_detected");
    assert_eq!(
        std::fs::read(conf(&rig, id)).unwrap(),
        b"{\"greeting\": \"edited by hand\"}",
        "Stackly never overwrites a drifted file on its own"
    );
    let Some(InstanceBlocker::ConfigDrift { files, .. }) = blocker(&rig, id) else {
        panic!("expected the drift blocker, got {:?}", blocker(&rig, id));
    };
    assert_eq!(
        files.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["fake.conf"]
    );
    assert_eq!(main_spawns(&rig), 1, "nothing was spawned");

    let keep = rig.keep_revision(id).await;
    rig.wait_state(keep, OperationState::Succeeded).await;
    assert_eq!(std::fs::read(conf(&rig, id)).unwrap(), original);
    assert_eq!(blocker(&rig, id), None);

    let (_second, _) = running(&rig, id).await;
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
}

#[tokio::test]
async fn a_deleted_file_is_drift_and_keep_revision_writes_it_back() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, _) = running(&rig, id).await;
    stopped(&rig, id, &first).await;
    std::fs::remove_file(conf(&rig, id)).unwrap();

    let report = stackly_core::supervisor::drift::observe_active(&rig.sup, id)
        .await
        .unwrap();

    assert_eq!(report.files.len(), 1);
    assert_eq!(
        report.files[0].kind,
        stackly_core::supervisor::drift::DriftKind::Missing
    );
    let keep = rig.keep_revision(id).await;
    rig.wait_state(keep, OperationState::Succeeded).await;
    assert!(conf(&rig, id).exists());
    assert_eq!(blocker(&rig, id), None);
}

#[tokio::test]
async fn an_instance_that_never_rendered_has_no_copy_and_never_drifts() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let report = stackly_core::supervisor::drift::observe_active(&rig.sup, id)
        .await
        .unwrap();

    assert!(report.is_clean());
    assert_eq!(blocker(&rig, id), None);
}

#[tokio::test]
async fn the_check_when_the_daemon_starts_puts_the_drift_blocker_on_the_instance() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, _) = running(&rig, id).await;
    stopped(&rig, id, &first).await;
    std::fs::write(conf(&rig, id), b"{}").unwrap();

    rig.sup.shutdown();
    let next = rig.reborn();
    next.reconcile(RegistryMode::Ready).await.unwrap();

    assert!(matches!(
        next.status(id).blocker,
        Some(InstanceBlocker::ConfigDrift { .. })
    ));
}

// Last known good (AC-7, AC-8)

#[tokio::test]
async fn the_first_ready_on_a_revision_marks_it_and_makes_it_the_last_known_good() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    assert_eq!(rig.registry.last_known_good(id).unwrap().value, None);

    let _running = running(&rig, id).await;
    ready_at_is_set(&rig, id).await;

    let good = rig.registry.last_known_good(id).unwrap().value.unwrap();
    assert_eq!(good.id, rig.active(id).id);
}

#[tokio::test]
async fn a_process_adopted_after_the_daemon_died_never_marks_a_revision_ready() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let _running = running(&rig, id).await;
    ready_at_is_set(&rig, id).await;
    // The daemon dies after revision 2 committed and before the restart.
    rig.sup.shutdown();
    let instance = rig.instance_row(id);
    let active = rig.active(id);
    let mut moved = instance.clone();
    moved
        .set_port_requests(&active.endpoints)
        .expect("the same requests apply");
    let record = ConfigRevisionRecord::following(
        &active,
        ConfigRevisionId::generate(),
        active.values.clone(),
        active.endpoints.clone(),
        ConfigRevisionOrigin::Applied,
        None,
        None,
        rig.fake_now(),
    );
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::ActivateConfigRevision {
            instance: moved,
            expected: instance.revision(),
            record: Box::new(record),
        }]))
        .expect("the revision commits");

    let next = rig.reborn();
    next.reconcile(RegistryMode::Ready).await.unwrap();
    wait_for("the adopted process is ready", || {
        next.status(id).readiness.state() == ReadinessState::Ready
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(rig.active(id).number.get(), 2);
    assert_eq!(
        rig.active(id).ready_at,
        None,
        "the old process read the old revision"
    );
    assert_eq!(
        rig.registry
            .last_known_good(id)
            .unwrap()
            .value
            .unwrap()
            .number
            .get(),
        1
    );
}

#[tokio::test]
async fn a_failing_apply_keeps_the_new_revision_unready_and_rollback_restores_the_good_one() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, _) = running(&rig, id).await;
    ready_at_is_set(&rig, id).await;
    let good = rig.active(id);

    // A restart setting that keeps the server from getting ready.
    let apply = rig.apply(id, json!({"readyDelayMs": 600_000})).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(first.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(apply, OperationState::Failed).await;

    let bad = rig.active(id);
    assert_eq!(bad.number.get(), 2, "the failed revision stays active");
    assert_eq!(bad.ready_at, None);
    assert_eq!(
        rig.registry
            .last_known_good(id)
            .unwrap()
            .value
            .unwrap()
            .number,
        good.number
    );

    // The process still runs, not ready. Roll back: a new revision with
    // the settings of the good one, applied by a restart.
    let rollback = rig
        .change(OperationKind::ApplyConfig, id, 2, ConfigChange::Rollback)
        .await;
    wait_for("the term arrives", || {
        !rig.processes().signals(second.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes()
        .exit(second.pid, Some(ExitStatus::Exited(0)));
    let third = rig.main_spawn(3).await;
    rig.serve(&third);
    rig.wait_state(rollback, OperationState::Succeeded).await;

    let restored = rig.active(id);
    assert_eq!(restored.number.get(), 3);
    assert_eq!(restored.origin, ConfigRevisionOrigin::Rollback);
    assert_eq!(restored.source_number, Some(good.number));
    let values: serde_json::Value = serde_json::from_str(restored.values.as_str()).unwrap();
    assert_eq!(
        values["readyDelayMs"], 0,
        "the settings of the good revision"
    );
    ready_at_is_set(&rig, id).await;
    assert_eq!(
        rig.registry
            .last_known_good(id)
            .unwrap()
            .value
            .unwrap()
            .number
            .get(),
        3
    );
}

#[tokio::test]
async fn rollback_without_another_ready_revision_is_refused() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig
        .change(OperationKind::ApplyConfig, id, 1, ConfigChange::Rollback)
        .await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "config.no_last_known_good");
    assert_eq!(rig.active(id).number.get(), 1);
}

// Apply (AC-3 to AC-6, AC-8, AC-9, AC-12)

#[tokio::test]
async fn a_reload_setting_on_a_running_instance_writes_a_revision_and_reloads_the_same_process() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (spawn, _) = running(&rig, id).await;
    let before = rig.instance_row(id);
    let first = rig.active(id);

    let op = rig
        .apply(id, json!({"greeting": "reloaded greeting"}))
        .await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let second = rig.active(id);
    assert_eq!(second.number.get(), 2);
    assert_eq!(second.previous, Some(first.id));
    assert_eq!(second.origin, stackly_domain::ConfigRevisionOrigin::Applied);
    let history = rig.registry.config_revisions(id, 20).unwrap().value;
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].status, ConfigRevisionStatus::Superseded);
    assert_eq!(
        rig.instance_row(id).revision().get(),
        before.revision().get() + 1
    );
    assert_eq!(rig.processes().signals(spawn.pid), [ProviderSignal::Hup]);
    assert_eq!(
        main_spawns(&rig),
        1,
        "a reload never runs the process again"
    );
    assert!(
        std::fs::read_to_string(conf(&rig, id))
            .unwrap()
            .contains("reloaded greeting")
    );
    ready_at_is_set(&rig, id).await;
}

#[tokio::test]
async fn a_restart_setting_on_a_running_instance_restarts_it_with_a_new_process() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, _) = running(&rig, id).await;

    let op = rig.apply(id, json!({"label": "second"})).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(first.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    assert_ne!(second.pid, first.pid);
    assert_eq!(rig.active(id).number.get(), 2);
    assert!(second.args.iter().any(|arg| arg == "--label=second"));
    let stages: Vec<_> = detail
        .steps
        .iter()
        .map(|step| step.name().as_str())
        .collect();
    assert!(
        stages.starts_with(&["validate", "lock", "commit", "apply", "journal", "stop"]),
        "{stages:?}"
    );
    ready_at_is_set(&rig, id).await;
}

#[tokio::test]
async fn an_instance_that_does_not_run_only_saves_the_revision_for_its_next_start() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.apply(id, json!({"label": "later"})).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.active(id).number.get(), 2);
    assert!(rig.processes().spawns().is_empty());
    let start = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(start, OperationState::Succeeded).await;
    assert!(spawn.args.iter().any(|arg| arg == "--label=later"));
}

#[tokio::test]
async fn a_lifecycle_that_cannot_take_a_change_is_refused_with_its_own_code() {
    for (lifecycle, code) in [
        (InstanceLifecycle::Starting, codes::INSTANCE_NOT_RUNNING),
        (InstanceLifecycle::Stopping, codes::INSTANCE_STOPPING),
        (InstanceLifecycle::Unknown, codes::IDENTITY_UNPROVEN),
    ] {
        let rig = Rig::new().await;
        let id = rig.instance(&Spec {
            lifecycle,
            ..Spec::default()
        });

        let op = rig.apply(id, json!({"label": "x"})).await;
        let detail = rig.wait_state(op, OperationState::Failed).await;

        assert_eq!(Rig::failure_code(&detail), code.to_string(), "{lifecycle}");
        assert_eq!(rig.active(id).number.get(), 1);
    }
}

#[tokio::test]
async fn invalid_settings_write_no_revision_and_no_file() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let base = rig.active(id).number;
    let mut values: serde_json::Value =
        serde_json::from_str(rig.active(id).values.as_str()).unwrap();
    values["readyDelayMs"] = json!(-5);

    let planned = rig
        .sup
        .plan_change(id, base, &ConfigChange::Settings(values.clone()), true)
        .await;
    let Err(Rejected::Fields(issues)) = planned else {
        panic!("expected field errors, got {planned:?}");
    };
    assert_eq!(issues[0].path, "settings.readyDelayMs");

    let op = rig
        .change(
            OperationKind::ApplyConfig,
            id,
            base.get(),
            ConfigChange::Settings(values),
        )
        .await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), "instance.settings_invalid");
    assert_eq!(rig.active(id).number, base);
    assert_eq!(
        rig.registry.config_revisions(id, 20).unwrap().value.len(),
        1
    );
}

#[tokio::test]
async fn a_set_once_field_and_a_secret_field_cannot_change() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let base = rig.active(id).number;
    let mut values: serde_json::Value =
        serde_json::from_str(rig.active(id).values.as_str()).unwrap();
    values["authToken"] = json!("local:test/other");

    let planned = rig
        .sup
        .plan_change(id, base, &ConfigChange::Settings(values), false)
        .await;

    let Err(Rejected::Fields(issues)) = planned else {
        panic!("expected field errors, got {planned:?}");
    };
    assert_eq!(issues[0].path, "settings.authToken");
}

#[tokio::test]
async fn a_stale_base_revision_is_a_conflict_and_the_second_of_two_queued_applies_loses() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let active = rig.active(id);
    let mut values: serde_json::Value = serde_json::from_str(active.values.as_str()).unwrap();
    values["label"] = json!("one");
    let mut other = values.clone();
    other["label"] = json!("two");

    let first = rig
        .change(
            OperationKind::ApplyConfig,
            id,
            1,
            ConfigChange::Settings(values),
        )
        .await;
    let second = rig
        .change(
            OperationKind::ApplyConfig,
            id,
            1,
            ConfigChange::Settings(other),
        )
        .await;

    rig.wait_state(first, OperationState::Succeeded).await;
    let detail = rig.wait_state(second, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), "config.revision_conflict");
    let error = detail.view.journal.failure_error().unwrap();
    assert!(
        error
            .details()
            .iter()
            .any(|(key, value)| key.as_str() == "activeRevision" && value == "2"),
        "{error:?}"
    );
    assert_eq!(rig.active(id).number.get(), 2);
}

#[tokio::test]
async fn an_apply_that_changes_nothing_is_unchanged() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let active = rig.active(id);
    let values: serde_json::Value = serde_json::from_str(active.values.as_str()).unwrap();

    let planned = rig
        .sup
        .plan_change(id, active.number, &ConfigChange::Settings(values), true)
        .await
        .unwrap();

    assert_eq!(planned, Planned::Unchanged);
}

#[tokio::test]
async fn the_history_keeps_twenty_revisions_and_numbers_are_never_reused() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    // A copy folder of revision 1, which the prune must remove with its row.
    let first = rig.active(id);
    let folder = rig.paths(id).config_history.join(first.id.to_string());
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("fake.conf"), b"{}").unwrap();
    std::fs::write(conf(&rig, id), b"{}").unwrap();

    for n in 0..25 {
        let op = rig.apply(id, json!({"label": format!("label-{n}")})).await;
        rig.wait_state(op, OperationState::Succeeded).await;
    }

    let history = rig.registry.config_revisions(id, 100).unwrap().value;
    assert_eq!(history.len(), 20);
    assert_eq!(history[0].number.get(), 26);
    assert_eq!(history[19].number.get(), 7);
    assert_eq!(rig.active(id).number.get(), 26);
    assert!(
        !folder.exists(),
        "the copies of a pruned revision go with it"
    );
}

// Rename (AC-13)

#[tokio::test]
async fn a_rename_changes_only_the_name_of_a_running_instance() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (spawn, _) = running(&rig, id).await;
    let before = rig.instance_row(id);
    let revision = rig.active(id);

    let op = rig.rename(id, "renamed-one").await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let after = rig.instance_row(id);
    assert_eq!(after.name().as_str(), "renamed-one");
    assert_eq!(after.id(), before.id());
    assert_eq!(after.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(after.revision().get(), before.revision().get() + 1);
    assert_eq!(rig.active(id), revision, "the config revision is untouched");
    assert_eq!(rig.sup.status(id).process.map(|p| p.pid), Some(spawn.pid));
    assert_eq!(main_spawns(&rig), 1);
}

#[tokio::test]
async fn a_name_another_instance_has_is_taken() {
    let rig = Rig::new().await;
    let one = rig.instance(&Spec::default());
    let two = rig.instance(&Spec::default());
    let taken = rig.instance_row(two).name().to_string();

    let op = rig.rename(one, &taken).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "instance.name_taken");
    assert_ne!(rig.instance_row(one).name().as_str(), taken);
}

#[tokio::test]
async fn renaming_to_the_current_name_changes_nothing() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let before = rig.instance_row(id);

    let op = rig.rename(id, before.name().as_str()).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.instance_row(id).revision(), before.revision());
}

#[tokio::test]
async fn an_initializing_instance_cannot_be_renamed() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Initializing,
        ..Spec::default()
    });

    let op = rig.rename(id, "anything-goes").await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::INSTANCE_INITIALIZING.to_string()
    );
}

// Endpoints (AC-14)

fn fixed(port: u16) -> PortRequest {
    PortRequest::Fixed(Port::new(port).expect("a test port is not 0"))
}

fn endpoint_change(request: PortRequest) -> ConfigChange {
    ConfigChange::Endpoints(vec![("http".parse().expect("an endpoint name"), request)])
}

#[tokio::test]
async fn a_free_fixed_port_is_saved_for_the_next_start_of_a_stopped_instance() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let port = Rig::free_port();

    let op = rig
        .change(
            OperationKind::ChangeEndpoints,
            id,
            1,
            endpoint_change(fixed(port)),
        )
        .await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let active = rig.active(id);
    assert_eq!(active.number.get(), 2);
    assert_eq!(
        active.origin,
        stackly_domain::ConfigRevisionOrigin::Endpoints
    );
    assert_eq!(
        active.endpoints.get(&"http".parse().unwrap()),
        Some(fixed(port))
    );
    let start = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(start, OperationState::Succeeded).await;
    assert!(
        spawn
            .args
            .iter()
            .any(|arg| *arg == format!("--port={port}"))
    );
}

#[tokio::test]
async fn a_port_another_instance_has_is_refused_with_a_suggestion() {
    let rig = Rig::new().await;
    let port = Rig::free_port();
    let _other = rig.instance(&Spec {
        request: Some(port),
        ..Spec::default()
    });
    let id = rig.instance(&Spec::default());

    let planned = rig
        .sup
        .plan_change(
            id,
            rig.active(id).number,
            &endpoint_change(fixed(port)),
            true,
        )
        .await;

    let Err(Rejected::Endpoints(issues)) = planned else {
        panic!("expected an endpoint issue, got {planned:?}");
    };
    assert_eq!(issues[0].endpoint, "http");
    assert_eq!(issues[0].error.code(), "supervisor.endpoint_conflict");
    assert!(issues[0].suggestion.is_some_and(|free| free.get() > port));
    assert_eq!(rig.active(id).number.get(), 1);
}

#[tokio::test]
async fn a_port_an_outside_process_holds_is_refused_but_the_instances_own_bound_port_passes() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (_spawn, bound) = running(&rig, id).await;
    rig.ports
        .answer(bound, stackly_core::supervisor::BindOutcome::InUse);
    let taken = Rig::free_port();
    rig.ports
        .answer(taken, stackly_core::supervisor::BindOutcome::InUse);
    let base = rig.active(id).number;

    let outside = rig
        .sup
        .plan_change(id, base, &endpoint_change(fixed(taken)), true)
        .await;
    let own = rig
        .sup
        .plan_change(id, base, &endpoint_change(fixed(bound)), true)
        .await;

    let Err(Rejected::Endpoints(issues)) = outside else {
        panic!("expected an endpoint issue, got {outside:?}");
    };
    assert_eq!(issues[0].error.code(), "supervisor.endpoint_in_use");
    assert!(
        matches!(own, Ok(Planned::Change(_))),
        "the bound port of the instance itself passes the bind probe: {own:?}"
    );
}

#[tokio::test]
async fn changing_a_port_of_a_running_instance_restarts_it() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, _) = running(&rig, id).await;
    let port = Rig::free_port();

    let op = rig
        .change(
            OperationKind::ChangeEndpoints,
            id,
            1,
            endpoint_change(fixed(port)),
        )
        .await;
    wait_for("the term arrives", || {
        !rig.processes().signals(first.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(op, OperationState::Succeeded).await;

    assert!(
        second
            .args
            .iter()
            .any(|arg| *arg == format!("--port={port}"))
    );
    assert_eq!(rig.active(id).number.get(), 2);
}

// More drift and plan cases

#[tokio::test]
async fn a_respawn_that_meets_drift_stops_the_instance_without_spending_the_retry_budget() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, _) = running(&rig, id).await;
    rig.kill_servers();
    std::fs::write(conf(&rig, id), b"{\"greeting\": \"by hand\"}").unwrap();

    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the respawn gives up on the drift", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Failed
    })
    .await;

    assert_eq!(main_spawns(&rig), 1, "nothing was spawned again");
    assert!(matches!(
        blocker(&rig, id),
        Some(InstanceBlocker::ConfigDrift { .. })
    ));
    assert_eq!(
        std::fs::read(conf(&rig, id)).unwrap(),
        b"{\"greeting\": \"by hand\"}"
    );
}

#[tokio::test]
async fn an_apply_and_a_reload_meet_drift_before_they_write_anything() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (_spawn, _) = running(&rig, id).await;
    std::fs::write(conf(&rig, id), b"{\"greeting\": \"by hand\"}").unwrap();

    let apply = rig.apply(id, json!({"greeting": "from apply"})).await;
    let detail = rig.wait_state(apply, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), "config.drift_detected");
    assert_eq!(rig.active(id).number.get(), 1, "no revision was written");

    let reload = rig.reload(id).await;
    let detail = rig.wait_state(reload, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&detail), "config.drift_detected");
    assert_eq!(
        std::fs::read(conf(&rig, id)).unwrap(),
        b"{\"greeting\": \"by hand\"}"
    );
    assert!(rig.processes().signals(_spawn.pid).is_empty(), "no signal");
}

#[tokio::test]
async fn a_restore_copies_an_older_revision_of_the_history() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    for label in ["one", "two"] {
        let op = rig.apply(id, json!({ "label": label })).await;
        rig.wait_state(op, OperationState::Succeeded).await;
    }

    let op = rig
        .change(
            OperationKind::ApplyConfig,
            id,
            3,
            ConfigChange::Restore(stackly_domain::ConfigRevisionNumber::new(2).unwrap()),
        )
        .await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let active = rig.active(id);
    assert_eq!(active.number.get(), 4);
    assert_eq!(active.origin, ConfigRevisionOrigin::Restored);
    assert_eq!(active.source_number.map(|n| n.get()), Some(2));
    let values: serde_json::Value = serde_json::from_str(active.values.as_str()).unwrap();
    assert_eq!(values["label"], "one");
}

#[tokio::test]
async fn a_restore_of_a_revision_the_history_lacks_is_not_found() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig
        .change(
            OperationKind::ApplyConfig,
            id,
            1,
            ConfigChange::Restore(stackly_domain::ConfigRevisionNumber::new(9).unwrap()),
        )
        .await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "resource.not_found");
}

#[tokio::test]
async fn a_port_request_for_an_unknown_endpoint_is_invalid() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let planned = rig
        .sup
        .plan_change(
            id,
            rig.active(id).number,
            &ConfigChange::Endpoints(vec![("nope".parse().unwrap(), PortRequest::Auto)]),
            true,
        )
        .await;

    let Err(Rejected::Endpoints(issues)) = planned else {
        panic!("expected an endpoint issue, got {planned:?}");
    };
    assert_eq!(issues[0].error.code(), "instance.endpoint_invalid");
}

#[tokio::test]
async fn the_audit_summary_names_settings_and_ports_and_never_a_value() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let port = Rig::free_port();
    let active = rig.active(id);
    let mut values: serde_json::Value = serde_json::from_str(active.values.as_str()).unwrap();
    values["label"] = json!("do-not-log-this");

    let Ok(Planned::Change(settings)) = rig
        .sup
        .plan_change(id, active.number, &ConfigChange::Settings(values), true)
        .await
    else {
        panic!("a valid change");
    };
    let Ok(Planned::Change(ports)) = rig
        .sup
        .plan_change(id, active.number, &endpoint_change(fixed(port)), true)
        .await
    else {
        panic!("a valid change");
    };

    let settings = settings.audit();
    assert_eq!(settings["oldRevision"], 1);
    assert_eq!(settings["newRevision"], 2);
    assert_eq!(settings["settings"], json!(["label"]));
    assert!(!settings.to_string().contains("do-not-log-this"));
    let ports = ports.audit();
    assert_eq!(ports["ports"][0]["endpoint"], "http");
    assert_eq!(ports["ports"][0]["old"], "auto");
    assert_eq!(ports["ports"][0]["new"], port);
}
