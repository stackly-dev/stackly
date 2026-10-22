//! The launch record of the background agent (spec 0015, *Konteks launch
//! dan launch record di daemon*).
//!
//! When launchd starts the production agent for the default root, the daemon
//! writes `<data>/background-agent.json` after it holds the lock and before
//! it binds the socket. The file is proof that the user allowed the agent to
//! run: the CLI and the app only check that it exists and read
//! `lastLaunchedAt` from it, which `classify` turns into `disabled` and the
//! restart wait uses to see a respawn.

use std::path::Path;

use serde::{Deserialize, Serialize};
use stackly_platform::bundle::AppBundle;
use stackly_platform::{Filesystem, LaunchContext, PlatformError};
use stackly_protocol::Timestamp;

/// The `schema` value of the launch record.
pub const LAUNCH_RECORD_SCHEMA: u32 = 1;
/// The most bytes the daemon reads from an existing launch record.
pub const LAUNCH_RECORD_MAX_LEN: u64 = 4096;

/// The content of `background-agent.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRecord {
    /// Always [`LAUNCH_RECORD_SCHEMA`].
    pub schema: u32,
    /// When launchd first started the agent, kept from the old file.
    pub first_launched_at: Timestamp,
    /// When launchd started this run.
    pub last_launched_at: Timestamp,
    /// The canonical bundle folder this run came from, or `null` when the
    /// daemon is not inside an app bundle.
    pub bundle_path: Option<String>,
    /// The daemon version.
    pub version: String,
}

/// Whether this start writes the launch record: launchd started the
/// production agent and `STACKLY_ROOT` is not set.
#[must_use]
pub fn writes_launch_record(context: &LaunchContext, root_override: bool) -> bool {
    !root_override && context.is_production_agent()
}

/// Writes the launch record at `path` through `fs`, keeping `firstLaunchedAt`
/// from an old record that can still be read.
///
/// An old file that is missing, too large, not a regular file, or not a
/// record is replaced, and its `firstLaunchedAt` is lost.
///
/// # Errors
///
/// Returns the filesystem error when the atomic write fails; the old file
/// is then intact.
pub fn write_launch_record(
    fs: &dyn Filesystem,
    path: &Path,
    now: Timestamp,
    bundle: Option<&AppBundle>,
    version: &str,
) -> Result<LaunchRecord, PlatformError> {
    let first_launched_at = read_launch_record(fs, path).map_or(now, |old| old.first_launched_at);
    let record = LaunchRecord {
        schema: LAUNCH_RECORD_SCHEMA,
        first_launched_at,
        last_launched_at: now,
        bundle_path: bundle.map(|bundle| bundle.root().to_string_lossy().into_owned()),
        version: version.to_owned(),
    };
    let mut bytes = serde_json::to_vec_pretty(&record)
        .expect("a launch record holds only strings and numbers, which always serialize");
    bytes.push(b'\n');
    fs.write_file_atomic(path, &bytes)?;
    Ok(record)
}

/// Reads the record at `path`; `None` when it cannot be read as one.
fn read_launch_record(fs: &dyn Filesystem, path: &Path) -> Option<LaunchRecord> {
    let bytes = fs.read_file_limited(path, LAUNCH_RECORD_MAX_LEN).ok()?;
    let record: LaunchRecord = serde_json::from_slice(&bytes).ok()?;
    (record.schema == LAUNCH_RECORD_SCHEMA).then_some(record)
}

#[cfg(test)]
mod tests {
    use stackly_testkit::{FakeFilesystem, Fault, FsOp, TestRoot};

    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().expect("a valid test instant")
    }

    #[test]
    fn only_the_production_agent_without_a_root_override_writes_the_record() {
        let production = LaunchContext::Launchd {
            label: stackly_platform::bundle::DAEMON_LABEL.to_owned(),
        };
        let test_agent = LaunchContext::Launchd {
            label: "dev.stackly.test.agent.1-1".to_owned(),
        };
        assert!(writes_launch_record(&production, false));
        assert!(!writes_launch_record(&production, true));
        assert!(!writes_launch_record(&test_agent, false));
        assert!(!writes_launch_record(&LaunchContext::Manual, false));
    }

    #[test]
    fn a_first_launch_sets_both_times_to_now() {
        let root = TestRoot::new().unwrap();
        let fs = FakeFilesystem::new(&root);
        let path = root.path().join("background-agent.json");
        let now = at("2026-10-22T01:00:00.000Z");

        let record = write_launch_record(&fs, &path, now, None, "0.1.0").unwrap();

        assert_eq!(record.first_launched_at, now);
        assert_eq!(record.last_launched_at, now);
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["firstLaunchedAt"], "2026-10-22T01:00:00.000Z");
        assert_eq!(json["bundlePath"], serde_json::Value::Null);
        assert_eq!(json["version"], "0.1.0");
    }

    #[test]
    fn a_later_launch_keeps_the_first_time_and_names_the_bundle() {
        let root = TestRoot::new().unwrap();
        let fs = FakeFilesystem::new(&root);
        let path = root.path().join("background-agent.json");
        let first = at("2026-10-22T01:00:00.000Z");
        let later = at("2026-10-22T02:30:00.000Z");
        write_launch_record(&fs, &path, first, None, "0.1.0").unwrap();
        let exe = Path::new("/Applications/Stackly.app/Contents/MacOS/stacklyd");
        let bundle = AppBundle::enclosing(exe).unwrap();

        let record = write_launch_record(&fs, &path, later, Some(&bundle), "0.2.0").unwrap();

        assert_eq!(record.first_launched_at, first);
        assert_eq!(record.last_launched_at, later);
        assert_eq!(
            record.bundle_path.as_deref(),
            Some("/Applications/Stackly.app")
        );
        assert_eq!(record.version, "0.2.0");
    }

    #[test]
    fn an_unreadable_old_record_is_replaced() {
        let root = TestRoot::new().unwrap();
        let fs = FakeFilesystem::new(&root);
        let path = root.path().join("background-agent.json");
        let now = at("2026-10-22T03:00:00.000Z");
        for old in [
            "not json".to_owned(),
            r#"{"schema":2,"firstLaunchedAt":"2026-10-01T00:00:00.000Z","lastLaunchedAt":"2026-10-01T00:00:00.000Z","bundlePath":null,"version":"x"}"#.to_owned(),
            "x".repeat(5000),
        ] {
            std::fs::write(&path, old).unwrap();

            let record = write_launch_record(&fs, &path, now, None, "0.1.0").unwrap();

            assert_eq!(record.first_launched_at, now);
        }
    }

    #[test]
    fn a_failed_write_is_an_error_and_leaves_the_old_record() {
        let root = TestRoot::new().unwrap();
        let fs = FakeFilesystem::new(&root);
        let path = root.path().join("background-agent.json");
        let first = at("2026-10-22T01:00:00.000Z");
        write_launch_record(&fs, &path, first, None, "0.1.0").unwrap();
        let before = std::fs::read(&path).unwrap();
        fs.fail_next(FsOp::WriteFileAtomic, Fault::Io);

        let result = write_launch_record(&fs, &path, at("2026-10-22T04:00:00.000Z"), None, "0.1.0");

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
