//! The audit rows of the engine paths on a real `SqliteRegistry` (spec 0035
//! AC-1, AC-3): a panic, a no operation answer that writes a batch, a
//! cancel before the start, a resume, the stop of the daemon, and the
//! recovery after a crash each leave exactly their rows, read back through
//! `audit.list`. The `FakeRegistry` side of the same table lives in
//! `crates/core/tests/operation_engine/audit.rs`; `stackly-core` cannot use
//! `stackly-storage`, so the SQLite side runs through the daemon here.

use std::time::Duration;

use serde_json::json;
use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_domain::OperationKind;
use stackly_ipc::Client;
use stackly_protocol::audit::{AuditActorKind, AuditEventInfo, AuditResultKind, ListAuditParams};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::handshake::ClientKind;
use stackly_protocol::operation::{
    CancelDisposition, CancelOperationParams, OperationKind as KindDto, OperationState,
    RecoveryAction, ResumeOperationParams,
};
use stackly_protocol::{Call, Reply};
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::support::{
    PATIENCE, Raw, Script, Scripts, Site, accepted, client, get, key, wait_state, wait_until,
};

/// Every audit row of `operation`, oldest first.
async fn rows(client: &Client, operation: Uuid) -> Vec<AuditEventInfo> {
    let Reply::AuditList(page) = client
        .call(Call::AuditList(ListAuditParams {
            operation_id: Some(operation),
            ..ListAuditParams::default()
        }))
        .await
        .expect("audit.list answers")
    else {
        panic!("not an audit.list reply");
    };
    let mut items = page.items;
    items.reverse();
    items
}

/// Waits until `operation` has at least `count` audit rows; a row is
/// written in the commit of the state, which a client may see a moment
/// after the state itself.
async fn rows_at_least(client: &Client, operation: Uuid, count: usize) -> Vec<AuditEventInfo> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let found = rows(client, operation).await;
        if found.len() >= count {
            return found;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "operation {operation} has {} audit rows, not {count}: {found:?}",
            found.len()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn actions(rows: &[AuditEventInfo]) -> Vec<(&str, AuditResultKind)> {
    rows.iter()
        .map(|row| (row.action.as_str(), row.result))
        .collect()
}

fn detail<'a>(row: &'a AuditEventInfo, key: &str) -> Option<&'a str> {
    row.details
        .iter()
        .find(|detail| detail.key == key)
        .map(|detail| detail.value.as_str())
}

async fn cancel(client: &Client, operation: Uuid) -> CancelDisposition {
    let Reply::OperationCancel(reply) = client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: operation,
        }))
        .await
        .expect("operation.cancel answers")
    else {
        panic!("not a cancel reply");
    };
    reply.disposition
}

async fn stop(daemon: stacklyd::server::DaemonHandle) {
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime starts")
}

