//! The lock that keeps one daemon per data root (spec 0006, *Urutan start*
//! step 6).
//!
//! The lock is an exclusive `flock` on `<data>/stacklyd.lock`, taken through
//! [`File::try_lock`]. The daemon that holds it writes who it is into the
//! file, so a second daemon can say which process is in the way. The file
//! is never replaced through a rename, because the lock belongs to the
//! inode.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use stackly_protocol::{ErrorBody, Timestamp, codes};
use uuid::Uuid;

/// The most bytes a daemon reads from a lock file another daemon holds.
pub const LOCK_INFO_MAX: u64 = 4096;

/// Who holds the lock, as the holder wrote it into the lock file:
/// `{"pid","serverInstanceId","startedAt","version"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LockOwner {
    /// The holder's process ID.
    pub pid: u32,
    /// The holder's server instance ID, new at every daemon start.
    pub server_instance_id: Uuid,
    /// When the holder started.
    pub started_at: Timestamp,
    /// The holder's Stackly version.
    pub version: String,
}

impl LockOwner {
    /// Describes the daemon that is about to take the lock.
    #[must_use]
    pub fn new(
        pid: u32,
        server_instance_id: Uuid,
        started_at: Timestamp,
        version: impl Into<String>,
    ) -> Self {
        Self {
            pid,
            server_instance_id,
            started_at,
            version: version.into(),
        }
    }
}

/// Why the daemon lock could not be taken or released.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LockError {
    /// Another process holds the lock. `owner` is what it wrote into the
    /// file, or `None` when the contents cannot be read.
    #[error("another stacklyd holds the lock {}", path.display())]
    Held {
        /// The lock file.
        path: PathBuf,
        /// The holder, when its contents could be read.
        owner: Option<LockOwner>,
    },
    /// Something other than a regular file sits at the lock path.
    #[error("the daemon lock {} is not a regular file", path.display())]
    NotARegularFile {
        /// The lock file.
        path: PathBuf,
    },
    /// An OS call on the lock file failed.
    #[error("cannot {op} the daemon lock {}: {source}", path.display())]
    Io {
        /// The lock file.
        path: PathBuf,
        /// The step that failed: `open`, `inspect`, `lock`, `write`, or
        /// `unlock`.
        op: &'static str,
        /// The OS error.
        source: io::Error,
    },
}

impl LockError {
    /// Returns the `daemon.lock_held` error for [`LockError::Held`], with
    /// the holder's `pid`, `serverInstanceId`, `version`, and `startedAt`
    /// as details when they are known, and `None` for every other case.
    #[must_use]
    pub fn to_error_body(&self) -> Option<ErrorBody> {
        let Self::Held { owner, .. } = self else {
            return None;
        };
        Some(match owner {
            Some(owner) => ErrorBody::new(
                &codes::daemon::LOCK_HELD,
                format!(
                    "another stacklyd holds the lock (pid {}, started {})",
                    owner.pid, owner.started_at
                ),
            )
            .with_detail("pid", owner.pid.to_string())
            .with_detail("serverInstanceId", owner.server_instance_id.to_string())
            .with_detail("version", owner.version.clone())
            .with_detail("startedAt", owner.started_at.to_string()),
            None => ErrorBody::new(
                &codes::daemon::LOCK_HELD,
                "another stacklyd holds the lock; owner unknown",
            ),
        })
    }

    fn io(path: &Path, op: &'static str, source: io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            op,
            source,
        }
    }
}

/// The exclusive daemon lock, held until [`DaemonLock::release`] or until
/// the value is dropped.
///
/// Prefer `release`: it unlocks at once. Dropping only closes the file,
/// and `flock` lasts until every copy of the open file is closed; a child
/// process spawned at that moment holds a copy until its `exec` closes it.
#[derive(Debug)]
pub struct DaemonLock {
    file: File,
    path: PathBuf,
    owner: LockOwner,
}

