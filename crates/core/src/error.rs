use std::path::PathBuf;

use stackly_platform::PlatformError;

/// Error returned by a core use case.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// An owned root or standard folder exists but is not a private folder
    /// of the current user. It is rejected, never repaired.
    #[error("{} is not a private Stackly folder: {reason}", path.display())]
    RootNotPrivate {
        /// The folder that was rejected.
        path: PathBuf,
        /// Why, for example `owned by another user`.
        reason: &'static str,
    },
    /// Two owned roots are the same folder or nest once resolved.
    #[error("owned roots {} and {} overlap", a.display(), b.display())]
    RootsOverlap {
        /// The first root, canonical.
        a: PathBuf,
        /// The second root, canonical.
        b: PathBuf,
    },
    /// The path does not lie inside any owned root.
    #[error("{} is outside the Stackly roots", path.display())]
    OutsideOwnedRoots {
        /// The path as given.
        path: PathBuf,
    },
    /// The path is an owned root itself, which no operation may target.
    #[error("{} is a Stackly root itself", path.display())]
    IsOwnedRoot {
        /// The path as given.
        path: PathBuf,
    },
    /// The path is relative or has a `.` or `..` component.
    #[error("{} must be absolute, without `.` or `..` components", path.display())]
    NotNormalized {
        /// The path as given.
        path: PathBuf,
    },
    /// A platform port failed.
    #[error(transparent)]
    Platform(#[from] PlatformError),
}
