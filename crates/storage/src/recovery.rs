//! Recovery reports: why an open did not reach ready, written atomically
//! as JSON to `registry/recovery/` and never deleted automatically.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use stackly_platform::{DirectoryLayout, FileKind, Filesystem};

use crate::error::StorageError;
use crate::files::RegistryFiles;
use crate::time::{file_stamp, is_file_stamp};

/// The `schema` value of a recovery report.
pub const RECOVERY_REPORT_SCHEMA: &str = "stackly.registry-recovery/v1";
/// Largest recovery report read back, in bytes.
const REPORT_MAX_LEN: u64 = 1024 * 1024;
/// Name of the WAL file next to the registry.
pub(crate) const WAL_SUFFIX: &str = "-wal";
/// Name of the shared memory file next to the registry.
pub(crate) const SHM_SUFFIX: &str = "-shm";

/// Why the registry is in recovery. Problems with the registry file or its
/// content land here; the registry is left exactly as it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "code",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum RecoveryReason {
    /// The file is not SQLite, or its `application_id` is not Stackly's.
    NotARegistry {
        /// What showed it.
        detail: String,
    },
    /// The registry, its WAL, its shared memory file, or its journal is a
    /// symlink or not a regular file.
    UnsafeRegistryFile {
        /// The file name, for example `stackly.sqlite-wal`.
        file: String,
        /// What it is instead.
        detail: String,
    },
    /// The file exists but cannot be opened.
    OpenFailed {
        /// The cause.
        detail: String,
    },
    /// `PRAGMA quick_check` failed, or `registry_info` is missing or
    /// invalid.
    IntegrityFailed {
        /// The first problems reported.
        detail: String,
    },
    /// The schema is newer than this app knows; the app never writes it.
    SchemaNewerThanApp,
    /// The migration history has a gap, an unknown version, or a different
    /// checksum.
    MigrationHistoryMismatch {
        /// The first difference found.
        detail: String,
    },
    /// A table, column, index, or trigger was changed outside the runner.
    SchemaFingerprintMismatch {
        /// The fingerprint this app expects for the schema version.
        expected: String,
        /// The fingerprint found.
        actual: String,
    },
    /// The registry file is missing, but a WAL, a snapshot, a report, or
    /// data from an earlier registry is still there.
    RegistryMissing {
        /// Every leftover found.
        evidence: Vec<String>,
    },
    /// Another process held a write transaction past the busy timeout.
    RegistryLocked,
    /// A migration failed; the transaction was rolled back.
    MigrationFailed {
        /// The schema version before the attempt.
        from: u32,
        /// The version the attempt aimed for.
        to: u32,
        /// The migration that failed, or `to` when a check after the last
        /// migration failed.
        failed_version: u32,
        /// What failed.
        error: String,
    },
    /// The snapshot before a migration could not be made or verified, so
    /// no migration was tried.
    BackupFailed {
        /// What failed.
        error: String,
    },
}

