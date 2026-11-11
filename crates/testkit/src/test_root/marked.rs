//! Test roots that carry an ownership marker, for the provider conformance
//! harness (spec 0036, *Root dan kepemilikan*, AC-12).
//!
//! A marked root is never removed on drop. It is removed only through
//! [`TestRoot::remove_checked`], which deletes a folder only when every
//! ownership check holds, or by [`sweep_marked`] once the run that made it
//! is gone. [`TestRoot::new`] and its drop are unchanged.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{PREFIX, RUN_PREFIX, TestRoot, run_base};
use crate::reap_guard::REAP_GRACE;
use crate::{ProcessEntry, ProcessTable};

/// The marker file in both folders of a marked root.
pub const MARKER_FILE: &str = ".stackly-test-run";
/// The `schema` of the marker.
pub const MARKER_SCHEMA: &str = "stackly.test-run/v1";

/// Why [`TestRoot::remove_checked`] left a folder in place.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CleanupRefused {
    /// The folder that was left.
    pub path: PathBuf,
    /// The first ownership check that failed, in lowercase words.
    pub reason: String,
}

impl std::fmt::Display for CleanupRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} was not removed: {}",
            self.path.display(),
            self.reason
        )
    }
}

impl std::error::Error for CleanupRefused {}

/// The two kinds of folder a marked root has, with the canonical parent
/// and the name prefix each must have.
#[derive(Debug, Clone, Copy)]
enum Folder {
    /// The data folder under the system temporary directory.
    Data,
    /// The short run folder under `/private/tmp`.
    Run,
}

impl Folder {
    fn base(self) -> io::Result<PathBuf> {
        match self {
            Self::Data => std::env::temp_dir().canonicalize(),
            Self::Run => run_base(),
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::Data => PREFIX,
            Self::Run => RUN_PREFIX,
        }
    }
}

impl TestRoot {
    /// Creates a test root like [`new`](Self::new), writes the marker
    /// [`MARKER_FILE`] (`{ schema, runId, uid, pid, createdAt }`) in both of
    /// its folders, and turns off removal on drop: only
    /// [`remove_checked`](Self::remove_checked) or [`sweep_marked`] delete
    /// it.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when a folder or a marker cannot be created.
    pub fn new_marked(run_id: &str) -> io::Result<Self> {
        let mut root = Self::new()?;
        root.dir.disable_cleanup(true);
        root.run.disable_cleanup(true);
        root.run_id = Some(run_id.to_owned());
        let uid = fs::metadata(root.path())?.uid();
        let marker = serde_json::json!({
            "schema": MARKER_SCHEMA,
            "runId": run_id,
            "uid": uid,
            "pid": std::process::id(),
            "createdAt": jiff::Timestamp::now().to_string(),
        });
        let text = format!("{marker:#}\n");
        for dir in [root.path(), root.run_dir()] {
            fs::write(dir.join(MARKER_FILE), &text)?;
        }
        Ok(root)
    }

    /// Removes both folders of a marked root, each only when all of these
    /// hold: its canonical parent is the system temporary directory (data)
    /// or `/private/tmp` (run); its name starts with `stackly-test-` or
    /// `skt-`; it is a folder, not a symlink; this process's uid owns it;
    /// its marker reads with `run_id` and that uid; and no process names
    /// the folder in its arguments. A folder that fails a check stays.
    ///
    /// # Errors
    ///
    /// Returns the first folder that was left and why. The other folder is
    /// still removed when its own checks hold.
    pub fn remove_checked(self) -> Result<(), CleanupRefused> {
        let (data, run, run_id) = self.into_marked()?;
        let first = remove_folder(&data, Folder::Data, Some(&run_id));
        let second = remove_folder(&run, Folder::Run, Some(&run_id));
        first.and(second)
    }

    /// Returns the run ID this marked root was made with, or none for a
    /// root from [`new`](Self::new).
    #[must_use]
    pub fn run_id(&self) -> Option<&str> {
        self.run_id.as_deref()
    }

    /// The two folders and the run ID of a marked root, with the handles
    /// dropped (which deletes nothing, since removal on drop is off).
    fn into_marked(self) -> Result<(PathBuf, PathBuf, String), CleanupRefused> {
        let data = self.path().to_path_buf();
        let run = self.run_dir().to_path_buf();
        match self.run_id.clone() {
            Some(run_id) => {
                drop(self);
                Ok((data, run, run_id))
            }
            None => Err(CleanupRefused {
                path: data,
                reason: "the root carries no marker; it is removed on drop".to_owned(),
            }),
        }
    }

