//! Downloading an artifact into the cache (spec 0018, *Download dan
//! cache*): a verified entry is used as it is, otherwise the bytes come from
//! the catalog URLs through the [`Fetcher`](stackly_platform::Fetcher) port,
//! into a `.part` file that a later attempt resumes, and only a file whose
//! size and digest match becomes the cache entry `<sha256>`.
//!
//! Everything here is synchronous: the install work runs it on a blocking
//! thread and raises the [`StopFlag`] when the operation is cancelled.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stackly_domain::Sha256Digest;
use stackly_platform::{
    AppendFile, FetchError, FetchRequest, FetchResponse, FetchSink, Filesystem, HttpsUrl, Platform,
    PlatformError, ResponseKind, Resume, SinkError, StopFlag,
};

/// The schema of `<sha256>.part.json`.
const PARTIAL_SCHEMA: &str = "stackly.download-partial/v1";

/// The schema of `<sha256>.json`.
const SOURCE_SCHEMA: &str = "stackly.download-source/v1";

/// Bytes between two `sync` calls of the `.part` file, and between two
/// writes of its sidecar.
pub const SYNC_EVERY: u64 = 8 * 1024 * 1024;

/// Free space kept beyond the bytes still to download.
pub const SPACE_RESERVE: u64 = 64 * 1024 * 1024;

/// Attempts per URL.
pub const ATTEMPTS_PER_URL: usize = 3;

/// The longest the sidecar JSON files may be, in bytes.
const SIDECAR_MAX: u64 = 4096;

/// How the retry pauses and the bounds around them are set. Tests shorten
/// them; the daemon uses [`DownloadPolicy::default`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadPolicy {
    /// The pause before the second and the third attempt on one URL.
    pub backoff: [Duration; 2],
    /// The longest `Retry-After` honored.
    pub retry_after_max: Duration,
    /// Bytes between two syncs of the partial file.
    pub sync_every: u64,
    /// Free space kept beyond the bytes still to download.
    pub space_reserve: u64,
}

impl Default for DownloadPolicy {
    fn default() -> Self {
        Self {
            backoff: [Duration::from_secs(1), Duration::from_secs(4)],
            retry_after_max: Duration::from_secs(60),
            sync_every: SYNC_EVERY,
            space_reserve: SPACE_RESERVE,
        }
    }
}

/// The artifact to bring into the cache, as the catalog states it.
#[derive(Debug, Clone, Copy)]
pub struct DownloadSource<'a> {
    /// The SHA-256 the bytes must have.
    pub sha256: Sha256Digest,
    /// The size the bytes must have.
    pub size: u64,
    /// The candidate URLs, in order of preference.
    pub urls: &'a [HttpsUrl],
}

/// How the cache entry came to be there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// The entry was already in the cache; nothing was fetched.
    Cached,
    /// The entry was fetched.
    Downloaded {
        /// The host that served it.
        host: String,
        /// Whether the final attempt continued an earlier partial file.
        resumed: bool,
    },
}

