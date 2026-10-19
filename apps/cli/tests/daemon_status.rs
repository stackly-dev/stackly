//! `stackly daemon status` against a real `stacklyd` and against endpoints
//! that must be refused (spec 0006 AC-13). The CLI never starts the
//! daemon and never creates a file under the test root.
#![cfg(target_os = "macos")]

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_ipc::{Accepted, Client, ConnectPolicy, Listener};
use stackly_platform::DirectoryLayout;
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::codes::protocol;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::{CONTRACT_DIGEST, ErrorBody, ProtocolVersion};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{SpawnedDaemon, TestRoot, cargo_bin};

const STACKLY: &str = env!("CARGO_BIN_EXE_stackly");

struct Paths {
    socket: PathBuf,
    runtime: PathBuf,
    registry_db: PathBuf,
    log: PathBuf,
}

fn paths(root: &TestRoot) -> Paths {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
    let layout = DirectoryLayout::new(platform.directories().roots().expect("roots resolve"));
    Paths {
        socket: layout.daemon_socket(),
        runtime: layout.roots().runtime().to_path_buf(),
        registry_db: layout.registry_db(),
        log: layout.daemon_log_dir().join("stacklyd.log"),
    }
}

fn uid() -> u32 {
    std::fs::metadata(env!("CARGO_MANIFEST_DIR"))
        .expect("the package folder can be read")
        .uid()
}

/// Whether a client completes a handshake on `socket` right now. A whole
/// handshake, not a bare connect: a client that leaves at once can be gone
/// before the daemon reads its credentials, and the daemon then logs a
/// refused peer after the test moved on.
fn handshakes(socket: &Path) -> bool {
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime starts")
            .block_on(async {
                let client = ClientInfo {
                    kind: ClientKind::Cli,
                    version: "0.1.0".to_owned(),
                    pid: Some(std::process::id()),
                    interactive: Some(false),
                };
                let connect = Client::connect(&socket, ConnectPolicy::same_user(uid()), client);
                tokio::time::timeout(Duration::from_secs(5), connect)
                    .await
                    .is_ok_and(|connected| connected.is_ok())
            })
    })
    .join()
    .unwrap_or(false)
}

/// How many lines of the daemon log file say `connection closed`.
fn closed_connections(log: &Path) -> usize {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| line["fields"]["message"] == "connection closed")
        .count()
}

/// Starts `stacklyd` and waits until it is ready and quiet: the start slot
/// ended, a probe handshake worked, and the daemon wrote the close of the
/// probe connection to its log file.
fn start_daemon(root: &TestRoot) -> SpawnedDaemon {
    let paths = paths(root);
    let closed_before = closed_connections(&paths.log);
    let daemon = SpawnedDaemon::start(
        &cargo_bin("stacklyd", "stacklyd"),
        root.path(),
        &paths.socket,
        &paths.runtime,
        |daemon| daemon.stderr().contains("registry_state") && handshakes(daemon.socket()),
    )
    .expect("stacklyd starts");
    let deadline = Instant::now() + Duration::from_secs(5);
    while closed_connections(&paths.log) <= closed_before {
        assert!(
            Instant::now() < deadline,
            "stacklyd did not log the close of the ready probe: {}",
            daemon.stderr()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon
}

fn status(root: &OsStr, json: bool) -> Output {
    let mut command = Command::new(STACKLY);
    command.args(["daemon", "status"]).env("STACKLY_ROOT", root);
    if json {
        command.arg("--json");
    }
    command.output().expect("stackly runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Every path under `dir`, relative to it.
fn listing(dir: &Path) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).expect("test directory can be listed") {
            let path = entry.expect("directory entry can be read").path();
            if path
                .symlink_metadata()
                .expect("listed path metadata can be read")
                .is_dir()
            {
                stack.push(path.clone());
            }
            paths.insert(
                path.strip_prefix(dir)
                    .expect("listed path is beneath the test root")
                    .to_path_buf(),
            );
        }
    }
    paths
}

/// A runtime root this test made by hand in the real user temporary
/// directory, removed again with whatever the test put in it.
struct HandMadeRuntime {
    dir: PathBuf,
}

impl HandMadeRuntime {
    fn new(dir: &Path) -> Self {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .expect("the runtime root is free");
        Self {
            dir: dir.to_path_buf(),
        }
    }
}

impl Drop for HandMadeRuntime {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dir.join("stacklyd.sock"));
        let _ = std::fs::remove_file(self.dir.join("stacklyd.sock.tmp"));
        let _ = std::fs::remove_dir(&self.dir);
    }
}

