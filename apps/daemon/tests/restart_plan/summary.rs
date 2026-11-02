//! What the daemon reports about a restart after the fact (spec 0030
//! AC-11, AC-15): `DaemonInfo.restart` holds the plan of this daemon run
//! while it is open and for `RESTART_SUMMARY_TTL` after it ended, and a
//! plan that could not be stored as aborted still gives the instances it
//! stopped a start.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde_json::json;
use stackly_domain::{DesiredState, InstanceId, PlanState as StoredState, Registry};
use stackly_platform::{Clock, SystemClock};
use stackly_protocol::operation::{ListOperationsParams, OperationKind, OperationState};
use stackly_protocol::restart::PlanState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::ipc::restart::RESTART_SUMMARY_TTL;

use crate::harness::RestartHarness;
use crate::plan::{create_at, pid_of, short_stop_package};
use crate::support::{Api, Site, alive};

/// The system clock plus an offset the test moves forward.
#[derive(Debug, Default)]
struct MovableClock {
    offset_seconds: AtomicI64,
}

impl MovableClock {
    fn advance(&self, by: Duration) {
        self.offset_seconds
            .fetch_add(by.as_secs().cast_signed(), Ordering::SeqCst);
    }
}

impl Clock for MovableClock {
    fn now(&self) -> jiff::Timestamp {
        SystemClock
            .now()
            .checked_add(jiff::SignedDuration::from_secs(
                self.offset_seconds.load(Ordering::SeqCst),
            ))
            .expect("a test sized offset stays in range")
    }
}

// covers: AC-15
#[tokio::test(flavor = "multi_thread")]
async fn daemon_info_keeps_a_finished_plan_for_the_summary_ttl_and_no_longer() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "daemon.get_info shows an open plan whatever its age, shows the finished plan until RESTART_SUMMARY_TTL has passed since it ended, then shows none, and the next daemon run shows none for a plan it never saw",
    );
    let clock = Arc::new(MovableClock::default());
    let mut harness = RestartHarness::new(Vec::new());
    harness.use_clock(clock.clone());
    let api = harness.start().await;
    let id = create_at(&api, "stays", "0.4.0", json!({})).await;
    api.start_ready(id).await;
    assert!(
        api.info().await.restart.is_none(),
        "no plan before a restart"
    );
    api.confirmed_restart()
        .await
        .expect("the restart is accepted");
    api.wait_plan(PlanState::HandedOver).await;

    clock.advance(RESTART_SUMMARY_TTL * 6);

    let open = api.info().await.restart.expect("an open plan has no age");
    assert_eq!(open.state, PlanState::HandedOver);
    drop(api);

    let api = harness.next_daemon().await;
    let finished = api.wait_plan(PlanState::Finished).await;
    assert!(finished.finished_at.is_some());
    clock.advance(RESTART_SUMMARY_TTL - Duration::from_secs(5));
    assert_eq!(
        api.info().await.restart.map(|plan| plan.state),
        Some(PlanState::Finished),
        "a plan that ended just under the TTL ago is still shown"
    );
    clock.advance(Duration::from_secs(10));
    assert!(
        api.info().await.restart.is_none(),
        "a plan that ended over the TTL ago is not shown"
    );
    drop(api);

    harness.stop().await;
    let api = harness.start().await;
    assert!(
        api.info().await.restart.is_none(),
        "a later daemon run shows no plan it never saw"
    );
    harness.stop().await;
    ev.pass();
}

/// Runs `sql` on the registry file of `site` through the `sqlite3` command
/// line tool.
fn registry_sql(site: &Site, sql: &str) {
    let status = Command::new("/usr/bin/sqlite3")
        .arg("-cmd")
        .arg(".timeout 10000")
        .arg(site.prepared.layout().registry_db())
        .arg(sql)
        .status()
        .expect("the sqlite3 tool runs");
    assert!(status.success(), "the statement runs: {sql}");
}

/// Refuses every write that turns a restart plan `aborted`, the way a
/// registry that fails to store the abort would.
fn refuse_aborted_plans(site: &Site) {
    registry_sql(
        site,
        "CREATE TRIGGER refuse_aborted_plan BEFORE UPDATE OF state ON daemon_restart_plan \
         WHEN NEW.state = 'aborted' BEGIN SELECT RAISE(ABORT, 'refused by the test'); END;",
    );
}

// covers: AC-11
#[tokio::test(flavor = "multi_thread")]
async fn an_abort_the_registry_cannot_store_still_fails_the_operation_and_starts_the_stopped_instance_again()
 {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "when the registry refuses to store the aborted plan, the daemon_restart operation still ends failed with the stop error and not a registry error, the instance the plan stopped starts again as a new process, requests are accepted again, the stuck instance is never killed, and the plan stays stopping in the registry",
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

    let operation = match api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
    {
        Ok(Reply::OperationList(page)) => {
            page.items
                .into_iter()
                .find(|item| item.kind == OperationKind::DaemonRestart)
                .expect("the restart is an operation")
                .id
        }
        other => panic!("operation.list: {other:?}"),
    };
    let detail = api.wait_final(operation).await;
    assert_eq!(detail.operation.state, OperationState::Failed);
    assert_eq!(
        Api::failure_code(&detail),
        "supervisor.stop_timeout",
        "the operation fails with the stop error, not the registry error"
    );
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
    assert!(alive(stuck_pid), "the stuck process is never killed");
    assert!(
        harness
            .stop_requested_within(Duration::from_millis(500))
            .await
            .is_none(),
        "the daemon keeps running"
    );
    api.stop(fine)
        .await
        .expect("the quiesce is lifted and the daemon takes requests again");
    let refused = api
        .confirmed_restart()
        .await
        .expect_err("the plan that stayed stopping holds the next restart off");
    // The spec names `daemon.restart_pending`; the guard that finds the open
    // plan answers `daemon.restart_plan_changed` today. Either way a conflict.
    assert!(
        matches!(
            refused.code(),
            "daemon.restart_pending" | "daemon.restart_plan_changed"
        ),
        "{refused:?}"
    );
    harness.stop().await;

    registry_sql(&harness.site, "DROP TRIGGER refuse_aborted_plan;");
    let registry = harness.site.open_registry();
    let plan = registry
        .latest_restart_plan()
        .unwrap()
        .value
        .expect("the plan is stored");
    assert_eq!(
        plan.state(),
        StoredState::Stopping,
        "the abort was never written"
    );
    let instance = registry
        .instance(InstanceId::from_uuid(stuck).unwrap())
        .unwrap()
        .value
        .expect("the instance exists");
    assert_eq!(instance.desired(), DesiredState::Running);
    ev.pass();
}
