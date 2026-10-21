//! The download adapter (spec 0018, "Download dan cache"): one `ureq` agent
//! with rustls and the macOS trust store, redirects followed by hand, and
//! an idle timeout on every read.

use std::io::{self, Read};
use std::time::Duration;

use stackly_platform::{
    FetchError, FetchRequest, FetchResponse, FetchSink, FetchSummary, Fetcher, HttpsUrl,
    ResponseKind,
};
use ureq::Agent;
use ureq::Timeout;
use ureq::config::Config;
use ureq::http::{Response, Uri};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport,
};

/// Redirects followed before the download fails.
const MAX_REDIRECTS: u32 = 5;

/// Longest `Retry-After` kept; a server asking for more is clamped.
const RETRY_AFTER_MAX: Duration = Duration::from_secs(60);

/// How long a connect may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a read may wait for the next byte.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Size of the buffer a body is read into.
const CHUNK: usize = 64 * 1024;

/// A [`Fetcher`] over `ureq` 3 with rustls and the platform verifier, so
/// certificates follow the macOS trust store and its settings, MDM roots
/// included.
///
/// Redirects are switched off in the agent and followed here (at most 5, each
/// `Location` `https`), `Accept-Encoding: identity` is sent and nothing is
/// decompressed, and there are no cookies, no credentials, and no proxy.
/// The connect timeout is 15 seconds and a read may wait 30 seconds for
/// its next byte.
///
/// A body is read in 64 KiB chunks, and each chunk goes to the sink before
/// the next is read.
#[derive(Debug, Clone)]
pub struct MacosFetcher {
    agent: Agent,
    #[cfg(feature = "loopback-http")]
    plain_loopback: bool,
}

impl MacosFetcher {
    /// Builds the fetcher with the production timeouts.
    #[must_use]
    pub fn new() -> Self {
        Self::with_timeouts(CONNECT_TIMEOUT, IDLE_TIMEOUT)
    }

    /// Builds the fetcher with other timeouts, for tests that cannot wait
    /// 30 seconds.
    #[must_use]
    pub fn with_timeouts(connect: Duration, idle: Duration) -> Self {
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::PlatformVerifier)
            .build();
        let config = Config::builder()
            .tls_config(tls)
            .proxy(None)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .user_agent(concat!(
                "Stackly/",
                env!("CARGO_PKG_VERSION"),
                " (macOS; arm64)"
            ))
            .timeout_resolve(Some(connect))
            .timeout_connect(Some(connect))
            .build();
        let connector = DefaultConnector::new().chain(IdleConnector { idle });
        Self {
            agent: Agent::with_parts(config, connector, DefaultResolver::default()),
            #[cfg(feature = "loopback-http")]
            plain_loopback: false,
        }
    }

    /// Sends a request for `https://127.0.0.1:<port>/...` (or `localhost`,
    /// or `[::1]`) as plain HTTP and accepts a plain HTTP redirect to
    /// loopback. Only integration tests turn this on: they cannot hold a
    /// certificate the platform trusts, and the feature does not exist in a
    /// build without `loopback-http`.
    #[cfg(feature = "loopback-http")]
    #[must_use]
    pub fn plain_loopback(mut self) -> Self {
        self.plain_loopback = true;
        self
    }

    /// The URL the request goes to, which differs from `url` only for a
    /// loopback host in plain loopback mode.
    #[cfg_attr(
        not(feature = "loopback-http"),
        expect(clippy::unused_self, reason = "only the test feature reads self")
    )]
    fn wire_url(&self, url: &str) -> String {
        #[cfg(feature = "loopback-http")]
        if self.plain_loopback
            && let Some(rest) = url.strip_prefix("https://")
            && is_loopback(authority_host(rest))
        {
            return format!("http://{rest}");
        }
        url.to_owned()
    }

    /// Resolves a `Location` against the URL it came from, and checks that
    /// it is `https`.
    fn next_url(&self, current: &str, location: &str, host: &str) -> Result<String, FetchError> {
        let absolute = resolve(current, location);
        #[cfg(feature = "loopback-http")]
        if self.plain_loopback
            && let Some(rest) = absolute.strip_prefix("http://")
            && is_loopback(authority_host(rest))
        {
            return Ok(format!("https://{rest}"));
        }
        if HttpsUrl::new(&absolute).is_ok() {
            Ok(absolute)
        } else {
            Err(FetchError::InsecureRedirect {
                host: host.to_owned(),
            })
        }
    }
}

