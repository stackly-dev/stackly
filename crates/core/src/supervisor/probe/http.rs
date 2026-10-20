//! The `http` probe: connect, send one `GET`, and read the status line, all
//! within one attempt timeout; success when the status is in
//! `expect_status`.

use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::ops::RangeInclusive;
use std::time::Duration;

use stackly_domain::Port;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use super::tcp::{connect_failed, connect_timed_out};
use super::{PROTOCOL_ERROR, ProbeContext, ProbeOutcome, TIMEOUT, UNEXPECTED_STATUS};
use crate::secret::Redactor;
use crate::supervisor::http::{HttpError, request, status};

/// One `http` probe from the plan.
#[derive(Debug, Clone, Copy)]
pub(super) struct HttpProbe<'a> {
    /// The loopback host.
    pub(super) host: IpAddr,
    /// The port.
    pub(super) port: Port,
    /// The literal path.
    pub(super) path: &'a str,
    /// The statuses that count as success.
    pub(super) expect_status: &'a RangeInclusive<u16>,
}

/// Runs one attempt over a real TCP connection.
pub(super) async fn run(
    probe: HttpProbe<'_>,
    limit: Duration,
    context: ProbeContext<'_>,
) -> ProbeOutcome {
    let addr = SocketAddr::new(probe.host, probe.port.get());
    attempt(
        TcpStream::connect(addr),
        probe,
        limit,
        context.config.http_header_max,
        context.redactor,
    )
    .await
}

/// Where an attempt was when its deadline passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Connect,
    Exchange,
}

/// Why an attempt failed before its deadline.
#[derive(Debug)]
enum Failure {
    Connect(io::Error),
    Exchange(HttpError),
}

