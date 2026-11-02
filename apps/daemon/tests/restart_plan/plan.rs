//! The old daemon's half of a planned restart (spec 0030 AC-7, AC-9 to
//! AC-12): the preview, the quiesce, the stop of the instances the plan
//! stops without a change to their desired state, the abort, and the digest.

use std::time::Duration;

use serde_json::json;
use stackly_core::supervisor::HandoverProof;
use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, PlanDecision, PlanState, Registry,
};
use stackly_protocol::daemon::ShutdownMode;
use stackly_protocol::restart as wire;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::RestartHarness;
use crate::support::{Api, PATIENCE, Site, alive, edited_fake_package, key};

/// The line of the newer fake package, which the tests hand over.
const NEWER: &str = "0.5.0";

pub fn handover_newer() -> Vec<HandoverProof> {
    vec![HandoverProof::new(
        "testing/fake",
        "0.5",
        "restart-plan-test",
    )]
}

/// A copy of the fake package at revision `0.5.0`, pinned on `site`.
pub fn pin_newer(site: &Site) -> stackly_testkit::TestRoot {
    let (scratch, newer) = edited_fake_package(|dir| {
        let manifest = dir.join("provider.yaml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
        std::fs::write(
            &manifest,
            text.replace("revision: \"0.4.0\"", "revision: \"0.5.0\""),
        )
        .expect("the manifest writes");
    });
    site.pin_package(&newer);
    scratch
}

/// A copy of the fake package whose stop gives up after 2 seconds.
pub fn short_stop_package() -> (stackly_testkit::TestRoot, std::path::PathBuf) {
    edited_fake_package(|dir| {
        let manifest = dir.join("provider.yaml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
        assert!(text.contains("stopSeconds: 10"));
        std::fs::write(
            &manifest,
            text.replacen("stopSeconds: 10", "stopSeconds: 2", 1),
        )
        .expect("the manifest writes");
    })
}

/// Creates an instance of the fake provider at `revision` and waits until
/// the create succeeded.
pub async fn create_at(api: &Api, name: &str, revision: &str, extra: serde_json::Value) -> Uuid {
    let mut params = api.params(name, extra);
    params.provider.revision = Some(revision.to_owned());
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    id
}

pub fn pid_of(summary: &stackly_protocol::instance::InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
}

async fn wait_gone(pid: u32) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(tokio::time::Instant::now() < deadline, "{pid} still runs");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// covers: AC-7
#[tokio::test(flavor = "multi_thread")]
async fn the_preview_sorts_the_running_instances_by_the_handover_list() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "daemon.restart_preview lists a running instance of a proven line as keep with handover_proven and one of another line as stop_restore with handover_unproven, leaves the stopped one out, and carries the digest, the launch context, and no blocking operation",
    );
    let mut harness = RestartHarness::new(handover_newer());
    let _scratch = pin_newer(&harness.site);
    let api = harness.start().await;
    let stays = create_at(&api, "stays", "0.4.0", json!({})).await;
    let keeps = create_at(&api, "keeps", NEWER, json!({})).await;
    let _idle = create_at(&api, "idle", "0.4.0", json!({})).await;
    api.start_ready(stays).await;
    api.start_ready(keeps).await;

    let preview = api.preview().await.expect("the preview is read");

    assert_eq!(preview.instances.len(), 2, "{preview:#?}");
    let decision = |id: Uuid| {
        let entry = preview
            .instances
            .iter()
            .find(|entry| entry.instance_id == id)
            .expect("the instance is in the plan");
        assert_eq!(entry.provider, "fake");
        (entry.decision, entry.reason)
    };
    assert_eq!(
        decision(stays),
        (
            wire::PlanDecision::StopRestore,
            wire::PlanReason::HandoverUnproven
        )
    );
    assert_eq!(
        decision(keeps),
        (wire::PlanDecision::Keep, wire::PlanReason::HandoverProven)
    );
    assert!(preview.supervisor_available);
    assert_eq!(
        preview.launch_context,
        stackly_protocol::daemon::LaunchContextKind::Launchd
    );
    assert!(wire::is_plan_digest(&preview.digest));
    assert!(preview.blocking_operations.is_empty());
    assert_eq!(api.preview().await.unwrap().digest, preview.digest);
    harness.stop().await;
    ev.pass();
}

