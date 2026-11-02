//! What a planned restart does when a step after the plan is written ends
//! early (spec 0030 AC-10, AC-11, AC-14): the plan closes as aborted, the
//! instances it stopped start again, and the daemon takes requests again.
//! Also the request that carries no idempotency key.

use std::time::Duration;

use serde_json::json;
use stackly_domain::{PlanState as StoredState, Registry};
use stackly_ipc::CallOptions;
use stackly_protocol::daemon::{ShutdownMode, ShutdownParams};
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::{
    CancelOperationParams, ListOperationsParams, OperationKind, OperationState,
};
use stackly_protocol::restart::PlanState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::RestartHarness;
use crate::plan::{create_at, handover_newer, pid_of, pin_newer, short_stop_package};
use crate::summary::{refuse_aborted_plans, registry_sql};
use crate::support::{Api, Site, alive, key};

/// The `daemon_restart` operation of the daemon.
async fn restart_operation(api: &Api) -> Uuid {
    let deadline = tokio::time::Instant::now() + crate::support::PATIENCE;
    loop {
        match api
            .client
            .call(Call::OperationList(ListOperationsParams::default()))
            .await
        {
            Ok(Reply::OperationList(page)) => {
                if let Some(item) = page
                    .items
                    .into_iter()
                    .find(|item| item.kind == OperationKind::DaemonRestart)
                {
                    return item.id;
                }
            }
            other => panic!("operation.list: {other:?}"),
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no restart operation"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// covers: AC-10
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_without_an_idempotency_key_is_refused_and_stops_nothing() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "daemon.shutdown with mode restart and no idempotency key is refused with operation.idempotency_key_required, no instance stops, and no plan is written",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let running = create_at(&api, "running", "0.4.0", json!({})).await;
    let pid = pid_of(&api.start_ready(running).await);
    let params = ShutdownParams {
        mode: ShutdownMode::Restart,
        expected_plan_digest: None,
        force: false,
    };

    let error = api
        .client
        .call_detailed(Call::DaemonShutdown(params), CallOptions::default())
        .await
        .expect_err("a restart needs a key");

    assert_eq!(
        error.code(),
        "operation.idempotency_key_required",
        "{error:?}"
    );
    assert!(alive(pid), "nothing was stopped");
    assert!(
        harness
            .stop_requested_within(Duration::from_millis(300))
            .await
            .is_none()
    );
    harness.stop().await;
    assert!(
        harness
            .site
            .open_registry()
            .latest_restart_plan()
            .unwrap()
            .value
            .is_none(),
        "no plan was written"
    );
    ev.pass();
}

// covers: AC-10
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_start_in_flight_is_waited_for_before_the_plan_is_written() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a restart asked while an instance.start of a kind that closes as interrupted is still running waits for that start to finish, so the start is not abandoned half way and the instance it started is ready when the reply comes",
    );
    let mut harness = RestartHarness::new(handover_newer());
    let _scratch = pin_newer(&harness.site);
    harness.tune(|config| {
        config.supervisor.restart_quiesce_wait = Duration::from_secs(20);
    });
    let api = harness.start().await;
    let keeps = create_at(&api, "keeps", "0.5.0", json!({})).await;
    let slow = create_at(&api, "slow", "0.5.0", json!({ "readyDelayMs": 1500 })).await;
    api.start_ready(keeps).await;
    let started = api.start(slow).await.expect("the start is accepted");

    let begun = tokio::time::Instant::now();
    let result = api.restart_daemon(&key(), None, false).await;

    assert!(
        begun.elapsed() >= Duration::from_millis(1000),
        "the restart answered after {:?}, before the start ended",
        begun.elapsed()
    );
    let detail = api
        .operation(
            started
                .accepted
                .as_ref()
                .expect("a start is an operation")
                .operation
                .id,
        )
        .await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    // A start that finished in the wait may change the plan, which the
    // daemon then refuses instead of stopping on a stale plan.
    match result {
        Ok(accepted) => assert!(accepted.plan_id.is_some()),
        Err(error) => assert_eq!(error.code(), "daemon.restart_plan_changed", "{error:?}"),
    }
    assert_eq!(
        api.get(slow).await.unwrap().readiness.state,
        InstanceReadinessState::Ready
    );
    harness.stop().await;
    ev.pass();
}

