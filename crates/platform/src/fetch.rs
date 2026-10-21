//! The download port (spec 0018, "Download dan cache"): an HTTPS GET that
//! streams the body into a sink and can resume with `Range`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::capability::PlatformCapability;
use stackly_domain::Os;

/// The longest [`HttpsUrl`], in bytes.
pub const HTTPS_URL_MAX_LEN: usize = 2048;

/// Why a text is not an [`HttpsUrl`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HttpsUrlError {
    /// The text is empty or longer than [`HTTPS_URL_MAX_LEN`].
    #[error("a url is 1 to 2048 bytes")]
    Length,
    /// The scheme is not `https`.
    #[error("a url must start with https://")]
    NotHttps,
    /// The host part is empty.
    #[error("a url needs a host")]
    EmptyHost,
    /// The text holds userinfo, a space, or a control character.
    #[error("a url holds no userinfo, spaces, or control characters")]
    InvalidCharacter,
}

/// A download URL: 1 to 2048 bytes starting with `https://`, a host that is
/// not empty, and no userinfo, space, or control character.
///
/// ```
/// use stackly_platform::HttpsUrl;
///
/// let url = HttpsUrl::new("https://github.com/nats-io/nats-server/x.zip")?;
/// assert_eq!(url.host(), "github.com");
/// assert!(HttpsUrl::new("http://github.com/x").is_err());
/// assert!(HttpsUrl::new("https://user@github.com/x").is_err());
/// # Ok::<(), stackly_platform::HttpsUrlError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HttpsUrl(String);

impl HttpsUrl {
    /// Checks `text` against the rules above.
    ///
    /// # Errors
    ///
    /// Returns the first [`HttpsUrlError`] the text breaks.
    pub fn new(text: &str) -> Result<Self, HttpsUrlError> {
        if text.is_empty() || text.len() > HTTPS_URL_MAX_LEN {
            return Err(HttpsUrlError::Length);
        }
        let Some(rest) = text.strip_prefix("https://") else {
            return Err(HttpsUrlError::NotHttps);
        };
        if text
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        {
            return Err(HttpsUrlError::InvalidCharacter);
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.contains('@') {
            return Err(HttpsUrlError::InvalidCharacter);
        }
        if authority.is_empty() || authority.starts_with(':') {
            return Err(HttpsUrlError::EmptyHost);
        }
        Ok(Self(text.to_owned()))
    }

    /// Returns the URL text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the host, without port or brackets.
    #[must_use]
    pub fn host(&self) -> &str {
        let rest = &self.0["https://".len()..];
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        match authority.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or_default(),
            None => authority.split(':').next().unwrap_or_default(),
        }
    }
}

impl fmt::Display for HttpsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a download continues from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resume<'a> {
    /// The byte the next response must start at.
    pub offset: u64,
    /// The `ETag` or `Last-Modified` of the earlier response, sent as
    /// `If-Range`.
    pub validator: &'a str,
}

/// One download request.
#[derive(Debug, Clone, Copy)]
pub struct FetchRequest<'a> {
    /// The URL to get.
    pub url: &'a HttpsUrl,
    /// Continue an earlier download, or start from byte 0.
    pub resume: Option<Resume<'a>>,
    /// The size of the whole artifact from the catalog. A `Content-Length`
    /// or `Content-Range` that disagrees is a [`FetchError::LengthMismatch`],
    /// and more bytes than this is [`FetchError::TooLarge`].
    pub size: u64,
    /// Raised to stop at the next chunk.
    pub stop: &'a StopFlag,
}

/// How a response relates to the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    /// The whole artifact, from byte 0 (HTTP 200).
    Full,
    /// A continuation from `offset` (HTTP 206).
    Resumed {
        /// The first byte of the body.
        offset: u64,
    },
}

/// What the server answered, before the first byte of the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    /// Whether the body is the whole artifact or a continuation.
    pub kind: ResponseKind,
    /// The `Content-Length` the server announced, if any.
    pub content_length: Option<u64>,
    /// The strong `ETag`, else the `Last-Modified`, as a validator for the
    /// next resume. A weak `ETag` is not a validator.
    pub validator: Option<String>,
    /// The host that answered, after redirects.
    pub host: String,
}

/// What a finished download received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchSummary {
    /// The bytes written to the sink.
    pub received: u64,
    /// The host that answered, after redirects.
    pub host: String,
}

/// A sink's own failure, such as a full disk, carried through the fetcher
/// without being read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SinkError(pub String);

/// Where a download's bytes go.
pub trait FetchSink {
    /// Called once, before the first byte, with what the server answered. A
    /// sink prepares for a [`ResponseKind::Full`] answer here, for example by
    /// dropping the bytes of an earlier attempt.
    ///
    /// # Errors
    ///
    /// Returns a [`SinkError`] to stop the download.
    fn start(&mut self, response: &FetchResponse) -> Result<(), SinkError>;

    /// Called with each chunk of the body, in order.
    ///
    /// # Errors
    ///
    /// Returns a [`SinkError`] to stop the download.
    fn write(&mut self, chunk: &[u8]) -> Result<(), SinkError>;
}

