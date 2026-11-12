//! A reader of `instance.get` never sees `running` without a process or
//! with readiness `unknown` (spec 0042 AC-13, the lifecycle fix found by
//! verify, spec 0010 AC-3): a second client polls as fast as the socket
//! answers while a real `mysqld` is started, restarted, and respawned after
//! it dies.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState, InstanceSummary};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::config_calls::pid_of;
use crate::harness::{MY84, MY97, My, Site, create_instance, stop_and_shutdown};

/// One `instance.get` answer, as much of it as the check needs.
#[derive(Debug, Clone)]
struct Seen {
    lifecycle: InstanceLifecycle,
    readiness: InstanceReadinessState,
    pid: Option<u32>,
}

impl Seen {
    fn of(summary: &InstanceSummary) -> Self {
        Self {
            lifecycle: summary.lifecycle,
            readiness: summary.readiness.state,
            pid: summary.process.as_ref().map(|process| process.pid),
        }
    }
}

async fn never_reads_running_without_a_process_around_start_restart_and_respawn(
    my: My,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "window").await;

    let poller = site.api().await;
    let done = Arc::new(AtomicBool::new(false));
    let polling = {
        let done = Arc::clone(&done);
        let id = made.id;
        tokio::spawn(async move {
            let mut seen = Vec::new();
            while !done.load(Ordering::Relaxed) {
                if let Ok(summary) = poller.get(id).await {
                    seen.push(Seen::of(&summary));
                }
            }
            seen
        })
    };

    let first = api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let restart = api
        .restart(made.id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart of a running instance begins an operation");
    api.wait_state(restart.operation.id, OperationState::Succeeded)
        .await;
    let restarted = api
        .wait_instance(made.id, "ready after the restart", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_ne!(pid_of(&restarted), pid_of(&first));

    // The mysqld this test's daemon started dies without a shutdown, so
    // the restart policy respawns it.
    let killed = pid_of(&restarted);
    let killed_it = std::process::Command::new("/bin/kill")
        .args(["-KILL", &killed.to_string()])
        .status()
        .expect("kill runs");
    assert!(killed_it.success(), "mysqld {killed} was killed");
    let respawned = api
        .wait_instance(made.id, "ready after the respawn", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != killed)
        })
        .await;
    assert!(respawned.restart_total.get() >= 1, "{respawned:?}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    done.store(true, Ordering::Relaxed);
    let seen = polling.await.expect("the poller ends");

    let running: Vec<&Seen> = seen
        .iter()
        .filter(|seen| seen.lifecycle == InstanceLifecycle::Running)
        .collect();
    let bad: Vec<&&Seen> = running
        .iter()
        .filter(|seen| seen.pid.is_none() || seen.readiness == InstanceReadinessState::Unknown)
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {} running answers had no process or read unknown: {bad:?}",
        bad.len(),
        running.len()
    );
    let mut pids: Vec<u32> = running.iter().filter_map(|seen| seen.pid).collect();
    pids.sort_unstable();
    pids.dedup();
    assert!(
        pids.len() >= 3,
        "the poller saw the start, the restart, and the respawn: {pids:?}"
    );
    assert!(
        running
            .iter()
            .any(|seen| seen.readiness == InstanceReadinessState::NotReady),
        "the poller saw a running mysqld that was not ready yet"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "{} instance.get answers, {} of them running, none without a process or unknown, across {} mysqld processes",
        seen.len(),
        running.len(),
        pids.len()
    ));
}

// covers: AC-13 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_never_reads_running_without_a_process_around_start_restart_and_respawn() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "a second client polling instance.get as fast as it answers, while a MySQL 8.4 instance is started, restarted, and respawned after its mysqld is killed, never reads lifecycle running without a process or with readiness unknown",
    );
    never_reads_running_without_a_process_around_start_restart_and_respawn(MY84, ev).await;
}

// covers: AC-13 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_never_reads_running_without_a_process_around_start_restart_and_respawn() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "a second client polling instance.get as fast as it answers, while a MySQL 9.7 instance is started, restarted, and respawned after its mysqld is killed, never reads lifecycle running without a process or with readiness unknown",
    );
    never_reads_running_without_a_process_around_start_restart_and_respawn(MY97, ev).await;
}