impl RecoveryReason {
    /// Returns the stable code, for example `migration_failed`.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotARegistry { .. } => "not_a_registry",
            Self::UnsafeRegistryFile { .. } => "unsafe_registry_file",
            Self::OpenFailed { .. } => "open_failed",
            Self::IntegrityFailed { .. } => "integrity_failed",
            Self::SchemaNewerThanApp => "schema_newer_than_app",
            Self::MigrationHistoryMismatch { .. } => "migration_history_mismatch",
            Self::SchemaFingerprintMismatch { .. } => "schema_fingerprint_mismatch",
            Self::RegistryMissing { .. } => "registry_missing",
            Self::RegistryLocked => "registry_locked",
            Self::MigrationFailed { .. } => "migration_failed",
            Self::BackupFailed { .. } => "backup_failed",
        }
    }

    /// Returns a sentence for the person reading the report.
    fn message(&self) -> &'static str {
        match self {
            Self::NotARegistry { .. } => {
                "the registry file is not a Stackly registry; it was left untouched"
            }
            Self::UnsafeRegistryFile { .. } => {
                "a registry file is a symlink or not a regular file; it was not opened"
            }
            Self::OpenFailed { .. } => "the registry file exists but cannot be opened",
            Self::IntegrityFailed { .. } => {
                "the registry failed its integrity check; restore it from a snapshot or backup"
            }
            Self::SchemaNewerThanApp => {
                "the registry was written by a newer Stackly; install that version or restore a \
                 matching snapshot"
            }
            Self::MigrationHistoryMismatch { .. } => {
                "the registry's migration history does not match this app"
            }
            Self::SchemaFingerprintMismatch { .. } => {
                "the registry schema was changed outside Stackly's migrations"
            }
            Self::RegistryMissing { .. } => {
                "the registry file is missing but data from an earlier registry remains; a new \
                 registry was not created"
            }
            Self::RegistryLocked => "another process holds a write lock on the registry",
            Self::MigrationFailed { .. } => {
                "a registry migration failed and was rolled back; the registry and its snapshot \
                 are unchanged"
            }
            Self::BackupFailed { .. } => {
                "the snapshot before a migration could not be made, so no migration was tried"
            }
        }
    }
}

/// Size and digest of one registry file when the problem was detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReportFile {
    /// The file name, for example `stackly.sqlite`.
    pub name: String,
    /// The size in bytes.
    pub len: u64,
    /// The SHA-256, for the registry and its WAL when they could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// The verified snapshot a report points to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReportSnapshot {
    /// The snapshot file.
    pub path: PathBuf,
    /// Its SHA-256.
    pub sha256: String,
    /// The schema version it holds.
    pub schema_version: u32,
    /// When it was made.
    pub created_at: Timestamp,
}

/// A recovery report: the reason, the versions involved, the state of the
/// registry files, and the snapshot to restore from, if any.
///
/// The daemon carries it in recovery mode; `path` names the JSON file,
/// which may be an earlier report with the same reason and file digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RecoveryReport {
    /// The report file. Not part of the JSON.
    #[serde(skip)]
    pub path: PathBuf,
    /// Always [`RECOVERY_REPORT_SCHEMA`].
    pub schema: String,
    /// When the problem was detected.
    pub detected_at: Timestamp,
    /// Why the registry is in recovery.
    pub reason: RecoveryReason,
    /// The app version that detected it.
    pub app_version: String,
    /// The newest schema version this app knows.
    pub app_schema_version: u32,
    /// The registry's schema version, when it could be read.
    pub db_schema_version: Option<u32>,
    /// The bundled SQLite version.
    pub sqlite_version: String,
    /// The registry files as they were.
    pub files: Vec<ReportFile>,
    /// The snapshot taken before a migration, if any.
    pub snapshot: Option<ReportSnapshot>,
    /// A sentence for the person reading it.
    pub message: String,
}

/// What an open knew when it ended in recovery.
#[derive(Debug, Clone)]
pub(crate) struct Detection {
    pub(crate) reason: RecoveryReason,
    pub(crate) db_schema_version: Option<u32>,
    pub(crate) snapshot: Option<Box<ReportSnapshot>>,
}

impl Detection {
    pub(crate) const fn new(reason: RecoveryReason) -> Self {
        Self {
            reason,
            db_schema_version: None,
            snapshot: None,
        }
    }

    pub(crate) const fn with_version(mut self, version: u32) -> Self {
        self.db_schema_version = Some(version);
        self
    }
}

/// The context every report shares.
#[derive(Clone, Copy)]
pub(crate) struct ReportContext<'a> {
    pub(crate) fs: &'a dyn Filesystem,
    pub(crate) files: &'a RegistryFiles,
    pub(crate) layout: &'a DirectoryLayout,
    pub(crate) app_version: &'a str,
    pub(crate) app_schema_version: u32,
}

