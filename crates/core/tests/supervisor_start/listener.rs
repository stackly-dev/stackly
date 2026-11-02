//! A fixed port that a listener outside Stackly holds when a respawn or a
//! start wants it (spec 0030 AC-6): nothing spawns, nothing signals the
//! listener, and the port is never replaced by another one.

use std::net::TcpListener;
use std::time::Duration;

use stackly_core::supervisor::{RegistryMode, codes};
use stackly_domain::{DesiredState, Endpoint, InstanceLifecycle, OperationState};
use stackly_testkit::ProcessOp;

use crate::common::{LOOPBACK, Plant, Rig, Spec};

fn fixed_port(rig: &Rig, id: stackly_domain::InstanceId) -> Option<u16> {
    rig.instance_row(id)
        .endpoints()
        .iter()
        .find_map(|endpoint| match endpoint {
            Endpoint::Ip(ip) => ip.effective_port().map(stackly_domain::Port::get),
            Endpoint::Unix(_) => None,
        })
}

#[tokio::test]
async fn a_foreign_listener_fails_the_respawn_with_endpoint_in_use_and_keeps_the_port() {
    // covers: spec 0030 AC-6 (respawn after reconciliation)
    let rig = Rig::new().await;
    let port = Rig::free_port();
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Running,
        desired: DesiredState::Running,
        request: Some(port),
        ..Spec::default()
    });
    let identity = rig.plant(id, Plant::live());
    rig.processes().exit(identity.pid, None);
    let _listener = TcpListener::bind((LOOPBACK, port)).expect("the foreign listener binds");

    rig.sup.reconcile(RegistryMode::Ready).await.unwrap();
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Recovering
    );
    rig.sup.start_reconciled();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while rig.instance_row(id).lifecycle() != InstanceLifecycle::Failed {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the respawn never failed"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let row = rig.instance_row(id);
    assert_eq!(
        row.last_failure().map(|code| code.as_str()),
        Some("endpoint_in_use")
    );
    assert!(rig.processes().spawns().is_empty(), "nothing spawned");
    assert_eq!(rig.processes().count(ProcessOp::Signal), 0);
    assert_eq!(fixed_port(&rig, id), Some(port), "no other port is chosen");
}

#[tokio::test]
async fn a_foreign_listener_refuses_a_start_with_endpoint_in_use_and_keeps_the_instance_stopped() {
    // covers: spec 0030 AC-6 (a start of a stopped instance)
    let rig = Rig::new().await;
    let port = Rig::free_port();
    let id = rig.instance(&Spec {
        desired: DesiredState::Running,
        request: Some(port),
        ..Spec::default()
    });
    let _listener = TcpListener::bind((LOOPBACK, port)).expect("the foreign listener binds");

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::ENDPOINT_IN_USE.to_string()
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert!(rig.processes().spawns().is_empty(), "nothing spawned");
    assert_eq!(rig.processes().count(ProcessOp::Signal), 0);
    assert_eq!(fixed_port(&rig, id), Some(port), "no other port is chosen");
}