/// Why the download stopped without a cache entry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DownloadError {
    /// No URL could be reached and there was nothing in the cache.
    #[error("offline: none of the download hosts could be reached")]
    Offline {
        /// The hosts that were tried.
        hosts: Vec<String>,
    },
    /// The server or the connection failed in a way that is not offline.
    #[error("the download from {host} failed: {reason}")]
    Failed {
        /// The last host tried.
        host: String,
        /// The HTTP status, when the failure was one.
        status: Option<u16>,
        /// A short reason.
        reason: String,
    },
    /// TLS failed or a redirect left `https`.
    #[error("the download from {host} is not trusted: {reason}")]
    Untrusted {
        /// The host.
        host: String,
        /// A short reason.
        reason: String,
    },
    /// The server announced or sent a different length than the catalog.
    #[error("the download is longer than expected: {received} bytes for {expected}")]
    TooLarge {
        /// The size from the catalog.
        expected: u64,
        /// The size announced or received.
        received: u64,
    },
    /// The SHA-256 of the bytes is not the catalog's.
    #[error("the digest of the download differs from the catalog")]
    DigestMismatch {
        /// The digest from the catalog.
        expected: Sha256Digest,
        /// The digest of the bytes.
        actual: Sha256Digest,
    },
    /// The size of the bytes is not the catalog's.
    #[error("the size of the download is {actual} bytes, expected {expected}")]
    SizeMismatch {
        /// The size from the catalog.
        expected: u64,
        /// The size of the bytes.
        actual: u64,
    },
    /// The cache volume lacks the room for the rest of the download.
    #[error("the cache volume has {available} bytes free, {needed} are needed")]
    InsufficientSpace {
        /// What the download needs.
        needed: u64,
        /// What the volume has.
        available: u64,
    },
    /// A filesystem call on the cache failed.
    #[error("{op} failed in the download cache: {detail}")]
    Io {
        /// The step that failed.
        op: &'static str,
        /// What the platform said.
        detail: String,
    },
    /// The stop flag was raised.
    #[error("the download was stopped")]
    Stopped,
}

/// The `.part.json` sidecar.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Partial {
    schema: String,
    url: String,
    validator: Option<String>,
    bytes: u64,
    updated_at: String,
}

/// The `<sha256>.json` file that names where the cache entry came from.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceNote {
    schema: String,
    url: String,
    fetched_at: String,
}

/// The paths of one cache entry.
#[derive(Debug, Clone)]
pub struct CachePaths {
    /// `<dir>/<sha256>`.
    pub entry: PathBuf,
    /// `<dir>/<sha256>.part`.
    pub part: PathBuf,
    /// `<dir>/<sha256>.part.json`.
    pub part_note: PathBuf,
    /// `<dir>/<sha256>.json`.
    pub source_note: PathBuf,
}

impl CachePaths {
    /// The paths of the entry for `sha256` in `downloads_dir`.
    #[must_use]
    pub fn new(downloads_dir: &Path, sha256: &Sha256Digest) -> Self {
        let name = sha256.to_string();
        Self {
            entry: downloads_dir.join(&name),
            part: downloads_dir.join(format!("{name}.part")),
            part_note: downloads_dir.join(format!("{name}.part.json")),
            source_note: downloads_dir.join(format!("{name}.json")),
        }
    }
}

/// What [`download`] needs besides the source.
pub struct DownloadContext<'a> {
    /// The platform ports.
    pub platform: &'a Platform,
    /// The cache folder, `DirectoryLayout::downloads_dir()`.
    pub downloads_dir: &'a Path,
    /// Raised to stop at the next chunk or the next pause.
    pub stop: &'a StopFlag,
    /// The retry settings.
    pub policy: &'a DownloadPolicy,
}

