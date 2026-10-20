use std::fs;
use std::io;
use std::path::Path;

use stackly_platform::PlatformError;

use super::fd::{c_path, sync_dir};

const OP: &str = "rename_no_replace";

/// [`Filesystem::rename_no_replace`](stackly_platform::Filesystem::rename_no_replace)
/// through `renamex_np(RENAME_EXCL)`, which refuses an existing target in
/// the same atomic step as the move, then a sync of both parents.
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> Result<(), PlatformError> {
    let no_parent = |path: &Path| PlatformError::Io {
        op: OP,
        path: path.to_path_buf(),
        kind: io::ErrorKind::InvalidInput,
        message: "the path has no parent directory".to_owned(),
    };
    let from_parent = from.parent().ok_or_else(|| no_parent(from))?;
    let to_parent = to.parent().ok_or_else(|| no_parent(to))?;
    let c_from = c_path(OP, from)?;
    let c_to = c_path(OP, to)?;
    // SAFETY: both pointers are NUL terminated strings that outlive the
    // call, and `renamex_np` keeps neither.
    let rc = unsafe { libc::renamex_np(c_from.as_ptr(), c_to.as_ptr(), libc::RENAME_EXCL) };
    if rc != 0 {
        let err = io::Error::last_os_error();
        return Err(match err.raw_os_error() {
            Some(libc::EEXIST | libc::ENOTEMPTY) => PlatformError::AlreadyExists {
                path: to.to_path_buf(),
            },
            Some(libc::EXDEV) => PlatformError::CrossesDevice {
                path: to.to_path_buf(),
            },
            // ENOENT names either a missing source or a missing parent of
            // the target; report the one that is missing.
            Some(libc::ENOENT) if fs::symlink_metadata(from).is_ok() => PlatformError::NotFound {
                path: to_parent.to_path_buf(),
            },
            _ => PlatformError::from_io(OP, from, &err),
        });
    }
    sync_dir(OP, from_parent)?;
    if to_parent != from_parent {
        sync_dir(OP, to_parent)?;
    }
    Ok(())
}