impl DaemonLock {
    /// Takes the lock at `path` and writes `owner` into it.
    ///
    /// The file is opened without following a symlink, created with mode
    /// 0600 when missing, and never truncated before the lock is held.
    /// Anything other than a regular file is refused before locking. Once
    /// the lock is held, the file is emptied through the same handle, the
    /// owner is written as JSON, and the data is synced.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::Held`] with the holder read from the file (at
    /// most [`LOCK_INFO_MAX`] bytes) when another process holds the lock,
    /// [`LockError::NotARegularFile`] when the path is a folder, FIFO, or
    /// other special file, and [`LockError::Io`] when an OS call fails (a
    /// symlink at the path fails to open).
    pub fn acquire(path: &Path, owner: LockOwner) -> Result<Self, LockError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            // O_NONBLOCK keeps a FIFO at the path from blocking the open;
            // it changes nothing for a regular file.
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|source| LockError::io(path, "open", source))?;
        let metadata = file
            .metadata()
            .map_err(|source| LockError::io(path, "inspect", source))?;
        if !metadata.file_type().is_file() {
            return Err(LockError::NotARegularFile {
                path: path.to_path_buf(),
            });
        }
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LockError::Held {
                    path: path.to_path_buf(),
                    owner: read_owner(&mut file),
                });
            }
            Err(TryLockError::Error(source)) => return Err(LockError::io(path, "lock", source)),
        }
        write_owner(&mut file, &owner).map_err(|source| LockError::io(path, "write", source))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            owner,
        })
    }

    /// Returns the lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the owner this daemon wrote into the lock file.
    #[must_use]
    pub const fn owner(&self) -> &LockOwner {
        &self.owner
    }

    /// Releases the lock. The file and its contents stay; the next daemon
    /// overwrites them.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::Io`] when the unlock call fails. The lock is
    /// still released when the file closes right after.
    pub fn release(self) -> Result<(), LockError> {
        self.file
            .unlock()
            .map_err(|source| LockError::io(&self.path, "unlock", source))
    }
}

/// Reads the holder from a lock file another process holds, or `None` when
/// the contents are not a valid owner.
fn read_owner(file: &mut File) -> Option<LockOwner> {
    let mut contents = Vec::new();
    file.take(LOCK_INFO_MAX).read_to_end(&mut contents).ok()?;
    serde_json::from_slice(&contents).ok()
}

