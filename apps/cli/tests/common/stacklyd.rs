//! The real `stacklyd` binary for the CLI tests (spec 0008, *Critical test
//! scenarios*): started with the test root as `STACKLY_ROOT`, and ready and
//! quiet before the test runs `stackly`.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_ipc::{Client, ConnectPolicy};
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_testkit::{SpawnedDaemon, TestRoot, cargo_bin};

use super::{paths, uid};

/// Whether a client completes a handshake on `socket` right now. A whole
/// handshake, not a bare connect: a client that leaves at once can be gone
/// before the daemon reads its credentials, and the daemon then logs a
/// refused peer after the test moved on.
pub fn handshakes(socket: &Path) -> bool {
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
pub fn closed_connections(log: &Path) -> usize {
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
pub fn start_daemon(root: &TestRoot) -> SpawnedDaemon {
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