// covers: AC-9
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_stops_what_it_cannot_hand_over_and_leaves_the_rest_running() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a restart under launchd stops the stop_restore instance with its desired state still running, leaves the keep instance's process alone, records the plan handed_over with stopped_at before the daemon asks for its stop order, and answers a repeated request with the same plan",
    );
    let mut harness = RestartHarness::new(handover_newer());
    let _scratch = pin_newer(&harness.site);
    let api = harness.start().await;
    let stays = create_at(&api, "stays", "0.4.0", json!({})).await;
    let keeps = create_at(&api, "keeps", NEWER, json!({})).await;
    let stays_pid = pid_of(&api.start_ready(stays).await);
    let keeps_pid = pid_of(&api.start_ready(keeps).await);
    let digest = api.preview().await.unwrap().digest;
    let restart_key = key();

    let accepted = api
        .restart_daemon(&restart_key, Some(digest.clone()), false)
        .await
        .expect("the restart is accepted");
    let plan_id = accepted.plan_id.expect("a restart names its plan");
    let again = api
        .restart_daemon(&restart_key, Some(digest), false)
        .await
        .expect("a repeated request gets its first answer");

    assert_eq!(accepted.mode, ShutdownMode::Restart);
    assert_eq!(again.plan_id, Some(plan_id));
    assert_eq!(harness.exit().await, ShutdownMode::Restart);
    wait_gone(stays_pid).await;
    assert!(alive(keeps_pid), "a kept instance is not touched");

    let registry = harness.site.open_registry();
    let plan = registry
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(plan.id().as_uuid(), plan_id);
    assert_eq!(plan.state(), PlanState::HandedOver);
    assert!(plan.stored().handed_over_at.is_some());
    let entry = |id: Uuid| {
        plan.instance(InstanceId::from_uuid(id).unwrap())
            .expect("the instance is in the plan")
            .clone()
    };
    assert_eq!(entry(stays).decision, PlanDecision::StopRestore);
    assert!(entry(stays).stopped_at.is_some());
    assert_eq!(entry(keeps).decision, PlanDecision::Keep);
    assert!(entry(keeps).stopped_at.is_none());
    let instance = |id: Uuid| {
        registry
            .instance(InstanceId::from_uuid(id).unwrap())
            .unwrap()
            .value
            .expect("the instance exists")
    };
    let stopped = instance(stays);
    assert_eq!(stopped.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(stopped.desired(), DesiredState::Running);
    assert_eq!(instance(keeps).lifecycle(), InstanceLifecycle::Running);
    ev.pass();
}

// covers: AC-10
#[tokio::test(flavor = "multi_thread")]
async fn an_operation_that_cannot_resume_refuses_the_restart_and_the_daemon_goes_on() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a create that outlasts the quiesce wait refuses the restart with daemon.restart_busy that names it, stops no instance, lifts the quiesce so the next request is accepted, and leaves the daemon running",
    );
    let mut harness = RestartHarness::new(Vec::new());
    harness.tune(|config| {
        config.supervisor.restart_quiesce_wait = Duration::from_millis(300);
    });
    let api = harness.start().await;
    let running = create_at(&api, "running", "0.4.0", json!({})).await;
    let idle = create_at(&api, "idle", "0.4.0", json!({})).await;
    let pid = pid_of(&api.start_ready(running).await);
    let slow = api
        .create(api.params("slow", json!({ "initDelayMs": 5000 })))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("an operation");

    let error = api.confirmed_restart().await.unwrap_err();

    assert_eq!(error.code(), "daemon.restart_busy", "{error:?}");
    let item = error
        .items()
        .first()
        .expect("the blocking operation is listed");
    assert_eq!(item.kind(), "operation");
    assert!(
        item.fields()
            .iter()
            .any(|field| field.key() == "operationId"
                && field.value() == slow.operation.id.to_string()),
        "{item:?}"
    );
    assert!(alive(pid), "nothing was stopped");
    api.start(idle).await.expect("the quiesce is lifted");
    assert!(
        harness
            .stop_requested_within(Duration::from_millis(300))
            .await
            .is_none()
    );
    harness.stop().await;
    ev.pass();
}

