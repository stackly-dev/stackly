//! The small HTTP client of the `http` probe (spec 0010, *Probe, readiness,
//! dan health*): one `GET` over HTTP/1.1 with `Host` and
//! `Connection: close`, no redirects, and only the status line read. The
//! body is never read.
//!
//! The `traefik-routes` native probe (spec 0009 AC-22) also reads one body
//! through [`response`], with `Content-Length`, chunked, or close delimited
//! framing and a byte limit checked before the body grows past it.

use std::fmt;
use std::io;
use std::net::IpAddr;

use stackly_domain::Port;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Longest piece of a bad status line a failure quotes, in characters.
const QUOTE_MAX: usize = 64;

/// Why no status came back.
#[derive(Debug)]
pub(crate) enum HttpError {
    /// Sending the request failed.
    Write(io::Error),
    /// Reading the response failed.
    Read(io::Error),
    /// The server closed the connection before the status line ended.
    Closed,
    /// The status line did not end within the header limit.
    TooLong {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The status line is not `HTTP/1.x <3 digits>`.
    Invalid {
        /// The start of the line, lossy, with control characters as `?`.
        quote: String,
    },
    /// The header block did not end within the header limit.
    HeadersTooLong {
        /// The limit, in bytes.
        limit: usize,
    },
    /// Reading the body failed.
    ReadBody(io::Error),
    /// The server closed the connection before the body ended.
    BodyClosed,
    /// The body is longer than the limit.
    BodyTooLong {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The framing of the body is broken.
    BadBody {
        /// What is wrong.
        what: &'static str,
    },
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(error) => write!(f, "sending the request failed: {}", error.kind()),
            Self::Read(error) => write!(f, "reading the status line failed: {}", error.kind()),
            Self::Closed => f.write_str("the connection closed before the status line"),
            Self::TooLong { limit } => {
                write!(f, "the status line is longer than {limit} bytes")
            }
            Self::Invalid { quote } => write!(f, "invalid status line \"{quote}\""),
            Self::HeadersTooLong { limit } => {
                write!(f, "the headers are longer than {limit} bytes")
            }
            Self::ReadBody(error) => write!(f, "reading the body failed: {}", error.kind()),
            Self::BodyClosed => f.write_str("the connection closed before the body ended"),
            Self::BodyTooLong { limit } => write!(f, "the body is longer than {limit} bytes"),
            Self::BadBody { what } => write!(f, "the body framing is invalid: {what}"),
        }
    }
}

/// Returns the request a probe sends: the literal `path`, and the host
/// with the port, an IPv6 host in square brackets.
pub(crate) fn request(host: IpAddr, port: Port, path: &str) -> String {
    let host = match host {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n")
}

/// Sends `request` on `stream` and reads the response until its status
/// line ends, at most `header_max` bytes, then returns the status code.
///
/// # Errors
///
/// Returns an [`HttpError`] when the write or a read fails, the
/// connection closes first, the line does not end within `header_max`, or
/// the line is not a valid HTTP/1.x status line.
pub(crate) async fn status<S>(
    stream: &mut S,
    request: &str,
    header_max: usize,
) -> Result<u16, HttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(HttpError::Write)?;
    stream.flush().await.map_err(HttpError::Write)?;

    let mut head = Vec::with_capacity(header_max.min(1024));
    let mut chunk = [0_u8; 1024];
    // Only the bytes read since the last search are searched again.
    let mut scanned = 0;
    loop {
        if let Some(found) = head[scanned..].iter().position(|byte| *byte == b'\n') {
            return parse_status_line(&head[..scanned + found]);
        }
        scanned = head.len();
        if head.len() >= header_max {
            return Err(HttpError::TooLong { limit: header_max });
        }
        let room = (header_max - head.len()).min(chunk.len());
        let read = stream
            .read(&mut chunk[..room])
            .await
            .map_err(HttpError::Read)?;
        if read == 0 {
            return Err(HttpError::Closed);
        }
        head.extend_from_slice(&chunk[..read]);
    }
}

/// A status and a whole body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    /// The status code.
    pub(crate) status: u16,
    /// The body, after chunked decoding, at most the body limit.
    pub(crate) body: Vec<u8>,
}

/// How the body of a response ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framing {
    Length(usize),
    Chunked,
    Close,
}