#[test]
fn status_of_a_running_daemon_prints_its_facts_as_text_and_json_and_creates_nothing() {
    let ev = evidence!(
        covers = ["REQ-028", "BR-011"],
        kind = Kind::Fixture,
        expected = "stackly daemon status shows the running daemon's status, pid, versions, roots, socket, and override as text and as JSON with the client block, exits 0, and adds no file under the root",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = paths(&root);
    let before = listing(root.path());

    let text = status(root.path().as_os_str(), false);
    let json = status(root.path().as_os_str(), true);

    assert_eq!(text.status.code(), Some(0), "{text:?}");
    let lines = stdout(&text);
    for expected in [
        "status: ready".to_owned(),
        format!("pid: {}", daemon.pid()),
        format!("uid: {}", uid()),
        format!(
            "protocol: {}.{}",
            ProtocolVersion::CURRENT.major,
            ProtocolVersion::CURRENT.minor
        ),
        format!("socket: {}", paths.socket.display()),
        format!("data root: {}", root.path().join("data").display()),
        "root override: yes".to_owned(),
    ] {
        assert!(
            lines.lines().any(|line| line == expected),
            "missing {expected:?} in:\n{lines}"
        );
    }
    assert!(!lines.contains("contract drift"));

    assert_eq!(json.status.code(), Some(0), "{json:?}");
    let value: Value = serde_json::from_str(stdout(&json).trim()).expect("one JSON object");
    assert_eq!(value["info"]["pid"], daemon.pid());
    assert_eq!(
        value["info"]["socketPath"],
        paths.socket.display().to_string()
    );
    assert_eq!(value["health"]["status"], "ready");
    assert_eq!(value["client"]["contractDigest"], CONTRACT_DIGEST);
    assert_eq!(value["client"]["contractDrift"], false);
    assert_eq!(
        value["client"]["protocol"]["major"],
        ProtocolVersion::CURRENT.major
    );
    assert!(!stdout(&json).contains('\u{1b}'), "no ANSI in --json");

    assert_eq!(
        listing(root.path()),
        before,
        "stackly created or removed a path"
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn status_without_a_daemon_names_the_socket_and_exits_6() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "with no daemon running, stackly daemon status says the daemon is not running, names the socket path, hints at stacklyd, exits 6, and creates nothing",
    );
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);

    let output = status(root.path().as_os_str(), false);
    let json = status(root.path().as_os_str(), true);

    assert_eq!(output.status.code(), Some(6), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(&format!(
            "daemon is not running (no socket at {})",
            paths.socket.display()
        )),
        "{message}"
    );
    assert!(
        message.contains("hint: start it with stacklyd"),
        "{message}"
    );
    assert_eq!(json.status.code(), Some(6));
    let error: ErrorBody = serde_json::from_str(stderr(&json).trim()).expect("an ErrorBody");
    assert_eq!(error.code(), "ipc.endpoint_missing");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(!paths.runtime.exists());
    ev.pass();
}

#[test]
fn status_refuses_an_endpoint_that_is_not_a_socket_and_exits_4() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-070"],
        kind = Kind::Fixture,
        expected = "a regular file at the socket path is refused as an untrusted endpoint without a byte sent, and the CLI exits 4",
    );
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);
    let _runtime = HandMadeRuntime::new(&paths.runtime);
    std::fs::write(&paths.socket, b"").unwrap();

    let output = status(root.path().as_os_str(), true);

    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let error: ErrorBody = serde_json::from_str(stderr(&output).trim()).expect("an ErrorBody");
    assert_eq!(error.code(), "ipc.endpoint_untrusted");
    assert_eq!(error.detail("reason"), Some("not_a_socket"));
    ev.pass();
}

