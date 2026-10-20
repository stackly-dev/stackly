//! Operations over the wire: events and revisions, idempotency, a client
//! that leaves, cancel, resume, abandon, paging, failures, and health.

use std::time::Duration;

use serde_json::json;
use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_core::operation::{OperationOutcome, PartialItem, PartialOutcome};
use stackly_domain::{FailureItem, OperationKind, Rule};
use stackly_ipc::PeerCred;
use stackly_protocol::codes::{operation, request, resource};
use stackly_protocol::daemon::{SubsystemStatus, subsystem};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::{
    AbandonOperationParams, CancelDisposition, CancelOperationParams, CreatedState,
    ListOperationsParams, OPERATION_PAGE_BYTES_MAX, OperationProgress, OperationState,
    RecoveryAction, RecoveryHint, ResumeOperationParams, StepState, TriggerOrigin,
};
use stackly_protocol::{Call, ErrorCategory, Reply, ResourceKind, ResourceRef, Revision};
use stacklyd::dispatch::RequestContext;
use stacklyd::ipc::operation::{StartRequest, TestStartingMethod, start_operation};
use uuid::Uuid;

use crate::support::{
    PATIENCE, Raw, Script, Scripts, Site, accepted, checked, client, error_of, get, key, list,
    wait_state, wait_until,
};

fn quick() -> Script {
    Script::new(
        OperationKind::UpdateManager,
        ScriptedWork::new().step("apply"),
    )
}

async fn stop(daemon: stacklyd::server::DaemonHandle) {
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test]
async fn events_follow_an_operation_in_revision_order_with_its_summary() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "three",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("fetch")
                .progress(1, 3)
                .step("verify")
                .progress(2, 3)
                .step("apply")
                .progress(3, 3),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let (_, mut events) = client
        .subscribe(SubscribeParams {
            kinds: vec!["operation.".to_owned()],
            ..SubscribeParams::default()
        })
        .await
        .unwrap();
    let mut raw = Raw::ready(&site.socket()).await;

    let started = accepted(&raw.start(Some(&key()), json!({ "script": "three" })).await);

    assert!(!started.replayed);
    assert_eq!(started.operation.state, OperationState::Queued);
    assert_eq!(started.operation.revision, Revision::new(1).unwrap());
    assert_eq!(
        started.operation.kind,
        stackly_protocol::operation::OperationKind::UpdateManager
    );
    assert_eq!(started.operation.resource, None);
    assert_eq!(started.operation.trigger.origin, TriggerOrigin::User);
    assert_eq!(started.operation.trigger.client, Some(ClientKind::Gui));
    let id = started.operation.id;
    let mut seen = Vec::new();
    loop {
        let frame = tokio::time::timeout(PATIENCE, events.next())
            .await
            .expect("events arrive in time")
            .unwrap()
            .expect("the stream stays open");
        let DecodedFrame::Event(event) = frame else {
            panic!("not an event: {frame:?}");
        };
        let (summary, changed_state) = match event.kind {
            EventKind::OperationStateChanged(summary) => (summary, true),
            EventKind::OperationProgress(summary) => (summary, false),
            other => panic!("not an operation event: {other:?}"),
        };
        assert_eq!(summary.id, id);
        assert_eq!(
            event.resource,
            Some(ResourceRef {
                kind: ResourceKind::Operation,
                id: id.to_string()
            })
        );
        assert_eq!(event.revision, Some(summary.revision), "{summary:?}");
        let done = summary.state == OperationState::Succeeded;
        seen.push((changed_state, *summary));
        if done {
            break;
        }
    }

    let revisions: Vec<u64> = seen.iter().map(|(_, s)| s.revision.get()).collect();
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "revisions rise: {revisions:?}"
    );
    assert_eq!(revisions[0], 1);
    let states: Vec<OperationState> = seen
        .iter()
        .filter(|(changed, _)| *changed)
        .map(|(_, s)| s.state)
        .collect();
    assert_eq!(
        states,
        [
            OperationState::Queued,
            OperationState::Running,
            OperationState::Succeeded
        ]
    );
    assert!(
        seen.iter().any(|(_, s)| matches!(
            s.progress,
            OperationProgress::Counted(progress) if progress.done.get() >= 1 && progress.total.get() == 3
        )),
        "a counted progress was published: {seen:?}"
    );
    let detail = get(&client, id).await;
    assert_eq!(detail.operation, seen.last().unwrap().1);
    assert_eq!(detail.steps.len(), 3);
    assert!(
        detail
            .steps
            .iter()
            .all(|step| step.state == StepState::Done)
    );
    stop(daemon).await;
}

