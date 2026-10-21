//! The launch record (spec 0015 AC-5): launchd starting the production
//! agent writes `background-agent.json` before the socket is bound; a manual
//! start, a test agent label, and a root override write nothing; a write that
//! fails never stops the start.

use std::io;
use std::sync::Arc;

use serde_json::Value;
use stackly_platform::bundle::{AppBundle, DAEMON_LABEL};
use stackly_platform::{Clock, LaunchContext, SystemClock};
use stackly_protocol::Timestamp;
use stackly_protocol::events::ShutdownReason;
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use crate::common::{Site, client, config};

fn launchd(label: &str) -> LaunchContext {
    LaunchContext::Launchd {
        label: label.to_owned(),
    }
}

/// Deps for `site` with the given override flag and launch facts.
fn deps(
    site: &Site,
    root_override: bool,
    launch: LaunchContext,
    bundle: Option<AppBundle>,
) -> DaemonDeps {
    let log =
        LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON).expect("the log writer starts");
    let facts = StartFacts::new(
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        root_override,
        std::process::id(),
    );
    DaemonDeps::new(
        site.platform.clone(),
        site.prepared.clone(),
        facts,
        Arc::new(log),
    )
    .with_config(config())
    .with_launch(launch, bundle)
}

async fn stop(handle: DaemonHandle) {
    handle
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

fn record_path(site: &Site) -> std::path::PathBuf {
    site.prepared.layout().agent_launch_record()
}

#[tokio::test]
async fn the_production_agent_has_written_the_record_before_the_first_handshake() {
    let site = Site::new();
    let exe = std::path::Path::new("/Applications/Stackly.app/Contents/MacOS/stacklyd");
    let bundle = AppBundle::enclosing(exe);
    let handle = server::start(deps(&site, false, launchd(DAEMON_LABEL), bundle))
        .await
        .expect("the server starts");

    // The record is there by the time start returns, so the first client
    // that connects can already read it.
    let text = std::fs::read_to_string(record_path(&site)).expect("the record exists");
    let _ = client(&site).await;
    let record: Value = serde_json::from_str(&text).expect("the record is JSON");
    assert_eq!(record["schema"], 1);
    assert_eq!(record["firstLaunchedAt"], record["lastLaunchedAt"]);
    assert_eq!(record["bundlePath"], "/Applications/Stackly.app");
    assert_eq!(record["version"], stackly_core::daemon_info().version);
    stop(handle).await;
}

#[tokio::test]
async fn a_manual_start_writes_no_record() {
    let site = Site::new();
    let handle = server::start(deps(&site, false, LaunchContext::Manual, None))
        .await
        .expect("the server starts");
    assert!(!record_path(&site).exists());
    stop(handle).await;
}

#[tokio::test]
async fn a_test_agent_label_writes_no_record() {
    let site = Site::new();
    let handle = server::start(deps(
        &site,
        false,
        launchd("dev.stackly.test.agent.7-1"),
        None,
    ))
    .await
    .expect("the server starts");
    assert!(!record_path(&site).exists());
    stop(handle).await;
}

#[tokio::test]
async fn a_root_override_writes_no_record_even_under_launchd() {
    let site = Site::new();
    let handle = server::start(deps(&site, true, launchd(DAEMON_LABEL), None))
        .await
        .expect("the server starts");
    assert!(!record_path(&site).exists());
    stop(handle).await;
}

#[tokio::test]
async fn a_failed_record_write_still_starts_the_daemon() {
    let site = Site::new();
    // A folder where the file belongs makes the atomic rename fail.
    std::fs::create_dir(record_path(&site)).expect("the blocking folder can be made");
    let handle = server::start(deps(&site, false, launchd(DAEMON_LABEL), None))
        .await
        .expect("the server starts although the record cannot be written");
    handle.started().await;
    let _ = client(&site).await;
    stop(handle).await;
}