// covers: AC-1 (spec 0035, a panic and a no operation answer, on SQLite)
#[tokio::test]
async fn a_panic_and_a_no_op_answer_each_write_one_finished_row_on_sqlite() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "panics",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").panics(),
        ),
    );
    scripts.add(
        "no-op",
        Script::new(
            OperationKind::StopInstance,
            ScriptedWork::new().step("stop"),
        )
        .creating()
        .answering_no_op(),
    );
    let daemon = site.start(&scripts).await;
    let cli = client(&site).await;
    let mut gui = Raw::ready(&site.socket()).await;

    let panicked = accepted(&gui.start(Some(&key()), json!({ "script": "panics" })).await);
    let id = panicked.operation.id;
    let failed = wait_state(&cli, id, OperationState::Failed).await;
    let found = rows_at_least(&cli, id, 1).await;
    assert_eq!(
        actions(&found),
        [("operation.finished", AuditResultKind::Failed)]
    );
    let row = &found[0];
    assert_eq!(detail(row, "reason"), Some("panicked"));
    assert_eq!(
        detail(row, "errorCode"),
        failed
            .operation
            .failure
            .as_ref()
            .map(|failure| failure.error.code())
    );
    assert_eq!(row.operation_id, Some(id), "the correlation ID");
    assert_eq!(row.operation_kind, Some(KindDto::UpdateManager));
    assert_eq!(row.actor, AuditActorKind::User);
    assert_eq!(row.client, Some(ClientKind::Gui), "the trigger's client");
    assert_eq!(row.resource, None, "the manager is not written");
    assert_eq!(row.instance_id, None);

    // The precheck answers without running the work, yet its batch binds
    // the key: one succeeded operation with no steps and one row.
    let answered = accepted(&gui.start(Some(&key()), json!({ "script": "no-op" })).await);
    let id = answered.operation.id;
    assert_eq!(answered.operation.state, OperationState::Succeeded);
    let target: Uuid = answered
        .operation
        .resource
        .as_ref()
        .expect("the no operation answer names its instance")
        .id
        .parse()
        .expect("an instance UUID");
    let found = rows_at_least(&cli, id, 1).await;
    assert_eq!(
        actions(&found),
        [("operation.finished", AuditResultKind::Succeeded)],
        "{found:?}"
    );
    assert_eq!(detail(&found[0], "state"), Some("succeeded"));
    assert_eq!(found[0].operation_kind, Some(KindDto::StopInstance));
    assert_eq!(found[0].instance_id, Some(target));
    assert_eq!(found[0].client, Some(ClientKind::Gui));
    assert!(
        get(&cli, id).await.steps.is_empty(),
        "the work never ran, so no step was written"
    );
    stop(daemon).await;
}

// covers: AC-1, AC-3 (spec 0035, a cancel before the start, on SQLite)
#[tokio::test]
async fn a_cancel_before_the_start_writes_the_senders_row_then_the_cancelled_row_on_sqlite() {
    let site = Site::new();
    let scripts = Scripts::new();
    let blocker = HoldHandle::new();
    scripts.add(
        "blocker",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("drain").hold(&blocker),
        ),
    );
    scripts.add(
        "queued",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap"),
        ),
    );
    let daemon = site.start(&scripts).await;
    let cli = client(&site).await;
    let mut gui = Raw::ready(&site.socket()).await;

    let first = accepted(
        &gui.start(Some(&key()), json!({ "script": "blocker" }))
            .await,
    );
    tokio::time::timeout(PATIENCE, blocker.started())
        .await
        .expect("the first operation runs");
    let queued = accepted(&gui.start(Some(&key()), json!({ "script": "queued" })).await);
    let id = queued.operation.id;
    wait_until(&cli, id, |detail| {
        detail.operation.waiting_on == Some(first.operation.id)
    })
    .await;

    assert_eq!(
        cancel(&cli, id).await,
        CancelDisposition::CancelledBeforeStart
    );
    let found = rows_at_least(&cli, id, 2).await;
    assert_eq!(
        actions(&found),
        [
            ("operation.cancel", AuditResultKind::Succeeded),
            ("operation.finished", AuditResultKind::Cancelled),
        ]
    );
    let (sent, closed) = (&found[0], &found[1]);
    assert_eq!(detail(sent, "disposition"), Some("cancelled_before_start"));
    assert_eq!(sent.client, Some(ClientKind::Cli), "the sender's row");
    assert_eq!(sent.actor, AuditActorKind::User);
    assert_eq!(closed.client, Some(ClientKind::Gui), "the trigger's row");
    assert_eq!(detail(closed, "state"), Some("cancelled"));
    // A nanosecond apart in the registry; the wire keeps milliseconds, so
    // the two may read the same here, and `audit.list` keeps the order.
    assert!(
        sent.occurred_at <= closed.occurred_at,
        "the sender's row comes first in the batch"
    );
    for row in &found {
        assert_eq!(row.operation_id, Some(id));
        assert_eq!(row.operation_kind, Some(KindDto::UpdateManager));
    }

    assert_eq!(cancel(&cli, id).await, CancelDisposition::AlreadyFinal);
    assert_eq!(
        rows(&cli, id).await.len(),
        2,
        "an answer that changes nothing writes no row"
    );
    blocker.release();
    wait_state(&cli, first.operation.id, OperationState::Succeeded).await;
    assert_eq!(
        actions(&rows_at_least(&cli, first.operation.id, 1).await),
        [("operation.finished", AuditResultKind::Succeeded)]
    );
    stop(daemon).await;
}