impl Default for MacosFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher for MacosFetcher {
    fn fetch(
        &self,
        request: &FetchRequest<'_>,
        sink: &mut dyn FetchSink,
    ) -> Result<FetchSummary, FetchError> {
        let mut current = request.url.as_str().to_owned();
        let mut host = request.url.host().to_owned();
        let mut redirects = 0;
        loop {
            if request.stop.is_stopped() {
                return Err(FetchError::Stopped);
            }
            let wire = self.wire_url(&current);
            let mut builder = self.agent.get(&wire).header("Accept-Encoding", "identity");
            if let Some(resume) = request.resume {
                builder = builder
                    .header("Range", format!("bytes={}-", resume.offset))
                    .header("If-Range", resume.validator);
            }
            let response = builder.call().map_err(|err| map_error(&err, &host, 0))?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = header(&response, "location")
            {
                if redirects >= MAX_REDIRECTS {
                    return Err(FetchError::TooManyRedirects);
                }
                redirects += 1;
                current = self.next_url(&current, location, &host)?;
                host = HttpsUrl::new(&current)
                    .map(|url| url.host().to_owned())
                    .unwrap_or(host);
                continue;
            }
            return read_response(request, sink, response, &host);
        }
    }
}

/// Checks the status and lengths of `response`, then streams its body.
fn read_response(
    request: &FetchRequest<'_>,
    sink: &mut dyn FetchSink,
    mut response: Response<ureq::Body>,
    host: &str,
) -> Result<FetchSummary, FetchError> {
    let status = response.status().as_u16();
    let size = request.size;
    let content_length = header(&response, "content-length").and_then(|v| v.trim().parse().ok());
    let kind = match (status, request.resume) {
        (200, _) => ResponseKind::Full,
        (206, Some(resume)) => {
            let expected_range =
                format!("bytes {}-{}/{size}", resume.offset, size.saturating_sub(1));
            let range = header(&response, "content-range").map(str::trim);
            if range != Some(expected_range.as_str()) {
                return Err(FetchError::LengthMismatch {
                    expected: size - resume.offset,
                    announced: content_length.unwrap_or(0),
                });
            }
            ResponseKind::Resumed {
                offset: resume.offset,
            }
        }
        (416, Some(_)) => return Err(FetchError::RangeNotSatisfiable),
        _ => {
            return Err(FetchError::Http {
                status,
                retry_after: retry_after(&response),
            });
        }
    };
    let expected = match kind {
        ResponseKind::Full => size,
        ResponseKind::Resumed { offset } => size - offset,
    };
    if let Some(announced) = content_length
        && announced != expected
    {
        return Err(FetchError::LengthMismatch {
            expected,
            announced,
        });
    }
    let info = FetchResponse {
        kind,
        content_length,
        validator: validator(&response),
        host: host.to_owned(),
    };
    sink.start(&info).map_err(FetchError::Sink)?;
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut buffer = vec![0_u8; CHUNK];
    let mut received = 0_u64;
    loop {
        if request.stop.is_stopped() {
            return Err(FetchError::Stopped);
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) => return Err(map_read_error(&err, host, received)),
        };
        received += read as u64;
        if received > expected {
            return Err(FetchError::TooLarge);
        }
        sink.write(&buffer[..read]).map_err(FetchError::Sink)?;
    }
    if received < expected {
        return Err(FetchError::Interrupted { received });
    }
    Ok(FetchSummary {
        received,
        host: host.to_owned(),
    })
}

fn header<'a>(response: &'a Response<ureq::Body>, name: &str) -> Option<&'a str> {
    response.headers().get(name)?.to_str().ok()
}

/// A strong `ETag`, else `Last-Modified`; a weak `ETag` is no validator.
fn validator(response: &Response<ureq::Body>) -> Option<String> {
    match header(response, "etag") {
        Some(tag) if !tag.starts_with("W/") => Some(tag.to_owned()),
        Some(_) => None,
        None => header(response, "last-modified").map(str::to_owned),
    }
}

