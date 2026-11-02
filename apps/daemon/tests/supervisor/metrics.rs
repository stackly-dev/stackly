//! `instance.get_metrics` through the real daemon (spec 0028 AC-1, AC-3,
//! AC-5, AC-6, AC-10, AC-11): the sampler reads the fake server's real
//! process without a client asking, the cursor rules, the not running case,
//! the `last_scan` slot, and the restart counter after a crash.

use std::time::Duration;

use serde_json::json;
use stackly_core::metrics::SamplerConfig;
use stackly_protocol::instance::{
    CountMetric, DataUsageMetric, GetMetricsParams, MetricsReply, MetricsState, NumberMetric,
    UnavailableReason,
};
use stackly_protocol::{Call, ErrorBody, Reply, Sequence};
use uuid::Uuid;

use crate::support::{Api, Site, fast};

const PATIENCE: Duration = Duration::from_secs(20);

fn quick() -> stacklyd::server::DaemonConfig {
    let mut config = fast();
    config.metrics = SamplerConfig::default().with_interval(Duration::from_millis(100));
    config.data_usage.schedule_interval = Duration::from_millis(300);
    config
}

async fn metrics(
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

/// Reads the metrics until `ready` holds.
async fn wait_metrics(
    api: &Api,
    id: Uuid,
    what: &str,
    ready: impl Fn(&MetricsReply) -> bool,
) -> MetricsReply {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let reply = metrics(api, id, None, None).await.expect("a reply");
        if ready(&reply) {
            return reply;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no {what}: {reply:#?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// covers: AC-1, AC-2, AC-3, AC-4, AC-5, AC-11
#[tokio::test(flavor = "multi_thread")]
async fn a_running_instance_is_sampled_without_any_client_asking() {
    let site = Site::new();
    let _daemon = site.start_with(quick()).await;
    let api = site.api().await;
    let id = api.create_ready("sampled", json!({})).await;
    let running = api.start_ready(id).await;
    let pid = running
        .process
        .expect("a running instance has a process")
        .pid;

    let reply = wait_metrics(&api, id, "two samples", |reply| reply.samples.len() >= 2).await;

    assert_eq!(reply.state, MetricsState::Live);
    assert_eq!(reply.process.map(|process| process.pid), Some(pid));
    assert_eq!(reply.interval_seconds, 1);
    assert_eq!(reply.capacity.get(), 60);
    assert!(reply.history_id.is_some());
    let first = &reply.samples[0];
    assert_eq!(first.seq.get(), 1);
    assert!(
        matches!(first.main.cpu, NumberMetric::Unavailable(content) if content.reason == UnavailableReason::WarmingUp),
        "the first sample has nothing to take a difference from: {first:?}"
    );
    assert!(
        matches!(first.main.disk_read, CountMetric::Unavailable(content) if content.reason == UnavailableReason::WarmingUp)
    );
    assert!(
        matches!(first.main.memory, CountMetric::Value(bytes) if bytes.get() > 0),
        "the footprint needs no earlier read: {first:?}"
    );
    assert!(matches!(reply.samples[1].main.cpu, NumberMetric::Value(_)));
    assert_eq!(first.main.process_count.get(), 1);
    assert!(first.tree.process_count.get() >= 1);
    let newest = reply
        .latest_processes
        .first()
        .expect("the newest sample lists the tree");
    assert!(newest.is_main && newest.parent_pid.is_none() && newest.depth == 0);
    assert_eq!(newest.pid, pid);
    assert!(!newest.name.is_empty());
    assert_eq!(reply.restart_total.get(), 0);
    assert!(reply.history_reset, "a client without a cursor starts over");
}

// covers: AC-11
#[tokio::test(flavor = "multi_thread")]
async fn the_cursor_returns_only_new_samples_and_resets_on_another_history() {
    let site = Site::new();
    let _daemon = site.start_with(quick()).await;
    let api = site.api().await;
    let id = api.create_ready("cursor", json!({})).await;
    api.start_ready(id).await;
    let all = wait_metrics(&api, id, "three samples", |reply| reply.samples.len() >= 3).await;
    let history = all.history_id.clone().expect("a live history");
    let last = all.samples.last().expect("samples").seq.get();

    let next = wait_metrics_after(&api, id, &history, last).await;
    let beyond = metrics(&api, id, Some(&history), Some(last + 10_000))
        .await
        .expect("a reply");
    let other = metrics(&api, id, Some("1-1.000000"), Some(last))
        .await
        .expect("a reply");

    assert!(!next.history_reset);
    assert!(next.samples.iter().all(|sample| sample.seq.get() > last));
    assert!(!next.samples.is_empty());
    assert_eq!(next.history_id.as_deref(), Some(history.as_str()));
    assert!(beyond.samples.is_empty() && !beyond.history_reset);
    assert!(beyond.latest_processes.is_empty());
    assert!(other.history_reset, "another history id resets the client");
    assert!(other.samples.len() >= 3, "and sends everything");
}

async fn wait_metrics_after(api: &Api, id: Uuid, history: &str, after: u64) -> MetricsReply {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let reply = metrics(api, id, Some(history), Some(after))
            .await
            .expect("a reply");
        if !reply.samples.is_empty() {
            return reply;
        }
        assert!(tokio::time::Instant::now() < deadline, "no new sample");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// covers: AC-1, AC-11
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_that_is_not_running_has_no_history_and_never_starts_a_scan() {
    let site = Site::new();
    let _daemon = site.start_with(quick()).await;
    let api = site.api().await;
    let id = api.create_ready("idle", json!({})).await;

    let before = metrics(&api, id, None, None).await.expect("a reply");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = metrics(&api, id, None, None).await.expect("a reply");

    for reply in [&before, &after] {
        assert_eq!(reply.state, MetricsState::NotRunning);
        assert_eq!(
            reply.lifecycle,
            stackly_protocol::instance::InstanceLifecycle::Stopped
        );
        assert!(reply.history_id.is_none() && reply.process.is_none());
        assert!(reply.samples.is_empty() && reply.latest_processes.is_empty());
        assert!(reply.history_reset);
        assert_eq!(
            reply.data_usage,
            DataUsageMetric::Unavailable(stackly_protocol::instance::UnavailableContent {
                reason: UnavailableReason::NotScanned
            }),
            "the method reads the slot and never scans"
        );
    }
}

// covers: AC-11
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_instance_is_not_found() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;

    let error = metrics(&api, Uuid::now_v7(), None, None)
        .await
        .expect_err("no such instance");

    assert_eq!(error.code(), "resource.not_found");
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn the_history_goes_when_the_instance_stops() {
    let site = Site::new();
    let _daemon = site.start_with(quick()).await;
    let api = site.api().await;
    let id = api.create_ready("stopping", json!({})).await;
    api.start_ready(id).await;
    wait_metrics(&api, id, "a sample", |reply| !reply.samples.is_empty()).await;

    api.stop_when_free(id).await;
    let stopped = wait_metrics(&api, id, "not running", |reply| {
        reply.state == MetricsState::NotRunning
    })
    .await;

    assert!(stopped.samples.is_empty() && stopped.history_id.is_none());
}

// covers: AC-10
#[tokio::test(flavor = "multi_thread")]
async fn a_running_instance_is_scanned_on_its_own_and_an_on_demand_scan_fills_the_slot() {
    let site = Site::new();
    let _daemon = site.start_with(quick()).await;
    let api = site.api().await;
    let id = api.create_ready("scanned", json!({})).await;
    api.start_ready(id).await;

    let first = wait_metrics(&api, id, "a scheduled scan", |reply| {
        matches!(reply.data_usage, DataUsageMetric::Scanned(_))
    })
    .await;
    let DataUsageMetric::Scanned(first_scan) = first.data_usage else {
        panic!("scanned above");
    };
    let later = wait_metrics(&api, id, "a second scheduled scan", |reply| {
        matches!(reply.data_usage, DataUsageMetric::Scanned(scan) if scan.scanned_at > first_scan.scanned_at)
    })
    .await;

    assert!(first_scan.bytes.get() > 0 && first_scan.complete);
    assert!(matches!(later.data_usage, DataUsageMetric::Scanned(_)));

    let other = api.create_ready("on-demand", json!({})).await;
    let none = metrics(&api, other, None, None).await.expect("a reply");
    assert!(matches!(none.data_usage, DataUsageMetric::Unavailable(_)));
    let measured = api
        .data_usage(other, false, false)
        .await
        .expect("a scan")
        .expect("a size");
    let slot = metrics(&api, other, None, None).await.expect("a reply");
    assert!(matches!(
        slot.data_usage,
        DataUsageMetric::Scanned(scan) if scan.bytes == measured.bytes && scan.scanned_at == measured.scanned_at
    ));
}

// covers: AC-6
#[tokio::test(flavor = "multi_thread")]
async fn a_respawn_after_a_crash_counts_one_restart_since_created() {
    let site = Site::new();
    let _daemon = site.start_with(quick()).await;
    let api = site.api().await;
    let id = api.create_ready("crashy", json!({})).await;
    let running = api.start_ready(id).await;
    let pid = running.process.expect("a process").pid;
    assert_eq!(running.restart_total.get(), 0);

    let status = std::process::Command::new("/bin/kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let respawned = api
        .wait_instance(id, "a respawned process", |summary| {
            summary.restart_total.get() == 1
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != pid)
                && summary.readiness.state
                    == stackly_protocol::instance::InstanceReadinessState::Ready
        })
        .await;

    let reply = metrics(&api, id, None, None).await.expect("a reply");
    assert_eq!(respawned.restart_total.get(), 1);
    assert_eq!(reply.restart_total.get(), 1);
}