// covers: AC-1, AC-3 (spec 0035, a resume, on SQLite)
#[tokio::test]
async fn a_resume_writes_the_senders_row_and_a_refused_resume_writes_none_on_sqlite() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "install",
        Script::new(
            OperationKind::InstallRuntime,
            ScriptedWork::new()
                .step("fetch")
                .irreversible()
                .step("verify")
                .repeatable()
                .fail("runtime.checksum_mismatch", true),
        )
        .creating()
        .rebuilt_as(
            ScriptedWork::new()
                .step("fetch")
                .irreversible()
                .step("verify")
                .repeatable(),
        ),
    );
    let daemon = site.start(&scripts).await;
    let cli = client(&site).await;
    let mut gui = Raw::ready(&site.socket()).await;

    let install = accepted(
        &gui.start(Some(&key()), json!({ "script": "install" }))
            .await,
    );
    let id = install.operation.id;
    let stuck = wait_state(&cli, id, OperationState::NeedsRecovery).await;
    let recovery = stuck
        .operation
        .recovery
        .as_ref()
        .expect("needs_recovery carries its recovery");
    assert!(recovery.actions.contains(&RecoveryAction::Resume));
    let found = rows_at_least(&cli, id, 1).await;
    assert_eq!(
        actions(&found),
        [("operation.needs_recovery", AuditResultKind::NeedsRecovery)]
    );
    assert_eq!(detail(&found[0], "hint"), Some("resume"));
    assert_eq!(
        detail(&found[0], "errorCode"),
        Some("runtime.checksum_mismatch")
    );

    let Reply::OperationResume(resumed) = cli
        .call(Call::OperationResume(ResumeOperationParams {
            operation_id: id,
            expected_revision: Some(stuck.operation.revision),
        }))
        .await
        .expect("operation.resume answers")
    else {
        panic!("not a resume reply");
    };
    assert_eq!(resumed.id, id);
    wait_state(&cli, id, OperationState::Succeeded).await;
    let found = rows_at_least(&cli, id, 3).await;
    assert_eq!(
        actions(&found),
        [
            ("operation.needs_recovery", AuditResultKind::NeedsRecovery),
            ("operation.resume", AuditResultKind::Succeeded),
            ("operation.finished", AuditResultKind::Succeeded),
        ]
    );
    assert_eq!(found[1].client, Some(ClientKind::Cli), "the sender's row");
    assert_eq!(found[2].client, Some(ClientKind::Gui), "the trigger's row");
    for row in &found {
        assert_eq!(row.operation_id, Some(id));
        assert_eq!(row.operation_kind, Some(KindDto::InstallRuntime));
    }

    let refused = cli
        .call(Call::OperationResume(ResumeOperationParams {
            operation_id: id,
            expected_revision: None,
        }))
        .await
        .expect_err("a finished operation does not resume");
    assert_eq!(refused.code(), "operation.not_resumable");
    assert_eq!(
        rows(&cli, id).await.len(),
        3,
        "a refused resume writes no row"
    );
    stop(daemon).await;
}

