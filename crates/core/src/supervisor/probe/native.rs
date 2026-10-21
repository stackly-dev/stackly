//! The runner of native probes (spec 0023, *Hasil probe dan loop
//! supervisor*): one attempt of a probe from the native registry.
//!
//! The runner owns the IO that the state machines in
//! `provider::probe` leave out. It opens one connection through a
//! [`NativeConnect`] (real TCP in production, an in memory stream in
//! tests), moves bytes between the stream and the state machine, and holds
//! one deadline, `now + attempt_timeout`, for the whole attempt: connect,
//! every read, and every write share what is left of it. Nothing runs in
//! the background, and the connection closes when the attempt returns.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout_at};

use super::{ProbeContext, ProbeOutcome};
use crate::provider::probe::NativeProbeFailure;
use crate::provider::probe::native::NativeProbe;
use crate::provider::probe::nats::{self, Exchange, Expectation};
use crate::provider::{NativeProbeSpec, ProbeTarget};
use crate::secret::Redactor;

/// The size of one read from the server.
const READ_CHUNK: usize = 8 * 1024;

/// A byte stream a native probe talks over.
pub trait NativeStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> NativeStream for T {}

/// The future [`NativeConnect::connect`] returns.
pub type Connecting<'a> =
    Pin<Box<dyn Future<Output = io::Result<Box<dyn NativeStream>>> + Send + 'a>>;

/// How a native probe opens its connection. The supervisor uses
/// [`TcpConnect`]; tests replace it with an in memory stream so the clock
/// can stay paused.
pub trait NativeConnect: std::fmt::Debug + Send + Sync + 'static {
    /// Opens a connection to `address`. The runner bounds the call with the
    /// attempt deadline.
    fn connect(&self, address: SocketAddr) -> Connecting<'_>;
}

/// Connects over TCP.
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpConnect;

impl NativeConnect for TcpConnect {
    fn connect(&self, address: SocketAddr) -> Connecting<'_> {
        Box::pin(async move {
            let stream = TcpStream::connect(address).await?;
            Ok(Box::new(stream) as Box<dyn NativeStream>)
        })
    }
}

/// Runs one attempt of `spec` within `attempt_timeout`.
pub(super) async fn run(
    spec: &NativeProbeSpec,
    attempt_timeout: Duration,
    context: &ProbeContext<'_>,
) -> ProbeOutcome {
    let result = attempt(spec, attempt_timeout, |address| {
        context.native.connect(address)
    })
    .await;
    outcome(result, context.redactor)
}

/// Turns a result into the outcome every runner reports: the detail is
/// redacted, cut to 256 characters, then made into an `ObservationDetail`.
fn outcome(result: Result<(), NativeProbeFailure>, redactor: &Redactor) -> ProbeOutcome {
    match result {
        Ok(()) => ProbeOutcome::Ok,
        Err(failure) => ProbeOutcome::failure(failure.reason, &failure.detail, redactor),
    }
}

/// Picks the state machine of the probe, the one `match` the registry
/// needs here, and runs it over the stream `connect` opens.
async fn attempt<S, C, F>(
    spec: &NativeProbeSpec,
    limit: Duration,
    connect: C,
) -> Result<(), NativeProbeFailure>
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: FnOnce(SocketAddr) -> F,
    F: Future<Output = io::Result<S>>,
{
    match &spec.probe {
        NativeProbe::NatsReadiness {
            server_name,
            jetstream,
        } => {
            let expectation = Expectation {
                server_name: server_name.clone(),
                jetstream: *jetstream,
            };
            nats_readiness(&spec.target, expectation, limit, connect).await
        }
    }
}