/// Brings the artifact into the cache and returns how.
///
/// A cache entry that exists as a regular file is used as it is, without
/// the network; `verify_digest` recomputes its digest next. Otherwise the
/// space is checked, then each URL gets up to 3 attempts with the pauses of
/// the policy. `progress` is called with the bytes in the partial file and
/// the artifact size after every chunk.
///
/// # Errors
///
/// Returns the [`DownloadError`] the spec's classification gives: offline,
/// untrusted, too large, or failed; digest and size mismatch of the bytes;
/// insufficient space; a cache I/O failure; or stopped.
pub fn download(
    ctx: &DownloadContext<'_>,
    source: &DownloadSource<'_>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<DownloadOutcome, DownloadError> {
    let fs = ctx.platform.fs();
    let paths = CachePaths::new(ctx.downloads_dir, &source.sha256);
    if fs
        .symlink_metadata(&paths.entry)
        .map_err(|err| io("inspect the cache entry", &err))?
        .is_some_and(|info| info.kind == stackly_platform::FileKind::File)
    {
        progress(source.size, source.size);
        return Ok(DownloadOutcome::Cached);
    }
    check_space(ctx, source, &paths)?;
    let mut failures = Failures::default();
    for url in source.urls {
        for attempt in 0..ATTEMPTS_PER_URL {
            if ctx.stop.is_stopped() {
                return Err(DownloadError::Stopped);
            }
            match attempt_once(ctx, source, &paths, url, progress) {
                Ok(outcome) => return Ok(outcome),
                Err(Attempt::Final(error)) => return Err(error),
                Err(Attempt::Fetch(error)) => {
                    let wait = match &error {
                        FetchError::Interrupted { .. }
                        | FetchError::IdleTimeout { .. }
                        | FetchError::ConnectTimeout { .. } => retry_wait(ctx, attempt, None),
                        FetchError::Http {
                            status: 408 | 429 | 500..=599,
                            retry_after,
                        } => retry_wait(ctx, attempt, *retry_after),
                        FetchError::RangeNotSatisfiable => {
                            discard_part(fs, &paths)?;
                            retry_wait(ctx, attempt, Some(Duration::ZERO))
                        }
                        _ => None,
                    };
                    failures.note(url.host(), &error);
                    match wait {
                        Some(wait) if attempt + 1 < ATTEMPTS_PER_URL => {
                            pause(ctx.stop, wait)?;
                        }
                        _ => break,
                    }
                }
            }
        }
    }
    Err(failures.into_error())
}

/// What one attempt ended with.
enum Attempt {
    /// The fetch failed; the caller decides whether to retry.
    Fetch(FetchError),
    /// The download cannot go on, whatever the URL.
    Final(DownloadError),
}

impl From<DownloadError> for Attempt {
    fn from(error: DownloadError) -> Self {
        Self::Final(error)
    }
}

