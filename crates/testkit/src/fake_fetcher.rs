//! A scripted download port for tests (spec 0018): a map from URL to
//! bytes, with a script per URL for the ways a download goes wrong, and a
//! record of every request.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use stackly_platform::{
    FetchError, FetchRequest, FetchResponse, FetchSink, FetchSummary, Fetcher, ResponseKind,
};

/// What one attempt at a URL does, in the order of [`FakeFetcher::script`].
/// When the script runs out, the URL serves its bytes normally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeStep {
    /// Serve the bytes, honoring `Range` and `If-Range`.
    Serve,
    /// Deliver this many bytes of the body, then end the connection.
    InterruptAfter(u64),
    /// Answer 200 with the whole body even when a range was asked for.
    IgnoreRange,
    /// Answer 416 to a resume request.
    RangeNotSatisfiable,
    /// Fail with this error without delivering a byte.
    Fail(FetchError),
    /// Answer with this HTTP status and `Retry-After`.
    Status(u16, Option<Duration>),
    /// Serve this other body instead of the stored one, for this attempt.
    ServeOther(Vec<u8>),
    /// Serve the body with another validator for this attempt.
    ChangeValidator(String),
}

/// One request the fake saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeRequest {
    /// The URL.
    pub url: String,
    /// The offset of the resume, if one was asked for.
    pub resume_offset: Option<u64>,
    /// The `If-Range` validator, if one was sent.
    pub validator: Option<String>,
    /// The size the request announced from the catalog.
    pub size: u64,
}

#[derive(Debug, Default)]
struct Resource {
    body: Vec<u8>,
    validator: Option<String>,
    script: VecDeque<FakeStep>,
}

#[derive(Debug, Default)]
struct State {
    resources: BTreeMap<String, Resource>,
    requests: Vec<FakeRequest>,
    offline: bool,
    chunk: usize,
}

type Hook = Box<dyn FnMut(u64) + Send>;

/// A [`Fetcher`] that serves bytes from memory. Clones share one state.
///
/// ```
/// use stackly_platform::{FetchRequest, FetchSink, Fetcher, HttpsUrl, StopFlag};
/// use stackly_testkit::FakeFetcher;
///
/// struct Collect(Vec<u8>);
/// impl FetchSink for Collect {
///     fn start(&mut self, _: &stackly_platform::FetchResponse) -> Result<(), stackly_platform::SinkError> { Ok(()) }
///     fn write(&mut self, chunk: &[u8]) -> Result<(), stackly_platform::SinkError> { self.0.extend_from_slice(chunk); Ok(()) }
/// }
///
/// let fetcher = FakeFetcher::new();
/// fetcher.serve("https://example.com/a.tgz", b"hello".to_vec());
/// let url = HttpsUrl::new("https://example.com/a.tgz")?;
/// let stop = StopFlag::new();
/// let mut sink = Collect(Vec::new());
/// fetcher.fetch(&FetchRequest { url: &url, resume: None, size: 5, stop: &stop }, &mut sink)?;
/// assert_eq!(sink.0, b"hello");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub struct FakeFetcher {
    state: Arc<Mutex<State>>,
    hook: Arc<Mutex<Option<Hook>>>,
}

impl std::fmt::Debug for FakeFetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeFetcher").finish_non_exhaustive()
    }
}

