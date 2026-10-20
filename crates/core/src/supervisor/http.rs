//! The small HTTP client of the `http` probe (spec 0010, *Probe, readiness,
//! dan health*): one `GET` over HTTP/1.1 with `Host` and
//! `Connection: close`, no redirects, and only the status line read. The
//! body is never read.

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
    loop {
        if let Some(end) = head.iter().position(|byte| *byte == b'\n') {
            return parse_status_line(&head[..end]);
        }
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
