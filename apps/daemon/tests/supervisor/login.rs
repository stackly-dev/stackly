//! The login session through the real daemon server (spec 0031 AC-3, AC-4,
//! AC-7, AC-8, AC-12): a launchd daemon with a scripted session starts the
//! instances that start at login once per session, a daemon outside launchd
//! never rotates, and a stop holds across a restart in the same session.

use std::io;
use std::sync::Arc;

use serde_json::json;
use stackly_domain::{InstanceId, Registry, StartupPolicy, WriteBatch, WriteOp};
use stackly_ipc::CallOptions;
use stackly_platform::{Clock, LaunchContext, SystemClock};
use stackly_protocol::Timestamp;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::instance::{
    AutostartSession, InstanceDesired, InstanceLifecycle, InstanceReadinessState, InstanceSummary,
    SetStartupParams, StartupPolicyKind,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::FakeLoginSessionInfo;
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use crate::support::{Site, fast, key};

fn launchd() -> LaunchContext {
    LaunchContext::Launchd {
        label: "com.stackly.test.login".to_owned(),
    }
}

/// A daemon with the launch context `launch`, reading its login session
/// from `session`.
async fn start(site: &Site, launch: LaunchContext, session: &FakeLoginSessionInfo) -> DaemonHandle {
    let log =
        LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON).expect("the log writer starts");
    let facts = StartFacts::new(
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        true,
        std::process::id(),
    );
    let platform = site
        .platform
        .clone()
        .with_login_session(Arc::new(session.clone()));
    let deps = DaemonDeps::new(platform, site.prepared.clone(), facts, Arc::new(log))
        .with_config(fast())
        .with_launch(launch, None);
    let handle = server::start(deps).await.expect("the server starts");
    handle.started().await;
    handle
}

