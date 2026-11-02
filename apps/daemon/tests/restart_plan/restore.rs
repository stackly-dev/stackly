//! The new daemon's half of a planned restart (spec 0030 AC-6, AC-9,
//! AC-13, AC-14): it adopts what the plan kept, starts again what the plan
//! stopped, and records what became of each instance. A new boot session
//! closes the plan and starts nothing.

use std::net::TcpListener;
use std::time::Duration;

use serde_json::json;
use stackly_domain::{OperationId, Registry};
use stackly_protocol::instance::{InstanceDesired, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::{ListOperationsParams, OperationKind};
use stackly_protocol::restart::{PlanCloseReason, PlanOutcome, PlanState};
use stackly_protocol::{Call, Reply, ResourceKind, ResourceRef};
use stackly_testkit::allocate_port;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::RestartHarness;
use crate::plan::{create_at, handover_newer, pid_of, pin_newer, short_stop_package};
use crate::support::{Api, PATIENCE, Site, alive};

/// The journal inputs of the `start_instance` operations on `instance`,
/// newest first, read from the registry after the daemon stopped.
async fn start_inputs(harness: &mut RestartHarness, api: &Api, instance: Uuid) -> Vec<String> {
    let page = match api
        .client
        .call(Call::OperationList(ListOperationsParams {
            resource: Some(ResourceRef {
                kind: ResourceKind::Instance,
                id: instance.to_string(),
            }),
            ..ListOperationsParams::default()
        }))
        .await
    {
        Ok(Reply::OperationList(page)) => page,
        other => panic!("operation.list: {other:?}"),
    };
    let ids: Vec<Uuid> = page
        .items
        .iter()
        .filter(|item| item.kind == OperationKind::StartInstance)
        .map(|item| item.id)
        .collect();
    harness.stop().await;
    let registry = harness.site.open_registry();
    ids.into_iter()
        .map(|id| {
            registry
                .operation_journal(OperationId::from_uuid(id).expect("an operation id"))
                .expect("the journal reads")
                .value
                .expect("the operation has a journal")
                .journal
                .input()
                .as_str()
                .to_owned()
        })
        .collect()
}

async fn wait_ready_with_new_pid(api: &Api, id: Uuid, old: u32) -> u32 {
    let summary = api
        .wait_instance(id, "a new ready process", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != old)
                && summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    pid_of(&summary)
}

// covers: AC-9
#[tokio::test(flavor = "multi_thread")]
async fn the_next_daemon_adopts_the_kept_instance_and_starts_the_stopped_one_again() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "after a planned restart the next daemon adopts the keep instance with its pid and start time, starts the stop_restore instance again as a new ready process without a change to its desired state, and records the plan finished with outcomes adopted and restored",
    );
    let mut harness = RestartHarness::new(handover_newer());
    let _scratch = pin_newer(&harness.site);
    let api = harness.start().await;
    let stays = create_at(&api, "stays", "0.4.0", json!({})).await;
    let keeps = create_at(&api, "keeps", "0.5.0", json!({})).await;
    let stays_pid = pid_of(&api.start_ready(stays).await);
    let kept = api.start_ready(keeps).await;
    let (keeps_pid, keeps_started) = {
        let process = kept.process.as_ref().expect("a process runs");
        (process.pid, process.started_at)
    };
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    drop(api);

    let api = harness.next_daemon().await;

    let plan = api.wait_plan(PlanState::Finished).await;
    let outcome = |id: Uuid| {
        plan.instances
            .iter()
            .find(|entry| entry.instance_id == id)
            .expect("the instance is in the plan")
            .clone()
    };
    assert_eq!(outcome(stays).outcome, Some(PlanOutcome::Restored));
    assert!(outcome(stays).ready_at.is_some());
    assert_eq!(outcome(keeps).outcome, Some(PlanOutcome::Adopted));
    assert!(plan.handed_over_at.is_some() && plan.reconciled_at.is_some());
    let restarted = wait_ready_with_new_pid(&api, stays, stays_pid).await;
    assert_ne!(restarted, stays_pid);
    let kept_again = api.get(keeps).await.expect("the kept instance reads");
    let process = kept_again.process.as_ref().expect("the process still runs");
    assert_eq!(
        (process.pid, process.started_at),
        (keeps_pid, keeps_started)
    );
    assert!(process.adopted);
    assert_eq!(
        api.get(stays).await.unwrap().desired,
        InstanceDesired::Running
    );
    // The start that restores the instance carries the trigger of the plan.
    let inputs = start_inputs(&mut harness, &api, stays).await;
    assert_eq!(
        inputs.first().map(String::as_str),
        Some(r#"{"trigger":"planned_restart"}"#),
        "{inputs:?}"
    );
    ev.pass();
}

