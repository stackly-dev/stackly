//! `MacosFetcher` against a loopback HTTP server (spec 0018, AC-3 to AC-5):
//! `Range` and `If-Range`, redirects, lengths, encodings, and timeouts. The
//! server speaks plain HTTP because the test build of the adapter maps
//! `https://127.0.0.1:<port>` to it (`loopback-http`); the rules for a real
//! certificate are in `fetch_tls.rs`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use stackly_platform::{
    FetchError, FetchRequest, FetchResponse, FetchSink, Fetcher, HttpsUrl, ResponseKind, Resume,
    SinkError, StopFlag,
};
use stackly_platform_macos::MacosFetcher;

/// One request as the server read it.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    headers: BTreeMap<String, String>,
}

type Handler = dyn Fn(&Seen, &mut TcpStream) + Send + Sync + 'static;

/// A loopback server that answers each connection with `handler`.
struct Server {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    done: Arc<AtomicBool>,
}

impl Server {
    fn start(handler: impl Fn(&Seen, &mut TcpStream) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let port = listener.local_addr().expect("a bound listener").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let (seen_in, done_in) = (Arc::clone(&seen), Arc::clone(&done));
        thread::spawn(move || {
            for stream in listener.incoming() {
                if done_in.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let (handler, seen) = (Arc::clone(&handler), Arc::clone(&seen_in));
                thread::spawn(move || {
                    while let Some(request) = read_request(&mut stream) {
                        seen.lock()
                            .expect("the log is not poisoned")
                            .push(request.clone());
                        handler(&request, &mut stream);
                    }
                });
            }
        });
        Self { port, seen, done }
    }

    fn url(&self, path: &str) -> HttpsUrl {
        HttpsUrl::new(&format!("https://127.0.0.1:{}{path}", self.port)).expect("a valid url")
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log is not poisoned").clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut data = Vec::new();
    let mut byte = [0_u8; 1];
    while !data.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        data.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&data).into_owned();
    let mut lines = text.lines();
    let path = lines.next()?.split(' ').nth(1)?.to_owned();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Some(Seen { path, headers })
}

