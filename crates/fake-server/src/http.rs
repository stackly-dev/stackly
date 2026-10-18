//! Just enough HTTP/1.1 for readiness and health probes and the shutdown
//! command: one request per connection, then `Connection: close`.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Read and write timeout on every connection, both ways.
pub(crate) const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Largest request line plus headers the server reads.
const MAX_HEAD_BYTES: usize = 16 * 1024;
/// Largest request body the server reads and throws away.
const MAX_BODY_BYTES: usize = 64 * 1024;
/// Largest response the shutdown client reads.
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

/// The parts of a request the fake server routes on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Request {
    /// The method, such as `GET`.
    pub(crate) method: String,
    /// The target without its query string, such as `/ready`.
    pub(crate) path: String,
}

/// Why no request could be read.
#[derive(Debug)]
pub(crate) enum RequestError {
    /// The peer went away, timed out, or closed early; nothing is worth
    /// answering.
    Closed,
    /// The bytes are not an HTTP/1.x request this server reads; answer 400.
    Malformed,
}

impl From<io::Error> for RequestError {
    fn from(_: io::Error) -> Self {
        Self::Closed
    }
}

/// Reads one request head and drains its body, if it has one.
pub(crate) fn read_request(stream: &mut impl Read) -> Result<Request, RequestError> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            break end;
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return Err(RequestError::Malformed);
        }
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(RequestError::Closed);
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = std::str::from_utf8(&buffer[..head_end]).map_err(|_| RequestError::Malformed)?;
    let mut lines = head.split("\r\n");
    let request = parse_request_line(lines.next().unwrap_or_default())?;
    let body_length = content_length(lines)?;

    let already_read = buffer.len() - (head_end + 4);
    let remaining = body_length.saturating_sub(already_read);
    io::copy(&mut stream.take(remaining as u64), &mut io::sink())?;
    Ok(request)
}

fn parse_request_line(line: &str) -> Result<Request, RequestError> {
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(RequestError::Malformed);
    };
    if method.is_empty() || !target.starts_with('/') || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
    {
        return Err(RequestError::Malformed);
    }
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    Ok(Request {
        method: method.to_owned(),
        path: path.to_owned(),
    })
}

fn content_length<'a>(headers: impl Iterator<Item = &'a str>) -> Result<usize, RequestError> {
    let mut length = 0;
    for header in headers {
        let Some((name, value)) = header.split_once(':') else {
            return Err(RequestError::Malformed);
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| RequestError::Malformed)?;
        }
    }
    if length > MAX_BODY_BYTES {
        return Err(RequestError::Malformed);
    }
    Ok(length)
}

/// Writes a complete response with `Connection: close`.
pub(crate) fn write_response(
    stream: &mut impl Write,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// Sends `POST <path>` with an empty body to `address` and returns the
/// response status.
pub(crate) fn post(address: SocketAddr, path: &str) -> io::Result<u16> {
    let mut stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;

    let mut response = Vec::new();
    // The server may exit right after it answers, so a reset after the
    // status line still counts as an answer.
    let read = stream.take(MAX_RESPONSE_BYTES).read_to_end(&mut response);
    match (status_code(&response), read) {
        (Some(status), _) => Ok(status),
        (None, Err(error)) => Err(error),
        (None, Ok(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the server did not answer with an http status line",
        )),
    }
}

fn status_code(response: &[u8]) -> Option<u16> {
    let line_end = find(response, b"\r\n")?;
    let line = std::str::from_utf8(&response[..line_end]).ok()?;
    let mut parts = line.split(' ');
    if !parts.next()?.starts_with("HTTP/1.") {
        return None;
    }
    parts.next()?.parse().ok()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(raw: &str) -> Result<Request, RequestError> {
        read_request(&mut raw.as_bytes())
    }

    #[test]
    fn reads_method_and_path_without_the_query() {
        let request = read("GET /ready?attempt=3 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();

        assert_eq!(
            request,
            Request {
                method: "GET".to_owned(),
                path: "/ready".to_owned()
            }
        );
    }

    #[test]
    fn drains_a_small_body() {
        let mut raw: &[u8] = b"POST /shutdown HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let request = read_request(&mut raw).unwrap();

        assert_eq!(request.path, "/shutdown");
        assert!(raw.is_empty());
    }

    #[test]
    fn rejects_requests_it_does_not_understand() {
        for raw in [
            "GET\r\n\r\n",
            "GET ready HTTP/1.1\r\n\r\n",
            "GET /ready HTTP/2\r\n\r\n",
            "GET /ready HTTP/1.1 extra\r\n\r\n",
            "GET /ready HTTP/1.1\r\nno colon\r\n\r\n",
            "POST /x HTTP/1.1\r\nContent-Length: nope\r\n\r\n",
            "POST /x HTTP/1.1\r\nContent-Length: 70000\r\n\r\n",
        ] {
            assert!(matches!(read(raw), Err(RequestError::Malformed)), "{raw:?}");
        }
    }

    #[test]
    fn rejects_an_endless_head() {
        let raw = format!("GET /{} HTTP/1.1\r\n", "a".repeat(MAX_HEAD_BYTES + 10));

        assert!(matches!(read(&raw), Err(RequestError::Malformed)));
    }

    #[test]
    fn a_connection_closed_early_is_not_malformed() {
        assert!(matches!(read("GET /ready HT"), Err(RequestError::Closed)));
    }

    #[test]
    fn response_carries_length_and_close() {
        let mut out = Vec::new();
        write_response(&mut out, 503, "text/plain", b"not ready\n").unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\n\
             Content-Length: 10\r\nConnection: close\r\n\r\nnot ready\n"
        );
    }

    #[test]
    fn status_code_comes_from_the_status_line() {
        assert_eq!(status_code(b"HTTP/1.1 200 OK\r\n\r\n"), Some(200));
        assert_eq!(status_code(b"HTTP/1.0 404 Not Found\r\n"), Some(404));
        assert_eq!(status_code(b"SSH-2.0\r\n"), None);
        assert_eq!(status_code(b"HTTP/1.1 200 OK"), None);
    }
}
