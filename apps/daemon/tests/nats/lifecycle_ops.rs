//! Reload refused and restart of a real NATS through the contract methods of
//! spec 0026 (AC-10, AC-11, AC-6): the provider declares no reload, so
//! `instance.reload` is refused before any operation, and a restart runs a
//! new `nats-server` that is ready again.

use serde_json::json;
use stackly_protocol::instance::{InstanceLastExit, InstanceReadinessState};
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{Serve, Site, alive, create_instance, stop_instance};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_refuses_reload_without_an_operation_and_restarts_with_a_new_process() {
    let ev = evidence!(
        covers = ["BR-016", "AC-026", "REQ-011"],
        kind = Kind::Native,
        expected = "a running NATS instance reads capabilities.reload false, instance.reload answers supervisor.capability_unsupported and operation.list gains nothing, and instance.restart runs a new nats-server that is ready with lastExit clean while the old one is gone",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let id = create_instance(&site, &api, "lifecycle-ops", json!({})).await;
    let running = api.start_ready(id).await;
    assert!(!running.capabilities.reload);
    let first = running.process.as_ref().expect("a process").pid;
    let count = async || match api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
    {
        Ok(Reply::OperationList(page)) => page.items.len(),
        other => panic!("operation.list: {other:?}"),
    };
    let before = count().await;

    let refused = api.reload(id).await.unwrap_err();
    assert_eq!(
        refused.code(),
        "supervisor.capability_unsupported",
        "{refused:?}"
    );
    assert_eq!(count().await, before, "a refused reload makes no operation");
    assert_eq!(
        api.get(id)
            .await
            .expect("instance.get")
            .process
            .map(|p| p.pid),
        Some(first)
    );

    let restart = api
        .restart(id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("an operation");
    api.wait_state(restart.operation.id, OperationState::Succeeded)
        .await;
    let restarted = api
        .wait_instance(id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    let second = restarted.process.as_ref().expect("a process").pid;
    assert_ne!(second, first);
    assert!(!alive(first), "the old nats-server is gone");
    assert_eq!(restarted.last_exit, Some(InstanceLastExit::Clean));

    stop_instance(&api, id).await;
    daemon.shutdown().await;
    ev.pass_with(&format!("reload refused, restart {first} -> {second}"));
}
