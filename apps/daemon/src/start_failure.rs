//! The record of a failed start (spec 0034, *Catatan gagal start daemon*,
//! AC-11): `<data>/daemon-start-failure.json`.
//!
//! launchd keeps no reason a client could read, so `stacklyd` writes this
//! record itself when it exits 1 after it knew its data root, and the CLI
//! and the app show it while the daemon is not reachable. It is written
//! only into a data root that is a folder of this user without group or
//! other write, never creates a folder, replaces an older record atomically
//! with mode 0600, and holds at most 4 KiB. Its message went through the
//! `Redactor`. A later start removes it once its socket is bound. Exits 2
//! (arguments), 3 (lock held), and 75 (a restart a client asked for) never
//! write it.

use std::path::Path;

use serde::{Deserialize, Serialize};
use stackly_core::secret::Redactor;
use stackly_platform::bundle::AppBundle;
use stackly_platform::{
    DAEMON_START_FAILURE_RECORD_FILE, FileKind, Filesystem, LaunchContext, PlatformError,
    UserIdentity,
};
use stackly_protocol::Timestamp;

/// The `schema` value of the record.
pub const START_FAILURE_SCHEMA: u32 = 1;
/// The most bytes a record holds.
pub const START_FAILURE_MAX_LEN: usize = 4096;
/// The most characters of the message.
pub const START_FAILURE_MESSAGE_MAX: usize = 512;
/// The code of a failure that has no code of the error catalog.
pub const START_FAILED_CODE: &str = "daemon.start_failed";

/// Why a start failed: the catalog code of its error, if it has one, and
/// the error text.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StartFailure {
    /// The code of the error catalog, such as `ipc.endpoint_occupied`.
    pub code: Option<String>,
    /// The error text, before it is redacted.
    pub message: String,
}

impl StartFailure {
    /// A failure with `code` and `message`.
    #[must_use]
    pub fn new(code: Option<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Who started the daemon that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchKind {
    /// launchd, through the background agent.
    Launchd,
    /// Anything else: a shell, a test, `stackly daemon start`.
    Manual,
}

/// The content of `daemon-start-failure.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFailureRecord {
    /// Always [`START_FAILURE_SCHEMA`].
    pub schema: u32,
    /// When the start failed.
    pub at: Timestamp,
    /// The exit code, always 1.
    pub exit_code: u8,
    /// The catalog code of the error, else [`START_FAILED_CODE`].
    pub code: String,
    /// The error text, redacted and at most
    /// [`START_FAILURE_MESSAGE_MAX`] characters.
    pub message: String,
    /// The daemon version.
    pub version: String,
    /// Who started the daemon.
    pub launch: LaunchKind,
    /// The canonical bundle folder the daemon ran from, or `null` outside a
    /// bundle.
    pub bundle_path: Option<String>,
}

impl StartFailureRecord {
    /// The record of `failure`: its message through `redactor`, then cut to
    /// [`START_FAILURE_MESSAGE_MAX`] characters.
    #[must_use]
    pub fn new(
        failure: &StartFailure,
        redactor: &Redactor,
        at: Timestamp,
        version: &str,
        launch: &LaunchContext,
        bundle: Option<&AppBundle>,
    ) -> Self {
        Self {
            schema: START_FAILURE_SCHEMA,
            at,
            exit_code: crate::startup::EXIT_START_FAILED,
            code: failure
                .code
                .clone()
                .unwrap_or_else(|| START_FAILED_CODE.to_owned()),
            message: redactor
                .redact(&failure.message)
                .chars()
                .take(START_FAILURE_MESSAGE_MAX)
                .collect(),
            version: version.to_owned(),
            launch: if launch.label().is_some() {
                LaunchKind::Launchd
            } else {
                LaunchKind::Manual
            },
            bundle_path: bundle.map(|bundle| bundle.root().to_string_lossy().into_owned()),
        }
    }

    /// The record as pretty JSON with a final newline, at most
    /// [`START_FAILURE_MAX_LEN`] bytes: a message whose escapes would make
    /// it longer is cut further, and as a last resort the bundle path is
    /// left out.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut record = self.clone();
        loop {
            let mut bytes = serde_json::to_vec_pretty(&record).expect(
                "a start failure record holds only strings and numbers, which always serialize",
            );
            bytes.push(b'\n');
            if bytes.len() <= START_FAILURE_MAX_LEN
                || (record.message.is_empty() && record.bundle_path.is_none())
            {
                return bytes;
            }
            if record.message.is_empty() {
                record.bundle_path = None;
            } else {
                let keep = record.message.chars().count() / 2;
                record.message = record.message.chars().take(keep).collect();
            }
        }
    }
}