/// One `nats-readiness` attempt: connect, then feed the bytes the server
/// sends to the [`Exchange`] and write what it asks for.
async fn nats_readiness<S, C, F>(
    target: &ProbeTarget,
    expectation: Expectation,
    limit: Duration,
    connect: C,
) -> Result<(), NativeProbeFailure>
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: FnOnce(SocketAddr) -> F,
    F: Future<Output = io::Result<S>>,
{
    let ProbeTarget::Ip { host, port } = target else {
        return Err(nats::unix_target());
    };
    let address = SocketAddr::new(*host, port.get());
    let deadline = Instant::now() + limit;
    let mut stream = match timeout_at(deadline, connect(address)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            return Err(nats::connect_failed(
                &address.to_string(),
                &error.kind().to_string(),
            ));
        }
        Err(_) => return Err(nats::connect_timed_out(&address.to_string(), limit)),
    };
    let mut token = [0u8; 16];
    getrandom::fill(&mut token).map_err(|_| {
        NativeProbeFailure::new(nats::PROTOCOL_ERROR, "no random bytes for the inbox")
    })?;
    let mut exchange = Exchange::new(expectation, token);
    let mut buffer = [0u8; READ_CHUNK];
    loop {
        let read = match timeout_at(deadline, stream.read(&mut buffer)).await {
            Err(_) => return Err(exchange.timed_out(limit)),
            Ok(Err(_)) => return Err(exchange.reset()),
            Ok(Ok(0)) => return Err(exchange.closed()),
            Ok(Ok(count)) => count,
        };
        let step = exchange.feed(&buffer[..read]);
        if let Some(done) = step.done {
            return done;
        }
        if !step.write.is_empty() {
            match timeout_at(deadline, stream.write_all(&step.write)).await {
                Err(_) => return Err(exchange.timed_out(limit)),
                Ok(Err(_)) => return Err(exchange.reset()),
                Ok(Ok(())) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use stackly_domain::Port;
    use tokio::io::{DuplexStream, ReadBuf, duplex};

    use super::*;
    use crate::supervisor::probe::{CONNECT_FAILED, PROTOCOL_ERROR, TIMEOUT};

    const LIMIT: Duration = Duration::from_secs(2);
    const ADDRESS: &str = "127.0.0.1:4222";
    const ACCOUNT_INFO: &str =
        r#"{"type":"io.nats.jetstream.api.v1.account_info_response","memory":0,"storage":0}"#;

    fn spec(jetstream: bool) -> NativeProbeSpec {
        NativeProbeSpec::new(
            ProbeTarget::Ip {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: Port::new(4222).expect("a test port is not 0"),
            },
            NativeProbe::NatsReadiness {
                server_name: "golden".to_owned(),
                jetstream,
            },
        )
    }

    fn info(name: &str, jetstream: bool) -> String {
        format!(
            "INFO {{\"server_id\":\"ID\",\"server_name\":\"{name}\",\"version\":\"2.15.1\",\"jetstream\":{jetstream}}}\r\n"
        )
    }

    /// Reads from `server` until what it has read holds `needle`.
    async fn read_until(server: &mut DuplexStream, needle: &str) -> String {
        let mut seen = Vec::new();
        let mut chunk = [0u8; 512];
        while !String::from_utf8_lossy(&seen).contains(needle) {
            let count = server
                .read(&mut chunk)
                .await
                .expect("the probe can be read");
            assert!(count > 0, "the probe closed the connection early");
            seen.extend_from_slice(&chunk[..count]);
        }
        String::from_utf8(seen).expect("the probe writes text")
    }

    /// A server that answers like `nats-server`: INFO first, PONG to the
    /// PING, and, when `reply` is set, that payload for `$JS.API.INFO`.
    /// `delay` is how long it waits before the INFO.
    async fn serve(mut server: DuplexStream, delay: Duration, reply: Option<&'static str>) {
        tokio::time::sleep(delay).await;
        server
            .write_all(info("golden", reply.is_some()).as_bytes())
            .await
            .expect("INFO can be written");
        let got = read_until(&mut server, "PING\r\n").await;
        assert_eq!(got.as_bytes(), nats::connect_bytes());
        server
            .write_all(b"PONG\r\n")
            .await
            .expect("PONG can be written");
        if let Some(payload) = reply {
            let got = read_until(&mut server, "\r\n\r\n").await;
            let inbox = got
                .strip_prefix("SUB ")
                .and_then(|rest| rest.split_once(" 1\r\n"))
                .map(|(inbox, _)| inbox.to_owned())
                .expect("the probe subscribes first");
            assert_eq!(
                got,
                format!("SUB {inbox} 1\r\nPUB $JS.API.INFO {inbox} 0\r\n\r\n")
            );
            let message = format!("MSG {inbox} 1 {}\r\n{payload}\r\n", payload.len());
            server
                .write_all(message.as_bytes())
                .await
                .expect("the reply can be written");
        }
        let mut rest = Vec::new();
        let _ = server.read_to_end(&mut rest).await;
    }

    /// A server that sends `script` at once, then reads and says nothing.
    async fn script_then_silence(mut server: DuplexStream, script: &'static str) {
        if !script.is_empty() {
            server
                .write_all(script.as_bytes())
                .await
                .expect("the script can be written");
        }
        std::future::pending::<()>().await;
    }

    /// Runs one attempt over the client end of a duplex pair and returns
    /// the result with the time it took.
    async fn over(
        spec: &NativeProbeSpec,
        client: DuplexStream,
    ) -> (Result<(), NativeProbeFailure>, Duration) {
        let start = Instant::now();
        let result = attempt(spec, LIMIT, |_| async move { Ok(client) }).await;
        (result, start.elapsed())
    }

    fn failure(result: Result<(), NativeProbeFailure>) -> (String, String) {
        let failure = result.expect_err("the attempt fails");
        (failure.reason.as_str().to_owned(), failure.detail)
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_that_answers_ping_is_ready_without_jetstream() {
        let (client, server) = duplex(4096);
        let served = tokio::spawn(serve(server, Duration::ZERO, None));

        let (result, took) = over(&spec(false), client).await;

        assert_eq!(result, Ok(()));
        assert_eq!(took, Duration::ZERO);
        served.await.expect("the server task ends");
    }

    #[tokio::test(start_paused = true)]
    async fn the_jetstream_api_reply_makes_a_jetstream_instance_ready() {
        let (client, server) = duplex(4096);
        let served = tokio::spawn(serve(server, Duration::ZERO, Some(ACCOUNT_INFO)));

        let (result, _) = over(&spec(true), client).await;

        assert_eq!(result, Ok(()));
        served.await.expect("the server task ends");
    }

    #[tokio::test(start_paused = true)]
    async fn a_jetstream_error_reply_is_jetstream_unavailable() {
        let (client, server) = duplex(4096);
        let served = tokio::spawn(serve(
            server,
            Duration::ZERO,
            Some(
                r#"{"error":{"code":503,"description":"JetStream system temporarily unavailable"}}"#,
            ),
        ));

        let (result, _) = over(&spec(true), client).await;

        assert_eq!(
            failure(result),
            (
                "jetstream_unavailable".to_owned(),
                "$JS.API.INFO error 503: JetStream system temporarily unavailable".to_owned()
            )
        );
        served.await.expect("the server task ends");
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_connect_is_connect_failed_with_the_address_and_the_error_kind() {
        let result = attempt(&spec(false), LIMIT, |_| async {
            Err::<DuplexStream, _>(io::Error::from(io::ErrorKind::ConnectionRefused))
        })
        .await;

        assert_eq!(
            failure(result),
            (
                CONNECT_FAILED.as_str().to_owned(),
                format!("connect to {ADDRESS} failed: connection refused")
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_connect_that_never_finishes_times_out_exactly_at_the_deadline() {
        let start = Instant::now();

        let result = attempt(&spec(false), LIMIT, |_| async {
            std::future::pending::<io::Result<DuplexStream>>().await
        })
        .await;

        assert_eq!(start.elapsed(), LIMIT);
        assert_eq!(
            failure(result),
            (
                TIMEOUT.as_str().to_owned(),
                format!("connect to {ADDRESS} timed out after 2000 ms")
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_that_sends_no_info_times_out_with_the_info_text() {
        let (client, server) = duplex(4096);
        tokio::spawn(script_then_silence(server, ""));

        let (result, took) = over(&spec(false), client).await;

        assert_eq!(took, LIMIT);
        assert_eq!(
            failure(result),
            (
                TIMEOUT.as_str().to_owned(),
                "no INFO from the server within 2000 ms".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_that_never_answers_ping_times_out_with_the_pong_text() {
        let (client, server) = duplex(4096);
        tokio::spawn(script_then_silence(
            server,
            "INFO {\"server_name\":\"golden\"}\r\n",
        ));

        let (result, took) = over(&spec(false), client).await;

        assert_eq!(took, LIMIT);
        assert_eq!(
            failure(result),
            (
                TIMEOUT.as_str().to_owned(),
                "no PONG from the server within 2000 ms".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_missing_jetstream_reply_times_out_with_the_api_text() {
        let (client, mut server) = duplex(4096);
        tokio::spawn(async move {
            server
                .write_all(info("golden", true).as_bytes())
                .await
                .expect("INFO can be written");
            read_until(&mut server, "PING\r\n").await;
            server
                .write_all(b"PONG\r\n")
                .await
                .expect("PONG can be written");
            std::future::pending::<()>().await;
        });

        let (result, took) = over(&spec(true), client).await;

        assert_eq!(took, LIMIT);
        assert_eq!(
            failure(result),
            (
                TIMEOUT.as_str().to_owned(),
                "no reply to $JS.API.INFO within 2000 ms".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_whole_attempt_shares_one_deadline() {
        // The INFO comes 1500 ms in; the PONG never comes. A deadline per
        // phase would end the attempt at 3500 ms.
        let (client, mut server) = duplex(4096);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            server
                .write_all(info("golden", false).as_bytes())
                .await
                .expect("INFO can be written");
            std::future::pending::<()>().await;
        });

        let (result, took) = over(&spec(false), client).await;

        assert_eq!(took, LIMIT);
        assert_eq!(
            failure(result),
            (
                TIMEOUT.as_str().to_owned(),
                "no PONG from the server within 2000 ms".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_closed_connection_is_a_protocol_error_naming_what_was_awaited() {
        let (client, server) = duplex(4096);
        drop(server);

        let (result, _) = over(&spec(false), client).await;

        assert_eq!(
            failure(result),
            (
                PROTOCOL_ERROR.as_str().to_owned(),
                "connection closed while waiting for INFO".to_owned()
            )
        );
    }

    /// A stream whose reads fail like a peer that reset the connection.
    struct Resetting;

    impl AsyncRead for Resetting {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::ConnectionReset)))
        }
    }

    impl AsyncWrite for Resetting {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buffer.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_reset_connection_is_a_protocol_error_naming_what_was_awaited() {
        let result = attempt(&spec(false), LIMIT, |_| async { Ok(Resetting) }).await;

        assert_eq!(
            failure(result),
            (
                PROTOCOL_ERROR.as_str().to_owned(),
                "connection reset while waiting for INFO".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_unix_target_fails_without_connecting() {
        let target = NativeProbeSpec::new(
            ProbeTarget::Unix {
                path: "/tmp/never.sock".into(),
            },
            NativeProbe::NatsReadiness {
                server_name: "golden".to_owned(),
                jetstream: false,
            },
        );

        let result = attempt(&target, LIMIT, |_| async {
            panic!("a unix target never connects");
            #[expect(unreachable_code, reason = "gives the closure its future type")]
            Ok::<DuplexStream, io::Error>(duplex(1).0)
        })
        .await;

        assert_eq!(
            failure(result),
            (
                PROTOCOL_ERROR.as_str().to_owned(),
                "nats-readiness supports tcp endpoints only".to_owned()
            )
        );
    }

    #[test]
    fn a_failure_detail_is_redacted_before_it_becomes_an_observation_detail() {
        let redactor = Redactor::new();
        let secret = stackly_platform::SecretValue::new("s3cret-token-value".to_owned())
            .expect("a secret value");
        redactor.learn(&secret);

        let outcome = outcome(
            Err(NativeProbeFailure::new(
                nats::IDENTITY_MISMATCH,
                "server_name is \"s3cret-token-value\", expected \"golden\"",
            )),
            &redactor,
        );

        assert_eq!(outcome.reason(), &nats::IDENTITY_MISMATCH);
        let detail = outcome.detail().expect("a detail").as_str();
        assert!(!detail.contains("s3cret"), "{detail}");
    }
}