// covers: AC-10
#[tokio::test(flavor = "multi_thread")]
async fn a_new_request_is_refused_while_the_restart_waits_for_the_operations_in_flight() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "while a restart waits for a create to finish, an instance.start is refused with daemon.restart_pending, and once the create ended the restart is accepted",
    );
    let mut harness = RestartHarness::new(Vec::new());
    harness.tune(|config| {
        config.supervisor.restart_quiesce_wait = Duration::from_secs(10);
    });
    let api = harness.start().await;
    let running = create_at(&api, "running", "0.4.0", json!({})).await;
    let idle = create_at(&api, "idle", "0.4.0", json!({})).await;
    api.start_ready(running).await;
    api.create(api.params("slow", json!({ "initDelayMs": 1500 })))
        .await
        .expect("the create is accepted");
    let digest = api.preview().await.unwrap().digest;
    let second = harness.site.api().await;
    let waiting =
        tokio::spawn(async move { second.restart_daemon(&key(), Some(digest), false).await });

    // A stop of an instance that does not run changes no plan, and the
    // engine refuses it before it looks at the instance.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let refused = loop {
        match api.stop(idle).await {
            Err(error) if error.code() == "daemon.restart_pending" => break error,
            Err(error) => panic!("an unexpected refusal: {error:?}"),
            Ok(_) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the restart never quiesced"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    };

    assert_eq!(refused.code(), "daemon.restart_pending");
    let accepted = waiting
        .await
        .expect("the request task ends")
        .expect("the restart is accepted");
    assert!(accepted.plan_id.is_some());
    assert_eq!(harness.exit().await, ShutdownMode::Restart);
    ev.pass();
}

// covers: AC-10
#[tokio::test(flavor = "multi_thread")]
async fn force_goes_on_although_a_create_is_still_running() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "with force a restart that would be refused for a running create is accepted, and the daemon asks for its stop order",
    );
    let mut harness = RestartHarness::new(Vec::new());
    harness.tune(|config| {
        config.supervisor.restart_quiesce_wait = Duration::from_millis(200);
    });
    let api = harness.start().await;
    api.create(api.params("slow", json!({ "initDelayMs": 3000 })))
        .await
        .expect("the create is accepted");
    let digest = api.preview().await.unwrap().digest;

    let refused = api
        .restart_daemon(&key(), Some(digest.clone()), false)
        .await
        .unwrap_err();
    let accepted = api
        .restart_daemon(&key(), Some(digest), true)
        .await
        .expect("force overrides the refusal");

    assert_eq!(refused.code(), "daemon.restart_busy");
    assert!(accepted.plan_id.is_some());
    assert_eq!(harness.exit().await, ShutdownMode::Restart);
    ev.pass();
}

