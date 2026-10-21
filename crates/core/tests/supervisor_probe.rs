//! The `tcp` and `http` probe runners against the real dummy server,
//! `stackly-fake-server` (spec 0010, step 6, AC-4): its `/ready` answers
//! 503 until its ready delay passes, then 200. A `native` probe runs over a
//! real loopback socket against a scripted NATS server (spec 0023, step 5).

use std::fs;
use std::net::{IpAddr, Ipv4Addr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use stackly_core::provider::probe::native::NativeProbe;
use stackly_core::provider::{NativeProbeSpec, ProbeSpec, ProbeTarget};
use stackly_core::secret::Redactor;
use stackly_core::supervisor::SupervisorConfig;
use stackly_core::supervisor::probe::{
    self, CONNECT_FAILED, ProbeContext, ProbeOutcome, TcpConnect, UNEXPECTED_STATUS,
};
use stackly_domain::Port;
use stackly_evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, TestRoot, allocate_port, fake_server};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const ATTEMPT: Duration = Duration::from_secs(2);
const START_LIMIT: Duration = Duration::from_secs(20);

/// A dummy server that is killed and reaped when the test ends.
struct Server {
    child: Child,
    port: Port,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts `serve` on a free loopback port and waits until it accepts
/// connections. A port another process takes first gets a fresh port.
fn serve(root: &TestRoot, name: &str, ready_delay_ms: u64) -> Server {
    let dir = root.path().join(name);
    fs::create_dir_all(&dir).expect("the server folder can be created");
    let config = dir.join("fake.json");
    fs::write(
        &config,
        r#"{"greeting":"hello","instanceName":"probe-test","authToken":"fixture-token"}"#,
    )
    .expect("the config can be written");
    for _ in 0..3 {
        let port = allocate_port().expect("a free loopback port");
        let mut server = Server {
            child: Command::new(fake_server::binary())
                .arg("serve")
                .arg(format!("--config={}", config.display()))
                .arg(format!("--data={}", dir.display()))
                .arg(format!("--port={port}"))
                .arg(format!("--ready-delay-ms={ready_delay_ms}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("the dummy server starts"),
            port: Port::new(port).expect("an allocated port is not 0"),
        };
        let started = Instant::now();
        while started.elapsed() < START_LIMIT {
            if TcpStream::connect((LOOPBACK, port)).is_ok() {
                return server;
            }
            if server
                .child
                .try_wait()
                .expect("the server can be polled")
                .is_some()
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        drop(server);
    }
    panic!("the dummy server did not listen on any of three ports");
}

fn ready_probe(port: Port) -> ProbeSpec {
    ProbeSpec::Http {
        host: LOOPBACK,
        port,
        path: "/ready".to_owned(),
        expect_status: 200..=299,
    }
}

fn tcp_probe(port: Port) -> ProbeSpec {
    ProbeSpec::Tcp {
        target: ProbeTarget::Ip {
            host: LOOPBACK,
            port,
        },
    }
}

async fn attempt(root: &TestRoot, probe: &ProbeSpec) -> ProbeOutcome {
    let platform = FakePlatform::new(root).platform();
    let redactor = Redactor::new();
    let config = SupervisorConfig::default();
    let context = ProbeContext {
        platform: &platform,
        redactor: &redactor,
        config: &config,
        native: &TcpConnect,
    };
    probe::run(probe, ATTEMPT, context).await
}

#[tokio::test]
async fn http_readiness_follows_the_dummy_servers_ready_endpoint() {
    let root = TestRoot::new().expect("a test root can be created");
    let waiting = serve(&root, "waiting", 600_000);
    let ready = serve(&root, "ready", 0);

    let not_ready = attempt(&root, &ready_probe(waiting.port)).await;
    let is_ready = attempt(&root, &ready_probe(ready.port)).await;

    assert_eq!(not_ready.reason(), &UNEXPECTED_STATUS);
    assert_eq!(
        not_ready.detail().map(|detail| detail.as_str()),
        Some("status 503")
    );
    assert_eq!(is_ready, ProbeOutcome::Ok);
}

#[tokio::test]
async fn tcp_readiness_is_ok_while_the_dummy_server_listens_and_fails_after_it_stops() {
    let root = TestRoot::new().expect("a test root can be created");
    let server = serve(&root, "tcp", 600_000);
    let port = server.port;

    let listening = attempt(&root, &tcp_probe(port)).await;
    drop(server);
    let stopped = attempt(&root, &tcp_probe(port)).await;

    // A listener that is not ready yet still accepts a TCP connect: that is
    // why readiness of a real provider never stops at an open port.
    assert_eq!(listening, ProbeOutcome::Ok);
    assert_eq!(stopped.reason(), &CONNECT_FAILED);
}

/// Serves one connection like a NATS server named `name`: INFO, then PONG to
/// the PING. Returns what the probe wrote.
async fn nats_server(listener: tokio::net::TcpListener, name: &'static str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _) = listener.accept().await.expect("the probe connects");
    let info = format!(
        "INFO {{\"server_id\":\"ID\",\"server_name\":\"{name}\",\"version\":\"2.15.1\",\"jetstream\":false}}\r\n"
    );
    stream
        .write_all(info.as_bytes())
        .await
        .expect("INFO can be written");
    let mut seen = Vec::new();
    let mut chunk = [0u8; 512];
    while !seen.ends_with(b"PING\r\n") {
        let count = stream
            .read(&mut chunk)
            .await
            .expect("the probe can be read");
        assert!(count > 0, "the probe closed the connection early");
        seen.extend_from_slice(&chunk[..count]);
    }
    stream
        .write_all(b"PONG\r\n")
        .await
        .expect("PONG can be written");
    String::from_utf8(seen).expect("the probe writes text")
}

fn nats_probe(port: Port, server_name: &str) -> ProbeSpec {
    ProbeSpec::Native(NativeProbeSpec::new(
        ProbeTarget::Ip {
            host: LOOPBACK,
            port,
        },
        NativeProbe::NatsReadiness {
            server_name: server_name.to_owned(),
            jetstream: false,
        },
    ))
}

#[tokio::test]
async fn a_native_probe_is_ready_over_a_real_socket_and_names_a_foreign_server() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "the nats-readiness probe runs over a real loopback socket: the server that is named as expected is ready, and another name is identity_mismatch with both names in the detail",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("a loopback port can be bound");
    let port = Port::new(
        listener
            .local_addr()
            .expect("the listener has an address")
            .port(),
    )
    .expect("a bound port is not 0");
    let server = tokio::spawn(nats_server(listener, "golden"));

    let ready = attempt(&root, &nats_probe(port, "golden")).await;

    assert_eq!(ready, ProbeOutcome::Ok);
    let written = server.await.expect("the server task ends");
    assert!(written.starts_with("CONNECT {"), "{written}");
    assert!(written.ends_with("}\r\nPING\r\n"), "{written}");
    assert!(
        !written.contains("user") && !written.contains("pass"),
        "{written}"
    );

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("a loopback port can be bound");
    let port = Port::new(
        listener
            .local_addr()
            .expect("the listener has an address")
            .port(),
    )
    .expect("a bound port is not 0");
    let server = tokio::spawn(nats_server(listener, "foreign"));

    let mismatch = attempt(&root, &nats_probe(port, "golden")).await;

    assert_eq!(mismatch.reason().as_str(), "identity_mismatch");
    assert_eq!(
        mismatch.detail().map(|detail| detail.as_str()),
        Some("server_name is \"foreign\", expected \"golden\"")
    );
    server.abort();
    ev.pass();
}