/// Runs one attempt over the stream `connecting` yields, under one
/// deadline for connect, request, and status line.
async fn attempt<S>(
    connecting: impl Future<Output = io::Result<S>>,
    probe: HttpProbe<'_>,
    limit: Duration,
    header_max: usize,
    redactor: &Redactor,
) -> ProbeOutcome
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let addr = SocketAddr::new(probe.host, probe.port.get());
    let request = request(probe.host, probe.port, probe.path);
    let mut phase = Phase::Connect;
    let result = tokio::time::timeout(limit, async {
        let mut stream = connecting.await.map_err(Failure::Connect)?;
        phase = Phase::Exchange;
        status(&mut stream, &request, header_max)
            .await
            .map_err(Failure::Exchange)
    })
    .await;
    match result {
        Ok(Ok(code)) if probe.expect_status.contains(&code) => ProbeOutcome::Ok,
        Ok(Ok(code)) => {
            ProbeOutcome::failure(UNEXPECTED_STATUS, &format!("status {code}"), redactor)
        }
        Ok(Err(Failure::Connect(error))) => connect_failed(&addr, &error, redactor),
        Ok(Err(Failure::Exchange(error))) => {
            let raw = format!("GET {} on {addr}: {error}", probe.path);
            ProbeOutcome::failure(PROTOCOL_ERROR, &raw, redactor)
        }
        Err(_) if phase == Phase::Connect => connect_timed_out(&addr, limit, redactor),
        Err(_) => {
            let raw = format!(
                "no status line for GET {} from {addr} within {} ms",
                probe.path,
                limit.as_millis()
            );
            ProbeOutcome::failure(TIMEOUT, &raw, redactor)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use stackly_platform::SecretValue;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

    use super::super::CONNECT_FAILED;
    use super::*;

    const LIMIT: Duration = Duration::from_secs(2);
    const READY: RangeInclusive<u16> = 200..=299;

    fn probe(path: &str) -> HttpProbe<'_> {
        HttpProbe {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: Port::new(8080).unwrap(),
            path,
            expect_status: &READY,
        }
    }

    fn detail(outcome: &ProbeOutcome) -> &str {
        outcome.detail().map_or("", |detail| detail.as_str())
    }

    /// A stream whose peer reads the request, then answers `response`
    /// (or nothing, when `None`) and keeps the connection open.
    fn answering(response: Option<&'static [u8]>) -> DuplexStream {
        let (client, mut server) = duplex(64 * 1024);
        tokio::spawn(async move {
            let mut request = vec![0_u8; 1024];
            let _ = server.read(&mut request).await;
            if let Some(response) = response {
                let _ = server.write_all(response).await;
            }
            std::future::pending::<()>().await;
        });
        client
    }

    async fn over(response: Option<&'static [u8]>, path: &str, limit: Duration) -> ProbeOutcome {
        let stream = answering(response);
        attempt(
            async { Ok(stream) },
            probe(path),
            limit,
            8192,
            &Redactor::new(),
        )
        .await
    }

    #[tokio::test]
    async fn a_status_in_the_expected_range_is_ok() {
        let outcome = over(Some(b"HTTP/1.1 204 No Content\r\n\r\n"), "/ready", LIMIT).await;

        assert_eq!(outcome, ProbeOutcome::Ok);
    }

    #[tokio::test]
    async fn a_status_outside_the_range_is_unexpected_status_with_the_status() {
        let outcome = over(
            Some(b"HTTP/1.1 503 Service Unavailable\r\n\r\nnot ready\n"),
            "/ready",
            LIMIT,
        )
        .await;

        assert_eq!(outcome.reason(), &UNEXPECTED_STATUS);
        assert_eq!(detail(&outcome), "status 503");
    }

    #[tokio::test]
    async fn a_response_that_is_not_http_is_a_protocol_error() {
        let outcome = over(Some(b"SSH-2.0-OpenSSH_9.9\r\n"), "/ready", LIMIT).await;

        assert_eq!(outcome.reason(), &PROTOCOL_ERROR);
        assert_eq!(
            detail(&outcome),
            "GET /ready on 127.0.0.1:8080: invalid status line \"SSH-2.0-OpenSSH_9.9\""
        );
    }

    #[tokio::test]
    async fn a_failure_detail_hides_known_secrets() {
        let redactor = Redactor::new();
        redactor.learn(&SecretValue::new("hunter2-hunter2".to_owned()).unwrap());
        let stream = answering(Some(b"garbage\r\n"));

        let outcome = attempt(
            async { Ok(stream) },
            probe("/ready?token=hunter2-hunter2"),
            LIMIT,
            8192,
            &redactor,
        )
        .await;

        assert_eq!(outcome.reason(), &PROTOCOL_ERROR);
        assert_eq!(
            detail(&outcome),
            "GET /ready?token=**** on 127.0.0.1:8080: invalid status line \"garbage\""
        );
    }

    #[tokio::test]
    async fn a_refused_connect_is_connect_failed_with_the_address() {
        let outcome = attempt(
            async { Err::<DuplexStream, _>(io::Error::from(io::ErrorKind::ConnectionRefused)) },
            probe("/ready"),
            LIMIT,
            8192,
            &Redactor::new(),
        )
        .await;

        assert_eq!(outcome.reason(), &CONNECT_FAILED);
        assert_eq!(
            detail(&outcome),
            "connect to 127.0.0.1:8080 failed: connection refused"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_that_never_answers_times_out_at_the_limit() {
        let started = tokio::time::Instant::now();

        let outcome = over(None, "/ready", Duration::from_millis(750)).await;

        assert_eq!(started.elapsed(), Duration::from_millis(750));
        assert_eq!(outcome.reason(), &TIMEOUT);
        assert_eq!(
            detail(&outcome),
            "no status line for GET /ready from 127.0.0.1:8080 within 750 ms"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_connect_that_never_finishes_times_out_as_a_connect() {
        let outcome = attempt(
            std::future::pending::<io::Result<DuplexStream>>(),
            probe("/ready"),
            Duration::from_millis(300),
            8192,
            &Redactor::new(),
        )
        .await;

        assert_eq!(outcome.reason(), &TIMEOUT);
        assert_eq!(
            detail(&outcome),
            "connect to 127.0.0.1:8080 timed out after 300 ms"
        );
    }
}