// covers: AC-11
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_that_does_not_stop_aborts_the_restart_and_the_others_start_again() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "an instance that ignores SIGTERM aborts the plan with stop_timeout, the instance stopped before it starts again as a new process, the stuck one stays watched, the daemon keeps running and takes requests again, and no SIGKILL is sent",
    );
    let (_scratch, package) = short_stop_package();
    let mut harness = RestartHarness::on(Site::with_package(&package), Vec::new());
    let api = harness.start().await;
    let stuck = create_at(&api, "stuck", "0.4.0", json!({ "ignoreTerm": true })).await;
    let fine = create_at(&api, "fine", "0.4.0", json!({})).await;
    let stuck_pid = pid_of(&api.start_ready(stuck).await);
    let fine_pid = pid_of(&api.start_ready(fine).await);

    let accepted = api
        .confirmed_restart()
        .await
        .expect("the restart is accepted");

    assert!(accepted.plan_id.is_some());
    let restarted = api
        .wait_instance(fine, "a new process", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != fine_pid)
                && summary.readiness.state
                    == stackly_protocol::instance::InstanceReadinessState::Ready
        })
        .await;
    assert_ne!(pid_of(&restarted), fine_pid);
    wait_gone(fine_pid).await;
    assert!(alive(stuck_pid), "the stuck process is never killed");
    assert!(
        harness
            .stop_requested_within(Duration::from_millis(500))
            .await
            .is_none()
    );
    api.stop(fine)
        .await
        .expect("the daemon takes requests again");
    harness.stop().await;

    let registry = harness.site.open_registry();
    let plan = registry
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(plan.state(), PlanState::Aborted);
    assert_eq!(
        plan.stored().close_reason,
        Some(stackly_domain::PlanCloseReason::StopTimeout)
    );
    assert!(plan.instances().iter().all(|entry| entry.outcome.is_some()));
    let instance = registry
        .instance(InstanceId::from_uuid(stuck).unwrap())
        .unwrap()
        .value
        .expect("the instance exists");
    assert_eq!(instance.desired(), DesiredState::Running);
    ev.pass();
}

// covers: AC-12
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_that_is_not_the_confirmed_one_is_refused_before_anything_stops() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a digest that differs from the plan computed now is refused with daemon.restart_plan_changed that names both, no instance stops, and the daemon takes requests again at once",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let running = create_at(&api, "running", "0.4.0", json!({})).await;
    let idle = create_at(&api, "idle", "0.4.0", json!({})).await;
    let pid = pid_of(&api.start_ready(running).await);
    let actual = api.preview().await.unwrap().digest;
    let stale = "ab".repeat(32);

    let error = api
        .restart_daemon(&key(), Some(stale.clone()), false)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "daemon.restart_plan_changed");
    assert_eq!(error.detail("expected"), Some(stale.as_str()));
    assert_eq!(error.detail("actual"), Some(actual.as_str()));
    assert!(alive(pid));
    api.start(idle).await.expect("no quiesce was left behind");
    assert!(
        harness
            .stop_requested_within(Duration::from_millis(300))
            .await
            .is_none()
    );
    harness.stop().await;
    ev.pass();
}

// covers: AC-7
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_in_recovery_mode_exits_75_without_a_plan() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "in recovery mode, where no supervisor serves, daemon.shutdown with mode restart is accepted without a plan, asks for the stop order of a restart, and a repeated request is refused with daemon.shutting_down instead of registry.unavailable",
    );
    let mut harness = RestartHarness::new(Vec::new());
    harness.start().await;
    harness.stop().await;
    // A registry newer than the app puts the next start in recovery mode.
    let db = harness.site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).expect("the registry file can be read");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).expect("the registry file can be written");
    let api = harness.start().await;
    assert!(
        harness.daemon().health().recovery.is_some(),
        "the daemon is in recovery mode"
    );
    let preview = api.preview().await.expect("the preview is served");
    assert!(!preview.supervisor_available);

    let accepted = api
        .restart_daemon(&key(), None, false)
        .await
        .expect("the restart is accepted");

    assert_eq!(accepted.mode, ShutdownMode::Restart);
    assert_eq!(accepted.plan_id, None);
    assert_eq!(
        harness.stop_requested_within(Duration::from_secs(2)).await,
        Some(ShutdownMode::Restart)
    );
    let again = api.restart_daemon(&key(), None, false).await.unwrap_err();
    assert_eq!(again.code(), "daemon.shutting_down", "{again:?}");
    harness.stop().await;
    ev.pass();
}