/// One fetch of `url`, resuming the partial file when there is one.
fn attempt_once(
    ctx: &DownloadContext<'_>,
    source: &DownloadSource<'_>,
    paths: &CachePaths,
    url: &HttpsUrl,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<DownloadOutcome, Attempt> {
    let fs = ctx.platform.fs();
    let mut part = fs
        .open_append(&paths.part)
        .map_err(|err| io("open the partial file", &err))?;
    let note = read_partial(fs, paths, url);
    let kept = note.as_ref().map_or(0, |note| note.bytes.min(part.len()));
    if part.len() != kept {
        part.truncate(kept)
            .map_err(|err| io("truncate the partial file", &err))?;
    }
    let mut hasher = Sha256::new();
    hash_prefix(fs, &paths.part, kept, &mut hasher)?;
    let validator = note.and_then(|note| note.validator);
    let resume = match (&validator, kept) {
        (Some(validator), offset) if offset > 0 && offset < source.size => Some(Resume {
            offset,
            validator: validator.as_str(),
        }),
        _ => None,
    };
    if resume.is_none() && kept > 0 {
        part.truncate(0)
            .map_err(|err| io("truncate the partial file", &err))?;
        hasher = Sha256::new();
    }
    let started_at = resume.map_or(0, |resume| resume.offset);
    let mut sink = PartSink {
        part,
        hasher,
        written: started_at,
        total: source.size,
        policy: ctx.policy,
        since_sync: 0,
        fs,
        paths,
        url,
        validator: validator.clone(),
        progress,
        failed: None,
        resumed: false,
        started_at,
    };
    let request = FetchRequest {
        url,
        resume,
        size: source.size,
        stop: ctx.stop,
    };
    let result = ctx.platform.fetcher().fetch(&request, &mut sink);
    // What reached the file stays, so the next attempt resumes there.
    let flush = sink.checkpoint();
    if let Some(error) = sink.failed.take() {
        return Err(error.into());
    }
    let summary = match result {
        Ok(summary) => summary,
        Err(FetchError::Stopped) => return Err(DownloadError::Stopped.into()),
        Err(FetchError::Sink(error)) => {
            return Err(DownloadError::Io {
                op: "write the partial file",
                detail: error.0,
            }
            .into());
        }
        Err(error) => {
            flush.map_err(Attempt::from)?;
            return Err(Attempt::Fetch(error));
        }
    };
    flush.map_err(Attempt::from)?;
    let resumed = sink.resumed;
    let actual_size = sink.written;
    let digest = Sha256Digest::from_bytes(sink.hasher.finalize_reset().into());
    drop(sink);
    finish_entry(
        ctx,
        source,
        paths,
        url,
        actual_size,
        digest,
        resumed,
        &summary.host,
        started_at,
    )
}

/// Checks the finished partial file and makes it the cache entry.
#[expect(
    clippy::too_many_arguments,
    reason = "one step that needs every fact of the finished download"
)]
fn finish_entry(
    ctx: &DownloadContext<'_>,
    source: &DownloadSource<'_>,
    paths: &CachePaths,
    url: &HttpsUrl,
    actual_size: u64,
    digest: Sha256Digest,
    resumed: bool,
    host: &str,
    started_at: u64,
) -> Result<DownloadOutcome, Attempt> {
    let fs = ctx.platform.fs();
    let wrong = if actual_size != source.size {
        Some(DownloadError::SizeMismatch {
            expected: source.size,
            actual: actual_size,
        })
    } else if digest != source.sha256 {
        Some(DownloadError::DigestMismatch {
            expected: source.sha256,
            actual: digest,
        })
    } else {
        None
    };
    if let Some(error) = wrong {
        discard_part(fs, paths)?;
        if started_at > 0 || resumed {
            // The earlier bytes may be what is wrong: start over once.
            return Err(Attempt::Fetch(FetchError::RangeNotSatisfiable));
        }
        return Err(error.into());
    }
    let note = SourceNote {
        schema: SOURCE_SCHEMA.to_owned(),
        url: url.as_str().to_owned(),
        fetched_at: ctx.platform.clock().now().to_string(),
    };
    let text = serde_json::to_vec(&note).map_err(|err| DownloadError::Io {
        op: "write the source note",
        detail: err.to_string(),
    })?;
    fs.write_file_atomic(&paths.source_note, &text)
        .map_err(|err| io("write the source note", &err))?;
    match fs.rename_no_replace(&paths.part, &paths.entry) {
        Ok(()) => {}
        Err(PlatformError::AlreadyExists { .. }) => discard_part(fs, paths)?,
        Err(err) => return Err(io("rename the partial file", &err).into()),
    }
    remove_file(fs, &paths.part_note)?;
    Ok(DownloadOutcome::Downloaded {
        host: host.to_owned(),
        resumed,
    })
}

/// Receives the chunks: writes them to the partial file and the hash, and
/// keeps the sidecar current.
struct PartSink<'a> {
    part: Box<dyn AppendFile>,
    hasher: Sha256,
    written: u64,
    total: u64,
    policy: &'a DownloadPolicy,
    since_sync: u64,
    fs: &'a dyn Filesystem,
    paths: &'a CachePaths,
    url: &'a HttpsUrl,
    validator: Option<String>,
    progress: &'a mut dyn FnMut(u64, u64),
    failed: Option<DownloadError>,
    resumed: bool,
    started_at: u64,
}

impl PartSink<'_> {
    /// Syncs the partial file and rewrites the sidecar to say how many
    /// bytes are safe.
    fn checkpoint(&mut self) -> Result<(), DownloadError> {
        self.part
            .sync()
            .map_err(|err| io("sync the partial file", &err))?;
        let note = Partial {
            schema: PARTIAL_SCHEMA.to_owned(),
            url: self.url.as_str().to_owned(),
            validator: self.validator.clone(),
            bytes: self.written,
            updated_at: String::new(),
        };
        let text = serde_json::to_vec(&note).map_err(|err| DownloadError::Io {
            op: "write the partial note",
            detail: err.to_string(),
        })?;
        self.fs
            .write_file_atomic(&self.paths.part_note, &text)
            .map_err(|err| io("write the partial note", &err))?;
        self.since_sync = 0;
        Ok(())
    }

    fn fail(&mut self, error: DownloadError) -> SinkError {
        let text = error.to_string();
        self.failed = Some(error);
        SinkError(text)
    }
}

