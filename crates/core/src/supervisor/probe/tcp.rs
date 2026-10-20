//! The `tcp` probe: a connect that succeeds within the attempt timeout, to
//! an IP endpoint, or to a Unix socket on a Unix OS.

use std::fmt::Display;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpStream;

use super::{CONNECT_FAILED, ProbeOutcome, TIMEOUT};
use crate::provider::ProbeTarget;
use crate::secret::Redactor;

/// Connects to `target` once and closes the connection at once.
pub(super) async fn run(
    target: &ProbeTarget,
    limit: Duration,
    redactor: &Redactor,
) -> ProbeOutcome {
    match target {
        ProbeTarget::Ip { host, port } => {
            let addr = SocketAddr::new(*host, port.get());
            connect(TcpStream::connect(addr), &addr, limit, redactor).await
        }
        ProbeTarget::Unix { path } => unix(path, limit, redactor).await,
    }
}

#[cfg(unix)]
async fn unix(path: &std::path::Path, limit: Duration, redactor: &Redactor) -> ProbeOutcome {
    let connecting = tokio::net::UnixStream::connect(path);
    connect(connecting, &path.display(), limit, redactor).await
}

#[cfg(not(unix))]
async fn unix(path: &std::path::Path, _limit: Duration, redactor: &Redactor) -> ProbeOutcome {
    let raw = format!(
        "connect to {} failed: unix sockets need a unix os",
        path.display()
    );
    ProbeOutcome::failure(CONNECT_FAILED, &raw, redactor)
}

/// Awaits `connecting` within `limit` and maps the result; the stream, if
/// any, closes when it drops.
pub(super) async fn connect<S>(
    connecting: impl Future<Output = io::Result<S>>,
    target: &(dyn Display + Sync),
    limit: Duration,
    redactor: &Redactor,
) -> ProbeOutcome {
    match tokio::time::timeout(limit, connecting).await {
        Ok(Ok(_stream)) => ProbeOutcome::Ok,
        Ok(Err(error)) => connect_failed(target, &error, redactor),
        Err(_) => connect_timed_out(target, limit, redactor),
    }
}

/// The failure of a refused or failed connect.
pub(super) fn connect_failed(
    target: &dyn Display,
    error: &io::Error,
    redactor: &Redactor,
) -> ProbeOutcome {
    let raw = format!("connect to {target} failed: {}", error.kind());
    ProbeOutcome::failure(CONNECT_FAILED, &raw, redactor)
}

/// The failure of a connect that ran past the limit.
pub(super) fn connect_timed_out(
    target: &dyn Display,
    limit: Duration,
    redactor: &Redactor,
) -> ProbeOutcome {
    let raw = format!(
        "connect to {target} timed out after {} ms",
        limit.as_millis()
    );
    ProbeOutcome::failure(TIMEOUT, &raw, redactor)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, TcpListener};

    use stackly_domain::Port;
    use stackly_testkit::{TestRoot, allocate_port};

    use super::*;

    const LIMIT: Duration = Duration::from_secs(2);

    fn ip_target(port: u16) -> ProbeTarget {
        ProbeTarget::Ip {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: Port::new(port).unwrap(),
        }
    }

    fn detail(outcome: &ProbeOutcome) -> &str {
        outcome.detail().map_or("", |detail| detail.as_str())
    }

    #[tokio::test]
    async fn a_listening_port_is_ok() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let outcome = run(&ip_target(port), LIMIT, &Redactor::new()).await;

        assert_eq!(outcome, ProbeOutcome::Ok);
    }

    #[tokio::test]
    async fn a_closed_port_is_connect_failed_with_the_address() {
        let port = allocate_port().unwrap();

        let outcome = run(&ip_target(port), LIMIT, &Redactor::new()).await;

        assert_eq!(outcome.reason(), &CONNECT_FAILED);
        assert_eq!(
            detail(&outcome),
            format!("connect to 127.0.0.1:{port} failed: connection refused")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_connect_that_never_finishes_times_out_at_the_limit() {
        let started = tokio::time::Instant::now();
        let target = "127.0.0.1:4222";

        let outcome = connect(
            std::future::pending::<io::Result<()>>(),
            &target,
            Duration::from_millis(1500),
            &Redactor::new(),
        )
        .await;

        assert_eq!(started.elapsed(), Duration::from_millis(1500));
        assert_eq!(outcome.reason(), &TIMEOUT);
        assert_eq!(
            detail(&outcome),
            "connect to 127.0.0.1:4222 timed out after 1500 ms"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_listening_unix_socket_is_ok_and_a_missing_one_fails() {
        let root = TestRoot::new().unwrap();
        let path = root.run_dir().join("p.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let missing = root.run_dir().join("missing.sock");

        let ok = run(&ProbeTarget::Unix { path }, LIMIT, &Redactor::new()).await;
        let failed = run(
            &ProbeTarget::Unix {
                path: missing.clone(),
            },
            LIMIT,
            &Redactor::new(),
        )
        .await;

        assert_eq!(ok, ProbeOutcome::Ok);
        assert_eq!(failed.reason(), &CONNECT_FAILED);
        assert_eq!(
            detail(&failed),
            format!("connect to {} failed: entity not found", missing.display())
        );
    }
}