/// Why a download did not finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The host could not be reached: DNS, no route, or refused.
    #[error("cannot reach {host}: {detail}")]
    Unreachable {
        /// The host.
        host: String,
        /// What the OS said.
        detail: String,
    },
    /// The connection did not open in time.
    #[error("connecting to {host} timed out")]
    ConnectTimeout {
        /// The host.
        host: String,
    },
    /// No byte arrived for the idle timeout.
    #[error("no data for too long after {received} bytes")]
    IdleTimeout {
        /// The bytes received before it stalled.
        received: u64,
    },
    /// The certificate was refused or the handshake failed.
    #[error("tls failed for {host}: {detail}")]
    Tls {
        /// The host.
        host: String,
        /// What went wrong.
        detail: String,
    },
    /// A redirect led to a URL that is not `https`.
    #[error("{host} redirected to a url that is not https")]
    InsecureRedirect {
        /// The host that redirected.
        host: String,
    },
    /// More than 5 redirects.
    #[error("too many redirects")]
    TooManyRedirects,
    /// The server answered with an HTTP status other than 200 or 206.
    #[error("http status {status}")]
    Http {
        /// The status code.
        status: u16,
        /// The `Retry-After` the server sent, if it sent seconds.
        retry_after: Option<Duration>,
    },
    /// `Content-Length` or `Content-Range` disagrees with the catalog size.
    #[error("the server announced {announced} bytes, expected {expected}")]
    LengthMismatch {
        /// What the request expected.
        expected: u64,
        /// What the server announced.
        announced: u64,
    },
    /// The body is longer than the catalog size.
    #[error("the body is longer than the catalog size")]
    TooLarge,
    /// HTTP 416 for a resume request.
    #[error("the server cannot serve that range")]
    RangeNotSatisfiable,
    /// The connection ended before the body did.
    #[error("the connection ended after {received} bytes")]
    Interrupted {
        /// The bytes received before it ended.
        received: u64,
    },
    /// The stop flag was raised.
    #[error("the download was stopped")]
    Stopped,
    /// The sink refused a chunk.
    #[error("the sink failed: {0}")]
    Sink(SinkError),
    /// The platform has no download adapter.
    #[error("{capability} is not supported on {os}")]
    NotSupported {
        /// The capability that was asked for.
        capability: PlatformCapability,
        /// The OS that lacks it.
        os: Os,
    },
}

/// The download port: one HTTPS GET at a time into a [`FetchSink`].
///
/// An implementation follows at most 5 redirects, requires every hop to be
/// `https`, asks for `Accept-Encoding: identity`, never decompresses, and
/// checks the lengths against [`FetchRequest::size`] (spec 0018 AC-5).
pub trait Fetcher: Send + Sync {
    /// Gets `request.url` and streams the body into `sink`.
    ///
    /// # Errors
    ///
    /// Returns a [`FetchError`] naming why the download did not finish.
    fn fetch(
        &self,
        request: &FetchRequest<'_>,
        sink: &mut dyn FetchSink,
    ) -> Result<FetchSummary, FetchError>;
}

/// The fetcher of an OS that has no adapter: every call answers
/// [`FetchError::NotSupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedFetcher {
    os: Os,
}

impl UnsupportedFetcher {
    /// Creates the fetcher for `os`.
    #[must_use]
    pub const fn new(os: Os) -> Self {
        Self { os }
    }
}

impl Fetcher for UnsupportedFetcher {
    fn fetch(
        &self,
        _request: &FetchRequest<'_>,
        _sink: &mut dyn FetchSink,
    ) -> Result<FetchSummary, FetchError> {
        Err(FetchError::NotSupported {
            capability: PlatformCapability::NetworkFetch,
            os: self.os,
        })
    }
}

/// A shared flag that asks a long running step to stop at its next chunk
/// boundary: the download of a `Fetcher` and the runtime extractor check
/// it between chunks (spec 0018). Clones share one flag.
#[derive(Debug, Clone, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    /// A flag that is not raised.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Raises the flag for every clone.
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Returns true once any clone raised the flag.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_https_url_needs_the_scheme_a_host_and_no_userinfo() {
        for ok in [
            "https://github.com/a/b.zip",
            "https://cdn.mysql.com:443/x?y=1#z",
            "https://[::1]:8443/x",
        ] {
            assert!(HttpsUrl::new(ok).is_ok(), "{ok}");
        }
        assert_eq!(
            HttpsUrl::new("https://[::1]:8443/x").map(|u| u.host().to_owned()),
            Ok("::1".to_owned())
        );
        for (bad, error) in [
            ("", HttpsUrlError::Length),
            ("http://github.com/x", HttpsUrlError::NotHttps),
            ("ftp://x/y", HttpsUrlError::NotHttps),
            ("https://", HttpsUrlError::EmptyHost),
            ("https:///x", HttpsUrlError::EmptyHost),
            ("https://u:p@host/x", HttpsUrlError::InvalidCharacter),
            ("https://host/a b", HttpsUrlError::InvalidCharacter),
            ("https://host/a\nb", HttpsUrlError::InvalidCharacter),
        ] {
            assert_eq!(HttpsUrl::new(bad), Err(error), "{bad:?}");
        }
        let long = format!("https://h/{}", "a".repeat(HTTPS_URL_MAX_LEN));
        assert_eq!(HttpsUrl::new(&long), Err(HttpsUrlError::Length));
    }

    #[test]
    fn a_raised_flag_is_seen_by_every_clone() {
        let flag = StopFlag::new();
        let clone = flag.clone();
        assert!(!clone.is_stopped());

        flag.stop();

        assert!(clone.is_stopped());
        assert!(flag.is_stopped());
    }
}
