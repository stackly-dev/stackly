//! The process of a NATS instance in `instance.get_metrics` (spec 0028,
//! milestone 5; AC-4, AC-5): `nats-server` is one process, so the tree is the
//! main process. The test is native, `#[ignore = "native: ..."]`, and ends as
//! `BLOCKED` when the artifact is not in the runtime cache.

use std::time::Duration;

use serde_json::json;
use stackly_core::metrics::SamplerConfig;
use stackly_protocol::instance::{MetricsState, UsageScope};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{Serve, Site, create_instance, port_of, stop_instance, varz};
use crate::metrics_calls::wait_metrics;

// covers: AC-4, AC-5
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_reports_one_process_and_the_tree_equals_the_main_process() {
    let ev = evidence!(
        covers = ["REQ-018", "AC-018"],
        kind = Kind::Native,
        expected = "a NATS 2.15.1 instance reports a tree of one process that is measured, with the same CPU and memory as the main process, a one row process list named nats-server, and a footprint above zero",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site
        .in_process(|config| {
            config.metrics = SamplerConfig::default().with_interval(Duration::from_millis(500));
        })
        .await;
    let api = site.api().await;
    let id = create_instance(&site, &api, "metrics", json!({})).await;
    let running = api.start_ready(id).await;
    let pid = running.process.as_ref().expect("a running process").pid;
    let report = varz(port_of(&running, "monitoring"));
    site.subject(&ev, report["version"].as_str().unwrap_or("unknown"));

    let reply = wait_metrics(
        &api,
        id,
        "two samples with CPU",
        Duration::from_secs(60),
        |reply| reply.samples.len() >= 3,
    )
    .await;
    assert_eq!(reply.state, MetricsState::Live);
    let sample = reply.samples.last().expect("a sample");
    assert_eq!(sample.main.scope, UsageScope::Main);
    assert_eq!(sample.tree.scope, UsageScope::Tree);
    assert_eq!(sample.main.process_count.get(), 1);
    assert_eq!(sample.tree.process_count.get(), 1, "{sample:#?}");
    assert_eq!(sample.tree.measured_count.get(), 1);
    assert_eq!(sample.tree.memory, sample.main.memory);
    assert_eq!(sample.tree.cpu, sample.main.cpu);
    assert_eq!(sample.tree.disk_read, sample.main.disk_read);
    assert_eq!(sample.tree.disk_write, sample.main.disk_write);
    let rows = &reply.latest_processes;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_main && rows[0].pid == pid && rows[0].depth == 0);
    assert_eq!(rows[0].name, "nats-server");
    assert!(!reply.processes_truncated);

    stop_instance(&api, id).await;
    daemon.shutdown().await;
    ev.pass_with(&format!("nats-server {pid} is a tree of one process"));
}
