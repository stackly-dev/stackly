//! Helpers shared by the daemon's integration tests that run the binary.
#![allow(dead_code, reason = "each test binary uses a part of these helpers")]

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_ipc::{Client, ConnectPolicy, connect_verified};
use stackly_platform::DirectoryLayout;
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind, HandshakeAccepted};
use stackly_testkit::{SpawnedDaemon, TestRoot};

/// The daemon binary cargo built for these tests.
pub const STACKLYD: &str = env!("CARGO_BIN_EXE_stacklyd");

/// Parses every JSON log line in `text`.
pub fn json_lines(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).expect("each log line is JSON"))
        .collect()
}

/// Returns one field of a log line, `Null` when it is absent.
pub fn field<'a>(line: &'a Value, name: &str) -> &'a Value {
    &line["fields"][name]
}

/// Returns the one line that carries the field `name`.
pub fn line_with<'a>(lines: &'a [Value], name: &str) -> &'a Value {
    let matching: Vec<_> = lines
        .iter()
        .filter(|line| !field(line, name).is_null())
        .collect();
    assert_eq!(matching.len(), 1, "lines with {name}: {lines:?}");
    matching[0]
}

/// Paths the daemon computes for a test root.
pub struct DaemonPaths {
    /// The layout under the root.
    pub layout: DirectoryLayout,
    /// The daemon socket.
    pub socket: PathBuf,
    /// The runtime root, in the real user temporary directory.
    pub runtime: PathBuf,
}

/// Resolves the paths exactly as the daemon does with `STACKLY_ROOT` set
/// to the test root.
pub fn daemon_paths(root: &TestRoot) -> DaemonPaths {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
    let roots = platform
        .directories()
        .roots()
        .expect("the macOS platform resolved its roots");
    let layout = DirectoryLayout::new(roots);
    DaemonPaths {
        socket: layout.daemon_socket(),
        runtime: layout.roots().runtime().to_path_buf(),
        layout,
    }
}

/// The longest wait for the daemon to log the close of the ready probe.
const PROBE_LOG_TIMEOUT: Duration = Duration::from_secs(5);

/// The message of the line that ends a served connection.
const CONNECTION_CLOSED: &str = "connection closed";

/// Starts `stacklyd` against `root` and waits until it is ready and quiet:
/// it logged the outcome of the registry open (the end of its start slot),
/// a probe client completed a handshake, and the daemon wrote the close of
/// that probe's connection to its log file.
///
/// The probe is a whole handshake, not a bare connect: a client that
/// connects and leaves at once can be gone before the daemon reads its
/// credentials, and the daemon then refuses it as a peer whose credentials
/// cannot be read and logs that after this function returned. Waiting for
/// the close line means no line of the probe lands after a test reads the
/// log.
pub fn start_daemon(root: &TestRoot) -> SpawnedDaemon {
    let paths = daemon_paths(root);
    let log = paths.layout.daemon_log_dir().join("stacklyd.log");
    let closed_before = count_messages(&log, CONNECTION_CLOSED);
    let daemon = SpawnedDaemon::start(
        Path::new(STACKLYD),
        root.path(),
        &paths.socket,
        &paths.runtime,
        |daemon| logged(&daemon.stderr(), "registry_state") && handshakes(daemon.socket()),
    )
    .expect("stacklyd starts");
    let deadline = Instant::now() + PROBE_LOG_TIMEOUT;
    while count_messages(&log, CONNECTION_CLOSED) <= closed_before {
        assert!(
            Instant::now() < deadline,
            "stacklyd did not log the close of the ready probe: {}",
            daemon.stderr()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon
}

/// How many complete lines of the file at `path` carry `message`; 0 when
/// the file does not exist yet.
pub fn count_messages(path: &Path, message: &str) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| field(line, "message") == message)
        .count()
}

/// Whether a `cli` client completes a handshake on `socket` right now. It
/// runs on a thread of its own, so any test (sync or async) can call it.
pub fn handshakes(socket: &Path) -> bool {
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(5), client_of(&socket))
                .await
                .is_ok_and(|connected| connected.is_ok())
        })
    })
    .join()
    .unwrap_or(false)
}

/// The uid this test runs as.
pub fn uid() -> u32 {
    std::fs::metadata(env!("CARGO_MANIFEST_DIR"))
        .expect("the package folder can be read")
        .uid()
}

/// Whether `connect_verified` to `socket` works right now. It runs on a
/// thread of its own, so any test (sync or async) can call it.
pub fn connects(socket: &Path) -> bool {
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        runtime()
            .block_on(connect_verified(&socket, ConnectPolicy::same_user(uid())))
            .is_ok()
    })
    .join()
    .unwrap_or(false)
}

/// A runtime for a sync test's client side.
pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a test runtime starts")
}

/// A typed client of the test's uid, as a `cli` client that cannot show
/// a dialog.
pub async fn client(socket: &Path) -> (Client, HandshakeAccepted) {
    client_of(socket).await.expect("the client connects")
}

async fn client_of(socket: &Path) -> Result<(Client, HandshakeAccepted), stackly_ipc::IpcError> {
    Client::connect(
        socket,
        ConnectPolicy::same_user(uid()),
        ClientInfo {
            kind: ClientKind::Cli,
            version: "0.1.0".to_owned(),
            pid: Some(std::process::id()),
            interactive: Some(false),
        },
    )
    .await
}

/// Whether a complete line of `text` carries the field `name`.
pub fn logged(text: &str, name: &str) -> bool {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|line| !field(&line, name).is_null())
}