/// Sends `request` on `stream` and reads the whole response: the header
/// block of at most `header_max` bytes, then a body of at most `body_max`
/// bytes.
///
/// # Errors
///
/// Returns an [`HttpError`] when a write or read fails, the connection
/// closes early, a limit is passed, or the status line, the headers, or the
/// body framing are invalid.
pub(crate) async fn response<S>(
    stream: &mut S,
    request: &str,
    header_max: usize,
    body_max: usize,
) -> Result<Response, HttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(HttpError::Write)?;
    stream.flush().await.map_err(HttpError::Write)?;

    let mut raw = Vec::with_capacity(header_max.min(4096));
    let mut chunk = [0_u8; 8192];
    let head_end = loop {
        if let Some(end) = find(&raw, b"\r\n\r\n") {
            break end;
        }
        if raw.len() >= header_max {
            return Err(HttpError::HeadersTooLong { limit: header_max });
        }
        let read = stream.read(&mut chunk).await.map_err(HttpError::Read)?;
        if read == 0 {
            return Err(HttpError::Closed);
        }
        raw.extend_from_slice(&chunk[..read]);
    };
    if head_end > header_max {
        return Err(HttpError::HeadersTooLong { limit: header_max });
    }
    let head = &raw[..head_end];
    let line_end = find(head, b"\r\n").unwrap_or(head.len());
    let status = parse_status_line(&head[..line_end])?;
    let framing = framing(&head[line_end..])?;
    if let Framing::Length(length) = framing
        && length > body_max
    {
        return Err(HttpError::BodyTooLong { limit: body_max });
    }
    // Chunk headers add a little per chunk; twice the body is plenty for
    // any real server and still a hard bound.
    let raw_max = match framing {
        Framing::Chunked => body_max.saturating_mul(2).saturating_add(1024),
        Framing::Length(length) => length,
        Framing::Close => body_max,
    };
    let mut body = raw.split_off(head_end + 4);
    loop {
        match framing {
            Framing::Length(length) if body.len() >= length => {
                body.truncate(length);
                return Ok(Response { status, body });
            }
            // The last chunk, and any trailers, end with an empty line.
            Framing::Chunked if body.ends_with(b"\r\n\r\n") => {
                if let Some(decoded) = decode_chunked(&body, body_max)? {
                    return Ok(Response {
                        status,
                        body: decoded,
                    });
                }
            }
            _ => {}
        }
        if body.len() > raw_max {
            return Err(HttpError::BodyTooLong { limit: body_max });
        }
        let room = (raw_max + 1 - body.len()).min(chunk.len());
        let read = stream
            .read(&mut chunk[..room])
            .await
            .map_err(HttpError::ReadBody)?;
        if read == 0 {
            return match framing {
                Framing::Close => Ok(Response { status, body }),
                _ => Err(HttpError::BodyClosed),
            };
        }
        body.extend_from_slice(&chunk[..read]);
        if framing == Framing::Close && body.len() > body_max {
            return Err(HttpError::BodyTooLong { limit: body_max });
        }
    }
}

/// The position of the first `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Reads the body framing from the header lines after the status line.
fn framing(headers: &[u8]) -> Result<Framing, HttpError> {
    let mut length = None;
    let mut chunked = false;
    for line in headers.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        let name = &line[..colon];
        let value = String::from_utf8_lossy(&line[colon + 1..])
            .trim()
            .to_owned();
        if name.eq_ignore_ascii_case(b"content-length") {
            let parsed = value.parse::<usize>().map_err(|_| HttpError::BadBody {
                what: "content-length is not a number",
            })?;
            if length.is_some_and(|seen| seen != parsed) {
                return Err(HttpError::BadBody {
                    what: "two content-length values",
                });
            }
            length = Some(parsed);
        } else if name.eq_ignore_ascii_case(b"transfer-encoding") {
            let last = value.rsplit(',').next().unwrap_or_default().trim();
            if !last.eq_ignore_ascii_case("chunked") {
                return Err(HttpError::BadBody {
                    what: "a transfer coding other than chunked",
                });
            }
            chunked = true;
        }
    }
    Ok(match (chunked, length) {
        (true, _) => Framing::Chunked,
        (false, Some(length)) => Framing::Length(length),
        (false, None) => Framing::Close,
    })
}

