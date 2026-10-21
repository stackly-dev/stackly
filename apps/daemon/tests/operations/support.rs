//! Helpers for the operation tests: a test root with the daemon lock, a
//! daemon started in process with the test's starting method, a raw
//! connection for that method, a typed client for the contract methods, and
//! a capture of every log line this test binary writes.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, Once, OnceLock, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::time::Duration;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use stackly_core::operation::testing::{ScriptedFactory, ScriptedWork};
use stackly_core::operation::{BeginRequest, BusyPolicy, OperationEngine, Origin, RebuildError};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{InstanceId, JournalPayload, OperationKind, OperationResource};
use stackly_ipc::{Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, SystemClock, UserId};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::codes::request;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::operation::{
    GetOperationParams, ListOperationsParams, OperationAccepted, OperationDetail, OperationState,
    OperationSummary,
};
use stackly_protocol::{CONTRACT_DIGEST, Call, ErrorBody, ProtocolVersion, Reply, Timestamp};
use stackly_testkit::TestRoot;
use stacklyd::dispatch::Handlers;
use stacklyd::ipc::operation::{TestStart, TestStartingMethod};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps, DaemonHandle};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::timeout;
use uuid::Uuid;

/// How long a test waits for something that should happen.
pub const PATIENCE: Duration = Duration::from_secs(15);
/// The test's method that starts an operation; never a contract method.
pub const METHOD: &str = "test.start_operation";
/// The params key whose value plays a secret: the method keeps it out of
/// the fingerprint and the journal input, as a real handler does with a
/// `SecretText`.
pub const SECRET_PARAM: &str = "password";

/// One script the test's method starts: a scripted work on the manager,
/// or on a new instance ID when `creates` is set (so scripts can run side
/// by side under separate lock keys).
#[derive(Clone)]
pub struct Script {
    /// The operation kind.
    pub kind: OperationKind,
    /// Whether the operation names a new instance ID instead of the
    /// manager.
    pub creates: bool,
    /// The work.
    pub work: ScriptedWork,
    /// What a resume or a restart rebuilds; scripts with one get a factory
    /// for their kind when the daemon starts.
    pub rebuild: Option<ScriptedWork>,
}

impl Script {
    /// A script of `kind` on the manager with no factory.
    pub fn new(kind: OperationKind, work: ScriptedWork) -> Self {
        Self {
            kind,
            creates: false,
            work,
            rebuild: None,
        }
    }

    /// Names a new instance ID instead of the manager.
    pub fn creating(mut self) -> Self {
        self.creates = true;
        self
    }

    /// Rebuilds `work` on a resume or a restart.
    pub fn rebuilt_as(mut self, work: ScriptedWork) -> Self {
        self.rebuild = Some(work);
        self
    }
}

/// The test's starting method: scripts by name, started through
/// `start_operation` like a real method would.
#[derive(Clone, Default)]
pub struct Scripts {
    scripts: Arc<Mutex<HashMap<String, Script>>>,
}