async fn stop(handle: DaemonHandle) {
    handle
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

/// Turns the startup policy of `id` to `at_login` with no daemon running.
fn set_at_login(site: &Site, id: Uuid) {
    let registry = site.open_registry();
    let id = InstanceId::from_uuid(id).expect("an instance id the daemon made");
    let stored = registry
        .instance(id)
        .expect("the registry reads")
        .value
        .expect("the instance exists");
    let expected = stored.revision();
    let mut changed = stored.clone();
    changed.set_startup(StartupPolicy::AtLogin);
    registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
            instance: changed,
            expected,
        }]))
        .expect("the policy is written");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_login_starts_the_at_login_instance_once_and_a_stop_holds_in_the_same_session() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();

    // A daemon outside launchd creates the instance and writes no session.
    let manual = site.start().await;
    let id = {
        let api = site.api().await;
        api.create_ready("pg", json!({})).await
    };
    stop(manual).await;
    set_at_login(&site, id);

    // The first launchd run only records the session: nothing starts (AC-3).
    let first = start(&site, launchd(), &session).await;
    let api = site.api().await;
    let summary = api.get(id).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(summary.desired, InstanceDesired::Stopped);
    drop(api);
    stop(first).await;

    // A new login starts it without any client (AC-4).
    session.set_audit_session(100_002);
    let second = start(&site, launchd(), &session).await;
    let api = site.api().await;
    api.wait_instance(id, "ready after the login", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;

    // The user stops it; the same session does not start it again (AC-8).
    api.stop_when_free(id).await;
    let stopped = api
        .wait_instance(id, "stopped", |summary| {
            summary.lifecycle == InstanceLifecycle::Stopped
        })
        .await;
    assert_eq!(
        stopped.autostart.session,
        AutostartSession::Suppressed,
        "a stop by the person suppresses the startup lease of this session"
    );
    drop(api);
    stop(second).await;
    let third = start(&site, launchd(), &session).await;
    let api = site.api().await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let summary = api.get(id).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(summary.desired, InstanceDesired::Stopped);
    drop(api);
    stop(third).await;

    // The next login starts it again.
    session.set_audit_session(100_003);
    let fourth = start(&site, launchd(), &session).await;
    let api = site.api().await;
    api.wait_instance(id, "ready after the next login", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    api.stop_when_free(id).await;
    drop(api);
    stop(fourth).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_outside_launchd_never_rotates_or_starts_the_at_login_instance() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();
    let manual = site.start().await;
    let id = {
        let api = site.api().await;
        api.create_ready("pg", json!({})).await
    };
    stop(manual).await;
    set_at_login(&site, id);

    for audit in [100_001, 100_002] {
        session.set_audit_session(audit);
        let daemon = start(&site, LaunchContext::Manual, &session).await;
        let api = site.api().await;
        let summary = api.get(id).await.expect("the instance reads");
        assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
        assert_eq!(summary.desired, InstanceDesired::Stopped);
        drop(api);
        stop(daemon).await;
    }
    assert_eq!(
        session.reads(),
        0,
        "a manual daemon never reads the session"
    );
}

/// `instance.set_startup` with a fresh key.
async fn set_startup(
    api: &crate::support::Api,
    id: Uuid,
    startup: StartupPolicyKind,
    expected: Option<stackly_protocol::Revision>,
) -> Result<InstanceSummary, Box<stackly_protocol::ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::InstanceSetStartup(SetStartupParams {
                instance_id: id,
                startup,
                expected_revision: expected,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::InstanceSetStartup(summary) => Ok(*summary),
        other => panic!("not a set_startup reply: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_policy_is_set_apart_from_the_process_and_the_same_value_changes_nothing() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("pg", json!({})).await;
    let created = api.get(id).await.expect("the instance reads");
    assert_eq!(created.autostart.policy, StartupPolicyKind::Manual);
    assert_eq!(created.autostart.session, AutostartSession::None);

    let changed = set_startup(&api, id, StartupPolicyKind::AtLogin, None)
        .await
        .expect("the policy is set");
    assert_eq!(changed.autostart.policy, StartupPolicyKind::AtLogin);
    assert_eq!(
        changed.autostart.session,
        AutostartSession::None,
        "setting the policy creates no lease"
    );
    assert_eq!(changed.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(changed.desired, InstanceDesired::Stopped);
    assert!(changed.revision.get() > created.revision.get());

    let again = set_startup(&api, id, StartupPolicyKind::AtLogin, Some(changed.revision))
        .await
        .expect("the same value answers");
    assert_eq!(again.revision, changed.revision, "nothing was written");

    let stale = set_startup(&api, id, StartupPolicyKind::Manual, Some(created.revision))
        .await
        .expect_err("a stale revision is refused");
    assert_eq!(stale.code(), "resource.revision_mismatch");
    drop(api);
    stop(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_instance_created_with_startup_at_login_carries_the_policy() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let mut params = api.params("pg", json!({}));
    params.startup = Some(StartupPolicyKind::AtLogin);
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id: Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;

    let created = api.get(id).await.expect("the instance reads");
    assert_eq!(created.autostart.policy, StartupPolicyKind::AtLogin);
    drop(api);
    stop(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_the_policy_off_releases_the_lease_of_this_session() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();
    let manual = site.start().await;
    let id = {
        let api = site.api().await;
        api.create_ready("pg", json!({})).await
    };
    stop(manual).await;
    set_at_login(&site, id);
    let first = start(&site, launchd(), &session).await;
    stop(first).await;
    session.set_audit_session(100_002);
    let second = start(&site, launchd(), &session).await;
    let api = site.api().await;
    let running = api
        .wait_instance(id, "ready after the login", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(running.autostart.session, AutostartSession::Active);

    let off = set_startup(&api, id, StartupPolicyKind::Manual, None)
        .await
        .expect("the policy is set");
    assert_eq!(off.autostart.policy, StartupPolicyKind::Manual);
    assert_eq!(off.autostart.session, AutostartSession::None);
    assert_eq!(
        off.lifecycle,
        InstanceLifecycle::Running,
        "it keeps running"
    );

    api.stop_when_free(id).await;
    drop(api);
    stop(second).await;
}

/// Writes desired `running` on `id` with no daemon running.
fn set_desired_running(site: &Site, id: Uuid) {
    let registry = site.open_registry();
    let id = InstanceId::from_uuid(id).expect("an instance id the daemon made");
    let stored = registry
        .instance(id)
        .expect("the registry reads")
        .value
        .expect("the instance exists");
    let expected = stored.revision();
    let mut changed = stored.clone();
    changed.set_desired(stackly_domain::DesiredState::Running);
    registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
            instance: changed,
            expected,
        }]))
        .expect("the desired state is written");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_cannot_be_read_leaves_the_start_queue_to_the_run_again_list() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();
    let manual = site.start().await;
    let id = {
        let api = site.api().await;
        api.create_ready("pg", json!({})).await
    };
    stop(manual).await;
    set_desired_running(&site, id);

    session.fail_with(stackly_platform::LoginSessionError::Unavailable {
        reason: "no session",
    });
    let blind = start(&site, launchd(), &session).await;
    let api = site.api().await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let summary = api.get(id).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    drop(api);
    stop(blind).await;

    session.recover();
    let sighted = start(&site, launchd(), &session).await;
    let api = site.api().await;
    api.wait_instance(id, "ready once the session reads", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    api.stop_when_free(id).await;
    drop(api);
    stop(sighted).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_all_holds_the_startup_lease_back_for_the_rest_of_the_session() {
    let site = Site::new();
    let session = FakeLoginSessionInfo::new();
    let manual = site.start().await;
    let id = {
        let api = site.api().await;
        api.create_ready("pg", json!({})).await
    };
    stop(manual).await;
    set_at_login(&site, id);
    let first = start(&site, launchd(), &session).await;
    stop(first).await;
    session.set_audit_session(100_002);
    let second = start(&site, launchd(), &session).await;
    let api = site.api().await;
    api.wait_instance(id, "ready after the login", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;

    let reply = api
        .client
        .call_detailed(
            Call::InstanceStopAll(stackly_protocol::stop_all::StopAllParams::default()),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("stop all answers");
    let Reply::InstanceStopAll(result) = reply else {
        panic!("not a stop_all reply");
    };
    assert_eq!(result.stopped, vec![id]);
    let stopped = api.get(id).await.expect("the instance reads");
    assert_eq!(stopped.autostart.session, AutostartSession::Suppressed);
    drop(api);
    stop(second).await;

    // The same session again: the instance stays stopped.
    let third = start(&site, launchd(), &session).await;
    let api = site.api().await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let summary = api.get(id).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(summary.autostart.session, AutostartSession::Suppressed);
    drop(api);
    stop(third).await;
}