// covers: AC-11
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_that_cannot_be_handed_over_is_aborted_and_the_stopped_instance_starts_again() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "when the registry refuses to store handed_over, the daemon_restart operation fails, the plan is stored aborted, the instance it stopped starts again as a new process, the daemon keeps running, and requests are accepted again",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let fine = create_at(&api, "fine", "0.4.0", json!({})).await;
    let fine_pid = pid_of(&api.start_ready(fine).await);
    registry_sql(
        &harness.site,
        "CREATE TRIGGER refuse_handed_over BEFORE UPDATE OF state ON daemon_restart_plan \
         WHEN NEW.state = 'handed_over' BEGIN SELECT RAISE(ABORT, 'refused by the test'); END;",
    );

    api.confirmed_restart()
        .await
        .expect("the restart is accepted");

    let detail = api.wait_final(restart_operation(&api).await).await;
    assert_eq!(detail.operation.state, OperationState::Failed);
    api.wait_instance(fine, "a new process", |summary| {
        summary
            .process
            .as_ref()
            .is_some_and(|process| process.pid != fine_pid)
            && summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    let aborted = api.wait_plan(PlanState::Aborted).await;
    assert!(aborted.instances[0].stopped_at.is_some());
    assert!(
        harness
            .stop_requested_within(Duration::from_millis(500))
            .await
            .is_none(),
        "the daemon keeps running"
    );
    api.stop(fine).await.expect("requests are accepted again");
    harness.stop().await;

    registry_sql(&harness.site, "DROP TRIGGER refuse_handed_over;");
    let plan = harness
        .site
        .open_registry()
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(plan.state(), StoredState::Aborted);
    ev.pass();
}

// covers: AC-11
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_while_the_plan_stops_aborts_it_and_the_daemon_takes_requests_again() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "operation.cancel on the daemon_restart operation while an instance is still stopping ends the operation cancelled, stores the plan aborted instead of leaving it stopping, lifts the quiesce, never kills the stuck process, and keeps the daemon running",
    );
    let (_scratch, package) = short_stop_package();
    let mut harness = RestartHarness::on(Site::with_package(&package), Vec::new());
    let api = harness.start().await;
    let stuck = create_at(&api, "stuck", "0.4.0", json!({ "ignoreTerm": true })).await;
    let other = create_at(&api, "other", "0.4.0", json!({})).await;
    let stuck_pid = pid_of(&api.start_ready(stuck).await);
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    let operation = restart_operation(&api).await;

    api.client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: operation,
        }))
        .await
        .expect("the cancel is accepted");

    let detail = api.wait_final(operation).await;
    assert_eq!(detail.operation.state, OperationState::Cancelled);
    api.wait_plan(PlanState::Aborted).await;
    assert!(alive(stuck_pid), "the stuck process is never killed");
    api.start(other)
        .await
        .expect("the quiesce is lifted and requests are accepted again");
    assert!(
        harness
            .stop_requested_within(Duration::from_millis(500))
            .await
            .is_none(),
        "the daemon keeps running"
    );
    harness.stop().await;
    let plan = harness
        .site
        .open_registry()
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(plan.state(), StoredState::Aborted);
    ev.pass();
}

// covers: AC-11, AC-14
#[tokio::test(flavor = "multi_thread")]
async fn a_stopping_plan_whose_operation_ended_is_closed_aborted_by_the_next_daemon() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "an abort the registry could not store leaves the plan stopping with a failed operation; the next daemon of the same boot closes that plan as aborted instead of restoring it, and does not stop the instance a second time",
    );
    let (_scratch, package) = short_stop_package();
    let mut harness = RestartHarness::on(Site::with_package(&package), Vec::new());
    let api = harness.start().await;
    let stuck = create_at(&api, "stuck", "0.4.0", json!({ "ignoreTerm": true })).await;
    let fine = create_at(&api, "fine", "0.4.0", json!({})).await;
    let stuck_pid = pid_of(&api.start_ready(stuck).await);
    let fine_pid = pid_of(&api.start_ready(fine).await);
    refuse_aborted_plans(&harness.site);
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    api.wait_final(restart_operation(&api).await).await;
    api.wait_instance(fine, "a new process", |summary| {
        summary
            .process
            .as_ref()
            .is_some_and(|process| process.pid != fine_pid)
            && summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    // The view of the aborted plan comes from memory although the registry
    // refused it.
    let view = api.wait_plan(PlanState::Aborted).await;
    assert_eq!(view.instances.len(), 2);
    drop(api);
    harness.stop().await;
    registry_sql(&harness.site, "DROP TRIGGER refuse_aborted_plan;");
    let stored = harness
        .site
        .open_registry()
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(stored.state(), StoredState::Stopping);

    let api = harness.start().await;

    let plan = api.wait_plan(PlanState::Aborted).await;
    assert!(plan.close_reason.is_some());
    assert!(alive(stuck_pid), "the stuck process is never killed");
    let again = harness
        .site
        .open_registry()
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(again.state(), StoredState::Aborted);
    harness.stop().await;
    ev.pass();
}

// covers: AC-11, AC-14
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_stop_that_the_daemon_never_finished_is_restored_by_the_next_one() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "a cancel asked while a stop of the plan is still running, followed by the end of the daemon before the plan could abort, leaves the operation cancelled after recovery; the next daemon closes the plan aborted and starts the instance the plan had stopped again, and never kills the stuck one",
    );
    let mut harness = RestartHarness::new(Vec::new());
    let api = harness.start().await;
    let stuck = create_at(&api, "stuck", "0.4.0", json!({ "ignoreTerm": true })).await;
    let fine = create_at(&api, "fine", "0.4.0", json!({})).await;
    let stuck_pid = pid_of(&api.start_ready(stuck).await);
    let fine_pid = pid_of(&api.start_ready(fine).await);
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    let operation = restart_operation(&api).await;
    api.wait_instance(fine, "stopped", |summary| {
        summary.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    api.client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: operation,
        }))
        .await
        .expect("the cancel is accepted");
    // The daemon ends while the stuck stop still runs, so the abandon wins
    // over the cancel at the next step boundary.
    drop(api);
    harness.stop().await;

    let api = harness.start().await;

    let plan = api.wait_plan(PlanState::Aborted).await;
    assert!(plan.close_reason.is_some());
    api.wait_instance(fine, "a new ready process", |summary| {
        summary
            .process
            .as_ref()
            .is_some_and(|process| process.pid != fine_pid)
            && summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    assert!(alive(stuck_pid), "the stuck process is never killed");
    harness.stop().await;
    let stored = harness
        .site
        .open_registry()
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(stored.state(), StoredState::Aborted);
    ev.pass();
}
