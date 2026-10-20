//! The path of one fake instance: created, started until ready, read in its
//! separate states, and stopped (spec 0010 AC-3, AC-4, AC-5, AC-13, AC-16).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_protocol::instance::{
    EndpointPortMode, InstanceDesired, InstanceHealthState, InstanceLifecycle,
    InstanceReadinessState,
};
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::{Site, TOKEN};

/// One `GET` of the dummy server; returns the status code.
fn http_status(port: u16, path: &str) -> u16 {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the dummy server accepts");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout is set");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is written");
    let mut text = String::new();
    stream.read_to_string(&mut text).expect("the reply is read");
    text.split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status line")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fake_instance_is_created_started_until_ready_and_stopped() {
    let ev = evidence!(
        covers = ["AC-012", "REQ-010", "REQ-011", "BR-010", "NFR-006"],
        kind = Kind::Fixture,
        expected = "instance.create, start, and stop take the fake instance through its lifecycle with process, readiness, health, and desired state apart, and instance.start answers within 500 ms",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;

    // Create: the operation ends `succeeded`, the instance is `stopped`,
    // initialized, and its token digest is the one of the token.
    let id = api
        .create_ready("orders", json!({ "greeting": "hello from the test" }))
        .await;
    let operations = api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("the operations can be listed");
    let Reply::OperationList(page) = operations else {
        panic!("not an operation list");
    };
    let create = api.operation(page.items[0].id).await;
    let create_stages: Vec<&str> = create.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(
        create_stages,
        ["validate", "secrets", "layout", "register", "init"]
    );
    let created = api.get(id).await.expect("the created instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.desired, InstanceDesired::Stopped);
    assert_eq!(created.initialized, Some(true));
    assert_eq!(created.provider.name, "testing/fake");
    assert_eq!(created.endpoints.len(), 1);
    assert_eq!(created.endpoints[0].port_mode, EndpointPortMode::Auto);
    assert!(!created.endpoints[0].bound);
    let paths = site.prepared.layout().instance_paths(
        id.to_string()
            .parse()
            .expect("an instance id the daemon made"),
    );
    let digest = std::fs::read_to_string(paths.data.join("token.sha256")).expect("init wrote it");
    assert_eq!(digest.trim(), hex(&Sha256::digest(TOKEN.as_bytes())));

    // Start: the reply is quick, the operation waits for readiness.
    let asked = std::time::Instant::now();
    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start from stopped begins an operation");
    assert!(
        asked.elapsed() < Duration::from_millis(500),
        "instance.start answered in {:?}",
        asked.elapsed()
    );
    let finished = api
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(finished.operation.state, OperationState::Succeeded);
    // The stages of the startup transaction, each a step of the operation.
    let stages: Vec<&str> = finished
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect();
    assert_eq!(
        stages,
        [
            "validate",
            "lock",
            "verify",
            "endpoints",
            "journal",
            "render",
            "spawn",
            "identity",
            "readiness",
            "publish",
            "complete"
        ]
    );
    let running = api
        .wait_instance(id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;

    // Four separate states.
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.desired, InstanceDesired::Running);
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    assert!(
        running.health.state == InstanceHealthState::Healthy
            || running.health.state == InstanceHealthState::Unknown,
        "{:?}",
        running.health
    );
    let process = running.process.expect("a running instance has a process");
    assert!(!process.adopted);
    let endpoint = &running.endpoints[0];
    assert!(endpoint.bound, "a ready instance shows its port as final");
    let port = endpoint.port.expect("the bound port is shown");
    assert_eq!(http_status(port, "/ready"), 200);

    // Stop: the strategy of the provider, then `stopped`.
    let accepted = api
        .stop(id)
        .await
        .expect("the stop is accepted")
        .accepted
        .expect("a stop of a running instance begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let stopped = api
        .wait_instance(id, "stopped", |summary| {
            summary.lifecycle == InstanceLifecycle::Stopped
        })
        .await;
    assert_eq!(stopped.desired, InstanceDesired::Stopped);
    assert!(stopped.process.is_none());
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "nothing listens after the stop"
    );
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