impl Scripts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a script; factories are only registered for scripts added
    /// before the daemon starts.
    pub fn add(&self, name: &str, script: Script) {
        self.lock().insert(name.to_owned(), script);
    }

    fn get(&self, name: &str) -> Option<Script> {
        self.lock().get(name).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Script>> {
        self.scripts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl TestStartingMethod for Scripts {
    fn method(&self) -> &'static str {
        METHOD
    }

    fn register(&self, engine: &OperationEngine) {
        let kinds: Vec<OperationKind> = self
            .lock()
            .values()
            .filter(|script| script.rebuild.is_some())
            .map(|script| script.kind)
            .collect();
        for kind in kinds {
            let scripts = self.clone();
            engine.register_factory(
                kind,
                Arc::new(ScriptedFactory::new(move |_, journal| {
                    let input: Value = serde_json::from_str(journal.journal.input().as_str())
                        .map_err(|_| RebuildError::InvalidInput)?;
                    input["script"]
                        .as_str()
                        .and_then(|name| scripts.get(name))
                        .and_then(|script| script.rebuild)
                        .ok_or(RebuildError::InvalidInput)
                })),
            );
        }
    }

    fn prepare(&self, params: Option<&Value>) -> Result<TestStart, Box<ErrorBody>> {
        let invalid = || {
            Box::new(
                ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
                    .with_detail("method", METHOD),
            )
        };
        let params = params.ok_or_else(invalid)?;
        let name = params["script"].as_str().ok_or_else(invalid)?.to_owned();
        let script = self.get(&name).ok_or_else(invalid)?;
        let fingerprint = fingerprint(params);
        let resource = if script.creates {
            OperationResource::Instance(InstanceId::generate())
        } else {
            OperationResource::Manager
        };
        let input = JournalPayload::new(json!({ "script": name }).to_string(), 16_384)
            .expect("a script name fits the journal input");
        Ok(TestStart {
            fingerprint,
            build: Box::new(move |origin: Origin| BeginRequest {
                kind: script.kind,
                resource,
                creates_resource: script.creates,
                extra_locks: Vec::new(),
                busy: BusyPolicy::Queue,
                expected_revision: None,
                origin,
                input,
                work: script.work.boxed(),
                precheck: None,
            }),
        })
    }
}

/// SHA-256 over `<method>\n<params with sorted keys>`, with the secret
/// param written as a fixed marker, as `request_fingerprint` does.
fn fingerprint(params: &Value) -> [u8; 32] {
    let mut params = params.clone();
    if let Some(secret) = params.get_mut(SECRET_PARAM) {
        *secret = Value::String("[secret]".to_owned());
    }
    let mut hasher = Sha256::new();
    hasher.update(METHOD.as_bytes());
    hasher.update(b"\n");
    hasher.update(serde_json::to_vec(&sorted(params)).expect("JSON serializes"));
    hasher.finalize().into()
}

fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// The default handlers plus the test's starting method.
struct WithScripts(Scripts);

impl Handlers for WithScripts {
    fn test_starting_method(&self) -> Option<Arc<dyn TestStartingMethod>> {
        Some(Arc::new(self.0.clone()))
    }
}

/// Turns of the tests in this binary: every test takes a shared turn, and
/// a test that times the registry takes the whole binary, so the fsyncs of
/// tests running beside it do not count in its numbers.
static TURNS: RwLock<()> = RwLock::new(());

/// A test's turn, held for as long as its site lives.
#[allow(dead_code, reason = "a turn is held, never read")]
enum Turn {
    Shared(RwLockReadGuard<'static, ()>),
    Exclusive(RwLockWriteGuard<'static, ()>),
}

/// One test's daemon site: a test root, its prepared layout, and the
/// daemon lock, which the server's caller must hold.
pub struct Site {
    _root: TestRoot,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    _lock: DaemonLock,
    _turn: Turn,
}

impl Site {
    /// A site that runs beside the other tests of this binary.
    pub fn new() -> Self {
        Self::with_turn(Turn::Shared(
            TURNS.read().unwrap_or_else(PoisonError::into_inner),
        ))
    }

    /// A site that waits until no other test of this binary runs, for a
    /// test that times the registry.
    pub fn exclusive() -> Self {
        Self::with_turn(Turn::Exclusive(
            TURNS.write().unwrap_or_else(PoisonError::into_inner),
        ))
    }

    fn with_turn(turn: Turn) -> Self {
        install_logs();
        let root = TestRoot::new().expect("a test root can be created");
        let platform =
            MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version");
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
            _turn: turn,
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

    pub fn deps(&self, scripts: &Scripts) -> DaemonDeps {
        self.deps_with(WithScripts(scripts.clone()))
    }

    /// The deps of a daemon with `handlers`.
    pub fn deps_with(&self, handlers: impl Handlers) -> DaemonDeps {
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
        .with_handlers(handlers)
    }

    /// Starts a daemon serving `scripts` and waits for its start slot.
    pub async fn start(&self, scripts: &Scripts) -> DaemonHandle {
        let handle = server::start(self.deps(scripts))
            .await
            .expect("the server starts");
        handle.started().await;
        handle
    }

    /// Every file of the registry: the database, its WAL, and its shared
    /// memory index.
    pub fn registry_files(&self) -> Vec<PathBuf> {
        let dir = self.prepared.layout().registry_dir();
        std::fs::read_dir(&dir)
            .expect("the registry folder can be read")
            .map(|entry| entry.expect("a registry folder entry").path())
            .filter(|path| path.is_file())
            .collect()
    }

    /// The root folder of an instance.
    pub fn instance_root(&self, id: &str) -> PathBuf {
        let id: InstanceId = id.parse().expect("an instance ID");
        self.prepared.layout().instance_paths(id).root
    }
}

/// A typed client of the test's uid, as an interactive CLI.
pub async fn client(site: &Site) -> Client {
    Client::connect(
        &site.socket(),
        ConnectPolicy::same_user(site.uid()),
        ClientInfo {
            kind: ClientKind::Cli,
            version: "0.1.0".to_owned(),
            pid: Some(std::process::id()),
            interactive: Some(true),
        },
    )
    .await
    .expect("the client connects")
    .0
}

/// A connection played by hand, for the test's starting method.
pub struct Raw {
    stream: UnixStream,
}

impl Raw {
    /// Connects and runs an accepted handshake as a GUI client.
    pub async fn ready(socket: &Path) -> Self {
        let mut raw = Self {
            stream: UnixStream::connect(socket)
                .await
                .expect("the daemon socket accepts connections"),
        };
        let handshake = json!({
            "requestId": "h",
            "call": {
                "type": "daemon.handshake",
                "content": {
                    "protocol": { "major": ProtocolVersion::CURRENT.major, "minor": ProtocolVersion::CURRENT.minor },
                    "client": { "kind": "gui", "version": "0.1.0", "pid": std::process::id(), "interactive": true },
                    "contractDigest": CONTRACT_DIGEST,
                },
            },
        });
        raw.send(&serde_json::to_vec(&handshake).expect("JSON serializes"))
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
        self.stream
            .write_all(&frame)
            .await
            .expect("the daemon reads what the test writes");
    }

    /// Reads the next frame as JSON, or `None` at the end of the stream.
    pub async fn recv(&mut self) -> Option<Value> {
        timeout(PATIENCE, async {
            let mut header = [0_u8; 4];
            match self.stream.read_exact(&mut header).await {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return None,
                Err(error) if error.kind() == io::ErrorKind::ConnectionReset => return None,
                Err(error) => panic!("reading a frame header failed: {error}"),
            }
            let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
            self.stream
                .read_exact(&mut payload)
                .await
                .expect("a frame arrives whole");
            Some(serde_json::from_slice(&payload).expect("the daemon sends JSON"))
        })
        .await
        .expect("a frame or the end of the stream arrives in time")
    }

    /// Sends the test's starting method and returns the frame that answers
    /// it.
    pub async fn start(&mut self, key: Option<&str>, params: Value) -> Value {
        let request_id = Uuid::now_v7().to_string();
        self.send(&start_request(&request_id, key, params)).await;
        loop {
            let frame = self.recv().await.expect("the request is answered");
            if frame["content"]["requestId"] == request_id.as_str() {
                return frame;
            }
        }
    }
}

/// A request of the test's starting method.
pub fn start_request(request_id: &str, key: Option<&str>, params: Value) -> Vec<u8> {
    let mut request = json!({
        "requestId": request_id,
        "call": { "type": METHOD, "content": params },
    });
    if let Some(key) = key {
        request["idempotencyKey"] = Value::String(key.to_owned());
    }
    serde_json::to_vec(&request).expect("JSON serializes")
}

/// A fresh idempotency key, as the CLI makes one.
pub fn key() -> String {
    Uuid::now_v7().to_string()
}

/// The accepted operation in a response frame of the starting method.
pub fn accepted(frame: &Value) -> OperationAccepted {
    assert_eq!(frame["type"], "response", "{frame}");
    assert_eq!(frame["content"]["reply"]["type"], METHOD, "{frame}");
    serde_json::from_value(frame["content"]["reply"]["content"].clone())
        .expect("the reply is an OperationAccepted")
}

/// The error in an error frame, checked against the catalog: a known code
/// with the catalog's category and `retryable`, and details only from the
/// code's keys (VAL-020).
pub fn error_of(frame: &Value) -> ErrorBody {
    assert_eq!(frame["type"], "error", "{frame}");
    let body: ErrorBody =
        serde_json::from_value(frame["content"]["error"].clone()).expect("the error decodes");
    checked(&body);
    body
}

/// Checks a typed client's error against the catalog, as [`error_of`].
pub fn checked(body: &ErrorBody) {
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
}

/// `operation.get`.
pub async fn get(client: &Client, id: Uuid) -> OperationDetail {
    match client
        .call(Call::OperationGet(GetOperationParams { operation_id: id }))
        .await
    {
        Ok(Reply::OperationGet(detail)) => *detail,
        other => panic!("operation.get of {id}: {other:?}"),
    }
}

/// `operation.list` with `params`.
pub async fn list(
    client: &Client,
    params: ListOperationsParams,
) -> (Vec<OperationSummary>, Option<String>) {
    match client.call(Call::OperationList(params)).await {
        Ok(Reply::OperationList(page)) => (page.items, page.next),
        other => panic!("operation.list: {other:?}"),
    }
}

/// Waits until the operation matches `ready`.
pub async fn wait_until(
    client: &Client,
    id: Uuid,
    ready: impl Fn(&OperationDetail) -> bool,
) -> OperationDetail {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let detail = get(client, id).await;
        if ready(&detail) {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "operation {id} did not get there in time: {detail:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Waits until the operation is in `state`.
pub async fn wait_state(client: &Client, id: Uuid, state: OperationState) -> OperationDetail {
    wait_until(client, id, |detail| detail.operation.state == state).await
}

static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

/// Installs, once per test binary, a global subscriber that writes every
/// line at `debug` and above, from every thread, as the daemon writes them
/// to its log file.
pub fn install_logs() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let buffer = Arc::clone(LOGS.get_or_init(|| Arc::new(Mutex::new(Vec::new()))));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || BufferWriter(Arc::clone(&buffer)))
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("no other global subscriber is set in the operation tests");
    });
}

struct BufferWriter(Arc<Mutex<Vec<u8>>>);

impl Write for BufferWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Every log line this test binary wrote so far, as text.
pub fn log_text() -> String {
    let buffer = LOGS.get().expect("the log capture is installed");
    let bytes = buffer
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Every log line this test binary wrote so far, parsed.
pub fn log_lines() -> Vec<Value> {
    log_text()
        .lines()
        .map(|line| serde_json::from_str(line).expect("each log line is JSON"))
        .collect()
}