// covers: AC-9
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_with_no_instance_in_the_plan_still_finishes() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a planned restart of a registry without instances writes an empty plan, the next daemon takes it over and finishes it once it reconciled, and a mutation afterwards is accepted",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let accepted = api
        .confirmed_restart()
        .await
        .expect("the restart is accepted");
    assert!(accepted.plan_id.is_some());
    drop(api);

    let api = harness.next_daemon().await;

    let plan = api.wait_plan(PlanState::Finished).await;
    assert!(plan.instances.is_empty());
    assert!(plan.reconciled_at.is_some() && plan.finished_at.is_some());
    assert_eq!(plan.close_reason, None);
    create_at(&api, "after", "0.4.0", json!({})).await;
    harness.stop().await;
    ev.pass();
}

// covers: AC-13
#[tokio::test(flavor = "multi_thread")]
async fn a_new_boot_session_expires_the_plan_and_starts_nothing() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a plan that the next daemon finds in another boot session expires with boot_changed, every outcome is skipped, the stopped instance stays stopped with its desired state untouched, and nothing starts",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let stays = create_at(&api, "stays", "0.4.0", json!({})).await;
    api.start_ready(stays).await;
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    drop(api);
    assert_eq!(
        harness.exit().await,
        stackly_protocol::daemon::ShutdownMode::Restart
    );
    // Another boot session: the plan names a boot ID the Mac never had.
    let status = std::process::Command::new("/usr/bin/sqlite3")
        .arg(harness.site.prepared.layout().registry_db())
        .arg("UPDATE daemon_restart_plan SET boot_id = '6f9619ff-8b86-4011-b42d-00c04fc964ff'")
        .status()
        .expect("sqlite3 runs");
    assert!(status.success());

    let api = harness.start().await;
    let plan = api.wait_plan(PlanState::Expired).await;
    assert_eq!(plan.close_reason, Some(PlanCloseReason::BootChanged));
    assert!(
        plan.instances
            .iter()
            .all(|entry| entry.outcome == Some(PlanOutcome::Skipped))
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    let summary = api.get(stays).await.unwrap();
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(summary.desired, InstanceDesired::Running);
    assert!(summary.process.is_none());
    harness.stop().await;
    ev.pass();
}

