//! The driver of the `traefik-routes` native probe (spec 0009 AC-22): the
//! IO of one attempt around the pure checks in `provider::probe::traefik`.
//!
//! One attempt reads the routes file, sends `GET /ping`, then
//! `GET /api/http/routers`, each over its own connection to the loopback
//! endpoint and all under one deadline. It only reads: nothing is ever
//! written to the API of Traefik.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, timeout_at};

use super::{CONNECT_FAILED, PROTOCOL_ERROR, TIMEOUT, UNEXPECTED_STATUS};
use crate::provider::ProbeTarget;
use crate::provider::probe::NativeProbeFailure;
use crate::provider::probe::traefik::{
    self, PING_PATH, ROUTERS_BODY_MAX, ROUTERS_PATH, compare, expected_routers,
};
use crate::supervisor::http::{self, HttpError};

/// What reading the routes file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RoutesFile {
    /// The file exists and holds these bytes.
    Bytes(Vec<u8>),
    /// No file is at the path.
    Missing,
    /// The file exists but cannot be read within the limit.
    Unreadable,
}

/// One `traefik-routes` attempt within `limit`.
///
/// `read` reads the routes file; `connect` opens one connection and is
/// called once per request.
pub(super) async fn attempt<S, C, F>(
    target: &ProbeTarget,
    routes_file: &Path,
    limit: Duration,
    header_max: usize,
    read: impl FnOnce(&Path) -> RoutesFile,
    mut connect: C,
) -> Result<(), NativeProbeFailure>
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: FnMut(SocketAddr) -> F,
    F: Future<Output = io::Result<S>>,
{
    let ProbeTarget::Ip { host, port } = target else {
        return Err(traefik::unix_target());
    };
    let deadline = Instant::now() + limit;
    let expected = match read(routes_file) {
        RoutesFile::Bytes(bytes) => expected_routers(Some(&bytes))?,
        RoutesFile::Missing => expected_routers(None)?,
        RoutesFile::Unreadable => return Err(traefik::routes_file_unreadable()),
    };
    let address = SocketAddr::new(*host, port.get());

    let ping = http::request(*host, *port, PING_PATH);
    let mut stream = open(&mut connect, address, deadline, limit).await?;
    let status = match timeout_at(deadline, http::status(&mut stream, &ping, header_max)).await {
        Err(_) => return Err(no_answer(PING_PATH, address, limit)),
        Ok(Err(error)) => return Err(exchange_failed(PING_PATH, address, &error)),
        Ok(Ok(status)) => status,
    };
    drop(stream);
    if status != 200 {
        return Err(NativeProbeFailure::new(
            UNEXPECTED_STATUS,
            format!("status {status}"),
        ));
    }

    let routers = http::request(*host, *port, ROUTERS_PATH);
    let mut stream = open(&mut connect, address, deadline, limit).await?;
    let reply = match timeout_at(
        deadline,
        http::response(&mut stream, &routers, header_max, ROUTERS_BODY_MAX),
    )
    .await
    {
        Err(_) => return Err(no_answer(ROUTERS_PATH, address, limit)),
        Ok(Err(error)) => return Err(exchange_failed(ROUTERS_PATH, address, &error)),
        Ok(Ok(reply)) => reply,
    };
    if reply.status != 200 {
        return Err(NativeProbeFailure::new(
            UNEXPECTED_STATUS,
            format!("status {} for GET {ROUTERS_PATH}", reply.status),
        ));
    }
    compare(&expected, &reply.body)
}

/// Opens one connection before the deadline.
async fn open<S, C, F>(
    connect: &mut C,
    address: SocketAddr,
    deadline: Instant,
    limit: Duration,
) -> Result<S, NativeProbeFailure>
where
    C: FnMut(SocketAddr) -> F,
    F: Future<Output = io::Result<S>>,
{
    match timeout_at(deadline, connect(address)).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(NativeProbeFailure::new(
            CONNECT_FAILED,
            format!("connect to {address} failed: {}", error.kind()),
        )),
        Err(_) => Err(NativeProbeFailure::new(
            TIMEOUT,
            format!(
                "connect to {address} timed out after {} ms",
                limit.as_millis()
            ),
        )),
    }
}

fn no_answer(path: &str, address: SocketAddr, limit: Duration) -> NativeProbeFailure {
    NativeProbeFailure::new(
        TIMEOUT,
        format!(
            "no answer to GET {path} from {address} within {} ms",
            limit.as_millis()
        ),
    )
}

