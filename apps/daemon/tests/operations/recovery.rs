//! A restart in the middle of an operation and the stop with operations
//! running: each daemon run gets its own runtime, so dropping that runtime
//! ends every task of the run the way a crash would, and the next run finds
//! what the registry recorded.

use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_domain::OperationKind;
use stackly_protocol::Call;
use stackly_protocol::Reply;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::operation::{OperationState, StepState};
use tokio::runtime::Runtime;

use crate::support::{
    PATIENCE, Raw, Script, Scripts, Site, accepted, checked, client, key, wait_state,
};

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime starts")
}

/// A resumable script held in its repeatable step, rebuilt without the
/// hold, and a script that cannot resume, held after it created an
/// instance folder.
fn scripts(resumable: &HoldHandle, other: &HoldHandle) -> Scripts {
    let scripts = Scripts::new();
    scripts.add(
        "download",
        Script::new(
            OperationKind::InstallRuntime,
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .hold(resumable),
        )
        .creating()
        .rebuilt_as(ScriptedWork::new().step("download").repeatable()),
    );
    scripts.add(
        "create",
        Script::new(
            OperationKind::CreateInstance,
            ScriptedWork::new()
                .step("reserve")
                .creates_instance_tree()
                .step("init")
                .hold(other),
        )
        .creating(),
    );
    scripts
}

#[test]
fn a_restart_in_the_middle_of_operations_resumes_one_and_undoes_the_other() {
    let site = Site::new();
    let resumable = HoldHandle::new();
    let other = HoldHandle::new();
    let first = scripts(&resumable, &other);

    let crashed = runtime();
    let (download, create) = crashed.block_on(async {
        let daemon = site.start(&first).await;
        let mut raw = Raw::ready(&site.socket()).await;
        let download = accepted(
            &raw.start(Some(&key()), json!({ "script": "download" }))
                .await,
        );
        let create = accepted(&raw.start(Some(&key()), json!({ "script": "create" })).await);
        tokio::time::timeout(PATIENCE, resumable.started())
            .await
            .unwrap();
        tokio::time::timeout(PATIENCE, other.started())
            .await
            .unwrap();
        // No stop runs: the handle and the connection go, and the runtime
        // drops every task of the run, as in a crash.
        drop(daemon);
        drop(raw);
        (download.operation.id, create.operation.id)
    });
    crashed.shutdown_timeout(Duration::from_secs(5));

    let restarted = runtime();
    restarted.block_on(async {
        let second = scripts(&HoldHandle::new(), &HoldHandle::new());
        let daemon = site.start(&second).await;
        let client = client(&site).await;

        let resumed = wait_state(&client, download, OperationState::Succeeded).await;
        let undone = wait_state(&client, create, OperationState::Failed).await;
        let Reply::OperationGetStats(stats) = client.call(Call::OperationGetStats).await.unwrap()
        else {
            panic!("not stats");
        };

        assert_eq!(resumed.steps.len(), 1);
        assert_eq!(resumed.steps[0].state, StepState::Done);
        assert_eq!(resumed.steps[0].attempts, 2);
        let failure = undone.operation.failure.expect("a failure");
        assert_eq!(failure.reason, "interrupted");
        assert!(failure.retryable);
        checked(&failure.error);
        assert_eq!(failure.error.code(), "operation.interrupted");
        assert_eq!(failure.error.operation_id(), Some(create));
        assert!(!site.instance_root(&undone.created[0].reference).exists());
        assert_eq!(stats.interrupted_at_start, 2);
        assert_eq!(stats.resumed_at_start, 1);
        assert_eq!(stats.auto_compensated_at_start, 1);
        daemon
            .shutdown(ShutdownReason::Requested, std::future::pending())
            .await;
    });
}

#[test]
fn a_stop_parks_a_resumable_operation_within_its_grace_and_the_next_start_runs_it() {
    let site = Site::new();
    let resumable = HoldHandle::new();
    let other = HoldHandle::new();
    let first = scripts(&resumable, &other);

    let stopping = runtime();
    let (download, create, took) = stopping.block_on(async {
        let daemon = site.start(&first).await;
        let client = client(&site).await;
        let mut raw = Raw::ready(&site.socket()).await;
        let download = accepted(
            &raw.start(Some(&key()), json!({ "script": "download" }))
                .await,
        );
        let create = accepted(&raw.start(Some(&key()), json!({ "script": "create" })).await);
        tokio::time::timeout(PATIENCE, resumable.started())
            .await
            .unwrap();
        tokio::time::timeout(PATIENCE, other.started())
            .await
            .unwrap();
        drop(client);
        drop(raw);
        let started = Instant::now();
        let stopped = daemon
            .shutdown(ShutdownReason::Requested, std::future::pending())
            .await;
        assert!(!stopped.forced);
        (
            download.operation.id,
            create.operation.id,
            started.elapsed(),
        )
    });
    drop(stopping);

    let restarted = runtime();
    restarted.block_on(async {
        let second = scripts(&HoldHandle::new(), &HoldHandle::new());
        let daemon = site.start(&second).await;
        let client = client(&site).await;

        let resumed = wait_state(&client, download, OperationState::Succeeded).await;
        let stopped = crate::support::get(&client, create).await;

        assert!(took < Duration::from_secs(11), "the stop took {took:?}");
        assert_eq!(resumed.steps[0].attempts, 2);
        let failure = stopped
            .operation
            .failure
            .expect("stopped before its point of no return");
        assert_eq!(failure.reason, "daemon_stopping");
        assert!(failure.retryable);
        assert_eq!(failure.error.code(), "operation.daemon_stopping");
        assert!(!site.instance_root(&stopped.created[0].reference).exists());
        daemon
            .shutdown(ShutdownReason::Requested, std::future::pending())
            .await;
    });
}

#[test]
fn a_second_signal_skips_the_engine_grace_and_the_next_start_recovers_the_operation() {
    let site = Site::new();
    let resumable = HoldHandle::new();
    let first = scripts(&resumable, &HoldHandle::new());

    let stopping = runtime();
    let (download, took) = stopping.block_on(async {
        let daemon = site.start(&first).await;
        let mut raw = Raw::ready(&site.socket()).await;
        let download = accepted(
            &raw.start(Some(&key()), json!({ "script": "download" }))
                .await,
        );
        tokio::time::timeout(PATIENCE, resumable.started())
            .await
            .unwrap();
        drop(raw);
        let started = Instant::now();
        let stopped = daemon
            .shutdown(ShutdownReason::Signal, std::future::ready(()))
            .await;
        assert!(stopped.forced);
        (download.operation.id, started.elapsed())
    });
    // The operation's task still holds its step; it ends with the runtime.
    stopping.shutdown_timeout(Duration::from_secs(5));

    let restarted = runtime();
    restarted.block_on(async {
        let second = scripts(&HoldHandle::new(), &HoldHandle::new());
        let daemon = site.start(&second).await;
        let client = client(&site).await;

        let resumed = wait_state(&client, download, OperationState::Succeeded).await;
        let Reply::OperationGetStats(stats) = client.call(Call::OperationGetStats).await.unwrap()
        else {
            panic!("not stats");
        };

        assert!(
            took < Duration::from_secs(2),
            "the forced stop took {took:?}"
        );
        assert_eq!(resumed.steps[0].attempts, 2);
        assert_eq!(stats.interrupted_at_start, 1);
        assert_eq!(stats.resumed_at_start, 1);
        daemon
            .shutdown(ShutdownReason::Requested, std::future::pending())
            .await;
    });
}
