use std::path::PathBuf;

/// A failure of the environment or of a file the storage adapter owns.
///
/// Problems with the registry file itself (its content, identity, or
/// schema) never surface here: they become a recovery report instead, so
/// the daemon can stay up in recovery mode. A `StorageError` means the
/// daemon cannot run at all and exits 1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// The `registry`, `pre-migration`, or `recovery` folder is missing, is
    /// not a folder, is a symlink, belongs to another user, or is open to
    /// group or others.
    #[error("registry folder {} is unusable: {reason}", path.display())]
    RegistryDirUnusable {
        /// The folder.
        path: PathBuf,
        /// The rule it broke, for example `a symlink`.
        reason: &'static str,
    },
    /// Another process is creating the registry right now.
    #[error("another process is creating the registry at {}", path.display())]
    CreateInProgress {
        /// The temporary file or registry path that showed it.
        path: PathBuf,
    },
    /// Creating a new registry failed, for example because the disk is
    /// full. The temporary file is cleaned up on the next start.
    #[error("cannot create the registry: {detail}")]
    CreateFailed {
        /// What failed.
        detail: String,
    },
    /// A connection pragma did not take effect.
    #[error("pragma {name} is {actual}, expected {expected}")]
    PragmaNotApplied {
        /// The pragma, for example `journal_mode`.
        name: &'static str,
        /// The value that was set.
        expected: String,
        /// The value read back.
        actual: String,
    },
    /// A file operation on a storage owned path failed.
    #[error("{op} failed for {}: {detail}", path.display())]
    Io {
        /// The operation, for example `hard_link`.
        op: &'static str,
        /// The path it worked on.
        path: PathBuf,
        /// The cause.
        detail: String,
    },
}
