//! Helpers the CLI integration tests share: the test root's paths, running
//! `stackly` with a clean environment, reading its envelope, a scripted
//! daemon, a pty runner, and golden files (spec 0008, *Critical test
//! scenarios*).
#![allow(dead_code, reason = "each test binary uses a part of these helpers")]

pub mod dto;
pub mod golden;
pub mod pty;
pub mod scripted;
pub mod stacklyd;

use std::collections::BTreeSet;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde::de::{Deserializer as _, MapAccess, Visitor};
use serde_json::Value;
use stackly_platform::DirectoryLayout;
use stackly_platform_macos::MacosPlatform;
use stackly_testkit::TestRoot;

/// The `stackly` binary under test.
pub const STACKLY: &str = env!("CARGO_BIN_EXE_stackly");

/// The envelope's top level keys, in order (spec 0008, AC-2).
pub const ENVELOPE_KEYS: [&str; 7] = [
    "schemaVersion",
    "type",
    "command",
    "result",
    "error",
    "requestId",
    "warnings",
];

/// The paths of one test root, as the daemon and the CLI resolve them.
pub struct Paths {
    /// The daemon socket.
    pub socket: PathBuf,
    /// The runtime root holding the socket.
    pub runtime: PathBuf,
    /// The registry database.
    pub registry_db: PathBuf,
    /// The daemon log file.
    pub log: PathBuf,
    /// The data root.
    pub data: PathBuf,
}

/// The paths of `root`.
pub fn paths(root: &TestRoot) -> Paths {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
    let layout = DirectoryLayout::new(platform.directories().roots().expect("roots resolve"));
    Paths {
        socket: layout.daemon_socket(),
        runtime: layout.roots().runtime().to_path_buf(),
        registry_db: layout.registry_db(),
        log: layout.daemon_log_dir().join("stacklyd.log"),
        data: layout.roots().data().to_path_buf(),
    }
}

/// The uid this test runs as.
pub fn uid() -> u32 {
    std::fs::metadata(env!("CARGO_MANIFEST_DIR"))
        .expect("the package folder can be read")
        .uid()
}

/// A runtime root this test made by hand in the real user temporary
/// directory, removed again with whatever the test put in it.
pub struct HandMadeRuntime {
    dir: PathBuf,
}

impl HandMadeRuntime {
    /// Makes `dir` with mode 0700.
    pub fn new(dir: &Path) -> Self {
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

/// Every path under `dir`, relative to it.
pub fn listing(dir: &Path) -> BTreeSet<PathBuf> {
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

/// `stackly` for `root` with a clean environment: `STACKLY_ROOT` set, the
/// color and input variables unset, `TZ=UTC`, and stdin closed.
pub fn stackly(root: &Path) -> Command {
    let mut command = Command::new(STACKLY);
    command
        .env("STACKLY_ROOT", root)
        .env("TZ", "UTC")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("STACKLY_NO_INPUT")
        .env_remove("CI")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::null());
    command
}

/// What one run of `stackly` left.
#[derive(Debug)]
pub struct Run {
    /// The exit code.
    pub code: Option<i32>,
    /// stdout as text.
    pub stdout: String,
    /// stderr as text.
    pub stderr: String,
    /// How long it took.
    pub elapsed: Duration,
}

/// Runs `command` to the end.
pub fn run_command(mut command: Command) -> Run {
    let started = Instant::now();
    let Output {
        status,
        stdout,
        stderr,
    } = command.output().expect("stackly runs");
    Run {
        code: status.code(),
        stdout: String::from_utf8(stdout).expect("stdout is utf-8"),
        stderr: String::from_utf8(stderr).expect("stderr is utf-8"),
        elapsed: started.elapsed(),
    }
}

/// Runs `stackly <args>` for `root`.
pub fn run(root: &Path, args: &[&str]) -> Run {
    let mut command = stackly(root);
    command.args(args);
    run_command(command)
}

/// The keys of a JSON object, in the order the text holds them.
pub fn key_order(object: &str) -> Vec<String> {
    struct Keys;
    impl<'de> Visitor<'de> for Keys {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut keys = Vec::new();
            while let Some((key, _)) = map.next_entry::<String, serde::de::IgnoredAny>()? {
                keys.push(key);
            }
            Ok(keys)
        }
    }
    serde_json::Deserializer::from_str(object)
        .deserialize_map(Keys)
        .expect("the text is a JSON object")
}

/// Whether JSON `text` holds whitespace between its tokens.
pub fn whitespace_outside_strings(text: &str) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    for c in text.chars() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if c == '"' {
            in_string = true;
        } else if c.is_whitespace() {
            return true;
        }
    }
    false
}

/// The one envelope line `text` holds, checked: exactly one line ending in
/// `\n`, compact, no ESC or carriage return, keys in contract order.
pub fn envelope_of(text: &str) -> Value {
    let line = text
        .strip_suffix('\n')
        .unwrap_or_else(|| panic!("the envelope ends with a newline: {text:?}"));
    assert!(!line.contains('\n'), "exactly one line on stdout: {text:?}");
    assert!(!line.contains('\u{1b}'), "no ESC on stdout: {text:?}");
    assert!(
        !line.contains('\r'),
        "no carriage return on stdout: {text:?}"
    );
    assert!(!whitespace_outside_strings(line), "compact JSON: {line}");
    assert_eq!(key_order(line), ENVELOPE_KEYS, "{line}");
    let value: Value = serde_json::from_str(line).expect("the line is JSON");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["type"], "result");
    value
}

/// The envelope of a run, with the exit code rule: 0 exactly when `error`
/// is null.
pub fn envelope(run: &Run) -> Value {
    let value = envelope_of(&run.stdout);
    assert_eq!(
        value["error"].is_null(),
        run.code == Some(0),
        "error is null exactly when the exit code is 0: {run:?}"
    );
    value
}

/// The value of detail `key` of a JSON error body.
pub fn detail(error: &Value, key: &str) -> Option<String> {
    error["details"]
        .as_array()?
        .iter()
        .find(|detail| detail["key"] == key)
        .and_then(|detail| detail["value"].as_str().map(str::to_owned))
}
