//! A process that does not stop (spec 0010 AC-5), a process that crashes
//! until its budget is gone (AC-7) and still reads exhausted after a daemon
//! restart, a start that is cancelled while the process is up but not ready
//! (AC-4, AC-8), and the events of one instance (AC-16).

use std::time::Duration;

use serde_json::json;
use stackly_protocol::events::{EventKind, ShutdownReason, SubscribeParams};
use stackly_protocol::instance::{InstanceHealthState, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::{CancelOperationParams, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use tokio::sync::watch;

use crate::support::{Api, Site, alive, fast, serving, shutdown_server};

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_the_process_ignores_times_out_keeps_it_stopping_and_ends_when_it_exits() {
    let ev = evidence!(
        covers = ["AC-011", "REQ-011"],
        kind = Kind::Fixture,
        expected = "a stop of a process that ignores SIGTERM fails with supervisor.stop_timeout, the instance stays stopping and its process keeps running, and a second stop that finds the process gone ends in stopped",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api
        .create_ready("stubborn", json!({ "ignoreTerm": true }))
        .await;
    let running = api.start_ready(id).await;
    let pid = running
        .process
        .expect("a running instance has a process")
        .pid;
    let port = running.endpoints[0].port.expect("the bound port is shown");

    let accepted = api
        .stop(id)
        .await
        .expect("the stop is accepted")
        .accepted
        .expect("a stop of a running instance begins an operation");
    let failed = api.wait_final(accepted.operation.id).await;
    assert_eq!(failed.operation.state, OperationState::Failed);
    assert_eq!(Api::failure_code(&failed), "supervisor.stop_timeout");
    let stopping = api.get(id).await.expect("the instance can be read");
    assert_eq!(stopping.lifecycle, InstanceLifecycle::Stopping);
    assert!(alive(pid), "the process was not killed");
    assert!(stopping.process.is_some(), "the process is still watched");

    // A second stop sends the strategy again with a new budget; the
    // process leaves on its own while it waits.
    let again = api
        .stop(id)
        .await
        .expect("the second stop is accepted")
        .accepted
        .expect("a stop of a stopping instance begins an operation");
    shutdown_server(port);
    let done = api
        .wait_state(again.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(done.operation.state, OperationState::Succeeded);
    let stopped = api
        .wait_instance(id, "stopped", |summary| {
            summary.lifecycle == InstanceLifecycle::Stopped
        })
        .await;
    assert!(stopped.process.is_none());
    assert!(!alive(pid));
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_process_that_keeps_crashing_ends_failed_with_crash_loop_and_an_exhausted_budget() {
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "a fake that crashes 150 ms after each start is restarted within its budget and then ends failed with last failure crash_loop, restart exhausted, and no process",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api
        .create_ready(
            "crashy",
            json!({ "crashAfterMs": 150, "crashExitCode": 70 }),
        )
        .await;

    // Whether the start operation itself succeeds before the first crash
    // is a race; the end state is not.
    api.start(id).await.expect("the start is accepted");
    let failed = api
        .wait_instance(id, "failed with an exhausted budget", |summary| {
            summary.lifecycle == InstanceLifecycle::Failed && summary.restart.exhausted
        })
        .await;
    let failure = failed.last_failure.expect("a failed instance says why");
    assert_eq!(failure.code, "crash_loop");
    assert!(failed.process.is_none());
    assert_eq!(failed.restart.budget, 5);
    let paths = site
        .prepared
        .layout()
        .instance_paths(id.to_string().parse().expect("an instance id"));
    assert!(serving(&paths.data).is_empty(), "no process is left");

    // A start of the user forgets the history and tries again.
    let accepted = api
        .start(id)
        .await
        .expect("a start from failed is accepted")
        .accepted
        .expect("a start from failed begins an operation");
    let _ = api.wait_final(accepted.operation.id).await;
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_loop_still_reads_exhausted_after_a_daemon_restart() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api
        .create_ready(
            "crashy",
            json!({ "crashAfterMs": 150, "crashExitCode": 70 }),
        )
        .await;
    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start from stopped begins an operation");
    api.wait_instance(id, "failed with an exhausted budget", |summary| {
        summary.lifecycle == InstanceLifecycle::Failed && summary.restart.exhausted
    })
    .await;
    let _ = api.wait_final(accepted.operation.id).await;
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;

    // Spec 0010, *Status dan event*: after a daemon restart `attempts` is
    // 0, and `exhausted` is true for a `failed` instance whose last
    // failure is `crash_loop`.
    let daemon = site.start().await;
    let api = site.api().await;
    let loaded = api.get(id).await.expect("the loaded instance can be read");
    assert_eq!(loaded.lifecycle, InstanceLifecycle::Failed);
    assert_eq!(
        loaded
            .last_failure
            .as_ref()
            .map(|failure| failure.code.as_str()),
        Some("crash_loop")
    );
    assert_eq!(loaded.restart.attempts, 0, "{loaded:?}");
    assert_eq!(loaded.restart.budget, 5, "{loaded:?}");
    assert!(loaded.restart.exhausted, "{loaded:?}");
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_process_that_runs_but_is_not_ready_shows_not_ready_and_a_cancel_stops_it() {
    let ev = evidence!(
        covers = ["AC-012", "BR-010", "REQ-010", "REQ-012"],
        kind = Kind::Fixture,
        expected = "while the fake delays its readiness the instance is running with readiness not_ready and its port not final, failing probes over a while neither restart nor stop the process, and a cancel of the start stops the process and ends the operation cancelled",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api
        .create_ready("slow", json!({ "readyDelayMs": 20_000 }))
        .await;

    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let waiting = api
        .wait_instance(id, "running and not ready", |summary| {
            summary.lifecycle == InstanceLifecycle::Running
                && summary.readiness.state == InstanceReadinessState::NotReady
        })
        .await;
    assert!(waiting.process.is_some());
    assert!(
        waiting.endpoints[0].port.is_some(),
        "the port is stored from the spawn"
    );
    assert!(!waiting.endpoints[0].bound, "but it is not final yet");
    assert_ne!(waiting.health.state, InstanceHealthState::Healthy);

    // Probes keep failing for a while, and a bad readiness never restarts or
    // stops anything (REQ-012): same process, still running, still not ready.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let later = api.get(id).await.expect("the instance reads");
    assert_eq!(later.lifecycle, InstanceLifecycle::Running);
    assert_eq!(later.readiness.state, InstanceReadinessState::NotReady);
    assert_eq!(
        later.process.as_ref().map(|process| process.pid),
        waiting.process.as_ref().map(|process| process.pid),
        "the same process still runs"
    );

    let reply = api
        .client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: accepted.operation.id,
        }))
        .await
        .expect("the cancel is accepted");
    assert!(matches!(reply, Reply::OperationCancel(_)));
    let cancelled = api.wait_final(accepted.operation.id).await;
    assert_eq!(cancelled.operation.state, OperationState::Cancelled);
    let stopped = api
        .wait_instance(id, "stopped", |summary| {
            summary.lifecycle == InstanceLifecycle::Stopped
        })
        .await;
    assert!(stopped.process.is_none());
    let paths = site
        .prepared
        .layout()
        .instance_paths(id.to_string().parse().expect("an instance id"));
    assert!(serving(&paths.data).is_empty());
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_readiness_deadline_that_passes_fails_the_start_but_leaves_the_process_to_become_ready() {
    let ev = evidence!(
        covers = ["AC-012", "BR-010"],
        kind = Kind::Fixture,
        expected = "a fake whose readiness comes after the 30 second deadline of its package ends the start failed with supervisor.readiness_timeout while the process keeps running, and the instance turns ready once the delay is over",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    // The deadline of the fake package is 30 seconds.
    let id = api
        .create_ready("late", json!({ "readyDelayMs": 33_000 }))
        .await;

    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let failed = loop {
        let detail = api.operation(accepted.operation.id).await;
        if detail.operation.state != OperationState::Running
            && detail.operation.state != OperationState::Queued
        {
            break detail;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the deadline never passed"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    assert_eq!(failed.operation.state, OperationState::Failed);
    assert_eq!(Api::failure_code(&failed), "supervisor.readiness_timeout");
    let waiting = api.get(id).await.expect("the instance can be read");
    assert_eq!(waiting.lifecycle, InstanceLifecycle::Running);
    assert!(waiting.process.is_some(), "the process was not stopped");
    assert_eq!(waiting.readiness.state, InstanceReadinessState::NotReady);

    let ready = api
        .wait_instance(id, "ready after the delay", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert!(
        ready.endpoints[0].bound,
        "the port is final after readiness"
    );
    let accepted = api
        .stop(id)
        .await
        .expect("the stop is accepted")
        .accepted
        .expect("a stop begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_events_of_an_instance_follow_its_lifecycle_and_its_status_seq_only_goes_up() {
    let ev = evidence!(
        covers = ["AC-012"],
        kind = Kind::Fixture,
        expected = "daemon.reconciling is published at the start and the end of the start slot, instance.state_changed follows the lifecycle of one instance, and the statusSeq of its instance.health_changed events only goes up",
    );
    let site = Site::new();
    let (gate, open) = watch::channel(false);
    let mut config = fast();
    config.start_gate = Some(open);
    let handle = tokio::spawn({
        let deps = site.deps(config);
        async move {
            let handle = stacklyd::server::start(deps)
                .await
                .expect("the server starts");
            handle.started().await;
            handle
        }
    });
    // The socket is bound before the start slot runs, so a client can
    // subscribe before the slot publishes anything.
    let api = loop {
        match tokio::time::timeout(Duration::from_millis(500), site.api_try()).await {
            Ok(Some(api)) => break api,
            _ => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");
    gate.send_replace(true);
    let daemon = handle.await.expect("the server task finishes");

    let id = api.create_ready("events", json!({})).await;
    api.start_ready(id).await;
    let accepted = api
        .stop(id)
        .await
        .expect("the stop is accepted")
        .accepted
        .expect("a stop begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    api.wait_instance(id, "stopped", |summary| {
        summary.lifecycle == InstanceLifecycle::Stopped
    })
    .await;

    let mut phases = Vec::new();
    let mut lifecycles = Vec::new();
    let mut sequences = Vec::new();
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_millis(500), events.next()).await
    {
        let DecodedFrame::Event(event) = frame else {
            continue;
        };
        match event.kind {
            EventKind::DaemonReconciling(reconciling) => phases.push(reconciling.phase),
            EventKind::InstanceStateChanged(changed) => {
                if lifecycles.last() != Some(&changed.instance.lifecycle) {
                    lifecycles.push(changed.instance.lifecycle);
                }
            }
            EventKind::InstanceHealthChanged(changed) => {
                sequences.push(changed.status_seq.get());
            }
            _ => {}
        }
    }
    use stackly_protocol::instance::ReconcilePhase;
    assert_eq!(phases, [ReconcilePhase::Started, ReconcilePhase::Finished]);
    assert_eq!(
        lifecycles,
        [
            InstanceLifecycle::Uninitialized,
            InstanceLifecycle::Initializing,
            InstanceLifecycle::Stopped,
            InstanceLifecycle::Starting,
            InstanceLifecycle::Running,
            InstanceLifecycle::Stopping,
            InstanceLifecycle::Stopped,
        ]
    );
    assert!(!sequences.is_empty());
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "statusSeq only goes up: {sequences:?}"
    );
    drop((api, events));
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}
