//! The output of a process (spec 0010 AC-12): read from the spool into the
//! sinks of the daemon, and the spool kept small by punch holes.

use std::os::unix::fs::MetadataExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_platform::OutputStream;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::operation::OperationState;

use crate::support::{LogRecorder, Site, fast};

/// The most a spool may hold in blocks at any sample (AC-12).
const SPOOL_BLOCKS_MAX: u64 = 2 * 1024 * 1024;

/// Allocated bytes of the file at `path`, 0 when it does not exist.
fn allocated(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map_or(0, |metadata| metadata.blocks() * 512)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chatty_process_reaches_the_sink_and_its_spools_stay_under_2_mib() {
    let site = Site::new();
    let recorder = Arc::new(LogRecorder::default());
    let mut config = fast();
    config
        .extra_log_sinks
        .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let id = api
        .create_ready("chatty", json!({ "logLinesPerSecond": 2000 }))
        .await;
    let key = id
        .to_string()
        .parse::<stackly_domain::InstanceId>()
        .expect("an instance id")
        .short_key();
    let layout = site.prepared.layout();
    let spools = [
        layout.instance_spool(key, OutputStream::Stdout),
        layout.instance_spool(key, OutputStream::Stderr),
    ];

    api.start_ready(id).await;
    // 25 seconds write more than 2 MiB to each spool.
    let started = Instant::now();
    let mut worst = 0;
    while started.elapsed() < Duration::from_secs(25) {
        for spool in &spools {
            worst = worst.max(allocated(spool));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let accepted = api
        .stop(id)
        .await
        .expect("the stop is accepted")
        .accepted
        .expect("a stop begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    for spool in &spools {
        worst = worst.max(allocated(spool));
    }

    assert!(
        worst < SPOOL_BLOCKS_MAX,
        "a spool held {worst} bytes in blocks"
    );
    let lines = recorder.lines_of(id);
    assert!(lines > 40_000, "the sink saw {lines} lines");
    assert!(
        recorder
            .text_of(id)
            .iter()
            .all(|line| line.len() <= 100 && !line.contains('\n')),
        "every line reached the sink whole"
    );
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}
