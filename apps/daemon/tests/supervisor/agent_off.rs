//! The part of `daemon.shutdown` `agent_off` that stops the instances (spec
//! 0031 AC-20), first called in process and then over the socket: it stops what `stop_all::select` chose, exits only when the last
//! selection is empty, answers a repeated key the same, and keeps the daemon
//! up with the gate lifted when an instance cannot be stopped.

use std::sync::Arc;

use serde_json::json;
use stackly_domain::{InstanceId, Registry, WriteBatch, WriteOp};
use stackly_ipc::PeerCred;
use stackly_protocol::Call;
use stackly_protocol::daemon::{ShutdownMode, ShutdownParams};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::{InstanceDesired, InstanceLifecycle};
use stackly_testkit::{EnvironmentBuilder, LeaseBuilder};
use stacklyd::dispatch::RequestContext;
use stacklyd::ipc::agent_off::agent_off;

use crate::support::{Site, key};

fn context(key: &str) -> RequestContext {
    let mut context = RequestContext::new(
        1,
        "in-process".to_owned(),
        PeerCred::new(0, None),
        ClientInfo {
            kind: ClientKind::Cli,
            version: "0.1.0".to_owned(),
            pid: None,
            interactive: None,
        },
    );
    context.idempotency_key = Some(key.to_owned());
    context
}

/// The fingerprint of the `agent_off` request.
fn fingerprint() -> [u8; 32] {
    request_fingerprint(&Call::DaemonShutdown(ShutdownParams::new(
        ShutdownMode::AgentOff,
    )))
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_off_stops_the_instances_exits_and_answers_a_repeated_key_the_same() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    let b = api.create_ready("bravo", json!({})).await;
    api.start_ready(a).await;
    api.start_ready(b).await;
    let core = Arc::clone(daemon.core());

    let once = key();
    let outcome = agent_off(&core, &context(&once), "daemon.shutdown", fingerprint())
        .await
        .expect("agent off runs");

    assert!(outcome.exiting, "nothing is left to stop");
    assert!(
        core.shutdown_pending(),
        "the stop of the daemon was requested"
    );
    assert_eq!(outcome.stop_all.stopped.len(), 2);
    assert!(outcome.stop_all.failed.is_empty() && outcome.stop_all.skipped.is_empty());
    for id in [a, b] {
        let summary = api.get(id).await.expect("the instance reads");
        assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
        assert_eq!(summary.desired, InstanceDesired::Stopped);
    }
    let again = agent_off(&core, &context(&once), "daemon.shutdown", fingerprint())
        .await
        .expect("the replay answers");
    assert_eq!(again, outcome);
    let refused = agent_off(&core, &context(&key()), "daemon.shutdown", fingerprint())
        .await
        .expect_err("a new request after the stop was accepted is refused");
    assert_eq!(refused.code(), "daemon.shutting_down");
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_instance_an_environment_holds_keeps_the_daemon_up_and_lifts_the_gate() {
    let site = Site::new();
    let manual = site.start().await;
    let (held, plain) = {
        let api = site.api().await;
        (
            api.create_ready("held", json!({})).await,
            api.create_ready("plain", json!({})).await,
        )
    };
    manual
        .shutdown(ShutdownReason::Requested, std::future::pending())
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
    api.start_ready(plain).await;
    let core = Arc::clone(daemon.core());

    let outcome = agent_off(&core, &context(&key()), "daemon.shutdown", fingerprint())
        .await
        .expect("agent off runs");

    assert!(!outcome.exiting, "an instance is left that nothing stopped");
    assert!(!core.shutdown_pending());
    assert!(!core.agent_off_active(), "the claim is lifted");
    assert!(
        !core
            .operations()
            .engine()
            .expect("the engine runs")
            .is_quiesced(),
        "the gate is lifted"
    );
    assert_eq!(outcome.stop_all.stopped, vec![plain]);
    assert_eq!(outcome.stop_all.skipped.len(), 1);
    assert_eq!(outcome.stop_all.skipped[0].instance_id, held);
    let summary = api.get(held).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Running);
    // The daemon takes requests again: a start goes through.
    api.start_ready(plain).await;
    api.stop_when_free(plain).await;
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_planned_restart_in_progress_and_a_second_agent_off_are_refused_with_no_effect() {
    let site = Site::new();
    let daemon = site.start().await;
    let core = Arc::clone(daemon.core());
    let engine = core.operations().engine().expect("the engine runs");

    engine.set_quiesce(true);
    let pending = agent_off(&core, &context(&key()), "daemon.shutdown", fingerprint())
        .await
        .expect_err("a quiesce for a restart refuses it");
    assert_eq!(pending.code(), "daemon.restart_pending");
    engine.set_quiesce(false);
    assert!(!core.agent_off_active());

    assert!(core.begin_agent_off(), "another agent off runs");
    let second = agent_off(&core, &context(&key()), "daemon.shutdown", fingerprint())
        .await
        .expect_err("only one agent off runs at a time");
    assert_eq!(second.code(), "daemon.shutting_down");
    core.end_agent_off();
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_off_over_the_socket_needs_a_key_and_asks_the_daemon_to_exit() {
    use stackly_ipc::CallOptions;
    use stackly_protocol::Reply;

    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let a = api.create_ready("alpha", json!({})).await;
    api.start_ready(a).await;

    let keyless = api
        .client
        .call_detailed(
            Call::DaemonShutdown(ShutdownParams::new(ShutdownMode::AgentOff)),
            CallOptions::default(),
        )
        .await
        .expect_err("agent off needs an idempotency key");
    assert_eq!(keyless.code(), "operation.idempotency_key_required");
    assert!(
        !daemon.core().shutdown_pending(),
        "a refused request changes nothing"
    );

    let accepted = match api
        .client
        .call_detailed(
            Call::DaemonShutdown(ShutdownParams::new(ShutdownMode::AgentOff)),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("agent off is accepted")
    {
        Reply::DaemonShutdown(accepted) => accepted,
        other => panic!("not a shutdown reply: {other:?}"),
    };

    assert_eq!(accepted.mode, ShutdownMode::AgentOff);
    assert!(accepted.exiting);
    assert_eq!(accepted.stop_all.expect("the stop result").stopped, vec![a]);
    let mode = tokio::time::timeout(std::time::Duration::from_secs(5), daemon.stop_requested())
        .await
        .expect("the daemon asks for its stop order");
    assert_eq!(mode, ShutdownMode::Stop);
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}