/// What [`write_record`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordWrite {
    /// The record is in the data root.
    Written,
    /// Nothing is at the data root path, so nothing was written.
    NoDataRoot,
    /// The data root is a symlink, not a folder, another user's, or
    /// writable by group or others, so nothing was written.
    NotPrivate,
}

/// Writes `record` as `<data_root>/daemon-start-failure.json` when
/// `data_root` is a folder of the current user that neither group nor
/// others can write; otherwise writes nothing. It never creates a folder.
///
/// # Errors
///
/// Returns the filesystem error of the inspection or of the atomic write;
/// an older record is then intact.
pub fn write_record(
    fs: &dyn Filesystem,
    identity: &dyn UserIdentity,
    data_root: &Path,
    record: &StartFailureRecord,
) -> Result<RecordWrite, PlatformError> {
    let Some(info) = fs.symlink_metadata(data_root)? else {
        return Ok(RecordWrite::NoDataRoot);
    };
    let private = info.kind == FileKind::Dir
        && info.owner == Some(identity.current_user()?)
        && info.mode.is_some_and(|mode| mode & 0o022 == 0);
    if !private {
        return Ok(RecordWrite::NotPrivate);
    }
    fs.write_file_atomic(
        &data_root.join(DAEMON_START_FAILURE_RECORD_FILE),
        &record.to_bytes(),
    )?;
    Ok(RecordWrite::Written)
}

/// Removes the record of an earlier start from `data_root`, the canonical
/// data root, without following a symlink. A folder of that name is left
/// alone, since the daemon never writes one.
///
/// # Errors
///
/// Returns the filesystem error of the inspection or the removal.
/// `Ok(false)` means there was no record.
pub fn remove_record(fs: &dyn Filesystem, data_root: &Path) -> Result<bool, PlatformError> {
    let name = Path::new(DAEMON_START_FAILURE_RECORD_FILE);
    match fs.symlink_metadata(&data_root.join(name))? {
        Some(info) if info.kind != FileKind::Dir => Ok(!fs.remove_tree(data_root, name)?.missing),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(message: String, bundle_path: Option<String>) -> StartFailureRecord {
        StartFailureRecord {
            schema: START_FAILURE_SCHEMA,
            at: Timestamp::from_jiff(jiff::Timestamp::UNIX_EPOCH),
            exit_code: 1,
            code: START_FAILED_CODE.to_owned(),
            message,
            version: "0.1.0".to_owned(),
            launch: LaunchKind::Manual,
            bundle_path,
        }
    }

    #[test]
    fn a_record_is_camel_case_json_with_a_final_newline() {
        let bytes = record("no".to_owned(), None).to_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.ends_with("}\n"));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["exitCode"], 1);
        assert_eq!(value["code"], "daemon.start_failed");
        assert_eq!(value["launch"], "manual");
        assert_eq!(value["bundlePath"], serde_json::Value::Null);
        let at: jiff::Timestamp = value["at"].as_str().unwrap().parse().unwrap();
        assert_eq!(at, jiff::Timestamp::UNIX_EPOCH);
    }

    #[test]
    fn a_record_never_holds_more_than_four_kibibytes() {
        let escaped = "\u{1}".repeat(START_FAILURE_MESSAGE_MAX);
        let bytes = record(escaped, Some("/a".repeat(2000))).to_bytes();
        assert!(bytes.len() <= START_FAILURE_MAX_LEN, "{}", bytes.len());
        let parsed: StartFailureRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.code, START_FAILED_CODE);
    }

    #[test]
    fn the_message_is_redacted_then_cut() {
        let failure = StartFailure::new(
            None,
            format!(
                "cannot open postgresql://stackly:hunter2@127.0.0.1/x {}",
                "y".repeat(600)
            ),
        );
        let made = StartFailureRecord::new(
            &failure,
            &Redactor::new(),
            Timestamp::from_jiff(jiff::Timestamp::UNIX_EPOCH),
            "0.1.0",
            &LaunchContext::detect(None, 4242),
            None,
        );
        assert!(!made.message.contains("hunter2"), "{}", made.message);
        assert_eq!(made.message.chars().count(), START_FAILURE_MESSAGE_MAX);
        assert_eq!(made.launch, LaunchKind::Manual);
    }
}
