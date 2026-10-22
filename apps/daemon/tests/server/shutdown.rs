//! `daemon.shutdown` (spec 0015 AC-6): `stop` and `restart` are accepted and
//! reach the loop that runs the stop order; `restart` needs a daemon that
//! launchd started; only the first request counts; the method is served
//! while the daemon starts and in recovery mode; `daemon.get_info` says who
//! started the daemon.

use std::time::Duration;

use serde_json::{Value, json};
use stackly_platform::LaunchContext;
use stackly_platform::bundle::DAEMON_LABEL;
use stackly_protocol::codes::{daemon, request};
use stackly_protocol::daemon::ShutdownMode;
use stackly_protocol::events::ShutdownReason;
use stacklyd::server;

use crate::common::{Raw, Site, error_of, request as request_payload};
use crate::launch::{deps, launchd, stop};

/// A short wait for something that must not happen.
const QUIET: Duration = Duration::from_millis(300);

async fn shutdown_reply(raw: &mut Raw, id: &str, mode: &str) -> Value {
    raw.send(&request_payload(
        id,
        "daemon.shutdown",
        Some(json!({ "mode": mode })),
    ))
    .await;
    raw.answer(id).await
}

#[tokio::test]
async fn stop_is_answered_and_reaches_the_stop_loop() {
    let site = Site::new();
    let handle = server::start(deps(&site, true, LaunchContext::Manual, None))
        .await
        .expect("the server starts");
    let mut raw = Raw::ready(&site.socket()).await;

    let reply = shutdown_reply(&mut raw, "s1", "stop").await;

    assert_eq!(reply["type"], "response", "{reply}");
    assert_eq!(reply["content"]["reply"]["type"], "daemon.shutdown");
    assert_eq!(reply["content"]["reply"]["content"]["mode"], "stop");
    assert_eq!(
        reply["content"]["reply"]["content"]["serverInstanceId"],
        handle.info().server_instance_id.to_string()
    );
    assert_eq!(handle.stop_requested().await, ShutdownMode::Stop);
    stop(handle).await;
}

#[tokio::test]
async fn restart_under_launchd_is_accepted() {
    let site = Site::new();
    let handle = server::start(deps(&site, true, launchd(DAEMON_LABEL), None))
        .await
        .expect("the server starts");
    let mut raw = Raw::ready(&site.socket()).await;

    let reply = shutdown_reply(&mut raw, "r1", "restart").await;

    assert_eq!(reply["content"]["reply"]["content"]["mode"], "restart");
    assert_eq!(handle.stop_requested().await, ShutdownMode::Restart);
    handle
        .shutdown(ShutdownReason::RestartRequested, std::future::pending())
        .await;
}

#[tokio::test]
async fn restart_of_a_manual_daemon_is_refused_and_the_daemon_keeps_running() {
    let site = Site::new();
    let handle = server::start(deps(&site, true, LaunchContext::Manual, None))
        .await
        .expect("the server starts");
    let mut raw = Raw::ready(&site.socket()).await;

    let reply = shutdown_reply(&mut raw, "r2", "restart").await;

    let error = error_of(&reply);
    assert!(error.is(&daemon::RESTART_UNAVAILABLE), "{error:?}");
    assert_eq!(error.detail("launchContext"), Some("manual"));
    assert!(
        tokio::time::timeout(QUIET, handle.stop_requested())
            .await
            .is_err(),
        "a refused restart must not stop the daemon"
    );
    raw.send(&request_payload("i1", "daemon.get_info", None))
        .await;
    assert_eq!(raw.answer("i1").await["type"], "response");
    stop(handle).await;
}

#[tokio::test]
async fn a_second_request_after_the_first_is_refused_as_shutting_down() {
    let site = Site::new();
    let handle = server::start(deps(&site, true, launchd(DAEMON_LABEL), None))
        .await
        .expect("the server starts");
    let mut raw = Raw::ready(&site.socket()).await;
    let first = shutdown_reply(&mut raw, "a", "stop").await;
    assert_eq!(first["type"], "response", "{first}");

    let second = shutdown_reply(&mut raw, "b", "restart").await;

    assert!(error_of(&second).is(&daemon::SHUTTING_DOWN), "{second}");
    assert_eq!(handle.stop_requested().await, ShutdownMode::Stop);
    stop(handle).await;
}

#[tokio::test]
async fn an_unknown_mode_is_invalid_params() {
    let site = Site::new();
    let handle = server::start(deps(&site, true, LaunchContext::Manual, None))
        .await
        .expect("the server starts");
    let mut raw = Raw::ready(&site.socket()).await;

    let reply = shutdown_reply(&mut raw, "u", "pause").await;

    assert!(error_of(&reply).is(&request::INVALID_PARAMS), "{reply}");
    assert!(
        tokio::time::timeout(QUIET, handle.stop_requested())
            .await
            .is_err()
    );
    stop(handle).await;
}

#[tokio::test]
async fn shutdown_is_served_while_the_daemon_starts() {
    let site = Site::new();
    let (handle, gate) = {
        let mut config = crate::common::config();
        let (sender, receiver) = tokio::sync::watch::channel(false);
        config.start_gate = Some(receiver);
        let handle =
            server::start(deps(&site, true, LaunchContext::Manual, None).with_config(config))
                .await
                .expect("the server starts");
        (handle, sender)
    };
    let mut raw = Raw::ready(&site.socket()).await;

    let reply = shutdown_reply(&mut raw, "st", "stop").await;

    assert_eq!(reply["type"], "response", "{reply}");
    assert_eq!(handle.stop_requested().await, ShutdownMode::Stop);
    gate.send_replace(true);
    stop(handle).await;
}

#[tokio::test]
async fn get_info_reports_the_launch_context_and_the_bundle() {
    let site = Site::new();
    let exe = std::path::Path::new("/Applications/Stackly.app/Contents/MacOS/stacklyd");
    let bundle = stackly_platform::bundle::AppBundle::enclosing(exe);
    let handle = server::start(deps(&site, true, launchd(DAEMON_LABEL), bundle))
        .await
        .expect("the server starts");
    let mut raw = Raw::ready(&site.socket()).await;
    raw.send(&request_payload("g", "daemon.get_info", None))
        .await;

    let reply = raw.answer("g").await;

    let launch = &reply["content"]["reply"]["content"]["launch"];
    assert_eq!(launch["context"], "launchd", "{reply}");
    assert_eq!(launch["label"], DAEMON_LABEL);
    assert_eq!(launch["bundlePath"], "/Applications/Stackly.app");
    stop(handle).await;
}

#[tokio::test]
async fn shutdown_is_served_in_recovery_mode() {
    let site = Site::new();
    let first = server::start(deps(&site, true, LaunchContext::Manual, None))
        .await
        .expect("the server starts");
    first.started().await;
    stop(first).await;
    // A registry newer than the app puts the next start in recovery mode.
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).expect("the registry file can be read");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).expect("the registry file can be written");
    let handle = site.start().await;
    assert!(
        handle.health().recovery.is_some(),
        "the daemon is in recovery mode"
    );
    let mut raw = Raw::ready(&site.socket()).await;

    let reply = shutdown_reply(&mut raw, "rec", "stop").await;

    assert_eq!(reply["type"], "response", "{reply}");
    assert_eq!(handle.stop_requested().await, ShutdownMode::Stop);
    stop(handle).await;
}
