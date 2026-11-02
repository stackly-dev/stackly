//! `instance.get_metrics` for the native provider tests (spec 0028,
//! milestone 5): one call with a cursor, and a wait until a reply holds what a
//! test needs.
#![allow(
    dead_code,
    reason = "each native test crate that mounts this file calls a part of it"
)]

use std::time::Duration;

use stackly_protocol::instance::{GetMetricsParams, MetricsReply};
use stackly_protocol::{Call, ErrorBody, Reply, Sequence};
use uuid::Uuid;

use crate::support::Api;

/// `instance.get_metrics` with the cursor of the caller.
pub async fn metrics(
    api: &Api,
    id: Uuid,
    history_id: Option<&str>,
    after_seq: Option<u64>,
) -> Result<MetricsReply, Box<ErrorBody>> {
    match api
        .client
        .call(Call::InstanceGetMetrics(GetMetricsParams {
            instance_id: id,
            history_id: history_id.map(str::to_owned),
            after_seq: after_seq.and_then(Sequence::new),
        }))
        .await?
    {
        Reply::InstanceGetMetrics(reply) => Ok(*reply),
        other => panic!("not a get_metrics reply: {other:?}"),
    }
}

/// Reads the whole history until `ready` holds, or panics after `limit`.
pub async fn wait_metrics(
    api: &Api,
    id: Uuid,
    what: &str,
    limit: Duration,
    ready: impl Fn(&MetricsReply) -> bool,
) -> MetricsReply {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let reply = metrics(api, id, None, None).await.expect("a reply");
        if ready(&reply) {
            return reply;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no {what} in {limit:?}: {reply:#?}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