impl FetchSink for PartSink<'_> {
    fn start(&mut self, response: &FetchResponse) -> Result<(), SinkError> {
        match response.kind {
            ResponseKind::Resumed { offset } if offset == self.started_at => {
                self.resumed = true;
            }
            ResponseKind::Resumed { .. } => {
                return Err(self.fail(DownloadError::Io {
                    op: "resume the download",
                    detail: "the server resumed at another offset".to_owned(),
                }));
            }
            ResponseKind::Full => {
                // The server ignored the range: start from byte 0.
                if self.written > 0 {
                    if let Err(err) = self.part.truncate(0) {
                        return Err(self.fail(io("truncate the partial file", &err)));
                    }
                    self.hasher = Sha256::new();
                    self.written = 0;
                    self.started_at = 0;
                }
            }
        }
        self.validator = response.validator.clone();
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), SinkError> {
        if let Err(err) = std::io::Write::write_all(&mut self.part, chunk) {
            let error = if err.kind() == std::io::ErrorKind::StorageFull {
                DownloadError::InsufficientSpace {
                    needed: chunk.len() as u64,
                    available: 0,
                }
            } else {
                DownloadError::Io {
                    op: "write the partial file",
                    detail: err.to_string(),
                }
            };
            return Err(self.fail(error));
        }
        self.hasher.update(chunk);
        self.written += chunk.len() as u64;
        self.since_sync += chunk.len() as u64;
        (self.progress)(self.written, self.total);
        if self.since_sync >= self.policy.sync_every
            && let Err(error) = self.checkpoint()
        {
            return Err(self.fail(error));
        }
        Ok(())
    }
}

