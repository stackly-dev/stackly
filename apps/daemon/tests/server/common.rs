//! Helpers for the in process server tests: a test root with the daemon
//! lock held, a server started on it, raw frame IO for clients written by
//! hand, and wrapped handlers that hold or break one method.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_ipc::{Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, SystemClock, UserId};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind, HandshakeAccepted};
use stackly_protocol::{CONTRACT_DIGEST, ErrorBody, ProtocolVersion, Timestamp};
use stackly_testkit::TestRoot;
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

    /// Starts a server whose start slot waits until the returned sender
    /// sends true.
    pub async fn start_held(&self) -> (DaemonHandle, watch::Sender<bool>) {
        let (gate, held) = watch::channel(false);
        let mut config = DaemonConfig::default();
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

/// A typed client of the test's uid.
pub async fn client(site: &Site) -> (Client, HandshakeAccepted) {
    Client::connect(
        &site.socket(),
        ConnectPolicy::same_user(site.uid()),
        ClientInfo {
            kind: ClientKind::Cli,
            version: "0.1.0".to_owned(),
            pid: Some(std::process::id()),
        },
    )
    .await
    .expect("the client connects")
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