// covers: AC-14
#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_that_ends_while_the_plan_stops_leaves_it_to_the_next_one() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a daemon that ends while a stop of the plan is still running leaves the plan stopping, the next daemon in the same boot takes it over as restoring, starts the instance that had stopped again, tries the stop of the stuck one once more without a change to its desired state, and ends the plan finished",
    );
    let (_scratch, package) = short_stop_package();
    let mut harness = RestartHarness::on(Site::with_package(&package), Vec::new());
    let api = harness.start().await;
    let stuck = create_at(&api, "stuck", "0.4.0", json!({ "ignoreTerm": true })).await;
    let fine = create_at(&api, "fine", "0.4.0", json!({})).await;
    let stuck_pid = pid_of(&api.start_ready(stuck).await);
    let fine_pid = pid_of(&api.start_ready(fine).await);
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    api.wait_instance(fine, "stopped", |summary| {
        summary.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    // The daemon ends before the stuck stop gives up.
    drop(api);
    harness.stop().await;

    let api = harness.start().await;

    let plan = api.wait_plan(PlanState::Finished).await;
    let outcome = |id: Uuid| {
        plan.instances
            .iter()
            .find(|entry| entry.instance_id == id)
            .expect("the instance is in the plan")
            .clone()
    };
    assert_eq!(outcome(fine).outcome, Some(PlanOutcome::Restored));
    assert_eq!(outcome(stuck).outcome, Some(PlanOutcome::Failed));
    assert_eq!(outcome(stuck).failure_code.as_deref(), Some("stop_timeout"));
    wait_ready_with_new_pid(&api, fine, fine_pid).await;
    assert!(alive(stuck_pid), "the stuck process is never killed");
    assert_eq!(
        api.get(stuck).await.unwrap().desired,
        InstanceDesired::Running
    );
    harness.stop().await;
    ev.pass();
}

// covers: AC-6
#[tokio::test(flavor = "multi_thread")]
async fn a_foreign_listener_fails_the_restore_with_endpoint_in_use() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a program that holds the fixed port while the next daemon restores the instance leaves it stopped with the failure endpoint_in_use, records the outcome failed with that code, never touches the listener, and picks no other port",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let port = allocate_port().expect("a free port");
    let mut params = api.params("fixed", json!({}));
    params.endpoints = vec![Api::http_port(port)];
    let accepted = api.create(params).await.expect("the create is accepted");
    let operation = accepted.accepted.expect("an operation");
    let id: Uuid = operation
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    api.wait_state(
        operation.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    api.start_ready(id).await;
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    drop(api);
    assert_eq!(
        harness.exit().await,
        stackly_protocol::daemon::ShutdownMode::Restart
    );
    // The old daemon stopped the instance, so the port is free to take.
    let deadline = std::time::Instant::now() + PATIENCE;
    let foreign = loop {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => break listener,
            Err(error) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "the port stays taken: {error}"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let api = harness.start().await;

    let plan = api.wait_plan(PlanState::Finished).await;
    assert_eq!(plan.instances[0].outcome, Some(PlanOutcome::Failed));
    assert_eq!(
        plan.instances[0].failure_code.as_deref(),
        Some("endpoint_in_use")
    );
    let summary = api.get(id).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(
        summary
            .last_failure
            .as_ref()
            .map(|failure| failure.code.as_str()),
        Some("endpoint_in_use")
    );
    assert_eq!(
        foreign.local_addr().expect("the listener lives").port(),
        port,
        "the foreign listener keeps its port"
    );
    drop(foreign);
    harness.stop().await;
    ev.pass();
}

// covers: AC-9
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_nobody_wants_any_more_is_skipped_and_the_plan_still_finishes() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a stop_restore instance whose desired state became stopped before the restore is not started, its outcome is skipped, and the plan finishes",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let stays = create_at(&api, "stays", "0.4.0", json!({})).await;
    api.start_ready(stays).await;
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    drop(api);
    assert_eq!(
        harness.exit().await,
        stackly_protocol::daemon::ShutdownMode::Restart
    );
    {
        use stackly_domain::{DesiredState, InstanceId, Registry, WriteBatch, WriteOp};
        let registry = harness.site.open_registry();
        let current = registry
            .instance(InstanceId::from_uuid(stays).unwrap())
            .unwrap()
            .value
            .expect("the instance exists");
        let expected = current.revision();
        let mut changed = current;
        changed.set_desired(DesiredState::Stopped);
        registry
            .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
                instance: changed,
                expected,
            }]))
            .expect("the desired state is rewritten");
    }

    let api = harness.start().await;

    let plan = api.wait_plan(PlanState::Finished).await;
    assert_eq!(plan.instances[0].outcome, Some(PlanOutcome::Skipped));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let summary = api.get(stays).await.unwrap();
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert!(summary.process.is_none());
    harness.stop().await;
    ev.pass();
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_whose_data_went_missing_is_skipped_and_the_stored_plan_finishes() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a stop_restore instance whose data folder is gone when the next daemon restores it gets the outcome skipped without a failure code, and the registry, whose CHECK rules accept a failure code only on a failed outcome, stores the plan finished",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let stays = create_at(&api, "stays", "0.4.0", json!({})).await;
    api.start_ready(stays).await;
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    drop(api);
    assert_eq!(
        harness.exit().await,
        stackly_protocol::daemon::ShutdownMode::Restart
    );
    std::fs::remove_dir_all(harness.site.instance_paths(stays).data).expect("the data folder goes");

    let api = harness.start().await;

    let plan = api.wait_plan(PlanState::Finished).await;
    assert_eq!(plan.instances[0].outcome, Some(PlanOutcome::Skipped));
    assert_eq!(plan.instances[0].failure_code, None);
    assert!(api.get(stays).await.unwrap().process.is_none());
    harness.stop().await;
    let stored = harness
        .site
        .open_registry()
        .latest_restart_plan()
        .expect("the plan reads")
        .value
        .expect("the plan is stored");
    assert_eq!(stored.state(), stackly_domain::PlanState::Finished);
    assert_eq!(
        stored.instances()[0].outcome,
        Some(stackly_domain::PlanOutcome::Skipped)
    );
    assert_eq!(stored.instances()[0].failure_code, None);
    ev.pass();
}
