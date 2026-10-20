//! Helpers for the in process server tests: a test root with the daemon
//! lock held, a server started on it, raw frame IO for clients written by
//! hand, wrapped handlers that hold or break one method, captured log
//! lines, and a second process that holds the registry's write lock.

use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use stackly_core::catalog::Catalog;
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_ipc::{Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, SystemClock, UserId};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind, HandshakeAccepted};
use stackly_protocol::{CONTRACT_DIGEST, ErrorBody, ProtocolVersion, Timestamp};
use stackly_testkit::TestRoot;
use stacklyd::catalog::CatalogOverride;
use stacklyd::dispatch::{HandlerFuture, Handlers, RequestContext, get_info};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonConfig, DaemonDeps, DaemonHandle};
use stacklyd::state::DaemonCore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::time::timeout;
use uuid::Uuid;

/// How long a test waits for a frame that should come.
pub const PATIENCE: Duration = Duration::from_secs(5);

/// The config of every in process server: the daemon's defaults with the
/// valid empty test catalog preloaded (spec 0017), so its health is
/// `ready`. A test of a daemon without a catalog sets `catalog` to `None`.
pub fn config() -> DaemonConfig {
    let mut config = DaemonConfig::default();
    config.catalog = Some(CatalogOverride::Preloaded(Catalog::for_tests(&[])));
    config
}

/// One test's daemon site: a test root, its prepared layout, and the
/// daemon lock, which the server's caller must hold.
pub struct Site {
    _root: TestRoot,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    _lock: DaemonLock,
}

impl Site {
    pub fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let platform = MacosPlatform::with_roots(root.roots());
        let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            "0.1.0",
        );
        let lock = DaemonLock::acquire(&prepared.layout().daemon_lock(), owner)
            .expect("the test root's lock is free");
        Self {
            _root: root,
            platform,
            prepared,
            _lock: lock,
        }
    }

    pub fn socket(&self) -> PathBuf {
        self.prepared.layout().daemon_socket()
    }

    pub fn uid(&self) -> u32 {
        match self
            .platform
            .identity()
            .current_user()
            .expect("the current user can be read")
        {
            UserId::Unix(uid) => uid,
            other => panic!("not a unix user: {other:?}"),
        }
    }

    pub fn deps(&self) -> DaemonDeps {
        let log = LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON)
            .expect("the log writer starts");
        let facts = StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            true,
            std::process::id(),
        );
        DaemonDeps::new(
            self.platform.clone(),
            self.prepared.clone(),
            facts,
            Arc::new(log),
        )
        .with_config(config())
    }

    /// Starts a server with the default handlers and waits for the start
    /// slot.
    pub async fn start(&self) -> DaemonHandle {
        let handle = server::start(self.deps()).await.expect("the server starts");
        handle.started().await;
        handle
    }

    /// Starts a server with `handlers` and waits for the start slot.
    pub async fn start_with(&self, handlers: impl Handlers) -> DaemonHandle {
        let handle = server::start(self.deps().with_handlers(handlers))
            .await
            .expect("the server starts");
        handle.started().await;
        handle
    }

    /// Starts a server with `config` and waits for the start slot.
    pub async fn start_configured(&self, config: DaemonConfig) -> DaemonHandle {
        let handle = server::start(self.deps().with_config(config))
            .await
            .expect("the server starts");
        handle.started().await;
        handle
    }

    /// Starts a server whose start slot waits until the returned sender
    /// sends true.
    pub async fn start_held(&self) -> (DaemonHandle, watch::Sender<bool>) {
        let (gate, held) = watch::channel(false);
        let mut config = config();
        config.start_gate = Some(held);
        let handle = server::start(self.deps().with_config(config))
            .await
            .expect("the server starts");
        (handle, gate)
    }
}

/// A connection played by hand, frame by frame.
pub struct Raw {
    pub stream: UnixStream,
}

impl Raw {
    pub async fn connect(socket: &Path) -> Self {
        Self {
            stream: UnixStream::connect(socket)
                .await
                .expect("the daemon socket accepts connections"),
        }
    }

    /// Connects and runs an accepted handshake.
    pub async fn ready(socket: &Path) -> Self {
        let mut raw = Self::connect(socket).await;
        raw.send(&handshake(
            "h",
            1,
            ProtocolVersion::CURRENT.minor,
            CONTRACT_DIGEST,
        ))
        .await;
        let reply = raw.recv().await.expect("the handshake is answered");
        assert_eq!(reply["type"], "response", "{reply}");
        raw
    }

