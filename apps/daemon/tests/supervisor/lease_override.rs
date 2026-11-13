//! The lease override on the actions other than a single stop (spec 0047
//! milestone 2, build step 6) through the real daemon: `overrideLeases` on
//! `instance.preview_stop_all`, `instance.stop_all`, `daemon.shutdown`
//! `agent_off`, and `instance.change_endpoints`. Without it an instance an
//! environment holds is left alone or refused; with it the action goes on
//! and the lease stays active.

use std::sync::Arc;

use serde_json::json;
use stackly_core::instance::stop_all::StopAllOptions;
use stackly_domain::{InstanceId, Registry, WriteBatch, WriteOp};
use stackly_ipc::{CallOptions, PeerCred};
use stackly_protocol::audit::{AuditPage, ListAuditParams};
use stackly_protocol::daemon::{ShutdownMode, ShutdownParams};
use stackly_protocol::env::EnvOwnership;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::config::{ChangeEndpointsParams, ChangeEndpointsReply};
use stackly_protocol::instance::{
    EndpointRequest, InstanceDesired, InstanceLifecycle, PortRequest,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::stop_all::{
    StopAllParams, StopAllPreview, StopAllPreviewParams, StopAllResult,
};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply};
use stackly_testkit::{EnvironmentBuilder, LeaseBuilder, allocate_port};
use stacklyd::dispatch::RequestContext;
use stacklyd::ipc::agent_off::agent_off_with;
use uuid::Uuid;

use crate::support::{Api, Site, key};

/// The name of the environment that holds the instance.
const SHOP: &str = "shop";

async fn preview(api: &Api, override_leases: bool) -> StopAllPreview {
    match api
        .client
        .call_detailed(
            Call::InstancePreviewStopAll(StopAllPreviewParams { override_leases }),
            CallOptions::default(),
        )
        .await
        .expect("the preview answers")
    {
        Reply::InstancePreviewStopAll(preview) => *preview,
        other => panic!("not a preview reply: {other:?}"),
    }
}

async fn stop_all(
    api: &Api,
    expected: Option<Vec<Uuid>>,
    override_leases: bool,
) -> Result<StopAllResult, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::InstanceStopAll(StopAllParams {
                expected_instance_ids: expected,
                override_leases,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::InstanceStopAll(result) => Ok(*result),
        other => panic!("not a stop_all reply: {other:?}"),
    }
}

async fn change_port(
    api: &Api,
    id: Uuid,
    base: u64,
    port: PortRequest,
    dry_run: bool,
    override_leases: bool,
) -> Result<ChangeEndpointsReply, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                instance_id: id,
                base_revision: ConfigRevisionNumber::new(base).expect("a revision number"),
                endpoints: vec![EndpointRequest {
                    name: "http".to_owned(),
                    port,
                }],
                dry_run,
                override_leases,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::InstanceChangeEndpoints(reply) => Ok(*reply),
        other => panic!("not a change endpoints reply: {other:?}"),
    }
}

async fn operation_count(api: &Api) -> usize {
    match api
        .client
        .call(Call::OperationList(Default::default()))
        .await
    {
        Ok(Reply::OperationList(page)) => page.items.len(),
        other => panic!("operation.list: {other:?}"),
    }
}

/// Every audit detail value named `key` of the rows of `instance`.
async fn audit_values(api: &Api, instance: Uuid, key: &str) -> Vec<String> {
    let page: AuditPage = match api
        .client
        .call(Call::AuditList(ListAuditParams {
            instance_id: Some(instance),
            ..ListAuditParams::default()
        }))
        .await
        .expect("audit.list answers")
    {
        Reply::AuditList(page) => page,
        other => panic!("not an audit.list reply: {other:?}"),
    };
    page.items
        .iter()
        .flat_map(|row| row.details.iter())
        .filter(|detail| detail.key == key)
        .map(|detail| detail.value.clone())
        .collect()
}

/// Stores the environment [`SHOP`] that shares `held`, and its active lease
/// on it, while no daemon runs; returns the environment ID.
fn hold(site: &Site, held: Uuid) -> Uuid {
    let registry = site.open_registry();
    let id = InstanceId::from_uuid(held).expect("an instance id");
    let environment = EnvironmentBuilder::new().name(SHOP).shared(id).build();
    let lease = LeaseBuilder::new(id).environment(environment.id()).build();
    registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertEnvironment(environment.clone()),
            WriteOp::InsertLease(lease),
        ]))
        .expect("the environment and its lease are stored");
    environment.id().as_uuid()
}