#[tokio::test]
async fn the_same_key_replays_and_another_request_with_it_or_without_a_key_is_refused() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add("quick", quick());
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let key = key();

    let first = accepted(
        &raw.start(Some(&key), json!({ "script": "quick", "note": "a" }))
            .await,
    );
    let replay = accepted(
        &raw.start(Some(&key), json!({ "script": "quick", "note": "a" }))
            .await,
    );
    let reused = error_of(
        &raw.start(Some(&key), json!({ "script": "quick", "note": "b" }))
            .await,
    );
    let (before, _) = list(&client, ListOperationsParams::default()).await;
    let required = error_of(&raw.start(None, json!({ "script": "quick" })).await);
    let (after, _) = list(&client, ListOperationsParams::default()).await;

    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(replay.operation.id, first.operation.id);
    assert!(reused.is(&operation::IDEMPOTENCY_KEY_REUSED));
    assert_eq!(
        reused.detail("operationId"),
        Some(first.operation.id.to_string().as_str())
    );
    assert_eq!(reused.detail("method"), Some("test.start_operation"));
    assert!(required.is(&operation::IDEMPOTENCY_KEY_REQUIRED));
    assert_eq!(required.detail("method"), Some("test.start_operation"));
    let ids = |items: &[stackly_protocol::operation::OperationSummary]| {
        items.iter().map(|summary| summary.id).collect::<Vec<_>>()
    };
    assert_eq!(ids(&before), [first.operation.id]);
    assert_eq!(ids(&after), ids(&before), "a refused request writes no row");
    stop(daemon).await;
}

#[tokio::test]
async fn a_bad_idempotency_key_or_unknown_params_are_refused_before_any_row() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add("quick", quick());
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let spaced = error_of(
        &raw.start(Some("has space"), json!({ "script": "quick" }))
            .await,
    );
    let unknown = error_of(&raw.start(Some(&key()), json!({ "script": "nope" })).await);

    assert!(spaced.is(&request::MALFORMED));
    assert!(unknown.is(&request::INVALID_PARAMS));
    let (items, _) = list(&client, ListOperationsParams::default()).await;
    assert!(items.is_empty());
    stop(daemon).await;
}