impl ReportContext<'_> {
    /// Returns the report for `detection`: the latest report on disk when it
    /// has the same reason code, both schema versions, and the same
    /// registry and WAL digests, otherwise a new report written atomically.
    pub(crate) fn report(
        &self,
        detection: Detection,
        now: Timestamp,
    ) -> Result<RecoveryReport, StorageError> {
        let files = self.registry_files()?;
        let report = RecoveryReport {
            path: PathBuf::new(),
            schema: RECOVERY_REPORT_SCHEMA.to_owned(),
            detected_at: now,
            message: detection.reason.message().to_owned(),
            reason: detection.reason,
            app_version: self.app_version.to_owned(),
            app_schema_version: self.app_schema_version,
            db_schema_version: detection.db_schema_version,
            sqlite_version: rusqlite::version().to_owned(),
            files,
            snapshot: detection.snapshot.map(|snapshot| *snapshot),
        };
        if let Some(latest) = self.latest_report()?
            && same_problem(&latest, &report)
        {
            return Ok(latest);
        }
        self.write(report, now)
    }

    /// Size and digest of `stackly.sqlite` and its WAL as they are now, plus
    /// the size of the shared memory file. A symlink or other file that is
    /// not regular gets its size only, and so does a file that cannot be
    /// read.
    pub(crate) fn registry_files(&self) -> Result<Vec<ReportFile>, StorageError> {
        let db = self.layout.registry_db();
        let mut out = Vec::new();
        for (suffix, hashed) in [("", true), (WAL_SUFFIX, true), (SHM_SUFFIX, false)] {
            let path = with_suffix(&db, suffix);
            let Some(info) = self
                .fs
                .symlink_metadata(&path)
                .map_err(platform_io(&path))?
            else {
                continue;
            };
            let name = file_name(&path);
            let sha256 = if hashed && info.kind == FileKind::File {
                self.files.sha256(&path).ok().map(|(_, sha)| sha)
            } else {
                None
            };
            out.push(ReportFile {
                name,
                len: info.len,
                sha256,
            });
        }
        Ok(out)
    }

    /// Every report file in `recovery/`, oldest first, with its parsed
    /// content or `None` when it cannot be read or parsed.
    pub(crate) fn reports(&self) -> Result<Vec<(PathBuf, Option<RecoveryReport>)>, StorageError> {
        let dir = self.layout.registry_recovery_dir();
        let mut named: Vec<(ReportName, PathBuf)> = self
            .fs
            .read_dir(&dir)
            .map_err(platform_io(&dir))?
            .into_iter()
            .filter(|entry| entry.info.kind == FileKind::File)
            .filter_map(|entry| {
                let name = entry.name.to_str()?;
                Some((ReportName::parse(name)?, dir.join(name)))
            })
            .collect();
        named.sort();
        Ok(named
            .into_iter()
            .map(|(_, path)| {
                let report = self.read(&path);
                (path, report)
            })
            .collect())
    }

    fn latest_report(&self) -> Result<Option<RecoveryReport>, StorageError> {
        Ok(self
            .reports()?
            .into_iter()
            .next_back()
            .and_then(|(_, report)| report))
    }

    fn read(&self, path: &Path) -> Option<RecoveryReport> {
        let bytes = self.fs.read_file_limited(path, REPORT_MAX_LEN).ok()?;
        let mut report: RecoveryReport = serde_json::from_slice(&bytes).ok()?;
        if report.schema != RECOVERY_REPORT_SCHEMA {
            return None;
        }
        path.clone_into(&mut report.path);
        Some(report)
    }

    fn write(
        &self,
        mut report: RecoveryReport,
        now: Timestamp,
    ) -> Result<RecoveryReport, StorageError> {
        let dir = self.layout.registry_recovery_dir();
        let stamp = file_stamp(now);
        let mut suffix = 1_u32;
        let path = loop {
            let name = if suffix == 1 {
                format!("{stamp}.json")
            } else {
                format!("{stamp}-{suffix}.json")
            };
            let path = dir.join(name);
            if self
                .fs
                .symlink_metadata(&path)
                .map_err(platform_io(&path))?
                .is_none()
            {
                break path;
            }
            suffix += 1;
        };
        let mut bytes = serde_json::to_vec_pretty(&report).map_err(|err| StorageError::Io {
            op: "encode",
            path: path.clone(),
            detail: err.to_string(),
        })?;
        bytes.push(b'\n');
        self.fs
            .write_file_atomic(&path, &bytes)
            .map_err(platform_io(&path))?;
        report.path = path;
        Ok(report)
    }
}

