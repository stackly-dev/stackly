//! Helpers shared by the daemon's integration tests.

use std::path::{Path, PathBuf};

use serde_json::Value;
use stackly_platform::DirectoryLayout;
use stackly_platform_macos::MacosPlatform;
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

/// Starts `stacklyd` against `root` and waits until it logged the outcome
/// of the registry open, the last start step before it waits for a signal.
pub fn start_daemon(root: &TestRoot) -> SpawnedDaemon {
    let paths = daemon_paths(root);
    SpawnedDaemon::start(
        Path::new(STACKLYD),
        root.path(),
        &paths.socket,
        &paths.runtime,
        |daemon| logged(&daemon.stderr(), "registry_state"),
    )
    .expect("stacklyd starts")
}

/// Whether a complete line of `text` carries the field `name`.
pub fn logged(text: &str, name: &str) -> bool {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|line| !field(&line, name).is_null())
}