    /// Writes one frame.
    pub async fn send(&mut self, payload: &[u8]) {
        let len = u32::try_from(payload.len()).expect("test payloads fit a frame header");
        let mut frame = len.to_be_bytes().to_vec();
        frame.extend_from_slice(payload);
        self.send_bytes(&frame).await;
    }

    /// Writes bytes as they are, header or not.
    pub async fn send_bytes(&mut self, bytes: &[u8]) {
        self.stream
            .write_all(bytes)
            .await
            .expect("the daemon reads what the test writes");
    }

    /// Reads the next frame as JSON, or `None` at the end of the stream.
    pub async fn recv(&mut self) -> Option<Value> {
        timeout(PATIENCE, self.read_frame())
            .await
            .expect("a frame or the end of the stream arrives in time")
    }

    /// Reads the next frame without a time limit of its own.
    pub async fn read_frame(&mut self) -> Option<Value> {
        let mut header = [0_u8; 4];
        match self.stream.read_exact(&mut header).await {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return None,
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => return None,
            Err(error) => panic!("reading a frame header failed: {error}"),
        }
        let len = u32::from_be_bytes(header) as usize;
        let mut payload = vec![0_u8; len];
        self.stream
            .read_exact(&mut payload)
            .await
            .expect("a frame arrives whole");
        Some(serde_json::from_slice(&payload).expect("the daemon sends JSON"))
    }

    /// Reads frames until the one answering `request_id`.
    pub async fn answer(&mut self, request_id: &str) -> Value {
        loop {
            let frame = self.recv().await.expect("the request is answered");
            if frame["content"]["requestId"] == request_id {
                return frame;
            }
        }
    }

    /// Expects the daemon to close the connection without another frame.
    pub async fn expect_end(&mut self) {
        let frame = self.recv().await;
        assert!(
            frame.is_none(),
            "expected the end of the stream, got {frame:?}"
        );
    }
}

/// A request payload.
pub fn request(id: &str, method: &str, content: Option<Value>) -> Vec<u8> {
    let call = match content {
        Some(content) => json!({ "type": method, "content": content }),
        None => json!({ "type": method }),
    };
    serde_json::to_vec(&json!({ "requestId": id, "call": call })).expect("JSON serializes")
}

/// A handshake request payload.
pub fn handshake(id: &str, major: u32, minor: u32, digest: &str) -> Vec<u8> {
    request(
        id,
        "daemon.handshake",
        Some(json!({
            "protocol": { "major": major, "minor": minor },
            "client": { "kind": "cli", "version": "0.1.0", "pid": std::process::id() },
            "contractDigest": digest,
        })),
    )
}

/// The error in an error frame, checked against the catalog: a known code
/// with the catalog's category and `retryable`, details only from the
/// code's keys, and a message of at most 200 characters (VAL-020).
pub fn error_of(frame: &Value) -> ErrorBody {
    assert_eq!(frame["type"], "error", "{frame}");
    let body: ErrorBody =
        serde_json::from_value(frame["content"]["error"].clone()).expect("the error decodes");
    let spec = body
        .spec()
        .unwrap_or_else(|| panic!("{} is not in the catalog", body.code()));
    assert_eq!(body.category(), spec.category, "{}", body.code());
    assert_eq!(body.retryable(), spec.retryable, "{}", body.code());
    for detail in body.details() {
        assert!(
            spec.details.contains(&detail.key()),
            "{} has detail {}",
            body.code(),
            detail.key()
        );
    }
    assert!(body.message().chars().count() <= 200);
    body
}

/// A typed client of the test's uid, as a `cli` client from before
/// `interactive` existed: it does not send the flag.
pub async fn client(site: &Site) -> (Client, HandshakeAccepted) {
    client_as(site, ClientKind::Cli, None).await
}

/// A typed client of the test's uid with the given kind and
/// `interactive` flag.
pub async fn client_as(
    site: &Site,
    kind: ClientKind,
    interactive: Option<bool>,
) -> (Client, HandshakeAccepted) {
    Client::connect(
        &site.socket(),
        ConnectPolicy::same_user(site.uid()),
        ClientInfo {
            kind,
            version: "0.1.0".to_owned(),
            pid: Some(std::process::id()),
            interactive,
        },
    )
    .await
    .expect("the client connects")
}

thread_local! {
    /// Where the log lines of this thread go while a [`Logs`] lives on it.
    static THREAD_LOGS: RefCell<Option<Arc<Mutex<Vec<u8>>>>> = const { RefCell::new(None) };
}