/// Whether two reports describe the same problem on the same files.
fn same_problem(old: &RecoveryReport, new: &RecoveryReport) -> bool {
    old.reason.code() == new.reason.code()
        && old.app_schema_version == new.app_schema_version
        && old.db_schema_version == new.db_schema_version
        && same_contents(&old.files, &new.files)
}

/// Whether the registry and WAL contents in two file lists are the same.
pub(crate) fn same_contents(recorded: &[ReportFile], current: &[ReportFile]) -> bool {
    content_digest(recorded, "") == content_digest(current, "")
        && content_digest(recorded, WAL_SUFFIX) == content_digest(current, WAL_SUFFIX)
}

/// The digest of one registry file in a file list, where an absent file and
/// an empty one count as the same content: a read only connection may leave
/// an empty WAL behind.
fn content_digest(files: &[ReportFile], suffix: &str) -> Option<String> {
    let name = format!("stackly.sqlite{suffix}");
    files
        .iter()
        .find(|file| file.name == name && file.len > 0)
        .map(|file| file.sha256.clone().unwrap_or_default())
}

/// The name of a report file: `<stamp>.json` or `<stamp>-<n>.json`, ordered
/// by stamp and then `n`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ReportName {
    stamp: String,
    suffix: u32,
}

impl ReportName {
    fn parse(name: &str) -> Option<Self> {
        let stem = name.strip_suffix(".json")?;
        let (stamp, suffix) = match stem.split_once('-') {
            Some((stamp, suffix)) => (stamp, suffix.parse().ok().filter(|n| *n >= 2)?),
            None => (stem, 1),
        };
        is_file_stamp(stamp).then(|| Self {
            stamp: stamp.to_owned(),
            suffix,
        })
    }
}

/// `path` with `suffix` appended to its file name.
pub(crate) fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(crate) fn platform_io(path: &Path) -> impl Fn(stackly_platform::PlatformError) -> StorageError {
    let path = path.to_path_buf();
    move |err| StorageError::Io {
        op: "filesystem",
        path: path.clone(),
        detail: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_names_sort_by_stamp_then_suffix() {
        let mut names: Vec<_> = [
            "20261018T054200Z-2.json",
            "20261018T054200Z.json",
            "20261018T054159Z.json",
            "20261018T054200Z-10.json",
        ]
        .into_iter()
        .map(|name| ReportName::parse(name).unwrap())
        .collect();
        names.sort();
        let order: Vec<_> = names.iter().map(|n| (n.stamp.as_str(), n.suffix)).collect();

        assert_eq!(
            order,
            [
                ("20261018T054159Z", 1),
                ("20261018T054200Z", 1),
                ("20261018T054200Z", 2),
                ("20261018T054200Z", 10),
            ]
        );
        for bad in ["x.json", "20261018T054200Z-1.json", "20261018T054200Z.txt"] {
            assert!(ReportName::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn reasons_serialize_with_a_snake_case_code_and_camel_case_details() {
        let reason = RecoveryReason::MigrationFailed {
            from: 1,
            to: 2,
            failed_version: 2,
            error: "no such table: x".to_owned(),
        };
        let json = serde_json::to_value(&reason).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "code": "migration_failed",
                "from": 1,
                "to": 2,
                "failedVersion": 2,
                "error": "no such table: x"
            })
        );
        assert_eq!(
            serde_json::to_value(RecoveryReason::RegistryLocked).unwrap(),
            serde_json::json!({ "code": "registry_locked" })
        );
        let back: RecoveryReason = serde_json::from_value(json).unwrap();
        assert_eq!(back, reason);
    }
}
