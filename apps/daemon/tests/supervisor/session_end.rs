//! The end of a login session through the real daemon server (spec 0031
//! AC-11 to AC-13): a logout stops the live instances and closes the session
//! row, a return in the same session reopens it and starts what the end
//! stopped, and only a `SIGTERM` at a launchd daemon with no earlier
//! `daemon.shutdown` is a logout.

use std::time::Duration;

use serde_json::json;
use stackly_domain::{InstanceId, Registry, SessionEndReason};
use stackly_platform::LaunchContext;
use stackly_protocol::daemon::ShutdownMode;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::instance::{
    AutostartSession, InstanceDesired, InstanceLifecycle, InstanceReadinessState,
};
use stackly_testkit::FakeLoginSessionInfo;
use stacklyd::server::DaemonHandle;
use tokio::time::Instant;
use uuid::Uuid;

use crate::login::{launchd, set_at_login, start, stop};
use crate::support::{Site, fast};

/// An `at_login` instance that runs in the launchd daemon of a new session,
/// with the daemon.
async fn running_after_login(site: &Site, session: &FakeLoginSessionInfo) -> (Uuid, DaemonHandle) {
    let manual = site.start().await;
    let id = {
        let api = site.api().await;
        api.create_ready("pg", json!({})).await
    };
    stop(manual).await;
    set_at_login(site, id);
    let first = start(site, launchd(), session).await;
    stop(first).await;
    session.set_audit_session(100_002);
    let daemon = start(site, launchd(), session).await;
    let api = site.api().await;
    api.wait_instance(id, "ready after the login", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    (id, daemon)
}

fn latest_row(site: &Site) -> stackly_domain::DaemonSession {
    site.open_registry()
        .recent_sessions(1)
        .expect("the rows are read")
        .value
        .remove(0)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_logout_stops_the_instance_and_coming_back_in_the_same_session_starts_it_again() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();
    let (id, daemon) = running_after_login(&site, &session).await;

    assert!(daemon.signal_ends_session("SIGTERM"));
    daemon.end_session(Instant::now()).await;
    daemon
        .shutdown(ShutdownReason::Signal, std::future::pending())
        .await;

    let registry = site.open_registry();
    let stored = registry
        .instance(InstanceId::from_uuid(id).expect("an instance id"))
        .expect("the registry reads")
        .value
        .expect("the instance exists");
    assert_eq!(
        stored.lifecycle(),
        stackly_domain::InstanceLifecycle::Stopped
    );
    assert_eq!(
        stored.desired(),
        stackly_domain::DesiredState::Running,
        "the end of a session leaves desired alone"
    );
    assert_eq!(
        latest_row(&site).end_reason(),
        Some(SessionEndReason::Clean)
    );
    drop(registry);

    // The same session again, as after `launchctl kickstart -k`.
    let back = start(&site, launchd(), &session).await;
    assert!(latest_row(&site).is_open(), "the row is open again");
    let api = site.api().await;
    let summary = api
        .wait_instance(id, "ready after the return", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(
        summary.autostart.session,
        AutostartSession::Active,
        "the lease of the session was never suppressed"
    );
    api.stop_when_free(id).await;
    drop(api);
    stop(back).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_budget_counts_from_the_signal_so_a_late_session_end_closes_the_row_interrupted() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();
    let (id, daemon) = running_after_login(&site, &session).await;

    let late = Instant::now()
        .checked_sub(fast().supervisor.session_end_stop_budget + Duration::from_secs(1))
        .expect("the clock is far enough from its origin");
    daemon.end_session(late).await;
    daemon
        .shutdown(ShutdownReason::Signal, std::future::pending())
        .await;
    assert_eq!(
        latest_row(&site).end_reason(),
        Some(SessionEndReason::Interrupted)
    );

    // The daemon that comes back finds the instance, stopped or still
    // stopping, and starts it again.
    let back = start(&site, launchd(), &session).await;
    let api = site.api().await;
    api.wait_instance(id, "ready after the return", |summary| {
        summary.lifecycle == InstanceLifecycle::Running
            && summary.readiness.state == InstanceReadinessState::Ready
            && summary.desired == InstanceDesired::Running
    })
    .await;
    api.stop_when_free(id).await;
    drop(api);
    stop(back).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_sigterm_at_a_launchd_daemon_before_any_shutdown_request_is_a_logout() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();

    let manual = start(&site, LaunchContext::Manual, &session).await;
    for signal in ["SIGTERM", "SIGINT", "SIGHUP"] {
        assert!(
            !manual.signal_ends_session(signal),
            "{signal} at a manual daemon is the stop of spec 0010"
        );
    }
    stop(manual).await;

    let agent = start(&site, launchd(), &session).await;
    assert!(!agent.signal_ends_session("SIGINT"));
    assert!(!agent.signal_ends_session("SIGHUP"));
    assert!(agent.signal_ends_session("SIGTERM"));
    assert!(agent.core().request_shutdown(ShutdownMode::Stop));
    assert!(
        !agent.signal_ends_session("SIGTERM"),
        "a SIGTERM after daemon.shutdown leaves the instances alone"
    );
    agent.close_session_clean().await;
    stop(agent).await;
    assert_eq!(
        latest_row(&site).end_reason(),
        Some(SessionEndReason::Clean)
    );
}
