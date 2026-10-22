//! A `native` probe in the supervisor loops (spec 0023 AC-6): readiness
//! before ready, the routine readiness check with its debounce, and health,
//! on `FakeProcessSupervision` with a scripted NATS server over
//! `tokio::io::duplex` and a paused clock.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_core::supervisor::probe::{Connecting, NativeConnect, NativeStream};
use stackly_domain::{HealthState, InstanceLifecycle, OperationState, ReadinessState};
use stackly_platform::ChildRole;
use stackly_testkit::evidence::{Kind, evidence};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

use crate::common::{Rig, Spec, replace};

/// How the scripted server answers the next connection.
#[derive(Debug, Clone, Copy, Default)]
enum Mode {
    /// Like `nats-server` named `fake`, JetStream off.
    #[default]
    Ready,
    /// The connect is refused.
    Refuse,
    /// The connect works and the server never says a word.
    Silent,
}

#[derive(Debug, Default)]
struct Scripted {
    mode: Mutex<Mode>,
    attempts: AtomicUsize,
}

impl Scripted {
    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap_or_else(PoisonError::into_inner) = mode;
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }
}

impl NativeConnect for Scripted {
    fn connect(&self, _address: std::net::SocketAddr) -> Connecting<'_> {
        let mode = *self.mode.lock().unwrap_or_else(PoisonError::into_inner);
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let (client, server) = duplex(4096);
            match mode {
                Mode::Refuse => return Err(io::Error::from(io::ErrorKind::ConnectionRefused)),
                Mode::Ready => {
                    tokio::spawn(serve(server));
                }
                Mode::Silent => {
                    tokio::spawn(async move {
                        let _held = server;
                        std::future::pending::<()>().await;
                    });
                }
            }
            Ok(Box::new(client) as Box<dyn NativeStream>)
        })
    }
}

/// INFO, then PONG to the PING.
async fn serve(mut server: DuplexStream) {
    let info = "INFO {\"server_id\":\"ID\",\"server_name\":\"fake\",\"version\":\"2.15.1\",\"jetstream\":false}\r\n";
    if server.write_all(info.as_bytes()).await.is_err() {
        return;
    }
    let mut seen = Vec::new();
    let mut chunk = [0u8; 512];
    while !seen.ends_with(b"PING\r\n") {
        match server.read(&mut chunk).await {
            Ok(count) if count > 0 => seen.extend_from_slice(&chunk[..count]),
            _ => return,
        }
    }
    let _ = server.write_all(b"PONG\r\n").await;
    let mut rest = Vec::new();
    let _ = server.read_to_end(&mut rest).await;
}

/// The fake provider with `nats-readiness` as readiness and health probe,
/// expecting the server name `fake` (the default `label`) and JetStream off,
/// and a health threshold of 2.
fn native_probes(text: &str) -> String {
    let probe = r#"{ type: native, name: nats-readiness, endpoint: http, params: { serverName: "${settings.label}", jetstream: false } }"#;
    let text = replace(text, "{ type: http, endpoint: http, path: /ready }", probe);
    let text = replace(
        &text,
        "{ type: http, endpoint: http, path: /health }",
        probe,
    );
    replace(
        &text,
        "intervalSeconds: 5",
        "intervalSeconds: 1\n  failureThreshold: 2",
    )
}

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(start_paused = true)]
async fn a_native_probe_makes_the_instance_ready_and_healthy_with_reason_probe_ok() {
    let ev = evidence!(
        covers = ["REQ-029", "REQ-012"],
        kind = Kind::Fixture,
        expected = "a native probe over a scripted duplex server makes startup reach ready and health healthy, both with reason probe_ok",
    );
    let server = Arc::new(Scripted::default());
    let rig =
        Rig::with_connector(Arc::clone(&server) as Arc<dyn NativeConnect>, native_probes).await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let _spawn = rig.main_spawn(1).await;
    rig.wait_state(op, OperationState::Succeeded).await;
    wait_for("health is healthy", || {
        rig.sup.status(id).health.state() == HealthState::Healthy
    })
    .await;

    let status = rig.sup.status(id);
    assert_eq!(status.readiness.state(), ReadinessState::Ready);
    assert_eq!(status.readiness.reason().as_str(), "probe_ok");
    assert_eq!(status.health.reason().as_str(), "probe_ok");
    assert!(server.attempts() >= 2, "readiness and health both probed");
    ev.pass();
}

#[tokio::test(start_paused = true)]
async fn a_silent_server_turns_readiness_not_ready_with_timeout_and_never_touches_the_process() {
    let ev = evidence!(
        covers = ["REQ-029", "REQ-012"],
        kind = Kind::Fixture,
        expected = "a server that accepts the connection and never answers turns routine readiness not_ready with reason timeout after the debounce, leaves the lifecycle running with no signal and no second spawn, and readiness recovers on the same process",
    );
    let server = Arc::new(Scripted::default());
    let rig =
        Rig::with_connector(Arc::clone(&server) as Arc<dyn NativeConnect>, native_probes).await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    server.set(Mode::Silent);
    wait_for("readiness falls back", || {
        rig.sup.status(id).readiness.state() == ReadinessState::NotReady
    })
    .await;

    let status = rig.sup.status(id);
    assert_eq!(status.readiness.reason().as_str(), "timeout");
    assert_eq!(
        status.readiness.detail().map(|detail| detail.as_str()),
        Some("no INFO from the server within 1000 ms")
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert!(rig.processes().signals(spawn.pid).is_empty());
    let mains = rig
        .processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count();
    assert_eq!(mains, 1, "a bad probe never restarts");

    server.set(Mode::Ready);
    wait_for("readiness recovers", || {
        rig.sup.status(id).readiness.state() == ReadinessState::Ready
    })
    .await;
    assert_eq!(
        rig.sup.status(id).process.map(|process| process.pid),
        Some(spawn.pid)
    );
    ev.pass();
}

#[tokio::test(start_paused = true)]
async fn native_health_degrades_after_one_failure_and_turns_unhealthy_at_the_threshold() {
    let ev = evidence!(
        covers = ["REQ-029", "REQ-012"],
        kind = Kind::Fixture,
        expected = "a refused connect makes native health degraded after one failure, then unhealthy at the threshold of 2, with reason connect_failed, and healthy again with reason probe_ok when the server returns",
    );
    let server = Arc::new(Scripted::default());
    let rig =
        Rig::with_connector(Arc::clone(&server) as Arc<dyn NativeConnect>, native_probes).await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.wait_state(op, OperationState::Succeeded).await;
    wait_for("health is healthy", || {
        rig.sup.status(id).health.state() == HealthState::Healthy
    })
    .await;

    server.set(Mode::Refuse);
    wait_for("health degrades", || {
        rig.sup.status(id).health.state() == HealthState::Degraded
    })
    .await;
    assert_eq!(
        rig.sup.status(id).health.reason().as_str(),
        "connect_failed"
    );
    wait_for("health turns unhealthy", || {
        rig.sup.status(id).health.state() == HealthState::Unhealthy
    })
    .await;
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert!(rig.processes().signals(spawn.pid).is_empty());

    server.set(Mode::Ready);
    wait_for("health recovers", || {
        rig.sup.status(id).health.state() == HealthState::Healthy
    })
    .await;
    assert_eq!(rig.sup.status(id).health.reason().as_str(), "probe_ok");
    ev.pass();
}
