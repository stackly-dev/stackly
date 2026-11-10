//! A start that cannot use a fixed port, through the real daemon (spec
//! 0033 AC-4, AC-5, AC-14): the failed start names the first free port
//! after it as `suggestedPort` with the static hint and its operation ID,
//! and neither the port, the config revision, the data, the lock, nor the
//! run folder changes because of the failure or the search.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;
use stackly_domain::{
    EndpointName, EndpointRequests, InstanceId, Port, PortRequest, Registry, WriteBatch, WriteOp,
};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_testkit::allocate_port;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::crash_loop::{error_details, started_operation, untouched};
use crate::support::{Api, PATIENCE, Site};

/// The static hint of `supervisor.endpoint_in_use` (spec 0033).
const IN_USE_HINT: &str = "Stackly never moves a fixed port on its own. Pick another port for this endpoint, or stop the process that holds it.";
/// The static hint of `supervisor.endpoint_conflict` (spec 0033).
const CONFLICT_HINT: &str =
    "Stackly never moves a fixed port on its own. Pick another port, or stop that instance.";

/// A fake instance whose `http` endpoint asks for the fixed `port`.
async fn create_fixed(api: &Api, name: &str, port: u16, extra: serde_json::Value) -> Uuid {
    let mut params = api.params(name, extra);
    params.endpoints = vec![Api::http_port(port)];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(detail.operation.state, OperationState::Succeeded);
    accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse().expect("an instance id"))
        .expect("the operation names the instance")
}