#[tokio::test]
async fn a_client_that_leaves_after_sending_does_not_cancel_its_operation() {
    let site = Site::new();
    let scripts = Scripts::new();
    let hold = HoldHandle::new();
    scripts.add(
        "held",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("wait").hold(&hold),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;

    let mut raw = Raw::ready(&site.socket()).await;
    raw.send(&crate::support::start_request(
        "gone",
        Some(&key()),
        json!({ "script": "held" }),
    ))
    .await;
    drop(raw);
    tokio::time::timeout(PATIENCE, hold.started())
        .await
        .expect("the operation runs although its client left");
    hold.release();
    let (items, _) = list(&client, ListOperationsParams::default()).await;
    let detail = wait_state(&client, items[0].id, OperationState::Succeeded).await;

    assert_eq!(detail.operation.cancel_requested_at, None);
    stop(daemon).await;
}

#[tokio::test]
async fn a_request_task_cancelled_right_after_begin_leaves_the_operation_running() {
    let site = Site::new();
    let scripts = Scripts::new();
    let hold = HoldHandle::new();
    scripts.add(
        "held",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("wait").hold(&hold),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let (_, mut events) = client
        .subscribe(SubscribeParams {
            kinds: vec!["operation.state_changed".to_owned()],
            ..SubscribeParams::default()
        })
        .await
        .unwrap();

    let core = std::sync::Arc::clone(daemon.core());
    let method = scripts.clone();
    let task = tokio::spawn(async move {
        let context = RequestContext::new(
            1,
            "in-process".to_owned(),
            PeerCred::new(0, None),
            ClientInfo {
                kind: ClientKind::Cli,
                version: "0.1.0".to_owned(),
                pid: None,
                interactive: None,
            },
        );
        let prepared = method.prepare(Some(&json!({ "script": "held" }))).unwrap();
        let key = key();
        let start = StartRequest::new("test.start_operation", Some(&key), prepared.fingerprint);
        let outcome = start_operation(&core, &context, start, prepared.build).await;
        // Never reached: the task is aborted while it waits or right after.
        tokio::time::sleep(Duration::from_secs(3600)).await;
        outcome
    });
    let first = tokio::time::timeout(PATIENCE, events.next())
        .await
        .expect("the operation is created in time")
        .unwrap()
        .unwrap();
    task.abort();
    let DecodedFrame::Event(event) = first else {
        panic!("not an event: {first:?}");
    };
    let EventKind::OperationStateChanged(created) = event.kind else {
        panic!("not a state change");
    };
    hold.release();
    let detail = wait_state(&client, created.id, OperationState::Succeeded).await;

    assert_eq!(created.state, OperationState::Queued);
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(detail.operation.cancel_requested_at, None);
    stop(daemon).await;
}

#[tokio::test]
async fn cancel_over_the_wire_undoes_before_the_point_of_no_return_and_reports_it_after() {
    let site = Site::new();
    let scripts = Scripts::new();
    let reversible = HoldHandle::new();
    let irreversible = HoldHandle::new();
    scripts.add(
        "undoable",
        Script::new(
            OperationKind::CreateInstance,
            ScriptedWork::new()
                .step("reserve")
                .creates_instance_tree()
                .step("wait")
                .hold(&reversible),
        )
        .creating(),
    );
    scripts.add(
        "committed",
        Script::new(
            OperationKind::RestoreInstance,
            ScriptedWork::new()
                .step("commit")
                .irreversible()
                .hold(&irreversible),
        )
        .creating(),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let undoable = accepted(
        &raw.start(Some(&key()), json!({ "script": "undoable" }))
            .await,
    );
    tokio::time::timeout(PATIENCE, reversible.started())
        .await
        .unwrap();
    let cancel = |id: Uuid| Call::OperationCancel(CancelOperationParams { operation_id: id });
    let Reply::OperationCancel(requested) =
        client.call(cancel(undoable.operation.id)).await.unwrap()
    else {
        panic!("not a cancel reply");
    };
    let cancelled = wait_state(&client, undoable.operation.id, OperationState::Cancelled).await;
    let Reply::OperationCancel(again) = client.call(cancel(undoable.operation.id)).await.unwrap()
    else {
        panic!("not a cancel reply");
    };

    assert_eq!(requested.disposition, CancelDisposition::Requested);
    assert!(requested.operation.cancel_requested_at.is_some());
    assert_eq!(again.disposition, CancelDisposition::AlreadyFinal);
    assert_eq!(cancelled.created.len(), 1);
    assert_eq!(cancelled.created[0].state, CreatedState::Compensated);
    assert!(!site.instance_root(&cancelled.created[0].reference).exists());

    let committed = accepted(
        &raw.start(Some(&key()), json!({ "script": "committed" }))
            .await,
    );
    tokio::time::timeout(PATIENCE, irreversible.started())
        .await
        .unwrap();
    let Reply::OperationCancel(late) = client.call(cancel(committed.operation.id)).await.unwrap()
    else {
        panic!("not a cancel reply");
    };
    irreversible.release();
    let finished = wait_state(&client, committed.operation.id, OperationState::Succeeded).await;

    assert_eq!(
        late.disposition,
        CancelDisposition::ContinuingPastPointOfNoReturn
    );
    assert!(late.operation.point_of_no_return_at.is_some());
    assert!(finished.operation.cancel_requested_at.is_some());
    assert!(finished.operation.point_of_no_return_at.is_some());

    let unknown = client.call(cancel(Uuid::now_v7())).await.unwrap_err();
    checked(&unknown);
    assert!(unknown.is(&resource::NOT_FOUND));
    stop(daemon).await;
}

#[tokio::test]
async fn resume_over_the_wire_runs_the_failed_step_again_and_health_follows() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "flaky",
        Script::new(
            OperationKind::InstallRuntime,
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .irreversible()
                .fail("internal.unexpected", false),
        )
        .rebuilt_as(
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .irreversible(),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let health = |name: &str| {
        daemon
            .health()
            .subsystems
            .into_iter()
            .find(|health| health.name == name)
            .unwrap()
    };
    assert_eq!(health(subsystem::OPERATIONS).status, SubsystemStatus::Ok);

    let started = accepted(&raw.start(Some(&key()), json!({ "script": "flaky" })).await);
    let id = started.operation.id;
    let stuck = wait_state(&client, id, OperationState::NeedsRecovery).await;
    let degraded = health(subsystem::OPERATIONS);
    let Reply::OperationGetStats(stats) = client.call(Call::OperationGetStats).await.unwrap()
    else {
        panic!("not stats");
    };
    let recovery = stuck.operation.recovery.clone().expect("recovery info");

    assert_eq!(recovery.hint, RecoveryHint::Resume);
    assert_eq!(
        recovery.actions,
        [RecoveryAction::Resume, RecoveryAction::Abandon]
    );
    let cause_error = recovery.error.expect("the cause's error");
    checked(&cause_error);
    assert_eq!(cause_error.code(), "internal.unexpected");
    assert_eq!(cause_error.operation_id(), Some(id));
    assert_eq!(stuck.operation.failure, None);
    assert_eq!(degraded.status, SubsystemStatus::Degraded);
    assert_eq!(
        degraded.detail.as_deref(),
        Some("1 operation needs recovery")
    );
    assert_eq!(stats.needs_recovery, 1);
    assert_eq!((stats.max_running, stats.max_queued), (8, 64));

    let resume = |expected: Option<u64>| {
        Call::OperationResume(ResumeOperationParams {
            operation_id: id,
            expected_revision: expected.map(|revision| Revision::new(revision).unwrap()),
        })
    };
    let stale = client
        .call(resume(Some(stuck.operation.revision.get() + 5)))
        .await
        .unwrap_err();
    checked(&stale);
    assert!(stale.is(&resource::REVISION_MISMATCH));
    let Reply::OperationResume(queued) = client
        .call(resume(Some(stuck.operation.revision.get())))
        .await
        .unwrap()
    else {
        panic!("not a resume reply");
    };
    let done = wait_state(&client, id, OperationState::Succeeded).await;
    let wrong_state = client.call(resume(None)).await.unwrap_err();

    assert_eq!(queued.state, OperationState::Queued);
    assert_eq!(done.steps[0].attempts, 2);
    checked(&wrong_state);
    assert!(wrong_state.is(&operation::NOT_RESUMABLE));
    assert_eq!(wrong_state.detail("reason"), Some("wrong_state"));
    assert_eq!(wrong_state.operation_id(), Some(id));
    assert_eq!(health(subsystem::OPERATIONS).status, SubsystemStatus::Ok);
    stop(daemon).await;
}

#[tokio::test]
async fn abandon_over_the_wire_undoes_the_reversible_steps_and_closes_the_operation() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "broken",
        Script::new(
            OperationKind::CreateInstance,
            ScriptedWork::new()
                .step("reserve")
                .creates_instance_tree()
                .step("commit")
                .irreversible()
                .fail("internal.unexpected", false),
        )
        .creating(),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let id = accepted(&raw.start(Some(&key()), json!({ "script": "broken" })).await)
        .operation
        .id;
    let stuck = wait_state(&client, id, OperationState::NeedsRecovery).await;
    let tree = site.instance_root(&stuck.created[0].reference);
    assert!(tree.exists());
    let abandon = |revision: u64| {
        Call::OperationAbandon(AbandonOperationParams {
            operation_id: id,
            expected_revision: Revision::new(revision).unwrap(),
        })
    };
    let not_resumable = client
        .call(Call::OperationResume(ResumeOperationParams {
            operation_id: id,
            expected_revision: None,
        }))
        .await
        .unwrap_err();
    let stale = client
        .call(abandon(stuck.operation.revision.get() + 1))
        .await
        .unwrap_err();
    let Reply::OperationAbandon(closed) = client
        .call(abandon(stuck.operation.revision.get()))
        .await
        .unwrap()
    else {
        panic!("not an abandon reply");
    };
    let again = client
        .call(abandon(closed.operation.revision.get()))
        .await
        .unwrap_err();

    checked(&not_resumable);
    assert!(not_resumable.is(&operation::NOT_RESUMABLE));
    assert_eq!(
        stuck.operation.recovery.as_ref().unwrap().actions,
        [RecoveryAction::Abandon]
    );
    checked(&stale);
    assert!(stale.is(&resource::REVISION_MISMATCH));
    assert_eq!(closed.operation.state, OperationState::Failed);
    let failure = closed.operation.failure.clone().expect("a failure");
    assert_eq!(failure.reason, "abandoned");
    checked(&failure.error);
    assert_eq!(failure.error.code(), "operation.abandoned");
    assert_eq!(failure.error.operation_id(), Some(id));
    assert!(!tree.exists(), "the reversible step's tree is undone");
    let partial = &closed.operation.result.as_ref().expect("a result").partial;
    assert!(
        partial
            .iter()
            .any(|item| item.outcome == stackly_protocol::operation::PartialOutcome::Done),
        "{partial:?}"
    );
    checked(&again);
    assert!(again.is(&operation::NOT_ABANDONABLE));
    assert_eq!(again.detail("reason"), Some("wrong_state"));
    assert_eq!(again.detail("state"), Some("failed"));
    stop(daemon).await;
}

#[tokio::test]
async fn a_work_that_panics_fails_with_a_catalog_error_naming_the_operation() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "panics",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("boom").panics(),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let id = accepted(&raw.start(Some(&key()), json!({ "script": "panics" })).await)
        .operation
        .id;
    let failed = wait_state(&client, id, OperationState::Failed).await;

    let failure = failed.operation.failure.expect("a failure");
    assert_eq!(failure.reason, "panicked");
    checked(&failure.error);
    assert_eq!(failure.error.code(), "internal.unexpected");
    assert_eq!(failure.error.category(), ErrorCategory::Internal);
    assert_eq!(failure.error.operation_id(), Some(id));
    assert!(matches!(
        client.call(Call::DaemonGetInfo).await,
        Ok(Reply::DaemonGetInfo(_))
    ));
    stop(daemon).await;
}

fn issue(location: &str, reason: &str) -> FailureItem {
    FailureItem::new(
        "issue".parse().expect("an item kind"),
        vec![
            (
                "location".parse().expect("a field key"),
                location.to_owned(),
            ),
            ("reason".parse().expect("a field key"), reason.to_owned()),
        ],
    )
    .expect("a valid item")
}

#[tokio::test]
async fn a_failed_operation_carries_its_items_rule_and_omitted_count_on_get_event_and_after_a_restart()
 {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "invalid",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("check")
                .fail("provider.invalid_settings", false)
                .failing_with(
                    vec![("issueCount".parse().unwrap(), "2".to_owned())],
                    vec![issue("/port", "too large"), issue("/name", "empty")],
                    Some(Rule::Br002),
                ),
        ),
    );
    scripts.add(
        "many",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("check")
                .fail("provider.invalid_settings", false)
                .failing_with(
                    vec![("issueCount".parse().unwrap(), "101".to_owned())],
                    (0..101).map(|n| issue(&format!("/f{n}"), "bad")).collect(),
                    None,
                ),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let (_, mut events) = client
        .subscribe(SubscribeParams {
            kinds: vec!["operation.".to_owned()],
            ..SubscribeParams::default()
        })
        .await
        .unwrap();
    let mut raw = Raw::ready(&site.socket()).await;

    let id = accepted(
        &raw.start(Some(&key()), json!({ "script": "invalid" }))
            .await,
    )
    .operation
    .id;
    let many = accepted(&raw.start(Some(&key()), json!({ "script": "many" })).await)
        .operation
        .id;
    let failed = wait_state(&client, id, OperationState::Failed).await;
    let crowded = wait_state(&client, many, OperationState::Failed).await;

    let error = failed.operation.failure.expect("a failure").error;
    checked(&error);
    assert_eq!(error.code(), "provider.invalid_settings");
    assert_eq!(error.rule(), Some("BR-002"));
    assert_eq!(error.detail("issueCount"), Some("2"));
    assert_eq!(error.detail("omitted"), None);
    assert_eq!(error.items().len(), 2);
    assert_eq!(error.items()[0].field("location"), Some("/port"));
    assert_eq!(error.items()[1].field("reason"), Some("empty"));
    let crowded_error = crowded.operation.failure.expect("a failure").error;
    assert_eq!(crowded_error.items().len(), 100);
    assert_eq!(crowded_error.detail("omitted"), Some("1"));
    assert_eq!(crowded_error.rule(), None);

    let mut from_event = None;
    while from_event.is_none() {
        let frame = tokio::time::timeout(PATIENCE, events.next())
            .await
            .expect("events arrive in time")
            .unwrap()
            .expect("the stream stays open");
        let DecodedFrame::Event(event) = frame else {
            continue;
        };
        if let EventKind::OperationStateChanged(summary) = event.kind
            && summary.id == id
            && summary.state == OperationState::Failed
        {
            from_event = summary.failure.map(|failure| failure.error);
        }
    }
    assert_eq!(
        from_event,
        Some(error.clone()),
        "the event carries the same error"
    );

    drop(events);
    drop(client);
    drop(raw);
    stop(daemon).await;
    let daemon = site.start(&scripts).await;
    let client = self::client(&site).await;
    let after = get(&client, id).await.operation.failure.expect("a failure");
    assert_eq!(after.error, error, "the same error after a restart");
    let crowded_after = get(&client, many)
        .await
        .operation
        .failure
        .expect("a failure");
    assert_eq!(crowded_after.error, crowded_error);
    stop(daemon).await;
}