    /// Keeps the data folder of a marked root and removes its run folder
    /// under the checks of [`remove_checked`](Self::remove_checked), for
    /// `--keep`. Returns the kept data folder.
    ///
    /// # Errors
    ///
    /// Returns the run folder and why when it was left.
    pub fn keep_data(self) -> Result<PathBuf, CleanupRefused> {
        let (data, run, run_id) = self.into_marked()?;
        remove_folder(&run, Folder::Run, Some(&run_id)).map(|()| data)
    }
}

/// Removes every marked root folder of this uid whose run is gone: the
/// checks of [`TestRoot::remove_checked`] with any `runId`, and the `pid`
/// of the marker no longer alive. The processes still running in such a
/// folder are stopped first, the way [`ReapGuard`](crate::ReapGuard) does.
/// Returns the folders it removed; a folder that fails a check, or cannot
/// be read, is left alone.
#[must_use]
pub fn sweep_marked() -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for folder in [Folder::Data, Folder::Run] {
        let Ok(base) = folder.base() else { continue };
        let Ok(entries) = fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let named = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(folder.prefix()));
            if !named {
                continue;
            }
            let Ok(marker) = check_folder(&path, folder, None) else {
                continue;
            };
            if pid_alive(marker.pid) {
                continue;
            }
            if ProcessTable::reap_under(&path, REAP_GRACE).is_err() {
                continue;
            }
            if remove_folder(&path, folder, None).is_ok() {
                removed.push(path);
            }
        }
    }
    removed
}

/// What a marker says, as far as the checks need it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Marker {
    run_id: String,
    uid: u32,
    pid: u32,
}

/// Runs every ownership check on `path` and removes it when they hold.
fn remove_folder(path: &Path, folder: Folder, run_id: Option<&str>) -> Result<(), CleanupRefused> {
    let refused = |reason: String| CleanupRefused {
        path: path.to_path_buf(),
        reason,
    };
    check_folder(path, folder, run_id).map_err(refused)?;
    let holders =
        holders(path).map_err(|err| refused(format!("cannot read the process table: {err}")))?;
    if !holders.is_empty() {
        let pids: Vec<String> = holders.iter().map(|entry| entry.pid.to_string()).collect();
        return Err(refused(format!(
            "processes {} still name the folder",
            pids.join(", ")
        )));
    }
    fs::remove_dir_all(path).map_err(|err| refused(format!("cannot remove the folder: {err}")))
}

/// The ownership checks of *Root dan kepemilikan* except the process one.
/// With `run_id`, the marker must carry it.
fn check_folder(path: &Path, folder: Folder, run_id: Option<&str>) -> Result<Marker, String> {
    let base = folder
        .base()
        .map_err(|err| format!("cannot resolve the temporary directory: {err}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "the folder has no parent".to_owned())?
        .canonicalize()
        .map_err(|err| format!("cannot resolve the parent folder: {err}"))?;
    if parent != base {
        return Err(format!("the folder is not directly in {}", base.display()));
    }
    let named = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(folder.prefix()));
    if !named {
        return Err(format!(
            "the folder name does not start with {}",
            folder.prefix()
        ));
    }
    let meta =
        fs::symlink_metadata(path).map_err(|err| format!("cannot read the folder: {err}"))?;
    if meta.file_type().is_symlink() {
        return Err("the path is a symlink".to_owned());
    }
    if !meta.is_dir() {
        return Err("the path is not a folder".to_owned());
    }
    let uid = own_uid().map_err(|err| format!("cannot read this process's uid: {err}"))?;
    if meta.uid() != uid {
        return Err(format!(
            "the folder belongs to uid {}, not {uid}",
            meta.uid()
        ));
    }
    let marker = read_marker(&path.join(MARKER_FILE))?;
    if marker.uid != uid {
        return Err(format!("the marker names uid {}, not {uid}", marker.uid));
    }
    if let Some(run_id) = run_id
        && marker.run_id != run_id
    {
        return Err("the marker belongs to another run".to_owned());
    }
    Ok(marker)
}

/// Reads a marker file, which must be a regular file of [`MARKER_SCHEMA`].
fn read_marker(path: &Path) -> Result<Marker, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "the folder has no marker".to_owned())?;
    if !meta.is_file() {
        return Err("the marker is not a regular file".to_owned());
    }
    let text = fs::read_to_string(path).map_err(|err| format!("cannot read the marker: {err}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "the marker is not JSON".to_owned())?;
    if value["schema"] != MARKER_SCHEMA {
        return Err("the marker has another schema".to_owned());
    }
    let number = |field: &str| {
        value[field]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("the marker has no {field}"))
    };
    Ok(Marker {
        run_id: value["runId"]
            .as_str()
            .ok_or_else(|| "the marker has no runId".to_owned())?
            .to_owned(),
        uid: number("uid")?,
        pid: number("pid")?,
    })
}