impl Default for FakeFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeFetcher {
    /// A fetcher that serves nothing: every URL is unreachable.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                chunk: 64 * 1024,
                ..State::default()
            })),
            hook: Arc::new(Mutex::new(None)),
        }
    }

    /// Serves `body` at `url`, with the strong validator `"v1"`.
    pub fn serve(&self, url: &str, body: Vec<u8>) {
        let mut state = self.lock();
        state.resources.insert(
            url.to_owned(),
            Resource {
                body,
                validator: Some("\"v1\"".to_owned()),
                script: VecDeque::new(),
            },
        );
    }

    /// Serves `body` at `url` without any validator, so no download of it
    /// can resume.
    pub fn serve_without_validator(&self, url: &str, body: Vec<u8>) {
        self.serve(url, body);
        if let Some(resource) = self.lock().resources.get_mut(url) {
            resource.validator = None;
        }
    }

    /// Sets what the next attempts at `url` do, one step per attempt.
    pub fn script(&self, url: &str, steps: impl IntoIterator<Item = FakeStep>) {
        if let Some(resource) = self.lock().resources.get_mut(url) {
            resource.script = steps.into_iter().collect();
        }
    }

    /// Makes every URL unreachable, as on a Mac without a network.
    pub fn go_offline(&self) {
        self.lock().offline = true;
    }

    /// Sets how many bytes one chunk holds, so a test can stop a download
    /// part way. The default is 64 KiB.
    pub fn set_chunk(&self, bytes: usize) {
        self.lock().chunk = bytes.max(1);
    }

    /// Calls `hook` with the bytes delivered so far after each chunk, for
    /// example to raise a stop flag at 40%.
    pub fn after_chunk(&self, hook: impl FnMut(u64) + Send + 'static) {
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }

    /// Returns every request seen so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<FakeRequest> {
        self.lock().requests.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Fetcher for FakeFetcher {
    fn fetch(
        &self,
        request: &FetchRequest<'_>,
        sink: &mut dyn FetchSink,
    ) -> Result<FetchSummary, FetchError> {
        let url = request.url.as_str().to_owned();
        let host = request.url.host().to_owned();
        let (body, validator, step, chunk) = {
            let mut state = self.lock();
            state.requests.push(FakeRequest {
                url: url.clone(),
                resume_offset: request.resume.map(|resume| resume.offset),
                validator: request.resume.map(|resume| resume.validator.to_owned()),
                size: request.size,
            });
            let chunk = state.chunk;
            if state.offline {
                return Err(FetchError::Unreachable {
                    host,
                    detail: "the network is down".to_owned(),
                });
            }
            let Some(resource) = state.resources.get_mut(&url) else {
                return Err(FetchError::Unreachable {
                    host,
                    detail: "no such host".to_owned(),
                });
            };
            let step = resource.script.pop_front().unwrap_or(FakeStep::Serve);
            (
                resource.body.clone(),
                resource.validator.clone(),
                step,
                chunk,
            )
        };
        let (body, validator, interrupt, force_full) = match step {
            FakeStep::Serve => (body, validator, None, false),
            FakeStep::InterruptAfter(bytes) => (body, validator, Some(bytes), false),
            FakeStep::IgnoreRange => (body, validator, None, true),
            FakeStep::ServeOther(other) => (other, validator, None, false),
            FakeStep::ChangeValidator(other) => (body, Some(other), None, false),
            FakeStep::RangeNotSatisfiable => {
                if request.resume.is_some() {
                    return Err(FetchError::RangeNotSatisfiable);
                }
                (body, validator, None, false)
            }
            FakeStep::Fail(error) => return Err(error),
            FakeStep::Status(status, retry_after) => {
                return Err(FetchError::Http {
                    status,
                    retry_after,
                });
            }
        };
        let total = body.len() as u64;
        let resume = request
            .resume
            .filter(|resume| !force_full && validator.as_deref() == Some(resume.validator));
        let offset = resume.map_or(0, |resume| resume.offset);
        if offset > total {
            return Err(FetchError::RangeNotSatisfiable);
        }
        let kind = if resume.is_some() {
            ResponseKind::Resumed { offset }
        } else {
            ResponseKind::Full
        };
        let remaining = total - offset;
        let expected = match kind {
            ResponseKind::Full => request.size,
            ResponseKind::Resumed { .. } => request.size.saturating_sub(offset),
        };
        if remaining > expected {
            return Err(FetchError::LengthMismatch {
                expected,
                announced: remaining,
            });
        }
        sink.start(&FetchResponse {
            kind,
            content_length: Some(remaining),
            validator: validator.clone(),
            host: host.clone(),
        })
        .map_err(FetchError::Sink)?;
        let bytes = &body[usize::try_from(offset).unwrap_or(0)..];
        let limit = interrupt.map_or(bytes.len(), |cut| {
            usize::try_from(cut).unwrap_or(usize::MAX).min(bytes.len())
        });
        let mut delivered = 0_u64;
        for piece in bytes[..limit].chunks(chunk) {
            if request.stop.is_stopped() {
                return Err(FetchError::Stopped);
            }
            sink.write(piece).map_err(FetchError::Sink)?;
            delivered += piece.len() as u64;
            if let Some(hook) = self
                .hook
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
            {
                hook(delivered);
            }
        }
        if request.stop.is_stopped() {
            return Err(FetchError::Stopped);
        }
        if interrupt.is_some() && limit < bytes.len() || delivered < expected {
            return Err(FetchError::Interrupted {
                received: delivered,
            });
        }
        Ok(FetchSummary {
            received: delivered,
            host,
        })
    }
}