/// A start held in its spawn step, past its point of no return, and an
/// update held in a step that gives way to a stop.
fn closing_scripts(spawning: &HoldHandle, staging: &HoldHandle) -> Scripts {
    let scripts = Scripts::new();
    scripts.add(
        "spawn",
        Script::new(
            OperationKind::StartInstance,
            ScriptedWork::new()
                .step("spawn")
                .irreversible()
                .hold(spawning),
        )
        .creating(),
    );
    scripts.add(
        "update",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("stage").hold(staging),
        ),
    );
    scripts
}

// covers: AC-1 (spec 0035, the recovery at the start and the stop, on SQLite)
#[test]
fn a_crash_and_a_stop_close_their_operations_with_one_finished_row_each_on_sqlite() {
    let site = Site::new();

    // A crash while a start is past its point of no return: no stop runs.
    let crashed = runtime();
    let spawning = HoldHandle::new();
    let (start, target) = crashed.block_on(async {
        let daemon = site
            .start(&closing_scripts(&spawning, &HoldHandle::new()))
            .await;
        let mut gui = Raw::ready(&site.socket()).await;
        let start = accepted(&gui.start(Some(&key()), json!({ "script": "spawn" })).await);
        tokio::time::timeout(PATIENCE, spawning.started())
            .await
            .expect("the start reaches its spawn step");
        let target: Uuid = start
            .operation
            .resource
            .as_ref()
            .expect("the start names its instance")
            .id
            .parse()
            .expect("an instance UUID");
        drop(daemon);
        drop(gui);
        (start.operation.id, target)
    });
    crashed.shutdown_timeout(Duration::from_secs(5));

    // The next daemon closes the start as interrupted; then it stops while
    // an update stages.
    let stopping = runtime();
    let staging = HoldHandle::new();
    let update = stopping.block_on(async {
        let daemon = site
            .start(&closing_scripts(&HoldHandle::new(), &staging))
            .await;
        let cli = client(&site).await;
        let closed = wait_state(&cli, start, OperationState::Failed).await;
        assert_eq!(
            closed.operation.failure.as_ref().map(|f| f.reason.as_str()),
            Some("interrupted")
        );
        let found = rows_at_least(&cli, start, 1).await;
        assert_eq!(
            actions(&found),
            [("operation.finished", AuditResultKind::Failed)],
            "{found:?}"
        );
        let row = &found[0];
        assert_eq!(detail(row, "reason"), Some("interrupted"));
        assert_eq!(detail(row, "errorCode"), Some("operation.interrupted"));
        assert_eq!(row.instance_id, Some(target));
        assert_eq!(row.operation_kind, Some(KindDto::StartInstance));
        assert_eq!(row.client, Some(ClientKind::Gui), "the trigger's client");
        assert_eq!(
            row.config_revision, None,
            "a work note never survives a restart"
        );

        let mut gui = Raw::ready(&site.socket()).await;
        let update = accepted(&gui.start(Some(&key()), json!({ "script": "update" })).await);
        tokio::time::timeout(PATIENCE, staging.started())
            .await
            .expect("the update stages");
        drop(cli);
        drop(gui);
        let stopped = daemon
            .shutdown(ShutdownReason::Requested, std::future::pending())
            .await;
        assert!(!stopped.forced);
        update.operation.id
    });
    drop(stopping);

    let reading = runtime();
    reading.block_on(async {
        let daemon = site.start(&Scripts::new()).await;
        let cli = client(&site).await;
        let stopped = get(&cli, update).await;
        assert_eq!(stopped.operation.state, OperationState::Failed);
        let found = rows(&cli, update).await;
        assert_eq!(
            actions(&found),
            [("operation.finished", AuditResultKind::Failed)],
            "{found:?}"
        );
        assert_eq!(detail(&found[0], "reason"), Some("daemon_stopping"));
        assert_eq!(found[0].operation_id, Some(update));
        assert_eq!(found[0].instance_id, None, "the manager has no instance");
        assert_eq!(
            rows(&cli, start).await.len(),
            1,
            "a later run writes nothing more"
        );
        stop(daemon).await;
    });
}