/// `Retry-After` in seconds, at most a minute.
fn retry_after(response: &Response<ureq::Body>) -> Option<Duration> {
    let seconds: u64 = header(response, "retry-after")?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(RETRY_AFTER_MAX))
}

/// Maps an error of the request or the response headers.
fn map_error(err: &ureq::Error, host: &str, received: u64) -> FetchError {
    let host = host.to_owned();
    match err {
        ureq::Error::Timeout(Timeout::Resolve | Timeout::Connect) => {
            FetchError::ConnectTimeout { host }
        }
        ureq::Error::Timeout(_) => FetchError::IdleTimeout { received },
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => FetchError::Unreachable {
            host,
            detail: err.to_string(),
        },
        ureq::Error::TooManyRedirects => FetchError::TooManyRedirects,
        ureq::Error::Tls(_) | ureq::Error::Pem(_) | ureq::Error::Rustls(_) => FetchError::Tls {
            host,
            detail: err.to_string(),
        },
        ureq::Error::Io(io) => match io.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                FetchError::IdleTimeout { received }
            }
            // rustls reports a refused certificate or a failed handshake
            // as invalid data, wrapped in the I/O error of the connection.
            io::ErrorKind::InvalidData => FetchError::Tls {
                host,
                detail: io.to_string(),
            },
            io::ErrorKind::ConnectionRefused
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkDown
            | io::ErrorKind::NotFound => FetchError::Unreachable {
                host,
                detail: io.to_string(),
            },
            _ => FetchError::Interrupted { received },
        },
        _ => FetchError::Interrupted { received },
    }
}

/// Maps an error that came while reading the body. `ureq` wraps its own
/// error in an `io::Error`, so that one is looked at first.
fn map_read_error(err: &io::Error, host: &str, received: u64) -> FetchError {
    if let Some(inner) = err
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ureq::Error>())
    {
        return map_error(inner, host, received);
    }
    match err.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => FetchError::IdleTimeout { received },
        _ => FetchError::Interrupted { received },
    }
}

/// Resolves `location` against `base`, both written as text.
fn resolve(base: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_owned();
    }
    let Ok(uri) = base.parse::<Uri>() else {
        return location.to_owned();
    };
    let scheme = uri.scheme_str().unwrap_or("https");
    let authority = uri.authority().map(|a| a.as_str()).unwrap_or_default();
    if let Some(rest) = location.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    if location.starts_with('/') {
        return format!("{scheme}://{authority}{location}");
    }
    let path = uri.path();
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    format!("{scheme}://{authority}{dir}/{location}")
}

#[cfg(feature = "loopback-http")]
fn authority_host(rest: &str) -> &str {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    }
}

#[cfg(feature = "loopback-http")]
fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// Puts an idle timeout on every wait for input of a transport: whatever
/// deadline `ureq` computed, a wait never lasts longer than `idle`.
#[derive(Debug)]
struct IdleConnector {
    idle: Duration,
}

impl Connector<Box<dyn Transport>> for IdleConnector {
    type Out = Box<dyn Transport>;

    fn connect(
        &self,
        _details: &ConnectionDetails,
        chained: Option<Box<dyn Transport>>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| {
            Box::new(IdleTransport {
                inner,
                idle: self.idle,
            }) as Box<dyn Transport>
        }))
    }
}

#[derive(Debug)]
struct IdleTransport {
    inner: Box<dyn Transport>,
    idle: Duration,
}

impl Transport for IdleTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let idle = ureq::unversioned::transport::time::Duration::from(self.idle);
        let timeout = if timeout.after > idle {
            NextTimeout {
                after: idle,
                reason: Timeout::RecvBody,
            }
        } else {
            timeout
        };
        self.inner.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_resolves_against_the_url_it_came_from() {
        let base = "https://example.com/a/b/c.zip?x=1";
        assert_eq!(resolve(base, "https://other.net/z"), "https://other.net/z");
        assert_eq!(resolve(base, "//cdn.net/z"), "https://cdn.net/z");
        assert_eq!(resolve(base, "/root/z"), "https://example.com/root/z");
        assert_eq!(resolve(base, "d.zip"), "https://example.com/a/b/d.zip");
    }
}