/// Writes a response with `headers` and `body`, announcing the body's
/// length unless a `content-length` header is given.
fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, String)], body: &[u8]) {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
    {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// A sink that keeps what it is given.
#[derive(Default)]
struct Collect {
    response: Option<FetchResponse>,
    bytes: Vec<u8>,
}

impl FetchSink for Collect {
    fn start(&mut self, response: &FetchResponse) -> Result<(), SinkError> {
        self.response = Some(response.clone());
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), SinkError> {
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }
}

fn fetcher() -> MacosFetcher {
    MacosFetcher::with_timeouts(Duration::from_secs(2), Duration::from_millis(400)).plain_loopback()
}

fn get(
    url: &HttpsUrl,
    size: u64,
    resume: Option<Resume<'_>>,
) -> (Result<u64, FetchError>, Collect) {
    let stop = StopFlag::new();
    let mut sink = Collect::default();
    let request = FetchRequest {
        url,
        resume,
        size,
        stop: &stop,
    };
    let result = fetcher()
        .fetch(&request, &mut sink)
        .map(|summary| summary.received);
    (result, sink)
}

const BODY: &[u8] = b"0123456789abcdefghij";

fn serve_body(seen: &Seen, stream: &mut TcpStream) {
    if seen.path == "/ok" {
        respond(stream, "200 OK", &[("ETag", "\"v1\"".to_owned())], BODY);
    }
}

#[test]
fn a_full_download_streams_the_body_and_sends_the_policy_headers() {
    let server = Server::start(serve_body);
    let url = server.url("/ok");

    let (result, sink) = get(&url, BODY.len() as u64, None);

    assert_eq!(result, Ok(BODY.len() as u64));
    assert_eq!(sink.bytes, BODY);
    let response = sink.response.expect("the sink was started");
    assert_eq!(response.kind, ResponseKind::Full);
    assert_eq!(response.validator.as_deref(), Some("\"v1\""));
    assert_eq!(response.content_length, Some(BODY.len() as u64));
    let headers = &server.seen()[0].headers;
    assert_eq!(
        headers.get("accept-encoding").map(String::as_str),
        Some("identity")
    );
    assert!(
        headers
            .get("user-agent")
            .is_some_and(|ua| ua.starts_with("Stackly/"))
    );
    assert!(!headers.contains_key("cookie") && !headers.contains_key("authorization"));
}

#[test]
fn a_resume_sends_range_and_if_range_and_accepts_a_matching_206() {
    let server = Server::start(|seen, stream| {
        let tail = &BODY[7..];
        respond(
            stream,
            "206 Partial Content",
            &[
                ("ETag", "\"v1\"".to_owned()),
                (
                    "Content-Range",
                    format!("bytes 7-{}/{}", BODY.len() - 1, BODY.len()),
                ),
            ],
            tail,
        );
        let _ = seen;
    });
    let url = server.url("/ok");

    let (result, sink) = get(
        &url,
        BODY.len() as u64,
        Some(Resume {
            offset: 7,
            validator: "\"v1\"",
        }),
    );

    assert_eq!(result, Ok(13));
    assert_eq!(sink.bytes, &BODY[7..]);
    assert_eq!(
        sink.response.expect("started").kind,
        ResponseKind::Resumed { offset: 7 }
    );
    let headers = &server.seen()[0].headers;
    assert_eq!(headers.get("range").map(String::as_str), Some("bytes=7-"));
    assert_eq!(headers.get("if-range").map(String::as_str), Some("\"v1\""));
}

#[test]
fn a_200_for_a_range_request_is_a_full_download_from_byte_zero() {
    let server = Server::start(serve_body);
    let url = server.url("/ok");

    let (result, sink) = get(
        &url,
        BODY.len() as u64,
        Some(Resume {
            offset: 7,
            validator: "\"v1\"",
        }),
    );

    assert_eq!(result, Ok(BODY.len() as u64));
    assert_eq!(sink.bytes, BODY);
    assert_eq!(sink.response.expect("started").kind, ResponseKind::Full);
}

#[test]
fn a_206_with_the_wrong_content_range_is_a_length_mismatch() {
    let server = Server::start(|_, stream| {
        respond(
            stream,
            "206 Partial Content",
            &[("Content-Range", "bytes 3-19/20".to_owned())],
            &BODY[3..],
        );
    });
    let url = server.url("/ok");

    let (result, sink) = get(
        &url,
        BODY.len() as u64,
        Some(Resume {
            offset: 7,
            validator: "\"v1\"",
        }),
    );

    assert!(
        matches!(result, Err(FetchError::LengthMismatch { .. })),
        "{result:?}"
    );
    assert!(sink.bytes.is_empty());
}

#[test]
fn a_416_for_a_resume_request_is_range_not_satisfiable() {
    let server = Server::start(|_, stream| {
        respond(stream, "416 Range Not Satisfiable", &[], b"");
    });
    let url = server.url("/ok");

    let (result, _) = get(
        &url,
        BODY.len() as u64,
        Some(Resume {
            offset: 7,
            validator: "\"v1\"",
        }),
    );

    assert_eq!(result, Err(FetchError::RangeNotSatisfiable));
}

#[test]
fn an_http_status_carries_retry_after_clamped_to_a_minute() {
    let server = Server::start(|seen, stream| {
        let retry = if seen.path == "/slow" { "120" } else { "7" };
        respond(
            stream,
            "503 Service Unavailable",
            &[("Retry-After", retry.to_owned())],
            b"",
        );
    });

    let (short, _) = get(&server.url("/soon"), 1, None);
    let (long, _) = get(&server.url("/slow"), 1, None);

    assert_eq!(
        short,
        Err(FetchError::Http {
            status: 503,
            retry_after: Some(Duration::from_secs(7))
        })
    );
    assert_eq!(
        long,
        Err(FetchError::Http {
            status: 503,
            retry_after: Some(Duration::from_secs(60))
        })
    );
}

#[test]
fn a_redirect_to_http_is_refused_before_any_connection() {
    let server = Server::start(|_, stream| {
        respond(
            stream,
            "302 Found",
            &[("Location", "http://downloads.example.invalid/a".to_owned())],
            b"",
        );
    });

    let (result, sink) = get(&server.url("/ok"), 1, None);

    assert_eq!(
        result,
        Err(FetchError::InsecureRedirect {
            host: "127.0.0.1".to_owned()
        })
    );
    assert!(sink.response.is_none());
}

#[test]
fn five_redirects_are_followed_to_the_final_answer() {
    let server = Server::start(|seen, stream| {
        let hop: u32 = seen
            .path
            .strip_prefix("/hop/")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        if hop < 5 {
            respond(
                stream,
                "302 Found",
                &[("Location", format!("/hop/{}", hop + 1))],
                b"",
            );
        } else {
            respond(stream, "200 OK", &[], BODY);
        }
    });

    let (result, sink) = get(&server.url("/hop/0"), BODY.len() as u64, None);

    assert_eq!(result, Ok(BODY.len() as u64), "{result:?}");
    assert_eq!(sink.bytes, BODY);
    assert_eq!(
        server.seen().len(),
        6,
        "the first request and five redirects"
    );
}

#[test]
fn six_redirects_in_a_row_are_too_many() {
    let server = Server::start(|seen, stream| {
        let hop: u32 = seen
            .path
            .strip_prefix("/loop/")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        respond(
            stream,
            "302 Found",
            &[("Location", format!("/loop/{}", hop + 1))],
            b"",
        );
    });

    let (result, _) = get(&server.url("/loop/0"), 1, None);

    assert_eq!(result, Err(FetchError::TooManyRedirects));
    assert_eq!(
        server.seen().len(),
        6,
        "the first request and five redirects"
    );
}

#[test]
fn range_and_if_range_are_sent_again_on_every_hop() {
    let server = Server::start(|seen, stream| {
        if seen.path == "/start" {
            respond(
                stream,
                "302 Found",
                &[("Location", "/final".to_owned())],
                b"",
            );
        } else {
            respond(
                stream,
                "206 Partial Content",
                &[
                    ("ETag", "\"v1\"".to_owned()),
                    (
                        "Content-Range",
                        format!("bytes 7-{}/{}", BODY.len() - 1, BODY.len()),
                    ),
                ],
                &BODY[7..],
            );
        }
    });

    let (result, _) = get(
        &server.url("/start"),
        BODY.len() as u64,
        Some(Resume {
            offset: 7,
            validator: "\"v1\"",
        }),
    );

    assert_eq!(result, Ok(13));
    for request in server.seen() {
        assert_eq!(
            request.headers.get("range").map(String::as_str),
            Some("bytes=7-")
        );
        assert_eq!(
            request.headers.get("if-range").map(String::as_str),
            Some("\"v1\"")
        );
    }
}

#[test]
fn a_gzip_content_encoding_is_not_decompressed() {
    let gzip = [
        0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    ];
    let server = Server::start(move |_, stream| {
        respond(
            stream,
            "200 OK",
            &[("Content-Encoding", "gzip".to_owned())],
            &gzip,
        );
    });

    let (result, sink) = get(&server.url("/ok"), gzip.len() as u64, None);

    assert_eq!(result, Ok(gzip.len() as u64));
    assert_eq!(sink.bytes, gzip);
}

#[test]
fn a_content_length_that_differs_from_the_catalog_size_is_a_length_mismatch() {
    let server = Server::start(serve_body);

    let (result, sink) = get(&server.url("/ok"), 5, None);

    assert_eq!(
        result,
        Err(FetchError::LengthMismatch {
            expected: 5,
            announced: BODY.len() as u64
        })
    );
    assert!(
        sink.bytes.is_empty(),
        "no byte is delivered after a bad length"
    );
}

#[test]
fn a_body_longer_than_the_size_stops_the_download() {
    let server = Server::start(|_, stream| {
        let head = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(BODY);
        let _ = stream.shutdown(std::net::Shutdown::Both);
    });

    let (result, _) = get(&server.url("/ok"), 5, None);

    assert_eq!(result, Err(FetchError::TooLarge));
}

#[test]
fn a_body_that_ends_early_is_interrupted() {
    let server = Server::start(|_, stream| {
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", BODY.len());
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&BODY[..6]);
        let _ = stream.shutdown(std::net::Shutdown::Both);
    });

    let (result, sink) = get(&server.url("/ok"), BODY.len() as u64, None);

    assert_eq!(result, Err(FetchError::Interrupted { received: 6 }));
    assert_eq!(sink.bytes, &BODY[..6]);
}

#[test]
fn a_server_that_goes_quiet_trips_the_idle_timeout() {
    let server = Server::start(|_, stream| {
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", BODY.len());
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&BODY[..2]);
        let _ = stream.flush();
        thread::sleep(Duration::from_secs(3));
    });

    let (result, sink) = get(&server.url("/ok"), BODY.len() as u64, None);

    assert_eq!(result, Err(FetchError::IdleTimeout { received: 2 }));
    assert_eq!(sink.bytes, &BODY[..2]);
}

