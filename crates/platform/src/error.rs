use std::io;
use std::path::{Path, PathBuf};

use stackly_domain::Os;

use crate::capability::PlatformCapability;

/// Error returned by a platform port or the portable layout logic.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlatformError {
    /// The capability has no adapter on this OS yet.
    #[error("{capability} is not supported on {os}")]
    NotSupported {
        /// The capability that was asked for.
        capability: PlatformCapability,
        /// The OS that lacks it.
        os: Os,
    },
    /// The current user's home directory could not be determined.
    #[error("cannot determine the home directory of the current user")]
    HomeDirUnavailable,
    /// The current user's private temporary directory could not be
    /// determined.
    #[error("cannot determine the temporary directory of the current user")]
    TempDirUnavailable,
    /// A root set breaks a layout rule, for example two roots overlap.
    #[error("invalid root set: {reason}")]
    InvalidRoots {
        /// The rule that was broken, naming the roots involved.
        reason: String,
    },
    /// The runtime root is too long to keep socket paths under the limit.
    #[error("runtime root {} is {len} bytes, more than the {max} byte limit", path.display())]
    RuntimeRootTooLong {
        /// The runtime root that was rejected.
        path: PathBuf,
        /// Its length in bytes.
        len: usize,
        /// The longest runtime root allowed.
        max: usize,
    },
    /// The alternative root given through `STACKLY_ROOT` was rejected.
    #[error("root override {path:?} rejected: {reason}")]
    InvalidRootOverride {
        /// The path as given.
        path: PathBuf,
        /// The rule it broke, for example `is a symlink`.
        reason: &'static str,
    },
    /// The path does not exist.
    #[error("{} does not exist", path.display())]
    NotFound {
        /// The missing path.
        path: PathBuf,
    },
    /// The path already exists, possibly as a symlink.
    #[error("{} already exists", path.display())]
    AlreadyExists {
        /// The existing path.
        path: PathBuf,
    },
    /// The OS denied access to the path.
    #[error("permission denied for {}", path.display())]
    PermissionDenied {
        /// The path that could not be accessed.
        path: PathBuf,
    },
    /// The disk holding the path is full.
    #[error("no space left on the disk holding {}", path.display())]
    NoSpace {
        /// The path that could not be written.
        path: PathBuf,
    },
    /// The path is a symlink, directory, FIFO, or device, not a regular file.
    #[error("{} is not a regular file", path.display())]
    NotARegularFile {
        /// The path that was opened.
        path: PathBuf,
    },
    /// The file is larger than the caller allows.
    #[error("{} is larger than {max} bytes", path.display())]
    FileTooLarge {
        /// The file that was read.
        path: PathBuf,
        /// The largest size the caller allows, in bytes.
        max: u64,
    },
    /// Any other I/O failure.
    #[error("{op} failed for {}: {message}", path.display())]
    Io {
        /// The operation or step that failed, for example `rename`.
        op: &'static str,
        /// The path it was working on.
        path: PathBuf,
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// The OS error message.
        message: String,
    },
    /// A component on the way to the target of a removal or of a
    /// [`TreeWriter`](crate::TreeWriter) entry, or the root of a tree
    /// writer, is a symlink, so nothing was removed or created.
    #[error("{} is a symlink on the way to the target", path.display())]
    SymlinkInPath {
        /// The symlink.
        path: PathBuf,
    },
    /// The path exists but is not a directory where one is needed.
    #[error("{} is not a directory", path.display())]
    NotADirectory {
        /// The path that is not a directory.
        path: PathBuf,
    },
    /// An entry lies on another device than the root of the removal, so
    /// the removal stopped before touching that device, or the two paths
    /// of a rename lie on different volumes.
    #[error("{} is on another device than its root", path.display())]
    CrossesDevice {
        /// The first entry found on another device.
        path: PathBuf,
    },
    /// A directory is nested deeper than the removal walks.
    #[error("{} is nested more than {max} directories deep", path.display())]
    TooDeep {
        /// The first directory past the limit.
        path: PathBuf,
        /// The deepest nesting allowed, the target counted as one level.
        max: usize,
    },
    /// A path lies outside the roots a test owns. Only the fake filesystem
    /// in `stackly-testkit` returns this.
    #[error("{} is outside the test roots", path.display())]
    OutsideTestRoots {
        /// The path that escaped.
        path: PathBuf,
    },
}

impl PlatformError {
    /// Maps an I/O error from `op` on `path` to the matching variant, and to
    /// [`PlatformError::Io`] when no variant fits.
    #[must_use]
    pub fn from_io(op: &'static str, path: &Path, error: &io::Error) -> Self {
        let path = path.to_path_buf();
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound { path },
            io::ErrorKind::AlreadyExists => Self::AlreadyExists { path },
            io::ErrorKind::PermissionDenied => Self::PermissionDenied { path },
            io::ErrorKind::StorageFull => Self::NoSpace { path },
            kind => Self::Io {
                op,
                path,
                kind,
                message: error.to_string(),
            },
        }
    }
}
