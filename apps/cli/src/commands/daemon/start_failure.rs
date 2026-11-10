//! The record of a failed daemon start (spec 0034, *Catatan gagal start
//! daemon*, AC-11): `<data>/daemon-start-failure.json` of the active root,
//! `STACKLY_ROOT` included. `stacklyd` writes it when it exits 1 after it
//! knew its data root, because launchd keeps no reason a client could read;
//! the CLI only reads it.
//!
//! The read follows the launch record (at most 4 KiB, and a file that does
//! not read is no record), plus: a file that is not a regular file, a
//! symlink, a file of another user, or a file with any mode bit beyond 0600
//! is no record either. The file is opened and then checked again through
//! its open descriptor, so a file swapped in between the two looks is no
//! record.

use std::path::PathBuf;

use serde::Deserialize;
use stackly_protocol::Timestamp;

/// The most bytes read from the record.
pub const START_FAILURE_MAX_LEN: u64 = 4096;

/// What the record says about the failed start. Fields this build does
/// not read are ignored, so a newer daemon can add some.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StartFailureRecord {
    /// When the start failed.
    pub at: Timestamp,
    /// The catalog code of the error, or `daemon.start_failed`.
    pub code: String,
    /// The error text, redacted by the daemon and at most 512 characters.
    pub message: String,
    /// The version of the daemon that failed.
    #[serde(default)]
    pub version: Option<String>,
    /// `launchd` or `manual`.
    #[serde(default)]
    pub launch: Option<String>,
}

/// The record file of the active root, read on demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartFailureFile {
    path: Option<PathBuf>,
    uid: u32,
}

impl StartFailureFile {
    /// The record at `path`, which only counts when `uid` owns it; `None`
    /// makes every read empty.
    #[must_use]
    pub const fn new(path: Option<PathBuf>, uid: u32) -> Self {
        Self { path, uid }
    }

    /// A file that is never there, for a CLI that does not know its root.
    #[must_use]
    pub const fn none() -> Self {
        Self::new(None, 0)
    }

    /// The record, or `None` when it is missing, not a private regular
    /// file of this user, above 4 KiB, or not a record.
    #[must_use]
    pub fn read(&self) -> Option<StartFailureRecord> {
        let path = self.path.as_deref()?;
        let bytes = read_private(path, self.uid)?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// The bytes of `path` when it is a regular file of `uid` with no mode bit
/// beyond 0600, both before it is opened and through the open descriptor.
#[cfg(unix)]
fn read_private(path: &std::path::Path, uid: u32) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;

    let private = |meta: &std::fs::Metadata| {
        meta.file_type().is_file() && meta.uid() == uid && meta.mode() & 0o7777 & !0o600 == 0
    };
    let before = std::fs::symlink_metadata(path).ok()?;
    if !private(&before) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let opened = file.metadata().ok()?;
    if !private(&opened) || opened.dev() != before.dev() || opened.ino() != before.ino() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(START_FAILURE_MAX_LEN + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= START_FAILURE_MAX_LEN).then_some(bytes)
}

/// Only unix has owners and modes to check, so elsewhere there is never a
/// record.
#[cfg(not(unix))]
fn read_private(_path: &std::path::Path, _uid: u32) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs::Permissions;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    use super::*;

    const RECORD: &str = r#"{
  "schema": 1,
  "at": "2026-11-10T08:00:00.000Z",
  "exitCode": 1,
  "code": "daemon.start_failed",
  "message": "the root override is writable by group or others",
  "version": "0.9.0",
  "launch": "manual",
  "bundlePath": null
}
"#;

    fn file_in(dir: &tempfile::TempDir, text: &str, mode: u32) -> (PathBuf, u32) {
        let path = dir.path().join("daemon-start-failure.json");
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(mode)).unwrap();
        let uid = std::fs::metadata(&path).unwrap().uid();
        (path, uid)
    }

    #[test]
    fn a_private_record_of_this_user_reads_with_its_time_code_and_message() {
        let dir = tempfile::tempdir().unwrap();
        let (path, uid) = file_in(&dir, RECORD, 0o600);

        let record = StartFailureFile::new(Some(path), uid).read().unwrap();

        assert_eq!(record.at, "2026-11-10T08:00:00.000Z".parse().unwrap());
        assert_eq!(record.code, "daemon.start_failed");
        assert_eq!(
            record.message,
            "the root override is writable by group or others"
        );
        assert_eq!(record.version.as_deref(), Some("0.9.0"));
        assert_eq!(record.launch.as_deref(), Some("manual"));
    }

    #[test]
    fn a_record_with_any_mode_bit_beyond_0600_is_no_record() {
        for mode in [0o644, 0o640, 0o604, 0o700, 0o610, 0o4600] {
            let dir = tempfile::tempdir().unwrap();
            let (path, uid) = file_in(&dir, RECORD, mode);
            assert_eq!(
                StartFailureFile::new(Some(path), uid).read(),
                None,
                "{mode:o}"
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let (path, uid) = file_in(&dir, RECORD, 0o400);
        assert!(StartFailureFile::new(Some(path), uid).read().is_some());
    }

    #[test]
    fn a_symlink_to_a_private_record_is_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let (target, uid) = file_in(&dir, RECORD, 0o600);
        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(StartFailureFile::new(Some(link), uid).read(), None);
    }

    #[test]
    fn a_record_of_another_uid_is_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let (path, uid) = file_in(&dir, RECORD, 0o600);

        assert_eq!(
            StartFailureFile::new(Some(path), uid.wrapping_add(1)).read(),
            None
        );
    }

    #[test]
    fn a_folder_a_missing_file_a_large_file_and_garbage_are_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let uid = std::fs::metadata(dir.path()).unwrap().uid();
        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(StartFailureFile::new(Some(folder), uid).read(), None);
        assert_eq!(
            StartFailureFile::new(Some(dir.path().join("missing.json")), uid).read(),
            None
        );
        assert_eq!(StartFailureFile::none().read(), None);

        let padded = RECORD.replacen(
            "\"manual\"",
            &format!("\"{}\"", "x".repeat(START_FAILURE_MAX_LEN as usize)),
            1,
        );
        let (path, uid) = file_in(&dir, &padded, 0o600);
        assert_eq!(StartFailureFile::new(Some(path), uid).read(), None);

        let other = tempfile::tempdir().unwrap();
        let (path, uid) = file_in(&other, "{\"at\": \"yesterday\"}", 0o600);
        assert_eq!(StartFailureFile::new(Some(path), uid).read(), None);
    }
}
