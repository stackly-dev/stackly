//! Wake from sleep (spec 0028 AC-9): readiness and health read `waking`,
//! the probes run at once, failures inside the grace change nothing and
//! count for nothing, and a wake while `starting` pushes the readiness
//! deadline back. The fake clock jumps ahead while tokio's clock does not,
//! which is exactly what the wake watch reads as a sleep; no test sleeps
//! for real.

use std::time::Duration;

use jiff::SignedDuration;
use stackly_core::supervisor::SupervisorConfig;
use stackly_domain::{HealthState, InstanceLifecycle, OperationState, ReadinessState};
use stackly_platform::ChildRole;

use crate::common::{Rig, Spec, fast_config, replace};

const GRACE: Duration = Duration::from_millis(2500);

fn wake_config(config: &mut SupervisorConfig) {
    fast_config(config);
    config.wake_check_interval = Duration::from_millis(20);
    config.wake_gap_threshold = Duration::from_millis(500);
    config.wake_grace = GRACE;
}

/// The Mac sleeps for an hour.
fn sleep_an_hour(rig: &Rig) {
    rig.fake.clock().advance(SignedDuration::from_secs(3600));
}

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A rig with an instance that is running and ready, health `healthy`.
async fn ready_rig(
    tweak: impl FnOnce(&mut SupervisorConfig),
) -> (Rig, stackly_domain::InstanceId, stackly_testkit::FakeSpawn) {
    let rig = Rig::build(
        |config| {
            wake_config(config);
            tweak(config);
        },
        |text| replace(text, "intervalSeconds: 5", "intervalSeconds: 1"),
    )
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
    (rig, id, spawn)
}

fn saw_waking(rig: &Rig, id: stackly_domain::InstanceId) -> bool {
    rig.events.statuses_of(id).iter().any(|status| {
        status.readiness.reason().as_str() == "waking"
            && status.health.reason().as_str() == "waking"
    })
}

#[tokio::test]
async fn a_wake_reads_waking_then_probes_at_once_without_touching_the_process() {
    // The routine probe would wait a minute; only the wake can bring the
    // answer sooner.
    let (rig, id, spawn) = ready_rig(|config| {
        config.readiness_refresh_interval = Duration::from_secs(60);
    })
    .await;
    let ports_before = rig.instance_row(id).endpoints().to_vec();
    let before = rig.sup.status(id).readiness;
    assert_eq!(before.reason().as_str(), "probe_ok");

    sleep_an_hour(&rig);
    wait_for("the wake is reported", || saw_waking(&rig, id)).await;

    let waking = rig
        .events
        .statuses_of(id)
        .into_iter()
        .find(|status| status.readiness.reason().as_str() == "waking")
        .expect("a waking status was reported");
    assert_eq!(waking.readiness.state(), ReadinessState::Unknown);
    assert_eq!(waking.health.state(), HealthState::Unknown);
    assert_eq!(
        waking.readiness.detail().map(|detail| detail.as_str()),
        Some("rechecking after sleep")
    );
    assert_eq!(waking.readiness.observed_at(), rig.fake_now());
    assert_eq!(waking.readiness.latency(), None);
    wait_for("readiness and health come back from the probes", || {
        let status = rig.sup.status(id);
        status.readiness.reason().as_str() == "probe_ok"
            && status.health.reason().as_str() == "probe_ok"
    })
    .await;
    let after = rig.sup.status(id);
    assert_eq!(after.readiness.state(), ReadinessState::Ready);
    assert!(after.readiness.latency().is_some());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(rig.instance_row(id).endpoints().to_vec(), ports_before);
    assert!(rig.processes().signals(spawn.pid).is_empty());
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1, "a wake never restarts");
}

#[tokio::test]
async fn failures_inside_the_grace_change_nothing_and_count_for_nothing() {
    let (rig, id, _spawn) = ready_rig(|_| {}).await;
    rig.kill_servers();
    let woke = std::time::Instant::now();

    sleep_an_hour(&rig);
    wait_for("the wake is reported", || saw_waking(&rig, id)).await;
    // Several probes of both loops fail inside the grace.
    tokio::time::sleep(Duration::from_millis(800)).await;

    let inside = rig.sup.status(id);
    assert!(woke.elapsed() < GRACE, "the test stays inside the grace");
    assert_eq!(inside.readiness.reason().as_str(), "waking");
    assert_eq!(inside.readiness.state(), ReadinessState::Unknown);
    assert_eq!(inside.health.reason().as_str(), "waking");
    assert!(
        rig.events
            .statuses_of(id)
            .iter()
            .filter(|status| status.readiness.reason().as_str() == "waking")
            .all(|status| status.health.state() == HealthState::Unknown),
        "no degraded health inside the grace"
    );

    // Past the grace the usual thresholds apply.
    wait_for("readiness falls back after the grace", || {
        rig.sup.status(id).readiness.state() == ReadinessState::NotReady
    })
    .await;
    assert!(
        woke.elapsed() >= GRACE,
        "readiness only fell back after the grace"
    );
    assert_eq!(
        rig.sup.status(id).readiness.reason().as_str(),
        "connect_failed"
    );
    assert!(
        rig.sup.status(id).readiness.latency().is_some(),
        "a failed attempt reports its latency too"
    );
    wait_for("health degrades after the grace", || {
        rig.sup.status(id).health.state() != HealthState::Unknown
    })
    .await;
    assert!(woke.elapsed() >= GRACE);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
}

#[tokio::test]
async fn the_first_success_after_a_wake_ends_its_grace_so_the_next_failure_counts() {
    let (rig, id, _spawn) = ready_rig(|_| {}).await;

    sleep_an_hour(&rig);
    wait_for("the wake is reported", || saw_waking(&rig, id)).await;
    wait_for("readiness comes back", || {
        rig.sup.status(id).readiness.reason().as_str() == "probe_ok"
    })
    .await;
    let back = std::time::Instant::now();
    rig.kill_servers();

    wait_for("readiness falls back", || {
        rig.sup.status(id).readiness.state() == ReadinessState::NotReady
    })
    .await;
    assert!(
        back.elapsed() < GRACE,
        "the failures counted before the grace ran out"
    );
}

#[tokio::test]
async fn a_wake_while_starting_pushes_the_readiness_deadline_back() {
    let rig = Rig::build(wake_config, |text| {
        replace(text, "readinessSeconds: 30", "readinessSeconds: 1")
    })
    .await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;

    // The Mac sleeps before the server is up, and the start stays past its
    // own deadline of one second.
    tokio::time::sleep(Duration::from_millis(300)).await;
    sleep_an_hour(&rig);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    rig.serve(&spawn);

    rig.wait_state(op, OperationState::Succeeded).await;
    assert!(
        rig.events
            .statuses_of(id)
            .iter()
            .all(|status| status.readiness.reason().as_str() != "waking"),
        "a starting instance gets no waking observation"
    );
    assert_eq!(rig.sup.status(id).readiness.state(), ReadinessState::Ready);
}
