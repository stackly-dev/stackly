//! `instance.preview_stop_all` and `instance.stop_all` through the real
//! daemon (spec 0031 AC-10): the preview and the operation choose the same
//! instances, the operation stops them as a person's stop does, an
//! environment lease keeps an instance out, and a request that comes again
//! with its key gets the same answer.

use serde_json::json;
use stackly_domain::{InstanceId, Registry, WriteBatch, WriteOp};
use stackly_ipc::CallOptions;
use stackly_protocol::instance::{InstanceDesired, InstanceLifecycle};
use stackly_protocol::stop_all::{StopAllParams, StopAllPreview, StopAllResult};
use stackly_protocol::{Call, Reply};
use stackly_testkit::{EnvironmentBuilder, LeaseBuilder};

use crate::support::{Api, Site, key};

async fn preview(api: &Api) -> StopAllPreview {
    match api
        .client
        .call_detailed(
            Call::InstancePreviewStopAll(Default::default()),
            CallOptions::default(),
        )
        .await
        .expect("the preview answers")
    {
        Reply::InstancePreviewStopAll(preview) => *preview,
        other => panic!("not a preview reply: {other:?}"),
    }
}

async fn stop_all(api: &Api, key: &str) -> Result<StopAllResult, Box<stackly_protocol::ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::InstanceStopAll(StopAllParams::default()),
            CallOptions::default().with_idempotency_key(key),
        )
        .await?
    {
        Reply::InstanceStopAll(result) => Ok(*result),
        other => panic!("not a stop_all reply: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_all_stops_what_the_preview_shows_and_answers_the_same_to_a_repeated_key() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    let b = api.create_ready("bravo", json!({})).await;
    let idle = api.create_ready("idle", json!({})).await;
    api.start_ready(a).await;
    api.start_ready(b).await;

    let shown = preview(&api).await;
    let names: Vec<_> = shown.items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, ["alpha", "bravo"]);
    assert!(shown.skipped.is_empty() && shown.already_stopping.is_empty());
    assert_eq!(shown.at_login_count.get(), 0);

    let once = key();
    let result = stop_all(&api, &once).await.expect("stop all runs");
    assert_eq!(result.stopped.len(), 2);
    assert!(result.stopped.contains(&a) && result.stopped.contains(&b));
    assert!(result.failed.is_empty() && result.skipped.is_empty());
    for id in [a, b] {
        let summary = api.get(id).await.expect("the instance reads");
        assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
        assert_eq!(summary.desired, InstanceDesired::Stopped);
    }
    let idle_summary = api.get(idle).await.expect("the instance reads");
    assert_eq!(idle_summary.desired, InstanceDesired::Stopped);

    let again = stop_all(&api, &once).await.expect("the replay answers");
    assert_eq!(again, result);
    let nothing = preview(&api).await;
    assert!(nothing.items.is_empty());
    let empty = stop_all(&api, &key())
        .await
        .expect("an empty stop all still answers");
    assert_eq!(
        empty,
        StopAllResult {
            stopped: Vec::new(),
            failed: Vec::new(),
            skipped: Vec::new()
        }
    );
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_instance_an_environment_holds_is_skipped_and_keeps_running() {
    let site = Site::new();
    let manual = site.start().await;
    let held = {
        let api = site.api().await;
        api.create_ready("held", json!({})).await
    };
    manual
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    {
        let registry = site.open_registry();
        let id = InstanceId::from_uuid(held).expect("an instance id");
        let environment = EnvironmentBuilder::new().shared(id).build();
        let lease = LeaseBuilder::new(id).environment(environment.id()).build();
        registry
            .commit(WriteBatch::new(vec![
                WriteOp::InsertEnvironment(environment),
                WriteOp::InsertLease(lease),
            ]))
            .expect("the environment and its lease are stored");
    }

    let daemon = site.start().await;
    let api = site.api().await;
    api.start_ready(held).await;
    let shown = preview(&api).await;
    assert!(shown.items.is_empty());
    assert_eq!(shown.skipped.len(), 1);
    assert_eq!(shown.skipped[0].instance_id, held);
    assert_eq!(shown.skipped[0].code, "supervisor.stop_requires_override");

    let result = stop_all(&api, &key()).await.expect("stop all runs");
    assert!(result.stopped.is_empty());
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(result.skipped[0].instance_id, held);
    let summary = api.get(held).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Running);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}