/// Decodes a chunked body. Returns `None` while the last chunk has not
/// arrived yet.
fn decode_chunked(raw: &[u8], body_max: usize) -> Result<Option<Vec<u8>>, HttpError> {
    let bad = |what| HttpError::BadBody { what };
    let mut body = Vec::new();
    let mut rest = raw;
    loop {
        let Some(end) = find(rest, b"\r\n") else {
            return Ok(None);
        };
        let line = &rest[..end];
        let size = line.split(|byte| *byte == b';').next().unwrap_or_default();
        let size = std::str::from_utf8(size)
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.len() <= 8)
            .and_then(|text| usize::from_str_radix(text, 16).ok())
            .ok_or_else(|| bad("a chunk size is not hex"))?;
        rest = &rest[end + 2..];
        if size == 0 {
            // Trailers, if any, end with an empty line.
            loop {
                let Some(end) = find(rest, b"\r\n") else {
                    return Ok(None);
                };
                if end == 0 {
                    return Ok(Some(body));
                }
                rest = &rest[end + 2..];
            }
        }
        if body.len() + size > body_max {
            return Err(HttpError::BodyTooLong { limit: body_max });
        }
        if rest.len() < size + 2 {
            return Ok(None);
        }
        if &rest[size..size + 2] != b"\r\n" {
            return Err(bad("a chunk does not end with crlf"));
        }
        body.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

/// Parses `HTTP/1.0` or `HTTP/1.1`, one space, three digits, then the end
/// of the line or a space and a reason. A trailing `\r` is dropped.
fn parse_status_line(line: &[u8]) -> Result<u16, HttpError> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let status = line
        .strip_prefix(b"HTTP/1.1 ")
        .or_else(|| line.strip_prefix(b"HTTP/1.0 "))
        .and_then(|rest| match rest {
            [a, b, c] | [a, b, c, b' ', ..] => Some([*a, *b, *c]),
            _ => None,
        })
        .filter(|digits| digits[0].is_ascii_digit() && digits[0] != b'0')
        .filter(|digits| digits.iter().all(u8::is_ascii_digit))
        .map(|digits| {
            digits
                .iter()
                .fold(0_u16, |code, digit| code * 10 + u16::from(digit - b'0'))
        });
    status.ok_or_else(|| HttpError::Invalid { quote: quote(line) })
}

