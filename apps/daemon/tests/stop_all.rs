//! `instance.stop_all` with `expectedInstanceIds` through the real daemon
//! (spec 0032 AC-7, AC-8): the operation stops exactly the instances of the
//! preview, a set that changed since the preview is refused with
//! `instance.stop_all_changed` before any operation exists, and the idempotency
//! record covers the field.
#![cfg(target_os = "macos")]

#[path = "supervisor/support.rs"]
mod support;

use serde_json::json;
use stackly_ipc::CallOptions;
use stackly_protocol::ErrorBody;
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::stop_all::{StopAllParams, StopAllPreview, StopAllResult};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use crate::support::{Api, Site, key};

async fn preview(api: &Api) -> StopAllPreview {
    match api
        .client
        .call_detailed(Call::InstancePreviewStopAll, CallOptions::default())
        .await
        .expect("the preview answers")
    {
        Reply::InstancePreviewStopAll(preview) => *preview,
        other => panic!("not a preview reply: {other:?}"),
    }
}

fn ids_of(preview: &StopAllPreview) -> Vec<Uuid> {
    preview.items.iter().map(|item| item.instance_id).collect()
}

async fn confirm(
    api: &Api,
    key: &str,
    expected: Vec<Uuid>,
) -> Result<StopAllResult, Box<ErrorBody>> {
    let params = StopAllParams {
        expected_instance_ids: Some(expected),
    };
    match api
        .client
        .call_detailed(
            Call::InstanceStopAll(params),
            CallOptions::default().with_idempotency_key(key),
        )
        .await?
    {
        Reply::InstanceStopAll(result) => Ok(*result),
        other => panic!("not a stop_all reply: {other:?}"),
    }
}

async fn lifecycle(api: &Api, id: Uuid) -> InstanceLifecycle {
    api.get(id).await.expect("the instance reads").lifecycle
}

#[tokio::test(flavor = "multi_thread")]
async fn confirming_the_preview_stops_exactly_its_instances_and_a_replay_answers_the_same() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    let b = api.create_ready("bravo", json!({})).await;
    let idle = api.create_ready("idle", json!({})).await;
    api.start_ready(a).await;
    api.start_ready(b).await;

    let shown = preview(&api).await;
    let once = key();
    let result = confirm(&api, &once, ids_of(&shown))
        .await
        .expect("the preview confirms");

    assert_eq!(result.stopped.len(), 2);
    assert!(result.stopped.contains(&a) && result.stopped.contains(&b));
    assert!(result.failed.is_empty() && result.skipped.is_empty());
    assert_eq!(lifecycle(&api, a).await, InstanceLifecycle::Stopped);
    assert_eq!(lifecycle(&api, b).await, InstanceLifecycle::Stopped);
    assert_eq!(lifecycle(&api, idle).await, InstanceLifecycle::Stopped);

    // The same set in another order, with a repeat, is the same request.
    let mut shuffled = ids_of(&shown);
    shuffled.reverse();
    shuffled.push(shuffled[0]);
    let again = confirm(&api, &once, shuffled)
        .await
        .expect("the replay answers");
    assert_eq!(again, result);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_instance_that_started_after_the_preview_stops_nothing() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    let late = api.create_ready("late", json!({})).await;
    api.start_ready(a).await;
    let shown = preview(&api).await;
    assert_eq!(ids_of(&shown), [a]);

    api.start_ready(late).await;
    let refused = key();
    let error = confirm(&api, &refused, ids_of(&shown))
        .await
        .expect_err("the set changed");

    assert_eq!(error.code(), "instance.stop_all_changed");
    assert_eq!(lifecycle(&api, a).await, InstanceLifecycle::Running);
    assert_eq!(lifecycle(&api, late).await, InstanceLifecycle::Running);

    // No operation was created, so the key is free, and a fresh preview
    // confirms with the same key.
    let fresh = preview(&api).await;
    assert_eq!(ids_of(&fresh), [a, late]);
    let result = confirm(&api, &refused, ids_of(&fresh))
        .await
        .expect("the new preview confirms with the key the refusal left free");
    assert_eq!(result.stopped.len(), 2);
    assert_eq!(lifecycle(&api, a).await, InstanceLifecycle::Stopped);
    assert_eq!(lifecycle(&api, late).await, InstanceLifecycle::Stopped);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_instance_that_stopped_after_the_preview_changes_the_set_too() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    let b = api.create_ready("bravo", json!({})).await;
    api.start_ready(a).await;
    api.start_ready(b).await;
    let shown = preview(&api).await;

    let accepted = api
        .stop_when_free(b)
        .await
        .accepted
        .expect("a stop of a running instance begins an operation");
    api.wait_final(accepted.operation.id).await;
    let error = confirm(&api, &key(), ids_of(&shown))
        .await
        .expect_err("the set changed");

    assert_eq!(error.code(), "instance.stop_all_changed");
    assert_eq!(lifecycle(&api, a).await, InstanceLifecycle::Running);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_id_that_is_no_instance_and_an_empty_set_over_a_running_instance_are_refused() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    api.start_ready(a).await;

    for expected in [vec![a, Uuid::nil()], vec![a, Uuid::now_v7()], Vec::new()] {
        let error = confirm(&api, &key(), expected)
            .await
            .expect_err("the set is not the preview");
        assert_eq!(error.code(), "instance.stop_all_changed");
    }
    assert_eq!(lifecycle(&api, a).await, InstanceLifecycle::Running);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_idempotency_record_covers_the_expected_set() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    let b = api.create_ready("bravo", json!({})).await;
    api.start_ready(a).await;
    api.start_ready(b).await;
    let shown = preview(&api).await;
    let once = key();
    confirm(&api, &once, ids_of(&shown))
        .await
        .expect("the preview confirms");

    let error = confirm(&api, &once, vec![a])
        .await
        .expect_err("the key belongs to another request");
    assert_eq!(error.code(), "operation.idempotency_key_reused");

    let params = StopAllParams {
        expected_instance_ids: None,
    };
    let error = api
        .client
        .call_detailed(
            Call::InstanceStopAll(params),
            CallOptions::default().with_idempotency_key(&once),
        )
        .await
        .expect_err("a request without the field is another request");
    assert_eq!(error.code(), "operation.idempotency_key_reused");
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}