/// The suggested port of a failed start, checked to lie after `port`
/// within the 64 candidates the search takes.
fn suggested_after(detail: &OperationDetail, port: u16) -> u16 {
    let details: BTreeMap<String, String> = error_details(detail).into_iter().collect();
    let suggested: u16 = details
        .get("suggestedPort")
        .unwrap_or_else(|| panic!("a free port is suggested: {details:?}"))
        .parse()
        .expect("a port number");
    assert!(
        suggested > port && u32::from(suggested) <= u32::from(port) + 64,
        "{suggested} is not one of the candidates after {port}"
    );
    suggested
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fixed_port_another_process_holds_fails_the_start_with_a_free_port_and_changes_nothing() {
    let ev = evidence!(
        covers = ["AC-010", "AC-055", "NFR-011"],
        kind = Kind::Fixture,
        expected = "a start of a fake whose fixed port a listener of the test holds fails at the endpoints stage with supervisor.endpoint_in_use, the endpoint and port, a free suggestedPort after it, the static hint, and its operation ID; the instance stays stopped without a last failure, and its ports, config revision, registry revision, data, lock, and run folder are the same as before",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let port = allocate_port().expect("a free port");
    let id = create_fixed(&api, "held", port, json!({})).await;
    let holder = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("the test holds the port");
    let before = untouched(&site, &api, id).await;
    let revision = api.get(id).await.expect("the instance reads").revision;

    let operation = started_operation(&api, id).await;
    let detail = api.wait_final(operation).await;

    assert_eq!(detail.operation.state, OperationState::Failed);
    assert_eq!(detail.operation.stage.as_deref(), Some("endpoints"));
    assert_eq!(Api::failure_code(&detail), "supervisor.endpoint_in_use");
    let error = &detail.operation.failure.as_ref().expect("a failure").error;
    assert_eq!(error.operation_id(), Some(operation));
    assert_eq!(error.hint(), Some(IN_USE_HINT));
    let details: BTreeMap<String, String> = error_details(&detail).into_iter().collect();
    assert_eq!(details.get("endpoint").map(String::as_str), Some("http"));
    assert_eq!(details.get("port"), Some(&port.to_string()));
    let suggested = suggested_after(&detail, port);
    TcpListener::bind((Ipv4Addr::LOCALHOST, suggested))
        .expect("the suggested port was free when the search ran and still is");

    let after = api.get(id).await.expect("the instance reads");
    assert_eq!(after.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(after.last_failure, None, "a refusal before the journal");
    assert_eq!(after.revision, revision, "the registry row is not written");
    assert_eq!(after.endpoints[0].port, Some(port), "the fixed port stays");
    assert_eq!(untouched(&site, &api, id).await, before);
    drop(holder);
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fixed_port_another_instance_has_fails_with_a_conflict_and_a_free_port() {
    let ev = evidence!(
        covers = ["AC-010", "AC-055"],
        kind = Kind::Fixture,
        expected = "a start of an instance whose fixed port another instance also asks for fails with supervisor.endpoint_conflict naming that instance, a suggestedPort after it that no instance has, and the static hint, and the port of neither instance changes",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let port = allocate_port().expect("a free port");
    let other = allocate_port().expect("another free port");
    let holder = create_fixed(&api, "holder", port, json!({})).await;
    let id = create_fixed(&api, "second", other, json!({})).await;
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    // Create and the Ports sheet refuse a port another instance has, so the
    // conflict is written with no daemon running, as a registry from an
    // older build could hold it.
    {
        let registry = site.open_registry();
        let id = InstanceId::from_uuid(id).expect("an instance id the daemon made");
        let stored = registry
            .instance(id)
            .expect("the registry reads")
            .value
            .expect("the instance exists");
        let expected = stored.revision();
        let mut changed = stored.clone();
        let name: EndpointName = "http".parse().expect("an endpoint name");
        changed
            .set_port_requests(&EndpointRequests::new(BTreeMap::from([(
                name,
                PortRequest::Fixed(Port::new(port).expect("a valid port")),
            )])))
            .expect("one endpoint asks for the port");
        registry
            .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
                instance: changed,
                expected,
            }]))
            .expect("the port request is written");
    }

    let daemon = site.start().await;
    let api = site.api().await;
    let before = untouched(&site, &api, id).await;
    let operation = started_operation(&api, id).await;
    let detail = api.wait_final(operation).await;

    assert_eq!(Api::failure_code(&detail), "supervisor.endpoint_conflict");
    let error = &detail.operation.failure.as_ref().expect("a failure").error;
    assert_eq!(error.hint(), Some(CONFLICT_HINT));
    assert_eq!(error.operation_id(), Some(operation));
    let details: BTreeMap<String, String> = error_details(&detail).into_iter().collect();
    assert_eq!(details.get("otherInstance"), Some(&holder.to_string()));
    suggested_after(&detail, port);
    assert_eq!(untouched(&site, &api, id).await, before);
    let holder_after = api.get(holder).await.expect("the holder reads");
    assert_eq!(holder_after.endpoints[0].port, Some(port));
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_port_taken_after_the_spawn_fails_the_start_with_endpoint_in_use_and_a_free_port() {
    let ev = evidence!(
        covers = ["AC-010", "AC-055"],
        kind = Kind::Fixture,
        expected = "when a listener of the test takes the fixed port while the fake runs and before it is ready, the start fails with supervisor.endpoint_in_use and a suggestedPort, using no restart budget, and the instance ends failed with last failure endpoint_in_use, its time, and the exit code of the fake",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let port = allocate_port().expect("a free port");
    let id = create_fixed(
        &api,
        "raced",
        port,
        json!({ "crashAfterMs": 400, "crashExitCode": 3, "readyDelayMs": 60_000 }),
    )
    .await;
    let before = untouched(&site, &api, id).await;

    let operation = started_operation(&api, id).await;
    api.wait_instance(id, "a process runs", |summary| summary.process.is_some())
        .await;
    // From now on the test binds the port the moment it is free: before
    // the fake listens (the fake then cannot listen and exits) or right
    // after the fake crashed, before the daemon looks at the port.
    let done = Arc::new(AtomicBool::new(false));
    let taker = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            while !done.load(Ordering::SeqCst) && started.elapsed() < PATIENCE {
                if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
                    while !done.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    return Some(listener);
                }
            }
            None
        })
    };
    let detail = api.wait_final(operation).await;
    done.store(true, Ordering::SeqCst);
    let held = taker.join().expect("the taker thread ends");
    assert!(held.is_some(), "the test took the port");

    assert_eq!(Api::failure_code(&detail), "supervisor.endpoint_in_use");
    let error = &detail.operation.failure.as_ref().expect("a failure").error;
    assert_eq!(error.hint(), Some(IN_USE_HINT));
    assert_eq!(error.operation_id(), Some(operation));
    suggested_after(&detail, port);
    let failed = api.get(id).await.expect("the instance reads");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed);
    assert_eq!(failed.restart.attempts, 0, "a taken port uses no budget");
    let failure = failed.last_failure.clone().expect("the instance says why");
    assert_eq!(failure.code, "endpoint_in_use");
    assert!(failure.at.is_some());
    assert!(
        failure.exit_code.is_some(),
        "the exit of the fake led to the check"
    );
    assert_eq!(failed.endpoints[0].port, Some(port), "the fixed port stays");
    assert_eq!(untouched(&site, &api, id).await, before);
    drop(held);
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}