#[test]
fn a_closed_port_is_unreachable() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        listener.local_addr().expect("a bound listener").port()
    };
    let url = HttpsUrl::new(&format!("https://127.0.0.1:{port}/a")).expect("a valid url");

    let (result, _) = get(&url, 1, None);

    assert!(
        matches!(result, Err(FetchError::Unreachable { .. })),
        "{result:?}"
    );
}

#[test]
fn only_a_strong_etag_or_last_modified_is_a_validator() {
    let server = Server::start(|seen, stream| match seen.path.as_str() {
        "/weak" => respond(stream, "200 OK", &[("ETag", "W/\"v1\"".to_owned())], BODY),
        "/dated" => respond(
            stream,
            "200 OK",
            &[("Last-Modified", "Wed, 21 Oct 2026 07:28:00 GMT".to_owned())],
            BODY,
        ),
        _ => respond(stream, "200 OK", &[], BODY),
    });
    let size = BODY.len() as u64;

    let validator = |path| {
        get(&server.url(path), size, None)
            .1
            .response
            .expect("started")
            .validator
    };

    assert_eq!(validator("/weak"), None);
    assert_eq!(
        validator("/dated").as_deref(),
        Some("Wed, 21 Oct 2026 07:28:00 GMT")
    );
    assert_eq!(validator("/plain"), None);
}

#[test]
fn a_raised_stop_flag_stops_before_the_request() {
    let server = Server::start(serve_body);
    let url = server.url("/ok");
    let stop = StopFlag::new();
    stop.stop();
    let mut sink = Collect::default();
    let request = FetchRequest {
        url: &url,
        resume: None,
        size: BODY.len() as u64,
        stop: &stop,
    };

    let result = fetcher().fetch(&request, &mut sink);

    assert_eq!(result.err(), Some(FetchError::Stopped));
    assert!(server.seen().is_empty());
}
