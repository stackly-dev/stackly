//! Readiness, health, and `statusSeq` of a running instance (spec 0010
//! AC-4): the four states stay apart, a bad probe never restarts or stops
//! anything, an observation that goes stale is reported, and every visible
//! change gets the next number.

use std::num::NonZeroU32;
use std::time::Duration;

use jiff::SignedDuration;
use stackly_domain::{HealthState, InstanceLifecycle, OperationState, ReadinessState};
use stackly_platform::ChildRole;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Rig, Spec, replace};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn two_failed_probes_turn_ready_into_not_ready_and_never_touch_the_process() {
    let ev = evidence!(
        covers = ["REQ-012", "AC-012"],
        kind = Kind::Fixture,
        expected = "two failed readiness probes turn ready into not_ready with reason connect_failed while the lifecycle stays running, nothing is signalled, no second spawn happens, and readiness recovers on the same process when the server returns",
    );
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    assert_eq!(rig.sup.status(id).readiness.state(), ReadinessState::Ready);

    // The server goes away; the process (a fake) is still "alive".
    rig.kill_servers();
    wait_for("readiness falls back", || {
        rig.sup.status(id).readiness.state() == ReadinessState::NotReady
    })
    .await;

    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert!(
        rig.processes().signals(spawn.pid).is_empty(),
        "nothing is signalled"
    );
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1, "a bad probe never restarts");
    let reason = rig.sup.status(id).readiness.reason().to_string();
    assert_eq!(reason, "connect_failed");

    // The server comes back: ready again, same PID.
    rig.serve(&spawn);
    wait_for("readiness recovers", || {
        rig.sup.status(id).readiness.state() == ReadinessState::Ready
    })
    .await;
    assert_eq!(
        rig.sup.status(id).process.map(|process| process.pid),
        Some(spawn.pid)
    );
    ev.pass();
}

#[tokio::test]
async fn every_visible_change_gets_the_next_status_number() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    rig.kill_servers();
    wait_for("readiness falls back", || {
        rig.sup.status(id).readiness.state() == ReadinessState::NotReady
    })
    .await;

    let statuses = rig.events.statuses_of(id);
    let numbers: Vec<_> = statuses.iter().map(|status| status.status_seq).collect();
    assert!(numbers.len() >= 3, "{numbers:?}");
    assert_eq!(numbers[0], 1, "the numbering starts at 1");
    assert!(
        numbers.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "no gap and no repeat: {numbers:?}"
    );
    // A renewed observation alone is not reported: the same state twice in
    // a row never appears as two updates.
    for pair in statuses.windows(2) {
        assert!(pair[0].differs_visibly(&pair[1]), "{:?}", pair);
    }
    assert!(
        statuses
            .iter()
            .any(|status| status.readiness.state() == ReadinessState::Ready),
        "the ready state was reported"
    );
}

#[tokio::test]
async fn an_observation_that_goes_stale_is_reported_as_unknown_with_reason_stale() {
    let rig = Rig::with_config(|config| {
        config.readiness_failure_threshold = NonZeroU32::new(1000).unwrap();
    })
    .await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;

    // The probes fail but not often enough to say `not_ready`; the old
    // `ready` ages, as after the Mac woke from sleep.
    rig.kill_servers();
    rig.fake.clock().advance(SignedDuration::from_secs(31));
    wait_for("the stale state is reported", || {
        rig.events.statuses_of(id).iter().any(|status| {
            status.readiness.state() == ReadinessState::Unknown
                && status.readiness.reason().as_str() == "stale"
        })
    })
    .await;

    let now = rig.sup.status(id);
    assert_eq!(now.readiness.state(), ReadinessState::Unknown);
    assert_eq!(now.readiness.reason().as_str(), "stale");
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
}

#[tokio::test]
async fn health_runs_on_its_own_interval_and_degrades_then_turns_unhealthy() {
    let rig = Rig::patched(|text| {
        replace(
            text,
            "intervalSeconds: 5",
            "intervalSeconds: 1\n  failureThreshold: 2",
        )
    })
    .await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    wait_for("health is healthy", || {
        rig.sup.status(id).health.state() == HealthState::Healthy
    })
    .await;

    rig.kill_servers();
    wait_for("health degrades", || {
        rig.sup.status(id).health.state() == HealthState::Degraded
    })
    .await;
    wait_for("health turns unhealthy", || {
        rig.sup.status(id).health.state() == HealthState::Unhealthy
    })
    .await;

    // Neither readiness nor health ever changed the lifecycle.
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert!(rig.processes().signals(spawn.pid).is_empty());
}

#[tokio::test]
async fn a_new_instance_reads_unknown_with_reason_daemon_restarted_until_probed() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let status = rig.sup.status(id);

    assert_eq!(status.readiness.state(), ReadinessState::Unknown);
    assert_eq!(status.readiness.reason().as_str(), "daemon_restarted");
    assert_eq!(status.health.state(), HealthState::Unknown);
    assert_eq!(status.status_seq, 0);
    assert!(status.process.is_none());
}

#[tokio::test]
async fn an_instance_the_supervisor_has_not_touched_reports_the_restart_policy() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let restart = rig.sup.status(id).restart;

    // Spec 0010, *Restart policy*: 5 attempts in a sliding window of 5
    // minutes, from the policy constants, not zero.
    assert_eq!(restart.attempts, 0);
    assert_eq!(restart.budget, 5);
    assert_eq!(restart.window_seconds, 300);
    assert_eq!(restart.next_attempt_at, None);
    assert!(!restart.exhausted);
}