/// Hashes the first `len` bytes of the partial file into `hasher`.
fn hash_prefix(
    fs: &dyn Filesystem,
    path: &Path,
    len: u64,
    hasher: &mut Sha256,
) -> Result<(), DownloadError> {
    if len == 0 {
        return Ok(());
    }
    let mut file = fs
        .open_read(path)
        .map_err(|err| io("read the partial file", &err))?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut left = len;
    while left > 0 {
        let want = usize::try_from(left.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = std::io::Read::read(&mut file, &mut buffer[..want]).map_err(|err| {
            DownloadError::Io {
                op: "read the partial file",
                detail: err.to_string(),
            }
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        left -= read as u64;
    }
    Ok(())
}

/// Reads the sidecar, and returns it only when it belongs to `url`.
fn read_partial(fs: &dyn Filesystem, paths: &CachePaths, url: &HttpsUrl) -> Option<Partial> {
    let bytes = fs.read_file_limited(&paths.part_note, SIDECAR_MAX).ok()?;
    let note: Partial = serde_json::from_slice(&bytes).ok()?;
    (note.schema == PARTIAL_SCHEMA && note.url == url.as_str()).then_some(note)
}

/// Removes the partial file and its sidecar.
fn discard_part(fs: &dyn Filesystem, paths: &CachePaths) -> Result<(), DownloadError> {
    remove_file(fs, &paths.part)?;
    remove_file(fs, &paths.part_note)
}

/// Removes one file of the cache folder, if it exists.
fn remove_file(fs: &dyn Filesystem, path: &Path) -> Result<(), DownloadError> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(());
    };
    fs.remove_tree(parent, Path::new(name))
        .map(|_| ())
        .map_err(|err| io("remove a cache file", &err))
}

/// Fails before the download when the volume cannot hold the rest.
fn check_space(
    ctx: &DownloadContext<'_>,
    source: &DownloadSource<'_>,
    paths: &CachePaths,
) -> Result<(), DownloadError> {
    let fs = ctx.platform.fs();
    let have = fs
        .symlink_metadata(&paths.part)
        .map_err(|err| io("inspect the partial file", &err))?
        .map_or(0, |info| info.len);
    let needed = source
        .size
        .saturating_sub(have)
        .saturating_add(ctx.policy.space_reserve);
    let available = fs
        .available_space(ctx.downloads_dir)
        .map_err(|err| io("read the free space", &err))?;
    if available < needed {
        return Err(DownloadError::InsufficientSpace { needed, available });
    }
    Ok(())
}

/// The pause before the next attempt, or `None` when the error is not one a
/// retry on the same URL helps.
fn retry_wait(
    ctx: &DownloadContext<'_>,
    attempt: usize,
    retry_after: Option<Duration>,
) -> Option<Duration> {
    let base = ctx.policy.backoff.get(attempt).copied()?;
    Some(retry_after.map_or(base, |wait| wait.min(ctx.policy.retry_after_max)))
}

/// Waits for `wait`, in short steps so a stop is seen at once.
fn pause(stop: &StopFlag, wait: Duration) -> Result<(), DownloadError> {
    let deadline = Instant::now() + wait;
    loop {
        if stop.is_stopped() {
            return Err(DownloadError::Stopped);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(Duration::from_millis(25)));
    }
}

fn io(op: &'static str, err: &PlatformError) -> DownloadError {
    match err {
        PlatformError::NoSpace { .. } => DownloadError::InsufficientSpace {
            needed: 0,
            available: 0,
        },
        _ => DownloadError::Io {
            op,
            detail: err.to_string(),
        },
    }
}

/// What every attempt of every URL ended with, to choose the final error.
#[derive(Default)]
struct Failures {
    hosts: Vec<String>,
    offline: usize,
    total: usize,
    untrusted: Option<(String, String)>,
    too_large: Option<(u64, u64)>,
    last: Option<(String, Option<u16>, String)>,
}

impl Failures {
    fn note(&mut self, host: &str, error: &FetchError) {
        if !self.hosts.iter().any(|known| known == host) {
            self.hosts.push(host.to_owned());
        }
        self.total += 1;
        match error {
            FetchError::Unreachable { .. } | FetchError::ConnectTimeout { .. } => {
                self.offline += 1;
            }
            FetchError::Tls { detail, .. } => {
                self.untrusted
                    .get_or_insert_with(|| (host.to_owned(), format!("tls: {detail}")));
            }
            FetchError::InsecureRedirect { .. } => {
                self.untrusted
                    .get_or_insert_with(|| (host.to_owned(), "redirect to http".to_owned()));
            }
            FetchError::TooManyRedirects => {
                self.untrusted
                    .get_or_insert_with(|| (host.to_owned(), "too many redirects".to_owned()));
            }
            FetchError::TooLarge => {
                self.too_large.get_or_insert((0, 0));
            }
            FetchError::LengthMismatch {
                expected,
                announced,
            } => {
                self.too_large.get_or_insert((*expected, *announced));
            }
            _ => {}
        }
        let (status, reason) = match error {
            FetchError::Http { status, .. } => (Some(*status), format!("http status {status}")),
            other => (None, other.to_string()),
        };
        self.last = Some((host.to_owned(), status, reason));
    }

    /// The final error: the first rule that fits, in the order of the spec.
    fn into_error(self) -> DownloadError {
        if self.total > 0 && self.offline == self.total {
            return DownloadError::Offline { hosts: self.hosts };
        }
        if let Some((host, reason)) = self.untrusted {
            return DownloadError::Untrusted { host, reason };
        }
        if let Some((expected, received)) = self.too_large {
            return DownloadError::TooLarge { expected, received };
        }
        let (host, status, reason) = self
            .last
            .unwrap_or_else(|| (String::new(), None, "no download url".to_owned()));
        DownloadError::Failed {
            host,
            status,
            reason,
        }
    }
}