/// Installs, once per test binary, the global subscriber whose writer sends
/// each line to the [`Logs`] of the thread that logged it. One global
/// subscriber, never replaced, keeps tracing's cached interest and maximum
/// level stable while tests run at the same time.
fn install_thread_logs() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(|| ThreadLogWriter)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("no other global subscriber is set in the server tests");
    });
}

/// Writes to the [`Logs`] of the current thread, or nowhere.
struct ThreadLogWriter;

impl Write for ThreadLogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        THREAD_LOGS.with(|logs| {
            if let Some(buffer) = &*logs.borrow() {
                buffer
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(bytes);
            }
        });
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The JSON log lines of the current thread while it lives. The in process
/// tests run on tokio's current thread runtime, so every task the server
/// spawns logs here; blocking tasks on other threads do not.
pub struct Logs {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl Drop for Logs {
    fn drop(&mut self) {
        THREAD_LOGS.with(|logs| logs.borrow_mut().take());
    }
}

impl Logs {
    /// Captures every line at `debug` and above that this thread logs, as
    /// the daemon writes them.
    pub fn capture() -> Self {
        install_thread_logs();
        let buffer = Arc::new(Mutex::new(Vec::new()));
        THREAD_LOGS.with(|logs| *logs.borrow_mut() = Some(Arc::clone(&buffer)));
        Self { buffer }
    }

    /// Every line captured so far.
    pub fn lines(&self) -> Vec<Value> {
        let bytes = self
            .buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        String::from_utf8_lossy(&bytes)
            .lines()
            .map(|line| serde_json::from_str(line).expect("each log line is JSON"))
            .collect()
    }

    /// The lines whose message is `message`.
    pub fn with_message(&self, message: &str) -> Vec<Value> {
        self.lines()
            .into_iter()
            .filter(|line| line["fields"]["message"] == message)
            .collect()
    }
}

/// Another process that holds the registry's write transaction open, the
/// way a second SQLite tool would: `/usr/bin/sqlite3` with `BEGIN
/// IMMEDIATE` pending. Dropping it ends the process and its transaction.
pub struct RegistryHolder {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl RegistryHolder {
    /// Starts the holder on `db` and returns once its transaction began.
    pub fn lock(db: &Path) -> Self {
        let mut child = Command::new("/usr/bin/sqlite3")
            .arg(db)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the system sqlite3 starts");
        let mut stdin = child.stdin.take().expect("the holder's stdin is piped");
        stdin
            .write_all(b"BEGIN IMMEDIATE;\nSELECT 'locked';\n")
            .expect("the holder reads its commands");
        stdin.flush().expect("the holder reads its commands");
        let stdout = child.stdout.take().expect("the holder's stdout is piped");
        let mut line = String::new();
        BufReader::new(stdout)
            .read_line(&mut line)
            .expect("the holder answers");
        assert_eq!(line.trim(), "locked", "the holder did not take the lock");
        Self {
            child,
            stdin: Some(stdin),
        }
    }

    /// Ends the transaction and the process.
    pub fn release(mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(b"ROLLBACK;\n.quit\n");
        }
        let _ = self.child.wait();
    }
}

impl Drop for RegistryHolder {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A gate a held handler waits behind.
#[derive(Clone)]
pub struct Hold {
    release: watch::Sender<bool>,
    entered: Arc<AtomicUsize>,
}

impl Hold {
    pub fn new() -> Self {
        Self {
            release: watch::Sender::new(false),
            entered: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn release(&self) {
        self.release.send_replace(true);
    }

    pub fn entered(&self) -> usize {
        self.entered.load(Ordering::SeqCst)
    }

    /// Waits until `count` handlers are held.
    pub async fn wait_entered(&self, count: usize) {
        timeout(PATIENCE, async {
            while self.entered() < count {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("the handlers are entered in time");
    }

    async fn wait(&self) {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let mut release = self.release.subscribe();
        let _ = release.wait_for(|released| *released).await;
    }
}

/// `daemon.get_info` waits behind the hold; the rest is the default.
pub struct HoldInfo(pub Hold);

impl Handlers for HoldInfo {
    fn get_info<'a>(
        &'a self,
        core: &'a DaemonCore,
        _context: &'a RequestContext,
    ) -> HandlerFuture<'a, stackly_protocol::daemon::DaemonInfo> {
        Box::pin(async move {
            self.0.wait().await;
            get_info(core).await
        })
    }
}
