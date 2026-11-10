//! The point of no return over the wire (spec 0035 AC-8, AC-9, and the
//! sender rows of AC-3) on a real `SqliteRegistry`: a staged work whose
//! commit step cannot be undone, started by a GUI connection and cancelled,
//! abandoned, or left to finish by a CLI one. Every summary carries
//! `cancel`, and `audit.list` holds the rows of each path in commit order.

use serde_json::json;
use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_domain::OperationKind;
use stackly_ipc::Client;
use stackly_protocol::audit::{AuditActorKind, AuditEventInfo, AuditResultKind, ListAuditParams};
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::handshake::ClientKind;
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::{
    AbandonOperationParams, CancelAvailability, CancelDisposition, CancelOperationParams,
    CreatedState, ListOperationsParams, OperationState, OperationSummary,
};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use crate::support::{
    PATIENCE, Raw, Script, Scripts, Site, accepted, client, get, key, list, wait_state,
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
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

/// A restore that stages a folder, then commits in a step that cannot be
/// undone; `staging` and `committing` hold each step, and `fails` breaks
/// the commit.
fn staged(staging: Option<&HoldHandle>, committing: &HoldHandle, fails: bool) -> Script {
    let mut work = ScriptedWork::new().step("stage").creates_instance_tree();
    if let Some(staging) = staging {
        work = work.hold(staging);
    }
    work = work.step("commit").irreversible().hold(committing);
    if fails {
        work = work.fail("internal.unexpected", false);
    }
    Script::new(OperationKind::RestoreInstance, work).creating()
}

// covers: AC-8, AC-9, AC-3 (spec 0035)
#[tokio::test]
async fn a_cancel_while_staging_undoes_it_and_one_past_the_commit_finishes_safely() {
    let site = Site::new();
    let scripts = Scripts::new();
    let staging = HoldHandle::new();
    let never = HoldHandle::new();
    let committing = HoldHandle::new();
    scripts.add("early", staged(Some(&staging), &never, false));
    scripts.add("late", staged(None, &committing, false));
    let daemon = site.start(&scripts).await;
    let cli = client(&site).await;
    let mut gui = Raw::ready(&site.socket()).await;
    let target = site.instance_root(&Uuid::now_v7().to_string());
    std::fs::create_dir_all(&target).expect("the sentinel folder is made");
    let sentinel = target.join("data.txt");
    std::fs::write(&sentinel, b"the data before any restore").expect("the sentinel is written");

    // Cancelled while it stages: undone, nothing of the target touched.
    let early = accepted(&gui.start(Some(&key()), json!({ "script": "early" })).await);
    let id = early.operation.id;
    tokio::time::timeout(PATIENCE, staging.started())
        .await
        .expect("the staging step runs");
    assert_eq!(
        get(&cli, id).await.operation.cancel,
        Some(CancelAvailability::Available)
    );
    assert_eq!(cancel(&cli, id).await, CancelDisposition::Requested);
    let cancelled = wait_state(&cli, id, OperationState::Cancelled).await;
    assert_eq!(cancelled.operation.cancel, Some(CancelAvailability::None));
    assert_eq!(cancelled.created[0].state, CreatedState::Compensated);
    assert!(
        !site.instance_root(&cancelled.created[0].reference).exists(),
        "the staging folder is gone"
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("the sentinel reads"),
        b"the data before any restore"
    );
    let found = rows(&cli, id).await;
    assert_eq!(
        actions(&found),
        [
            ("operation.cancel", AuditResultKind::Succeeded),
            ("operation.finished", AuditResultKind::Cancelled),
        ]
    );
    assert_eq!(detail(&found[0], "disposition"), Some("requested"));
    assert_eq!(found[0].client, Some(ClientKind::Cli), "the sender's row");
    assert_eq!(found[0].actor, AuditActorKind::User);
    assert_eq!(found[1].client, Some(ClientKind::Gui), "the trigger's row");
    for row in &found {
        assert_eq!(row.operation_id, Some(id));
    }

    // Past the commit: Cancel is off, a cancel continues to a safe end.
    let late = accepted(&gui.start(Some(&key()), json!({ "script": "late" })).await);
    let id = late.operation.id;
    tokio::time::timeout(PATIENCE, committing.started())
        .await
        .expect("the commit step runs");
    assert_eq!(
        get(&cli, id).await.operation.cancel,
        Some(CancelAvailability::PastPointOfNoReturn)
    );
    assert_eq!(
        cancel(&cli, id).await,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    assert_eq!(
        get(&cli, id).await.operation.cancel,
        Some(CancelAvailability::Continuing)
    );
    assert_eq!(
        cancel(&cli, id).await,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    committing.release();
    let finished = wait_state(&cli, id, OperationState::Succeeded).await;
    assert!(finished.operation.cancel_requested_at.is_some());
    let found = rows(&cli, id).await;
    assert_eq!(
        actions(&found),
        [
            ("operation.cancel", AuditResultKind::Succeeded),
            ("operation.finished", AuditResultKind::Succeeded),
        ],
        "a second cancel writes nothing"
    );
    assert_eq!(
        detail(&found[0], "disposition"),
        Some("continuing_past_point_of_no_return")
    );
    assert_eq!(detail(&found[1], "cancelRequested"), Some("true"));
    stop(daemon).await;
}

// covers: AC-9, AC-3 (spec 0035)
#[tokio::test]
async fn a_commit_that_fails_needs_recovery_never_cancelled_and_an_abandon_names_its_sender() {
    let site = Site::new();
    let scripts = Scripts::new();
    let committing = HoldHandle::new();
    scripts.add("broken", staged(None, &committing, true));
    let daemon = site.start(&scripts).await;
    let cli = client(&site).await;
    let mut gui = Raw::ready(&site.socket()).await;

    let broken = accepted(&gui.start(Some(&key()), json!({ "script": "broken" })).await);
    let id = broken.operation.id;
    tokio::time::timeout(PATIENCE, committing.started())
        .await
        .expect("the commit step runs");
    assert_eq!(
        cancel(&cli, id).await,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    committing.release();
    let stuck = wait_state(&cli, id, OperationState::NeedsRecovery).await;
    assert_eq!(stuck.operation.cancel, Some(CancelAvailability::None));
    assert_eq!(cancel(&cli, id).await, CancelDisposition::RecoveryRequired);
    let before = rows(&cli, id).await;
    assert_eq!(
        actions(&before),
        [
            ("operation.cancel", AuditResultKind::Succeeded),
            ("operation.needs_recovery", AuditResultKind::NeedsRecovery),
        ],
        "a cancel in needs_recovery writes nothing"
    );
    assert_eq!(detail(&before[1], "errorCode"), Some("internal.unexpected"));

    let Reply::OperationAbandon(abandoned) = cli
        .call(Call::OperationAbandon(AbandonOperationParams {
            operation_id: id,
            expected_revision: stuck.operation.revision,
        }))
        .await
        .expect("operation.abandon answers")
    else {
        panic!("not an abandon reply");
    };
    assert_eq!(abandoned.operation.state, OperationState::Failed);
    assert!(
        cli.call(Call::OperationAbandon(AbandonOperationParams {
            operation_id: id,
            expected_revision: abandoned.operation.revision,
        }))
        .await
        .is_err(),
        "a second abandon is refused"
    );
    let found = rows(&cli, id).await;
    assert_eq!(
        actions(&found)[2..],
        [
            ("operation.abandon", AuditResultKind::Succeeded),
            ("operation.finished", AuditResultKind::Failed),
        ],
        "a refused abandon writes nothing"
    );
    assert_eq!(found[2].client, Some(ClientKind::Cli));
    assert_eq!(
        detail(&found[2], "leftovers"),
        Some(abandoned.leftovers.len().to_string().as_str())
    );
    assert_eq!(found[3].client, Some(ClientKind::Gui));
    assert_eq!(detail(&found[3], "reason"), Some("abandoned"));
    stop(daemon).await;
}

/// What `cancel` must say for `summary` at its moment (spec 0035,
/// *Ketersediaan cancel*), for an operation nobody asked to cancel.
fn expected_cancel(summary: &OperationSummary) -> CancelAvailability {
    match summary.state {
        OperationState::Queued | OperationState::Running
            if summary.point_of_no_return_at.is_some() =>
        {
            CancelAvailability::PastPointOfNoReturn
        }
        OperationState::Queued | OperationState::Running => CancelAvailability::Available,
        _ => CancelAvailability::None,
    }
}

// covers: AC-8 (spec 0035, `cancel` in every reply and event)
#[tokio::test]
async fn every_summary_in_an_event_or_a_reply_carries_the_cancel_of_its_moment() {
    let site = Site::new();
    let scripts = Scripts::new();
    let staging = HoldHandle::new();
    let committing = HoldHandle::new();
    scripts.add("restore", staged(Some(&staging), &committing, false));
    let daemon = site.start(&scripts).await;
    let cli = client(&site).await;
    let (_, mut events) = cli
        .subscribe(SubscribeParams {
            kinds: vec!["operation.".to_owned()],
            ..SubscribeParams::default()
        })
        .await
        .expect("the client subscribes to operation events");
    let mut gui = Raw::ready(&site.socket()).await;

    let started = accepted(
        &gui.start(Some(&key()), json!({ "script": "restore" }))
            .await,
    );
    let id = started.operation.id;
    assert_eq!(
        started.operation.cancel,
        Some(expected_cancel(&started.operation)),
        "the start reply"
    );
    tokio::time::timeout(PATIENCE, staging.started())
        .await
        .expect("the staging step runs");
    let (page, _) = list(
        &cli,
        ListOperationsParams {
            active_only: true,
            ..ListOperationsParams::default()
        },
    )
    .await;
    let listed = page
        .iter()
        .find(|summary| summary.id == id)
        .expect("operation.list holds the running operation");
    assert_eq!(listed.cancel, Some(CancelAvailability::Available));
    staging.release();
    tokio::time::timeout(PATIENCE, committing.started())
        .await
        .expect("the commit step runs");
    let (page, _) = list(&cli, ListOperationsParams::default()).await;
    let listed = page
        .iter()
        .find(|summary| summary.id == id)
        .expect("operation.list holds the operation");
    assert_eq!(listed.cancel, Some(CancelAvailability::PastPointOfNoReturn));
    committing.release();

    let mut seen = Vec::new();
    loop {
        let frame = tokio::time::timeout(PATIENCE, events.next())
            .await
            .expect("events arrive in time")
            .expect("the event stream reads")
            .expect("the stream stays open");
        let DecodedFrame::Event(event) = frame else {
            panic!("not an event: {frame:?}");
        };
        let (EventKind::OperationStateChanged(summary) | EventKind::OperationProgress(summary)) =
            event.kind
        else {
            continue;
        };
        if summary.id != id {
            continue;
        }
        assert_eq!(
            summary.cancel,
            Some(expected_cancel(&summary)),
            "the event of revision {} in {:?}",
            summary.revision.get(),
            summary.state
        );
        let done = summary.state == OperationState::Succeeded;
        seen.push(summary.cancel);
        if done {
            break;
        }
    }
    for wanted in [
        CancelAvailability::Available,
        CancelAvailability::PastPointOfNoReturn,
        CancelAvailability::None,
    ] {
        assert!(
            seen.contains(&Some(wanted)),
            "an event carried {wanted:?}: {seen:?}"
        );
    }
    stop(daemon).await;
}
