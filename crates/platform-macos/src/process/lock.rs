//! The instance lock (spec 0010, *Lock instance* and *Adapter macOS*, row
//! Lock): an exclusive `flock` on `instance.lock`, opened with
//! `O_RDWR | O_CREAT | O_NOFOLLOW | O_CLOEXEC` and mode 0600, which must be
//! a regular file of the current user.
//!
//! The lock lives on the open file description, so a child that inherits
//! it at fd 3 keeps it after the daemon closes its copy. Nothing here ever
//! calls `flock(LOCK_UN)`: an [`InstanceLock`] releases by closing.

use std::fs::{File, Metadata, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use stackly_platform::{FileIdentity, InstanceLock, LockOutcome, LockState, ProcessError};

use crate::identity::current_uid;

/// Opens or creates the lock file and takes the lock without waiting.
///
/// # Errors
///
/// Returns [`ProcessError::LockFileInvalid`] for a symlink, a file of
/// another kind, or one another user owns, and [`ProcessError::Io`] when
/// the file cannot be opened or locked.
pub(super) fn lock_instance(path: &Path) -> Result<LockOutcome, ProcessError> {
    let Some((file, identity)) = open(path, true)? else {
        return Err(ProcessError::Io {
            op: "open",
            path: Some(path.to_path_buf()),
            kind: io::ErrorKind::NotFound,
            message: "the lock file vanished while it was created".to_owned(),
        });
    };
    match file.try_lock() {
        Ok(()) => Ok(LockOutcome::Acquired(InstanceLock::new(
            file,
            path.to_path_buf(),
            identity,
        ))),
        Err(TryLockError::WouldBlock) => Ok(LockOutcome::Held),
        Err(TryLockError::Error(error)) => Err(ProcessError::io("flock", Some(path), &error)),
    }
}

/// Tells whether anyone holds the lock, through a new open file it closes
/// at once. A missing lock file is free (spec 0010, K10), and the probe
/// never creates one.
///
/// # Errors
///
/// The errors of [`lock_instance`].
pub(super) fn probe_lock(path: &Path) -> Result<LockState, ProcessError> {
    let Some((file, _)) = open(path, false)? else {
        return Ok(LockState::Free);
    };
    match file.try_lock() {
        // Closing the probe's file releases the lock it just took.
        Ok(()) => Ok(LockState::Free),
        Err(TryLockError::WouldBlock) => Ok(LockState::Held),
        Err(TryLockError::Error(error)) => Err(ProcessError::io("flock", Some(path), &error)),
    }
}

/// Opens the lock file without following a symlink, creating it when
/// `create` is true, and checks it. `None` when it does not exist and
/// `create` is false.
fn open(path: &Path, create: bool) -> Result<Option<(File, FileIdentity)>, ProcessError> {
    let invalid = |reason| ProcessError::LockFileInvalid {
        path: path.to_path_buf(),
        reason,
    };
    let opened = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path);
    let file = match opened {
        Ok(file) => file,
        Err(error) => {
            return match error.raw_os_error() {
                Some(libc::ENOENT) if !create => Ok(None),
                Some(libc::ELOOP) => Err(invalid("is a symlink")),
                Some(libc::EISDIR) => Err(invalid("is not a regular file")),
                _ => Err(ProcessError::io("open", Some(path), &error)),
            };
        }
    };
    let metadata = file
        .metadata()
        .map_err(|error| ProcessError::io("fstat", Some(path), &error))?;
    check(&metadata).map_err(invalid)?;
    Ok(Some((file, identity(&metadata))))
}

/// Checks what `fstat` says about an open lock file.
fn check(metadata: &Metadata) -> Result<(), &'static str> {
    if !metadata.is_file() {
        return Err("is not a regular file");
    }
    if metadata.uid() != current_uid() {
        return Err("is not owned by the current user");
    }
    Ok(())
}

/// The device and inode of an open file, as `MetadataExt` gives them, the
/// same numbers the filesystem port reports.
pub(super) fn identity(metadata: &Metadata) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}