/// The processes whose arguments name `path`, without this process.
fn holders(path: &Path) -> io::Result<Vec<ProcessEntry>> {
    Ok(ProcessTable::snapshot()?
        .under(path)
        .into_iter()
        .cloned()
        .collect())
}

/// The uid of this process, read from a file it creates, so testkit needs
/// no `unsafe`.
fn own_uid() -> io::Result<u32> {
    let probe = tempfile::tempfile()?;
    Ok(probe.metadata()?.uid())
}

/// Whether a process with `pid` exists (`ps -p <pid>` succeeds).
fn pid_alive(pid: u32) -> bool {
    Command::new("/bin/ps")
        .args(["-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite_marker(dir: &Path, field: &str, value: serde_json::Value) {
        let path = dir.join(MARKER_FILE);
        let mut marker: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        marker[field] = value;
        fs::write(path, marker.to_string()).unwrap();
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn a_marked_root_carries_the_marker_in_both_folders_and_survives_drop() {
        let root = TestRoot::new_marked("run-1").unwrap();
        let data = root.path().to_path_buf();
        let run = root.run_dir().to_path_buf();
        for dir in [&data, &run] {
            let marker = read_marker(&dir.join(MARKER_FILE)).unwrap();
            assert_eq!(marker.run_id, "run-1");
            assert_eq!(marker.pid, std::process::id());
            assert_eq!(marker.uid, own_uid().unwrap());
        }

        drop(root);

        assert!(data.is_dir() && run.is_dir());
        for (dir, folder) in [(&data, Folder::Data), (&run, Folder::Run)] {
            remove_folder(dir, folder, Some("run-1")).unwrap();
        }
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn remove_checked_removes_both_folders_of_its_own_run() {
        let root = TestRoot::new_marked("run-2").unwrap();
        let data = root.path().to_path_buf();
        let run = root.run_dir().to_path_buf();
        fs::create_dir_all(data.join("data/instances")).unwrap();

        root.remove_checked().unwrap();

        assert!(!data.exists() && !run.exists());
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn a_root_of_another_run_or_without_a_marker_is_left_in_place() {
        let root = TestRoot::new_marked("run-3").unwrap();
        let data = root.path().to_path_buf();
        let run = root.run_dir().to_path_buf();
        fs::remove_file(run.join(MARKER_FILE)).unwrap();

        rewrite_marker(&data, "runId", serde_json::json!("another-run"));

        let refused = root.remove_checked().unwrap_err();

        assert_eq!(refused.path, data);
        assert_eq!(refused.reason, "the marker belongs to another run");
        assert!(data.is_dir() && run.is_dir());
        assert_eq!(
            remove_folder(&run, Folder::Run, Some("run-3"))
                .unwrap_err()
                .reason,
            "the folder has no marker"
        );
        remove_folder(&data, Folder::Data, Some("another-run")).unwrap();
        fs::remove_dir_all(&run).unwrap();
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn a_marker_with_another_uid_or_schema_is_refused() {
        let root = TestRoot::new_marked("run-4").unwrap();
        let data = root.path().to_path_buf();
        let run = root.run_dir().to_path_buf();
        rewrite_marker(&data, "uid", serde_json::json!(own_uid().unwrap() + 1));
        rewrite_marker(&run, "schema", serde_json::json!("other/v1"));

        let reason = |dir: &Path, folder| check_folder(dir, folder, Some("run-4")).unwrap_err();

        assert!(reason(&data, Folder::Data).starts_with("the marker names uid"));
        assert_eq!(reason(&run, Folder::Run), "the marker has another schema");
        drop(root);
        fs::remove_dir_all(&data).unwrap();
        fs::remove_dir_all(&run).unwrap();
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn a_symlink_or_a_folder_outside_the_temporary_directory_is_refused() {
        let root = TestRoot::new_marked("run-5").unwrap();
        let base = std::env::temp_dir().canonicalize().unwrap();
        let link = base.join(format!("{PREFIX}link-{}", std::process::id()));
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        let nested = root.path().join(format!("{PREFIX}nested"));
        fs::create_dir(&nested).unwrap();
        fs::copy(root.path().join(MARKER_FILE), nested.join(MARKER_FILE)).unwrap();
        let unnamed = tempfile::Builder::new()
            .prefix("other-")
            .tempdir_in(&base)
            .unwrap();
        fs::copy(
            root.path().join(MARKER_FILE),
            unnamed.path().join(MARKER_FILE),
        )
        .unwrap();

        assert_eq!(
            remove_folder(&link, Folder::Data, Some("run-5"))
                .unwrap_err()
                .reason,
            "the path is a symlink"
        );
        assert!(
            remove_folder(&nested, Folder::Data, Some("run-5"))
                .unwrap_err()
                .reason
                .starts_with("the folder is not directly in")
        );
        assert!(
            remove_folder(unnamed.path(), Folder::Data, Some("run-5"))
                .unwrap_err()
                .reason
                .starts_with("the folder name does not start with")
        );
        assert!(root.path().join(MARKER_FILE).is_file() && nested.is_dir());
        fs::remove_file(&link).unwrap();
        root.remove_checked().unwrap();
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn a_folder_that_a_process_still_names_is_left_until_the_process_ends() {
        let root = TestRoot::new_marked("run-6").unwrap();
        let data = root.path().to_path_buf();
        let run = root.run_dir().to_path_buf();
        let mut child = Command::new("/bin/sh")
            // `read` is a shell builtin, so the one process that names the
            // folder is the shell itself, blocked on its open stdin.
            .args(["-c", "read line", "holder"])
            .arg(&data)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        ProcessTable::wait_for(
            &[&data.to_string_lossy()],
            std::time::Duration::from_secs(5),
            |rows| !rows.is_empty(),
        )
        .unwrap();

        let refused = root.remove_checked().unwrap_err();

        assert_eq!(refused.path, data);
        assert!(
            refused.reason.contains("still name the folder"),
            "{}",
            refused.reason
        );
        assert!(data.is_dir() && !run.exists());
        ProcessTable::reap_under(&data, REAP_GRACE).unwrap();
        child.wait().unwrap();
        remove_folder(&data, Folder::Data, Some("run-6")).unwrap();
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn an_unmarked_root_is_refused_and_still_removed_on_drop() {
        let root = TestRoot::new().unwrap();
        let data = root.path().to_path_buf();

        let refused = root.remove_checked().unwrap_err();

        assert!(refused.reason.contains("no marker"), "{}", refused.reason);
        assert!(!data.exists());
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn keep_data_keeps_the_data_folder_and_removes_the_run_folder() {
        let root = TestRoot::new_marked("run-7").unwrap();
        let run = root.run_dir().to_path_buf();

        let data = root.keep_data().unwrap();

        assert!(data.is_dir() && !run.exists());
        remove_folder(&data, Folder::Data, Some("run-7")).unwrap();
    }

    // covers: AC-12 (spec 0036)
    #[test]
    fn sweep_removes_a_root_whose_run_is_gone_and_keeps_one_whose_run_lives() {
        let gone = TestRoot::new_marked("run-gone").unwrap();
        let alive = TestRoot::new_marked("run-alive").unwrap();
        // A pid that is not running: one past the highest pid macOS hands out.
        for dir in [gone.path(), gone.run_dir()] {
            rewrite_marker(dir, "pid", serde_json::json!(99_999_999_u32));
        }
        let gone_dirs = [gone.path().to_path_buf(), gone.run_dir().to_path_buf()];
        let alive_dirs = [alive.path().to_path_buf(), alive.run_dir().to_path_buf()];

        let removed = sweep_marked();

        for dir in &gone_dirs {
            assert!(removed.contains(dir), "{removed:?}");
            assert!(!dir.exists());
        }
        for dir in &alive_dirs {
            assert!(!removed.contains(dir));
            assert!(dir.is_dir());
        }
        drop(gone);
        alive.remove_checked().unwrap();
    }
}