/// Creates `held` and `plain`, puts `held` in [`SHOP`] with an active
/// lease, starts the daemon again, and starts both.
async fn two_running(site: &Site) -> (stacklyd::server::DaemonHandle, Uuid, Uuid, Uuid) {
    let first = site.start().await;
    let (held, plain) = {
        let api = site.api().await;
        (
            api.create_ready("held", json!({})).await,
            api.create_ready("plain", json!({})).await,
        )
    };
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let environment = hold(site, held);
    let daemon = site.start().await;
    let api = site.api().await;
    api.start_ready(held).await;
    api.start_ready(plain).await;
    (daemon, held, plain, environment)
}

/// Whether the summary of `id` still shows the lease of [`SHOP`] active.
async fn lease_active(api: &Api, id: Uuid) -> bool {
    api.get(id)
        .await
        .expect("the instance reads")
        .environments
        .iter()
        .any(|entry| entry.name == SHOP && entry.lease_active)
}

// covers: AC-16 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn the_preview_and_stop_all_with_the_override_stop_an_instance_an_environment_holds() {
    let site = Site::new();
    let (daemon, held, plain, environment) = two_running(&site).await;
    let api = site.api().await;

    let without = preview(&api, false).await;
    let names: Vec<_> = without
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["plain"]);
    assert_eq!(without.skipped.len(), 1);
    assert_eq!(without.skipped[0].instance_id, held);
    assert_eq!(without.skipped[0].code, "supervisor.stop_requires_override");
    assert_eq!(without.skipped[0].environments.len(), 1);
    assert_eq!(
        without.skipped[0].environments[0].environment_id,
        environment
    );
    assert!(without.affected_environments.is_empty());

    let with = preview(&api, true).await;
    let names: Vec<_> = with.items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, ["held", "plain"]);
    assert!(with.skipped.is_empty());
    let entry = &with.items[0].environments;
    assert_eq!(entry.len(), 1);
    assert_eq!(entry[0].name, SHOP);
    assert_eq!(entry[0].ownership, EnvOwnership::Shared);
    assert!(entry[0].lease_active);
    assert!(with.items[1].environments.is_empty());
    assert_eq!(with.affected_environments.len(), 1);
    assert_eq!(with.affected_environments[0].environment_id, environment);
    assert_eq!(with.affected_environments[0].name, SHOP);
    assert_eq!(with.affected_environments[0].instance_count.get(), 1);

    // The set the override preview showed is not what a request without
    // the override would stop (spec 0032 is unchanged).
    let shown: Vec<Uuid> = with.items.iter().map(|item| item.instance_id).collect();
    let before = operation_count(&api).await;
    let changed = stop_all(&api, Some(shown.clone()), false)
        .await
        .expect_err("without the override the set differs");
    assert_eq!(changed.code(), "instance.stop_all_changed");
    assert_eq!(operation_count(&api).await, before, "nothing started");
    assert_eq!(
        api.get(held).await.expect("reads").lifecycle,
        InstanceLifecycle::Running
    );

    let result = stop_all(&api, Some(shown), true)
        .await
        .expect("stop all with the override runs");
    assert_eq!(result.stopped.len(), 2, "{result:?}");
    assert!(result.stopped.contains(&held) && result.stopped.contains(&plain));
    assert!(result.skipped.is_empty() && result.failed.is_empty());
    let summary = api.get(held).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(summary.desired, InstanceDesired::Stopped);
    assert!(
        lease_active(&api, held).await,
        "the override keeps the lease"
    );

    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

