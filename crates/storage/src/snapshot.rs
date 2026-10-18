//! Verified snapshots taken with `VACUUM INTO` before a migration, in
//! `registry/pre-migration/v<from>-to-v<to>-<stamp>.sqlite`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rusqlite::Connection;
use stackly_platform::{DirectoryLayout, FileKind, Filesystem};

use crate::connection::{open_verify, read_int};
use crate::error::StorageError;
use crate::files::{LinkOutcome, RegistryFiles};
use crate::open::APPLICATION_ID;
use crate::recovery::{ReportSnapshot, platform_io};
use crate::time::{file_stamp, is_file_stamp};

/// How many snapshots stay after a successful migration, besides the ones a
/// recovery report names.
pub(crate) const SNAPSHOTS_KEPT: usize = 3;

/// Takes a snapshot of the registry `conn` writes, publishes it without
/// replacing any file, and verifies it. Returns a description of what
/// failed otherwise.
pub(crate) fn take(
    conn: &Connection,
    files: &RegistryFiles,
    layout: &DirectoryLayout,
    from: u32,
    to: u32,
    now: Timestamp,
) -> Result<ReportSnapshot, String> {
    let dir = layout.registry_snapshots_dir();
    let base = format!("v{from}-to-v{to}-{}", file_stamp(now));
    let temp = dir.join(format!(".{base}.sqlite.tmp"));
    let temp_text = temp
        .to_str()
        .ok_or_else(|| format!("{} is not UTF-8", temp.display()))?;
    files.remove(&temp).map_err(|err| err.to_string())?;
    conn.execute("VACUUM INTO ?1", [temp_text])
        .map_err(|err| format!("VACUUM INTO {}: {err}", temp.display()))?;
    let published = publish(files, &dir, &temp, &base);
    let removed = files.remove(&temp);
    let path = published?;
    removed.map_err(|err| err.to_string())?;
    files.sync_dir(&dir).map_err(|err| err.to_string())?;
    let sha256 = verify(files, &path, from)?;
    Ok(ReportSnapshot {
        path,
        sha256,
        schema_version: from,
        created_at: now,
    })
}

fn publish(files: &RegistryFiles, dir: &Path, temp: &Path, base: &str) -> Result<PathBuf, String> {
    files.sync_file(temp).map_err(|err| err.to_string())?;
    files.make_private(temp).map_err(|err| err.to_string())?;
    for suffix in 1_u32.. {
        let name = if suffix == 1 {
            format!("{base}.sqlite")
        } else {
            format!("{base}-{suffix}.sqlite")
        };
        let path = dir.join(name);
        match files
            .hard_link(temp, &path)
            .map_err(|err| err.to_string())?
        {
            LinkOutcome::Linked => return Ok(path),
            LinkOutcome::TargetExists => {}
            LinkOutcome::SourceMissing => {
                return Err(format!(
                    "{} vanished before it was published",
                    temp.display()
                ));
            }
        }
    }
    Err("no free snapshot name".to_owned())
}

/// Checks a snapshot through a read only connection (`quick_check` ok,
/// `user_version` equal to `version`, Stackly's `application_id`) and
/// returns its SHA-256.
pub(crate) fn verify(files: &RegistryFiles, path: &Path, version: u32) -> Result<String, String> {
    let failed = |err: rusqlite::Error| format!("cannot verify {}: {err}", path.display());
    {
        let conn = open_verify(path).map_err(failed)?;
        let check: String = conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(failed)?;
        if check != "ok" {
            return Err(format!("{} failed quick_check: {check}", path.display()));
        }
        let user_version = read_int(&conn, "user_version").map_err(failed)?;
        if user_version != i64::from(version) {
            return Err(format!(
                "{} holds schema version {user_version}, expected {version}",
                path.display()
            ));
        }
        let application_id = read_int(&conn, "application_id").map_err(failed)?;
        if application_id != i64::from(APPLICATION_ID) {
            return Err(format!(
                "{} has application_id {application_id:#x}",
                path.display()
            ));
        }
    }
    files
        .sha256(path)
        .map(|(_, sha)| sha)
        .map_err(|err| format!("cannot hash {}: {err}", path.display()))
}

/// Deletes the snapshots beyond the [`SNAPSHOTS_KEPT`] newest, ordered by
/// the time in their name and then the target version, except the ones in
/// `protected`.
pub(crate) fn prune(
    fs: &dyn Filesystem,
    files: &RegistryFiles,
    layout: &DirectoryLayout,
    protected: &HashSet<PathBuf>,
) -> Result<Vec<PathBuf>, StorageError> {
    let dir = layout.registry_snapshots_dir();
    let mut snapshots: Vec<(SnapshotName, PathBuf)> = fs
        .read_dir(&dir)
        .map_err(platform_io(&dir))?
        .into_iter()
        .filter(|entry| entry.info.kind == FileKind::File)
        .filter_map(|entry| {
            let name = entry.name.to_str()?;
            Some((SnapshotName::parse(name)?, dir.join(name)))
        })
        .collect();
    snapshots.sort_by(|a, b| b.0.cmp(&a.0));
    let mut removed = Vec::new();
    for (_, path) in snapshots.into_iter().skip(SNAPSHOTS_KEPT) {
        if !protected.contains(&path) {
            files.remove(&path)?;
            removed.push(path);
        }
    }
    if !removed.is_empty() {
        files.sync_dir(&dir)?;
    }
    Ok(removed)
}

/// The parts of a snapshot file name, ordered by stamp, then target
/// version, then suffix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SnapshotName {
    stamp: String,
    to: u32,
    suffix: u32,
    from: u32,
}

impl SnapshotName {
    /// Parses `v<from>-to-v<to>-<stamp>.sqlite` or
    /// `v<from>-to-v<to>-<stamp>-<n>.sqlite`.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let rest = name.strip_suffix(".sqlite")?.strip_prefix('v')?;
        let (from, rest) = rest.split_once("-to-v")?;
        let (to, rest) = rest.split_once('-')?;
        let (stamp, suffix) = match rest.split_once('-') {
            Some((stamp, suffix)) => (stamp, suffix.parse().ok().filter(|n| *n >= 2)?),
            None => (rest, 1),
        };
        is_file_stamp(stamp).then_some(())?;
        Some(Self {
            stamp: stamp.to_owned(),
            to: to.parse().ok()?,
            suffix,
            from: from.parse().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_names_parse_and_sort_by_time_then_target_version() {
        let mut names: Vec<_> = [
            "v1-to-v2-20261018T054200Z.sqlite",
            "v2-to-v3-20261018T054200Z.sqlite",
            "v1-to-v2-20261018T054200Z-2.sqlite",
            "v1-to-v2-20261017T000000Z.sqlite",
        ]
        .into_iter()
        .map(|name| SnapshotName::parse(name).unwrap())
        .collect();
        names.sort();
        let order: Vec<_> = names
            .iter()
            .map(|n| (n.stamp.as_str(), n.from, n.to, n.suffix))
            .collect();

        assert_eq!(
            order,
            [
                ("20261017T000000Z", 1, 2, 1),
                ("20261018T054200Z", 1, 2, 1),
                ("20261018T054200Z", 1, 2, 2),
                ("20261018T054200Z", 2, 3, 1),
            ]
        );
        for bad in [
            ".v1-to-v2-20261018T054200Z.sqlite.tmp",
            "v1-to-v2-20261018T054200Z-1.sqlite",
            "v1-to-v2-2026.sqlite",
            "stackly.sqlite",
        ] {
            assert!(SnapshotName::parse(bad).is_none(), "{bad}");
        }
    }
}