/// The start of `line` as text a failure detail can carry.
fn quote(line: &[u8]) -> String {
    String::from_utf8_lossy(line)
        .chars()
        .take(QUOTE_MAX)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use tokio::io::duplex;

    use super::*;

    fn port(number: u16) -> Port {
        Port::new(number).unwrap()
    }

    /// Runs `status` against a peer that reads the request and answers
    /// `response`, then closes.
    async fn exchange(
        response: &'static [u8],
        header_max: usize,
    ) -> (Result<u16, HttpError>, String) {
        let (mut client, mut server) = duplex(64 * 1024);
        let peer = tokio::spawn(async move {
            let mut seen = vec![0_u8; 1024];
            let read = server.read(&mut seen).await.unwrap();
            server.write_all(response).await.unwrap();
            String::from_utf8(seen[..read].to_vec()).unwrap()
        });
        let request = request(IpAddr::V4(Ipv4Addr::LOCALHOST), port(8080), "/ready");
        let result = status(&mut client, &request, header_max).await;
        drop(client);
        (result, peer.await.unwrap())
    }

    #[test]
    fn the_request_names_the_host_and_port_and_closes_the_connection() {
        assert_eq!(
            request(IpAddr::V4(Ipv4Addr::LOCALHOST), port(8080), "/ready?x=1"),
            "GET /ready?x=1 HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(
            request(IpAddr::V6(Ipv6Addr::LOCALHOST), port(443), "/"),
            "GET / HTTP/1.1\r\nHost: [::1]:443\r\nConnection: close\r\n\r\n"
        );
    }

    #[tokio::test]
    async fn the_status_comes_from_the_status_line_and_the_body_is_never_read() {
        let (result, sent) = exchange(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 10\r\n\r\nnot ready\n",
            8192,
        )
        .await;

        assert_eq!(result.unwrap(), 503);
        assert!(sent.starts_with("GET /ready HTTP/1.1\r\n"));
    }

    #[tokio::test]
    async fn http_1_0_and_a_status_line_without_reason_are_read() {
        assert_eq!(
            exchange(b"HTTP/1.0 200\r\n\r\n", 8192).await.0.unwrap(),
            200
        );
        assert_eq!(exchange(b"HTTP/1.1 204 \n", 8192).await.0.unwrap(), 204);
    }

    #[tokio::test]
    async fn a_status_line_that_arrives_one_byte_at_a_time_is_read() {
        let (mut client, mut server) = duplex(64 * 1024);
        let peer = tokio::spawn(async move {
            let mut seen = vec![0_u8; 1024];
            let _ = server.read(&mut seen).await.unwrap();
            for byte in b"HTTP/1.1 418 Teapot\r\nX: y\r\n\r\n" {
                server.write_all(&[*byte]).await.unwrap();
                server.flush().await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        let request = request(IpAddr::V4(Ipv4Addr::LOCALHOST), port(8080), "/ready");

        let result = status(&mut client, &request, 8192).await;

        assert_eq!(result.unwrap(), 418);
        peer.await.unwrap();
    }

    #[tokio::test]
    async fn a_connection_closed_before_the_status_line_ends_is_reported() {
        let (result, _) = exchange(b"HTTP/1.1 200 O", 8192).await;

        assert!(matches!(result, Err(HttpError::Closed)), "{result:?}");
    }

    #[tokio::test]
    async fn a_status_line_longer_than_the_header_limit_is_refused() {
        let (result, _) = exchange(b"HTTP/1.1 200 OOOOOOOOOOOOOOOOOOOOOOOOOOOOOO\r\n", 16).await;

        assert!(
            matches!(result, Err(HttpError::TooLong { limit: 16 })),
            "{result:?}"
        );
    }

    /// Runs `response` against a peer that reads the request and answers
    /// `reply`, then closes when `close` holds or stays open.
    async fn whole(reply: Vec<u8>, close: bool, body_max: usize) -> Result<Response, HttpError> {
        let (mut client, mut server) = duplex(256 * 1024);
        tokio::spawn(async move {
            let mut seen = vec![0_u8; 1024];
            let _ = server.read(&mut seen).await;
            let _ = server.write_all(&reply).await;
            if !close {
                std::future::pending::<()>().await;
            }
        });
        let request = request(IpAddr::V4(Ipv4Addr::LOCALHOST), port(8080), "/api");
        response(&mut client, &request, 8192, body_max).await
    }

    #[tokio::test]
    async fn a_content_length_body_is_read_without_waiting_for_the_close() {
        let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello".to_vec();

        let got = whole(reply, false, 1024).await.unwrap();

        assert_eq!(got.status, 200);
        assert_eq!(got.body, b"hello");
    }

    #[tokio::test]
    async fn a_chunked_body_is_decoded_without_waiting_for_the_close() {
        let reply = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n7;x=y\r\n, world\r\n0\r\n\r\n".to_vec();

        let got = whole(reply, false, 1024).await.unwrap();

        assert_eq!(got.body, b"hello, world");
    }

    #[tokio::test]
    async fn a_body_without_framing_ends_at_the_close() {
        let reply = b"HTTP/1.0 200 OK\r\n\r\n[1,2]".to_vec();

        let got = whole(reply, true, 1024).await.unwrap();

        assert_eq!(got.body, b"[1,2]");
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused_for_every_framing() {
        let length = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello world".to_vec();
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nb\r\nhello world\r\n0\r\n\r\n"
                .to_vec();
        let close = b"HTTP/1.1 200 OK\r\n\r\nhello world".to_vec();

        for reply in [length, chunked, close] {
            let result = whole(reply, true, 10).await;
            assert!(
                matches!(result, Err(HttpError::BodyTooLong { limit: 10 })),
                "{result:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_body_cut_short_by_the_close_is_reported() {
        let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nhello".to_vec();

        let result = whole(reply, true, 1024).await;

        assert!(matches!(result, Err(HttpError::BodyClosed)), "{result:?}");
    }

    #[test]
    fn broken_chunk_framing_is_refused() {
        assert!(matches!(
            decode_chunked(b"zz\r\nhello\r\n0\r\n\r\n", 1024),
            Err(HttpError::BadBody { .. })
        ));
        assert!(matches!(
            decode_chunked(b"2\r\nhello\r\n0\r\n\r\n", 1024),
            Err(HttpError::BadBody { .. })
        ));
        assert_eq!(decode_chunked(b"5\r\nhel", 1024).unwrap(), None);
    }

    #[test]
    fn a_transfer_coding_other_than_chunked_is_refused() {
        assert!(matches!(
            framing(b"\r\nTransfer-Encoding: gzip\r\n"),
            Err(HttpError::BadBody { .. })
        ));
        assert_eq!(
            framing(b"\r\ncontent-length: 12\r\n").unwrap(),
            Framing::Length(12)
        );
    }

    #[test]
    fn invalid_status_lines_are_refused_with_a_quote() {
        for line in [
            &b"HTTP/2 200 OK"[..],
            b"HTTP/1.1 20 OK",
            b"HTTP/1.1 2000 OK",
            b"HTTP/1.1 abc OK",
            b"HTTP/1.1 099 OK",
            b"SSH-2.0-OpenSSH_9.9",
            b"",
        ] {
            assert!(
                matches!(parse_status_line(line), Err(HttpError::Invalid { .. })),
                "{}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn a_quote_is_cut_and_hides_control_characters() {
        let mut line = b"\x1b[31mHELLO\x07".to_vec();
        line.extend(std::iter::repeat_n(b'x', 100));

        let quote = quote(&line);

        assert!(quote.starts_with("?[31mHELLO?"));
        assert_eq!(quote.chars().count(), QUOTE_MAX);
    }
}
