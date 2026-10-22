//! What the instance methods answer by the state of the instance (spec
//! 0010 AC-23), the refusal of a second process on the same data (AC-6),
//! a port someone else holds (AC-17), and a secret that cannot be read
//! (spec 0011 AC-11).

use std::net::TcpListener;
use std::sync::Arc;

use serde_json::json;
use stackly_domain::SecretRef;
use stackly_protocol::codes::resource;
use stackly_protocol::instance::{InstanceBlocker, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::{Call, Reply, Revision};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::support::{Api, Site, key, serving};

async fn operation_count(api: &Api) -> usize {
    match api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
    {
        Ok(Reply::OperationList(page)) => page.items.len(),
        other => panic!("operation.list: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_method_answers_by_the_state_of_the_instance_and_a_no_op_creates_no_operation() {
    let ev = evidence!(
        covers = ["REQ-011"],
        kind = Kind::Fixture,
        expected = "start on a running instance, stop on a stopped one, a wrong expected revision, and an unknown instance are answered by the method with alreadyInState or an error, and none of them creates an operation",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("answers", json!({})).await;

    // Stop on a stopped instance with desired `stopped`: nothing to do.
    let before = operation_count(&api).await;
    let stopped = api.stop(id).await.expect("a stop on a stopped instance");
    assert!(stopped.accepted.is_none());
    assert!(stopped.already_in_state);
    assert_eq!(operation_count(&api).await, before);

    // A wrong expected revision and an unknown instance are refused.
    let stale = api
        .start_with_key(id, &key(), Some(Revision::saturating(9999)))
        .await
        .unwrap_err();
    assert_eq!(stale.code(), "resource.revision_mismatch", "{stale:?}");
    let unknown = api.start(Uuid::now_v7()).await.unwrap_err();
    assert!(unknown.is(&resource::NOT_FOUND), "{unknown:?}");
    assert_eq!(operation_count(&api).await, before);

    // Start on a running instance: nothing to do.
    api.start_ready(id).await;
    let before = operation_count(&api).await;
    let again = api.start(id).await.expect("a start on a running instance");
    assert!(again.accepted.is_none());
    assert!(again.already_in_state);
    assert_eq!(operation_count(&api).await, before);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_starts_from_two_clients_spawn_one_process_and_a_held_lock_blocks_a_start() {
    let ev = evidence!(
        covers = ["REQ-013"],
        kind = Kind::Fixture,
        expected = "RISK-03 (two servers on one data folder): a held instance.lock makes the start fail with supervisor.data_in_use and no spawn, and ten simultaneous starts from two clients leave exactly one dummy server process for the data folder",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let first = Arc::new(site.api().await);
    let second = Arc::new(site.api().await);
    let id = first.create_ready("one-process", json!({})).await;
    let paths = site
        .prepared
        .layout()
        .instance_paths(id.to_string().parse().expect("an instance id"));

    // Another process holds the lock: no spawn.
    let held = match site
        .platform
        .processes()
        .lock_instance(&paths.lock)
        .expect("the lock file can be opened")
    {
        stackly_platform::LockOutcome::Acquired(lock) => lock,
        stackly_platform::LockOutcome::Held => panic!("nobody holds the lock yet"),
    };
    let accepted = first
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let failed = first.wait_final(accepted.operation.id).await;
    assert_eq!(failed.operation.state, OperationState::Failed);
    assert_eq!(Api::failure_code(&failed), "supervisor.data_in_use");
    assert!(serving(&paths.data).is_empty(), "nothing was spawned");
    drop(held);

    // Ten starts at once.
    let mut starts = Vec::new();
    for index in 0..10 {
        let api = Arc::clone(if index % 2 == 0 { &first } else { &second });
        starts.push(tokio::spawn(async move { api.start(id).await }));
    }
    let mut operations = Vec::new();
    for start in starts {
        let reply = start
            .await
            .expect("a start task finishes")
            .expect("each start is answered");
        if let Some(accepted) = reply.accepted {
            operations.push(accepted.operation.id);
        }
    }
    for operation in operations {
        let detail = first.wait_final(operation).await;
        assert_eq!(
            detail.operation.state,
            OperationState::Succeeded,
            "{detail:?}"
        );
    }
    first
        .wait_instance(id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(serving(&paths.data).len(), 1, "exactly one spawn");
    drop((first, second));
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fixed_port_someone_else_uses_fails_the_start_before_a_spawn_and_a_clash_fails_the_create()
 {
    let ev = evidence!(
        covers = ["AC-010", "AC-055"],
        kind = Kind::Fixture,
        expected = "a create judged while an outside listener holds its Fixed port is refused with supervisor.endpoint_in_use, a Fixed port that passes the create and is taken before the start fails the start with supervisor.endpoint_in_use before any spawn and leaves the listener alone, and a second create with the same Fixed port is refused with supervisor.endpoint_conflict",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    // A port that is free when the create is judged: the outside program
    // takes it afterwards, before the start (spec 0020 AC-16). Judged while
    // it holds the port, the create itself is refused.
    let held = TcpListener::bind(("127.0.0.1", 0)).expect("a free port binds");
    let port = held
        .local_addr()
        .expect("the listener has an address")
        .port();
    let mut refused = api.params("held", json!({}));
    refused.endpoints = vec![Api::http_port(port)];
    let error = api.create(refused).await.unwrap_err();
    assert_eq!(error.code(), "supervisor.endpoint_in_use", "{error:?}");
    assert!(
        error
            .fields()
            .iter()
            .any(|field| field.path() == "endpoints[0].port"),
        "{error:?}"
    );
    drop(held);

    let mut params = api.params("fixed", json!({}));
    params.endpoints = vec![Api::http_port(port)];
    let created = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create begins an operation");
    api.wait_state(created.operation.id, OperationState::Succeeded)
        .await;
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("the freed port binds again");
    let id: Uuid = created
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");

    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let failed = api.wait_final(accepted.operation.id).await;
    assert_eq!(failed.operation.state, OperationState::Failed);
    assert_eq!(Api::failure_code(&failed), "supervisor.endpoint_in_use");
    let paths = site
        .prepared
        .layout()
        .instance_paths(id.to_string().parse().expect("an instance id"));
    assert!(serving(&paths.data).is_empty(), "nothing was spawned");
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
        "the outside listener is untouched"
    );
    let summary = api.get(id).await.expect("the instance can be read");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);

    // The same Fixed port again is a clash with the first instance.
    let mut clash = api.params("clash", json!({}));
    clash.endpoints = vec![Api::http_port(port)];
    let error = api.create(clash).await.unwrap_err();
    assert_eq!(error.code(), "supervisor.endpoint_conflict", "{error:?}");
    assert_eq!(api.list(Default::default()).await.items.len(), 1);
    drop((api, listener));
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_that_cannot_be_read_blocks_the_start_with_needs_secret_and_the_blocker_clears() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "a start whose secret has no value fails with secret.needs_secret, the instance stays stopped with a needsSecret blocker, and the blocker is gone after the value is back and the start succeeds",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("needs-secret", json!({})).await;
    let reference: SecretRef = format!("local:instance/{id}/auth-token")
        .parse()
        .expect("the reference the daemon made");
    assert!(site.secrets.contains(&reference), "create stored the token");
    site.secrets.remove(&reference);

    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let failed = api.wait_final(accepted.operation.id).await;
    assert_eq!(failed.operation.state, OperationState::Failed);
    assert_eq!(Api::failure_code(&failed), "secret.needs_secret");
    let error = &failed
        .operation
        .failure
        .as_ref()
        .expect("the failed operation carries its error")
        .error;
    assert_eq!(error.items().len(), 1, "{error:?}");
    assert_eq!(error.items()[0].kind(), "reference");
    assert_eq!(
        error.items()[0].field("reference"),
        Some(reference.to_string().as_str())
    );
    assert_eq!(error.items()[0].field("reason"), Some("missing"));
    assert_eq!(error.items()[0].field("generateAllowed"), Some("false"));
    assert_eq!(error.detail("count"), Some("1"));
    assert_eq!(error.detail("omitted"), None);
    assert!(
        error
            .hint()
            .is_some_and(|hint| hint.contains("stackly secret set")),
        "{error:?}"
    );
    let blocked = api
        .wait_instance(id, "a needsSecret blocker", |summary| {
            matches!(summary.blocker, Some(InstanceBlocker::NeedsSecret(_)))
        })
        .await;
    assert_eq!(blocked.lifecycle, InstanceLifecycle::Stopped);

    site.secrets.insert(&reference, "fixture-token-is-back");
    api.start_ready(id).await;
    let ready = api
        .wait_instance(id, "no blocker", |summary| summary.blocker.is_none())
        .await;
    assert_eq!(ready.lifecycle, InstanceLifecycle::Running);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}