#[test]
fn status_against_a_daemon_of_another_major_exits_5() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "a test server that answers the handshake with protocol.major_mismatch makes stackly daemon status exit 5",
    );
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);
    let _runtime = HandMadeRuntime::new(&paths.runtime);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let listener = rt
        .block_on(async { Listener::bind(&paths.socket, uid()) })
        .unwrap();
    let server = rt.spawn(async move {
        let Accepted::Authorized { mut connection, .. } = listener.accept().await.unwrap() else {
            panic!("the CLI runs as this user");
        };
        let request = connection
            .read_frame(None)
            .await
            .unwrap()
            .expect("the handshake arrives");
        let request: Value = serde_json::from_slice(&request).unwrap();
        let error = ErrorBody::new(&protocol::MAJOR_MISMATCH, "the daemon speaks protocol 2.0")
            .with_detail("serverMajor", "2")
            .with_detail("serverMinor", "0");
        connection
            .close_with_error(request["requestId"].as_str().map(str::to_owned), error)
            .await
            .unwrap();
        listener.unlink_if_own().unwrap();
    });

    let output = status(root.path().as_os_str(), false);

    assert_eq!(output.status.code(), Some(5), "{output:?}");
    assert!(
        stderr(&output).contains("protocol.major_mismatch"),
        "{}",
        stderr(&output)
    );
    rt.block_on(async { tokio::time::timeout(Duration::from_secs(5), server).await })
        .unwrap()
        .unwrap();
    ev.pass();
}

#[test]
fn a_degraded_daemon_still_exits_0() {
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);
    let first = start_daemon(&root);
    assert_eq!(first.stop().unwrap().code(), Some(0));
    let mut bytes = std::fs::read(&paths.registry_db).unwrap();
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&paths.registry_db, &bytes).unwrap();
    let daemon = start_daemon(&root);

    let text = status(root.path().as_os_str(), false);
    let json = status(root.path().as_os_str(), true);

    assert_eq!(text.status.code(), Some(0), "{text:?}");
    assert!(
        stdout(&text).contains("status: degraded"),
        "{}",
        stdout(&text)
    );
    assert!(
        stdout(&text).contains(
            "subsystem registry: unavailable (the registry needs recovery (schema_newer_than_app))"
        ),
        "{}",
        stdout(&text)
    );
    let value: Value = serde_json::from_str(stdout(&json).trim()).unwrap();
    assert_eq!(value["health"]["status"], "degraded");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
}

#[test]
fn after_a_second_daemon_was_refused_status_names_the_first() {
    let ev = evidence!(
        covers = ["BR-011", "REQ-028"],
        kind = Kind::Fixture,
        expected = "after a second stacklyd for the same root exits 3, stackly daemon status shows the first daemon's pid and server instance",
    );
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);
    let first = start_daemon(&root);
    let second = SpawnedDaemon::spawn(
        &cargo_bin("stacklyd", "stacklyd"),
        root.path(),
        &paths.socket,
        &paths.runtime,
        &[],
    )
    .unwrap();
    assert_eq!(
        second.wait_exit(Duration::from_secs(10)).unwrap().code(),
        Some(3)
    );

    let output = status(root.path().as_os_str(), true);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let value: Value = serde_json::from_str(stdout(&output).trim()).unwrap();
    assert_eq!(value["info"]["pid"], first.pid());
    assert!(
        first
            .stderr()
            .contains(value["info"]["serverInstanceId"].as_str().unwrap())
    );
    assert_eq!(first.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn a_rejected_root_override_exits_2_before_anything_else() {
    let output = status(OsStr::new("relative/root"), false);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        stderr(&output).contains("not absolute"),
        "{}",
        stderr(&output)
    );
}