fn exchange_failed(path: &str, address: SocketAddr, error: &HttpError) -> NativeProbeFailure {
    NativeProbeFailure::new(PROTOCOL_ERROR, format!("GET {path} on {address}: {error}"))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, Mutex};

    use stackly_domain::Port;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

    use super::*;

    const LIMIT: Duration = Duration::from_secs(2);
    const ROUTES: &str = "http:\n  routers:\n    \"api\":\n      rule: \"Host(`api.localhost`)\"\n      service: \"api\"\n";
    const API_ROUTER: &str = r#"[{"name":"ping@internal","provider":"internal","rule":"PathPrefix(`/ping`)","status":"enabled"},{"name":"api@file","provider":"file","rule":"Host(`api.localhost`)","status":"enabled","service":"api"}]"#;

    fn target() -> ProbeTarget {
        ProbeTarget::Ip {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: Port::new(8082).expect("a test port is not 0"),
        }
    }

    fn file(text: &str) -> impl FnOnce(&Path) -> RoutesFile + use<> {
        let bytes = text.as_bytes().to_vec();
        move |_| RoutesFile::Bytes(bytes)
    }

    /// The request lines a stand in saw.
    type Seen = Arc<Mutex<Vec<String>>>;

    /// A Traefik stand in: each connection reads one request and answers
    /// from `answer`, by request line. Returns the request lines it saw.
    fn traefik(
        answer: fn(&str) -> Option<Vec<u8>>,
    ) -> (
        impl FnMut(SocketAddr) -> std::future::Ready<io::Result<DuplexStream>>,
        Seen,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let connect = move |_| {
            let (client, mut server) = duplex(64 * 1024);
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut request = vec![0_u8; 2048];
                let read = server.read(&mut request).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&request[..read]).into_owned();
                let line = text.lines().next().unwrap_or_default().to_owned();
                log.lock().unwrap().push(line.clone());
                match answer(&line) {
                    Some(reply) => {
                        let _ = server.write_all(&reply).await;
                    }
                    None => std::future::pending::<()>().await,
                }
            });
            std::future::ready(Ok(client))
        };
        (connect, seen)
    }

    fn ok(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn healthy(line: &str) -> Option<Vec<u8>> {
        if line.starts_with("GET /ping ") {
            Some(ok("OK"))
        } else {
            Some(ok(API_ROUTER))
        }
    }

    fn failure(result: Result<(), NativeProbeFailure>) -> (String, String) {
        let failure = result.expect_err("the attempt fails");
        (failure.reason.as_str().to_owned(), failure.detail)
    }

    // covers: AC-22
    #[tokio::test(start_paused = true)]
    async fn loaded_routes_matching_the_file_are_ready_after_ping_then_the_api() {
        let (connect, seen) = traefik(healthy);

        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            file(ROUTES),
            connect,
        )
        .await;

        assert_eq!(result, Ok(()));
        assert_eq!(
            *seen.lock().unwrap(),
            [
                "GET /ping HTTP/1.1",
                "GET /api/http/routers?per_page=1000 HTTP/1.1"
            ]
        );
    }

    // covers: AC-22
    #[tokio::test(start_paused = true)]
    async fn a_new_route_traefik_has_not_loaded_is_a_routes_mismatch() {
        let (connect, _) = traefik(healthy);
        let routes = format!(
            "{ROUTES}    \"docs\":\n      rule: \"Host(`docs.localhost`)\"\n      service: \"docs\"\n"
        );

        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            file(&routes),
            connect,
        )
        .await;

        assert_eq!(
            failure(result),
            (
                "routes_mismatch".to_owned(),
                "router \"docs\" is not loaded".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_ping_other_than_200_is_unexpected_status_before_the_api_is_asked() {
        let (connect, seen) =
            traefik(|_| Some(b"HTTP/1.1 503 Service Unavailable\r\n\r\n".to_vec()));

        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            file(ROUTES),
            connect,
        )
        .await;

        assert_eq!(
            failure(result),
            ("unexpected_status".to_owned(), "status 503".to_owned())
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unreadable_routes_file_is_a_protocol_error_without_connecting() {
        let (connect, seen) = traefik(healthy);

        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            |_| RoutesFile::Unreadable,
            connect,
        )
        .await;

        assert_eq!(
            failure(result),
            (
                "protocol_error".to_owned(),
                "routes file is unreadable".to_owned()
            )
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_missing_routes_file_expects_no_file_router() {
        let (connect, _) = traefik(|line| {
            if line.starts_with("GET /ping ") {
                Some(ok("OK"))
            } else {
                Some(ok("[]"))
            }
        });

        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            |_| RoutesFile::Missing,
            connect,
        )
        .await;

        assert_eq!(result, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_body_that_is_not_json_is_a_protocol_error() {
        let (connect, _) = traefik(|line| {
            if line.starts_with("GET /ping ") {
                Some(ok("OK"))
            } else {
                Some(ok("<html>404</html>"))
            }
        });

        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            file(ROUTES),
            connect,
        )
        .await;

        assert_eq!(
            failure(result),
            (
                "protocol_error".to_owned(),
                "the routers reply is not a list of routers".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_api_that_never_answers_times_out_at_the_one_deadline() {
        let (connect, _) = traefik(|line| line.starts_with("GET /ping ").then(|| ok("OK")));
        let started = Instant::now();

        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            file(ROUTES),
            connect,
        )
        .await;

        assert_eq!(started.elapsed(), LIMIT);
        assert_eq!(
            failure(result),
            (
                "timeout".to_owned(),
                "no answer to GET /api/http/routers?per_page=1000 from 127.0.0.1:8082 within 2000 ms"
                    .to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_connect_is_connect_failed() {
        let result = attempt(
            &target(),
            Path::new("/r"),
            LIMIT,
            8192,
            file(ROUTES),
            |_| async { Err::<DuplexStream, _>(io::Error::from(io::ErrorKind::ConnectionRefused)) },
        )
        .await;

        assert_eq!(
            failure(result),
            (
                "connect_failed".to_owned(),
                "connect to 127.0.0.1:8082 failed: connection refused".to_owned()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_unix_target_fails_without_reading_or_connecting() {
        let result = attempt(
            &ProbeTarget::Unix {
                path: "/tmp/never.sock".into(),
            },
            Path::new("/r"),
            LIMIT,
            8192,
            |_| panic!("a unix target never reads the file"),
            |_| async {
                panic!("a unix target never connects");
                #[expect(unreachable_code, reason = "gives the closure its future type")]
                Ok::<DuplexStream, io::Error>(duplex(1).0)
            },
        )
        .await;

        assert_eq!(
            failure(result),
            (
                "protocol_error".to_owned(),
                "traefik-routes supports tcp endpoints only".to_owned()
            )
        );
    }
}
