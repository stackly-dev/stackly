use std::path::{Path, PathBuf};

use crate::error::PlatformError;
use crate::fs::{FileKind, Filesystem, is_same_or_inside};
use crate::identity::UserIdentity;

/// Name of the environment variable that points Stackly at an alternative
/// root. Only binaries read it, once in `main`; libraries never read the
/// environment. It is meant for integration tests and development.
pub const ROOT_OVERRIDE_ENV: &str = "STACKLY_ROOT";

/// Checks an alternative root and returns its canonical path.
///
/// The root is accepted when it is not empty, absolute, exists, is a
/// directory and not a symlink, belongs to the current user, is not
/// writable by group or others, and is neither the default data root nor
/// inside it (compared by file identity when the default data root exists).
///
/// # Errors
///
/// Returns [`PlatformError::InvalidRootOverride`] naming the path and the
/// rule it broke, or the error of a filesystem call that failed.
pub fn validate_root_override(
    path: &Path,
    fs: &dyn Filesystem,
    identity: &dyn UserIdentity,
    default_data_root: &Path,
) -> Result<PathBuf, PlatformError> {
    let reject = |reason| PlatformError::InvalidRootOverride {
        path: path.to_path_buf(),
        reason,
    };
    if path.as_os_str().is_empty() {
        return Err(reject("empty"));
    }
    if !path.is_absolute() {
        return Err(reject("not absolute"));
    }
    let info = fs
        .symlink_metadata(path)?
        .ok_or_else(|| reject("does not exist"))?;
    match info.kind {
        FileKind::Dir => {}
        FileKind::Symlink => return Err(reject("is a symlink")),
        FileKind::File | FileKind::Other => return Err(reject("not a directory")),
    }
    if info.owner != Some(identity.current_user()?) {
        return Err(reject("owned by another user"));
    }
    match info.mode {
        Some(mode) if mode & 0o022 == 0 => {}
        Some(_) => return Err(reject("writable by group or others")),
        None => return Err(reject("permissions cannot be read")),
    }
    let canonical = fs.canonicalize(path)?;
    let default = match fs.canonicalize(default_data_root) {
        Ok(default) => Some(default),
        Err(PlatformError::NotFound { .. }) => None,
        Err(err) => return Err(err),
    };
    if let Some(default) = default
        && is_same_or_inside(fs, &canonical, &default)?
    {
        return Err(reject("inside the default data root"));
    }
    Ok(canonical)
}