fn write_owner(file: &mut File, owner: &LockOwner) -> io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    let mut contents = serde_json::to_vec(owner).map_err(io::Error::other)?;
    contents.push(b'\n');
    file.write_all(&contents)?;
    file.sync_data()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use std::thread;
    use std::time::{Duration, Instant};

    use stackly_testkit::{TestRoot, make_fifo};

    use super::*;

    fn owner(pid: u32) -> LockOwner {
        LockOwner::new(
            pid,
            Uuid::now_v7(),
            "2026-10-18T03:04:05.123Z".parse().unwrap(),
            "0.1.0",
        )
    }

    fn lock_path(root: &TestRoot) -> PathBuf {
        root.path().join("stacklyd.lock")
    }

    #[test]
    fn acquire_creates_a_private_lock_file_holding_the_owner_as_json() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let me = owner(41);

        let lock = DaemonLock::acquire(&path, me.clone()).unwrap();

        assert_eq!(lock.owner(), &me);
        assert_eq!(lock.path(), path);
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["pid", "serverInstanceId", "startedAt", "version"]);
        assert_eq!(json["pid"], 41);
        assert_eq!(json["startedAt"], "2026-10-18T03:04:05.123Z");
        assert_eq!(
            json["serverInstanceId"],
            me.server_instance_id.to_string().as_str()
        );
    }

    #[test]
    fn a_second_acquire_reports_the_holder_and_changes_nothing() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let first = owner(41);
        let _held = DaemonLock::acquire(&path, first.clone()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let inode = std::fs::metadata(&path).unwrap().ino();

        let error = DaemonLock::acquire(&path, owner(42)).unwrap_err();

        match &error {
            LockError::Held { path: held, owner } => {
                assert_eq!(held, &path);
                assert_eq!(owner.as_ref(), Some(&first));
            }
            other => panic!("expected Held, got {other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
    }

    #[test]
    fn lock_held_maps_to_daemon_lock_held_with_the_owner_details() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let first = owner(41);
        let _held = DaemonLock::acquire(&path, first.clone()).unwrap();

        let body = DaemonLock::acquire(&path, owner(42))
            .unwrap_err()
            .to_error_body()
            .unwrap();

        assert!(body.is(&codes::daemon::LOCK_HELD));
        assert_eq!(body.detail("pid"), Some("41"));
        assert_eq!(
            body.detail("serverInstanceId"),
            Some(first.server_instance_id.to_string().as_str())
        );
        assert_eq!(body.detail("version"), Some("0.1.0"));
        assert_eq!(body.detail("startedAt"), Some("2026-10-18T03:04:05.123Z"));
        assert_eq!(
            body.message(),
            "another stacklyd holds the lock (pid 41, started 2026-10-18T03:04:05.123Z)"
        );
    }

    #[test]
    fn unreadable_contents_of_a_held_lock_report_an_unknown_owner() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let held = DaemonLock::acquire(&path, owner(41)).unwrap();
        std::fs::write(&path, b"not json").unwrap();

        let error = DaemonLock::acquire(&path, owner(42)).unwrap_err();

        assert!(
            matches!(error, LockError::Held { owner: None, .. }),
            "{error:?}"
        );
        let body = error.to_error_body().unwrap();
        assert_eq!(
            body.message(),
            "another stacklyd holds the lock; owner unknown"
        );
        assert!(body.details().is_empty());
        drop(held);
    }

    #[test]
    fn contents_beyond_the_read_limit_report_an_unknown_owner() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let mut big = owner(41);
        big.version = "v".repeat(usize::try_from(LOCK_INFO_MAX).unwrap());
        let _held = DaemonLock::acquire(&path, big).unwrap();

        let error = DaemonLock::acquire(&path, owner(42)).unwrap_err();

        assert!(
            matches!(error, LockError::Held { owner: None, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_released_lock_can_be_taken_again_and_its_contents_are_replaced() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let mut long = owner(41);
        long.version = "0.1.0-with-a-much-longer-version-tag".to_owned();
        DaemonLock::acquire(&path, long).unwrap().release().unwrap();
        let inode = std::fs::metadata(&path).unwrap().ino();
        let next = owner(42);

        let lock = DaemonLock::acquire(&path, next.clone()).unwrap();

        let written: LockOwner = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written, next);
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        drop(lock);
    }

    #[test]
    fn dropping_the_lock_releases_it() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        drop(DaemonLock::acquire(&path, owner(41)).unwrap());

        // Other tests in this process spawn `mkfifo`; a child spawned while
        // the first lock was open holds a copy of it until its exec, so the
        // lock may stay held for a moment after the drop.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match DaemonLock::acquire(&path, owner(42)) {
                Ok(_) => break,
                Err(LockError::Held { .. }) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("the dropped lock was not released: {error:?}"),
            }
        }
    }

    #[test]
    fn a_symlink_at_the_lock_path_is_refused_and_its_target_is_not_touched() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let target = root.path().join("elsewhere");
        std::fs::write(&target, b"keep").unwrap();
        symlink(&target, &path).unwrap();

        let error = DaemonLock::acquire(&path, owner(41)).unwrap_err();

        assert!(
            matches!(error, LockError::Io { op: "open", .. }),
            "{error:?}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        assert!(error.to_error_body().is_none());
    }

    #[test]
    fn a_dangling_symlink_at_the_lock_path_creates_nothing() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        let target = root.path().join("missing");
        symlink(&target, &path).unwrap();

        let error = DaemonLock::acquire(&path, owner(41)).unwrap_err();

        assert!(
            matches!(error, LockError::Io { op: "open", .. }),
            "{error:?}"
        );
        assert!(!target.exists());
    }

    #[test]
    fn a_fifo_at_the_lock_path_is_refused_without_blocking() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        make_fifo(&path).unwrap();

        let error = DaemonLock::acquire(&path, owner(41)).unwrap_err();

        assert!(
            matches!(error, LockError::NotARegularFile { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_folder_at_the_lock_path_is_refused() {
        let root = TestRoot::new().unwrap();
        let path = lock_path(&root);
        std::fs::create_dir(&path).unwrap();

        let error = DaemonLock::acquire(&path, owner(41)).unwrap_err();

        assert!(
            matches!(
                error,
                LockError::Io { op: "open", .. } | LockError::NotARegularFile { .. }
            ),
            "{error:?}"
        );
        assert!(path.is_dir());
    }
}