// covers: AC-16 (spec 0047): without the override Stop All keeps the rule
// 6 of spec 0031
#[tokio::test(flavor = "multi_thread")]
async fn stop_all_without_the_override_skips_an_instance_an_environment_holds() {
    let site = Site::new();
    let (daemon, held, plain, _) = two_running(&site).await;
    let api = site.api().await;

    let result = stop_all(&api, Some(vec![plain]), false)
        .await
        .expect("stop all runs");

    assert_eq!(result.stopped, vec![plain]);
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(result.skipped[0].instance_id, held);
    assert_eq!(result.skipped[0].code, "supervisor.stop_requires_override");
    assert_eq!(
        api.get(held).await.expect("reads").lifecycle,
        InstanceLifecycle::Running
    );
    assert!(lease_active(&api, held).await);

    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

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

// covers: AC-17 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn agent_off_with_the_override_stops_an_instance_an_environment_holds_and_exits() {
    let site = Site::new();
    let (daemon, held, plain, _) = two_running(&site).await;
    let api = site.api().await;
    let core = Arc::clone(daemon.core());
    let mut params = ShutdownParams::new(ShutdownMode::AgentOff);
    params.override_leases = true;
    let fingerprint = request_fingerprint(&Call::DaemonShutdown(params));

    let outcome = agent_off_with(
        &core,
        &context(&key()),
        "daemon.shutdown",
        fingerprint,
        StopAllOptions::new(true),
    )
    .await
    .expect("agent off runs");

    assert!(outcome.exiting, "nothing is left that runs");
    assert!(core.shutdown_pending());
    assert_eq!(outcome.stop_all.stopped.len(), 2, "{outcome:?}");
    assert!(outcome.stop_all.stopped.contains(&held));
    assert!(outcome.stop_all.stopped.contains(&plain));
    assert!(outcome.stop_all.skipped.is_empty());
    let summary = api.get(held).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert!(
        lease_active(&api, held).await,
        "the override keeps the lease"
    );
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

// covers: AC-17 (spec 0047): `overrideLeases` of `daemon.shutdown` reaches
// `agent_off` over the socket, and without it the daemon stays up
#[tokio::test(flavor = "multi_thread")]
async fn the_shutdown_params_carry_the_override_to_agent_off_over_the_socket() {
    let site = Site::new();
    let (daemon, held, _, _) = two_running(&site).await;
    let api = site.api().await;
    let shutdown = |override_leases: bool| {
        let mut params = ShutdownParams::new(ShutdownMode::AgentOff);
        params.override_leases = override_leases;
        Call::DaemonShutdown(params)
    };

    let staying = match api
        .client
        .call_detailed(
            shutdown(false),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("agent off answers")
    {
        Reply::DaemonShutdown(accepted) => accepted,
        other => panic!("not a shutdown reply: {other:?}"),
    };
    assert!(!staying.exiting, "the held instance keeps the daemon up");
    let skipped = staying.stop_all.expect("a stop all result").skipped;
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].instance_id, held);

    let exiting = match api
        .client
        .call_detailed(
            shutdown(true),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("agent off answers")
    {
        Reply::DaemonShutdown(accepted) => accepted,
        other => panic!("not a shutdown reply: {other:?}"),
    };
    assert!(exiting.exiting, "{exiting:?}");
    assert_eq!(
        exiting.stop_all.expect("a stop all result").stopped,
        vec![held]
    );
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

// covers: AC-15 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_port_change_of_an_instance_an_environment_holds_needs_the_override() {
    let site = Site::new();
    let (daemon, held, plain, environment) = two_running(&site).await;
    let api = site.api().await;
    api.stop_when_free(plain).await;
    let free = allocate_port().expect("a free port");

    let ChangeEndpointsReply::DryRun(dry) =
        change_port(&api, held, 1, PortRequest::Fixed(free), true, false)
            .await
            .expect("a dry run is never refused for a lease")
    else {
        panic!("not a dry run");
    };
    assert!(dry.valid);
    assert_eq!(dry.leases.len(), 1);
    assert_eq!(dry.leases[0].environment_id, environment);
    assert_eq!(dry.leases[0].name, SHOP);
    assert!(dry.leases[0].lease_active);

    let before = operation_count(&api).await;
    let refused = change_port(&api, held, 1, PortRequest::Fixed(free), false, false)
        .await
        .expect_err("environments hold the instance");
    assert_eq!(refused.code(), "instance.endpoints_require_override");
    assert_eq!(refused.detail("count"), Some("1"));
    assert!(refused.hint().is_some());
    let items = refused.items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind(), "environment");
    let field = |name: &str| {
        items[0]
            .fields()
            .iter()
            .find(|field| field.key() == name)
            .map(|field| field.value().to_owned())
    };
    assert_eq!(field("environmentId"), Some(environment.to_string()));
    assert_eq!(field("name").as_deref(), Some(SHOP));
    assert_eq!(field("ownership").as_deref(), Some("shared"));
    assert_eq!(operation_count(&api).await, before, "no operation started");

    // A request equal to the active one (the instance was created with an
    // automatic port) is answered `unchanged`, with no lease check.
    let ChangeEndpointsReply::Applied(same) =
        change_port(&api, held, 1, PortRequest::Auto, false, false)
            .await
            .expect("an equal request is not refused")
    else {
        panic!("not an applied reply");
    };
    assert!(same.accepted.is_none(), "{same:?}");

    let ChangeEndpointsReply::Applied(applied) =
        change_port(&api, held, 1, PortRequest::Fixed(free), false, true)
            .await
            .expect("the override lets the change go on")
    else {
        panic!("not an applied reply");
    };
    let accepted = applied.accepted.expect("a port change begins an operation");
    let detail = api.wait_final(accepted.operation.id).await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    api.wait_instance(held, "ready on the new port", |summary| {
        summary.endpoints[0].port == Some(free) && summary.lifecycle == InstanceLifecycle::Running
    })
    .await;
    assert!(
        lease_active(&api, held).await,
        "the override keeps the lease"
    );
    assert!(
        audit_values(&api, held, "overriddenEnvironments")
            .await
            .iter()
            .any(|value| value == SHOP),
        "the audit row names the environment"
    );

    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}