#[tokio::test]
async fn items_past_the_stored_size_limit_are_cut_by_the_engine_before_sqlite_checks_them() {
    let site = Site::new();
    let scripts = Scripts::new();
    let wide = |n: usize| {
        FailureItem::new(
            "issue".parse().unwrap(),
            ["file", "location", "code", "message", "reason"]
                .iter()
                .map(|key| (key.parse().unwrap(), format!("{n:03}{}", "x".repeat(509))))
                .collect(),
        )
        .unwrap()
    };
    scripts.add(
        "heavy",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("check")
                .fail("provider.invalid_settings", false)
                .failing_with(Vec::new(), (0..100).map(wide).collect(), None),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let id = accepted(&raw.start(Some(&key()), json!({ "script": "heavy" })).await)
        .operation
        .id;
    let failed = wait_state(&client, id, OperationState::Failed).await;

    let error = failed.operation.failure.expect("a failure").error;
    assert_eq!(error.code(), "provider.invalid_settings");
    let kept = error.items().len();
    assert!((1..100).contains(&kept), "kept {kept}");
    assert_eq!(
        error.detail("omitted"),
        Some((100 - kept).to_string().as_str())
    );
    assert_eq!(
        error.items()[kept - 1].field("file").map(|v| &v[..3]),
        Some(format!("{:03}", kept - 1).as_str())
    );
    stop(daemon).await;
}

#[test]
fn every_code_the_engine_stores_is_in_the_catalog() {
    for code in stackly_core::operation::codes::ALL {
        assert!(
            stackly_protocol::codes::lookup(code.as_str()).is_some(),
            "{code} is not in the catalog"
        );
    }
}

#[tokio::test]
async fn operation_list_pages_newest_first_with_filters_and_get_answers_not_found() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add("quick", quick());
    let hold = HoldHandle::new();
    scripts.add(
        "held",
        Script::new(
            OperationKind::InstallRuntime,
            ScriptedWork::new().step("wait").hold(&hold),
        )
        .creating(),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let mut ids = Vec::new();
    for _ in 0..4 {
        let id = accepted(&raw.start(Some(&key()), json!({ "script": "quick" })).await)
            .operation
            .id;
        wait_state(&client, id, OperationState::Succeeded).await;
        ids.push(id);
    }
    let running = accepted(&raw.start(Some(&key()), json!({ "script": "held" })).await).operation;
    tokio::time::timeout(PATIENCE, hold.started())
        .await
        .unwrap();

    let (first, next) = list(
        &client,
        ListOperationsParams {
            limit: Some(2),
            ..ListOperationsParams::default()
        },
    )
    .await;
    let (second, next_two) = list(
        &client,
        ListOperationsParams {
            limit: Some(2),
            before: next.clone(),
            ..ListOperationsParams::default()
        },
    )
    .await;
    let (third, last) = list(
        &client,
        ListOperationsParams {
            limit: Some(2),
            before: next_two,
            ..ListOperationsParams::default()
        },
    )
    .await;
    let (active, _) = list(
        &client,
        ListOperationsParams {
            active_only: true,
            ..ListOperationsParams::default()
        },
    )
    .await;
    let (on_instance, _) = list(
        &client,
        ListOperationsParams {
            resource: running.resource.clone(),
            ..ListOperationsParams::default()
        },
    )
    .await;
    let (on_manager, _) = list(
        &client,
        ListOperationsParams {
            resource: Some(ResourceRef::manager()),
            ..ListOperationsParams::default()
        },
    )
    .await;

    let paged: Vec<Uuid> = first
        .iter()
        .chain(&second)
        .chain(&third)
        .map(|summary| summary.id)
        .collect();
    let mut newest_first = ids.clone();
    newest_first.push(running.id);
    newest_first.reverse();
    assert_eq!(paged, newest_first);
    assert!(next.is_some());
    assert_eq!(last, None);
    assert_eq!(
        active.iter().map(|s| s.id).collect::<Vec<_>>(),
        [running.id]
    );
    assert_eq!(
        on_instance.iter().map(|s| s.id).collect::<Vec<_>>(),
        [running.id]
    );
    assert_eq!(on_manager.len(), 4);

    let bad_id = client
        .call(Call::OperationList(ListOperationsParams {
            resource: Some(ResourceRef {
                kind: ResourceKind::Instance,
                id: "not-a-uuid".to_owned(),
            }),
            ..ListOperationsParams::default()
        }))
        .await
        .unwrap_err();
    let bad_limit = client
        .call(Call::OperationList(ListOperationsParams {
            limit: Some(0),
            ..ListOperationsParams::default()
        }))
        .await
        .unwrap_err();
    let missing = client
        .call(Call::OperationGet(
            stackly_protocol::operation::GetOperationParams {
                operation_id: Uuid::now_v7(),
            },
        ))
        .await
        .unwrap_err();
    for error in [&bad_id, &bad_limit, &missing] {
        checked(error);
    }
    assert!(bad_id.is(&request::INVALID_PARAMS));
    assert_eq!(bad_id.fields()[0].path(), "resource.id");
    assert!(bad_limit.is(&request::INVALID_PARAMS));
    assert!(missing.is(&resource::NOT_FOUND));
    hold.release();
    stop(daemon).await;
}

#[tokio::test]
async fn a_page_stops_before_900_kib_and_its_next_names_the_last_operation_on_it() {
    let site = Site::new();
    let scripts = Scripts::new();
    let big = OperationOutcome {
        resources: Vec::new(),
        partial: vec![PartialItem {
            item: "x".repeat(15_000),
            resource: None,
            outcome: PartialOutcome::Done,
            code: None,
        }],
    };
    scripts.add(
        "big",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("apply").result(big),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let count = 70;
    for _ in 0..count {
        let id = accepted(&raw.start(Some(&key()), json!({ "script": "big" })).await)
            .operation
            .id;
        wait_state(&client, id, OperationState::Succeeded).await;
    }

    let (first, next) = list(
        &client,
        ListOperationsParams {
            limit: Some(100),
            ..ListOperationsParams::default()
        },
    )
    .await;
    let (rest, end) = list(
        &client,
        ListOperationsParams {
            limit: Some(100),
            before: next.clone(),
            ..ListOperationsParams::default()
        },
    )
    .await;

    let bytes: usize = first
        .iter()
        .map(|summary| serde_json::to_vec(summary).unwrap().len())
        .sum();
    assert!(first.len() < count, "the page was cut: {}", first.len());
    assert!(bytes <= OPERATION_PAGE_BYTES_MAX, "{bytes} bytes");
    assert!(next.is_some());
    assert_eq!(first.len() + rest.len(), count);
    assert_eq!(end, None);
    let mut all: Vec<Uuid> = first.iter().chain(&rest).map(|s| s.id).collect();
    all.dedup();
    assert_eq!(all.len(), count, "no operation repeats across the pages");
    stop(daemon).await;
}

#[tokio::test]
async fn the_request_line_of_a_starting_method_and_of_a_cancel_carries_the_operation_id() {
    let site = Site::new();
    let scripts = Scripts::new();
    let hold = HoldHandle::new();
    scripts.add(
        "held",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("wait").hold(&hold),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let id = accepted(&raw.start(Some(&key()), json!({ "script": "held" })).await)
        .operation
        .id;
    tokio::time::timeout(PATIENCE, hold.started())
        .await
        .unwrap();
    client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: id,
        }))
        .await
        .unwrap();
    hold.release();
    wait_until(&client, id, |detail| detail.operation.state.is_final()).await;
    stop(daemon).await;

    let lines = crate::support::log_lines();
    let id_text = id.to_string();
    let requests: Vec<_> = lines
        .iter()
        .filter(|line| {
            line["fields"]["message"] == "request"
                && line["fields"]["operation_id"] == id_text.as_str()
        })
        .map(|line| {
            line["fields"]["method"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert!(
        requests.contains(&"test.start_operation".to_owned()),
        "{requests:?}"
    );
    assert!(
        requests.contains(&"operation.cancel".to_owned()),
        "{requests:?}"
    );
    let changes: Vec<_> = lines
        .iter()
        .filter(|line| {
            line["fields"]["message"] == "operation state changed"
                && line["fields"]["operation_id"] == id_text.as_str()
        })
        .collect();
    assert!(!changes.is_empty());
    assert!(changes.iter().all(|line| line["level"] == "INFO"));
    assert!(
        changes
            .iter()
            .all(|line| line["fields"]["kind"] == "update_manager")
    );
    let in_runner: Vec<_> = changes
        .iter()
        .filter(|line| {
            matches!(
                line["fields"]["state"].as_str(),
                Some("running" | "cancelled")
            )
        })
        .collect();
    assert_eq!(in_runner.len(), 2, "{changes:?}");
    for line in in_runner {
        assert_eq!(line["span"]["name"], "operation", "{line}");
        assert_eq!(line["span"]["operation_id"], id_text.as_str(), "{line}");
        assert_eq!(line["span"]["kind"], "update_manager", "{line}");
    }
    let in_task: Vec<_> = lines
        .iter()
        .filter(|line| {
            line["span"]["name"] == "operation" && line["span"]["operation_id"] == id_text.as_str()
        })
        .collect();
    assert!(
        in_task
            .iter()
            .all(|line| line["span"]["kind"] == "update_manager")
    );
}

#[tokio::test]
async fn in_recovery_mode_every_operation_method_answers_recovery_mode() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add("quick", quick());
    let first = site.start(&scripts).await;
    stop(first).await;
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).unwrap();
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).unwrap();

    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let id = Uuid::now_v7();
    let calls = [
        Call::OperationGet(stackly_protocol::operation::GetOperationParams { operation_id: id }),
        Call::OperationList(ListOperationsParams::default()),
        Call::OperationCancel(CancelOperationParams { operation_id: id }),
        Call::OperationResume(ResumeOperationParams {
            operation_id: id,
            expected_revision: None,
        }),
        Call::OperationAbandon(AbandonOperationParams {
            operation_id: id,
            expected_revision: Revision::new(1).unwrap(),
        }),
        Call::OperationGetStats,
    ];
    for call in calls {
        let method = call.method();
        let error = client.call(call).await.unwrap_err();
        checked(&error);
        assert!(
            error.is(&stackly_protocol::codes::registry::RECOVERY_MODE),
            "{method}"
        );
    }
    let started = error_of(&raw.start(Some(&key()), json!({ "script": "quick" })).await);
    let operations = daemon
        .health()
        .subsystems
        .into_iter()
        .find(|health| health.name == subsystem::OPERATIONS)
        .unwrap();

    assert!(started.is(&stackly_protocol::codes::registry::RECOVERY_MODE));
    assert_eq!(operations.status, SubsystemStatus::Unavailable);
    assert_eq!(operations.detail.as_deref(), Some("registry in recovery"));
    stop(daemon).await;
}

#[tokio::test]
async fn while_the_start_slot_waits_operation_methods_answer_daemon_starting() {
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add("quick", quick());
    let (gate, held) = tokio::sync::watch::channel(false);
    let mut config = stacklyd::server::DaemonConfig::default();
    config.start_gate = Some(held);
    let daemon = stacklyd::server::start(site.deps(&scripts).with_config(config))
        .await
        .unwrap();
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let listed = client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .unwrap_err();
    let stats = client.call(Call::OperationGetStats).await.unwrap_err();
    let started = error_of(&raw.start(Some(&key()), json!({ "script": "quick" })).await);
    gate.send_replace(true);
    daemon.started().await;
    let accepted_after = accepted(&raw.start(Some(&key()), json!({ "script": "quick" })).await);

    checked(&listed);
    assert!(listed.is(&stackly_protocol::codes::daemon::STARTING));
    assert_eq!(listed.detail("subsystem"), Some("registry"));
    checked(&stats);
    assert_eq!(stats.detail("subsystem"), Some("operations"));
    assert!(started.is(&stackly_protocol::codes::daemon::STARTING));
    assert_eq!(started.detail("subsystem"), Some("operations"));
    assert!(!accepted_after.replayed);
    stop(daemon).await;
}
