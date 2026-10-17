use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::PlatformError;
use crate::identity::UserId;

/// The filesystem operations Stackly's use cases need, on paths the caller
/// already derived from the layout.
///
/// No method follows a symlink in the last path component except
/// [`canonicalize`](Self::canonicalize), and no method deletes anything
/// except the temporary file of a failed atomic write.
pub trait Filesystem: Send + Sync {
    /// Returns the absolute path with every symlink resolved and no `.` or
    /// `..` component.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::NotFound`] when the path does not exist, and
    /// other variants when it cannot be resolved.
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, PlatformError>;

    /// Describes the path without following a symlink, or returns `None`
    /// when nothing exists there.
    ///
    /// # Errors
    ///
    /// Returns an error when the path exists but cannot be inspected.
    fn symlink_metadata(&self, path: &Path) -> Result<Option<FileInfo>, PlatformError>;

    /// Creates one directory level with mode 0700.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::AlreadyExists`] when anything, a symlink
    /// included, already exists at the path, and [`PlatformError::NotFound`]
    /// when the parent does not exist.
    fn create_dir_private(&self, path: &Path) -> Result<(), PlatformError>;

    /// Lists the entries of a directory with their metadata, without
    /// following symlinks. The order is unspecified.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be read.
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntryInfo>, PlatformError>;

    /// Reads a whole regular file of at most `max_len` bytes, without
    /// following a symlink in the last component and without ever reading
    /// a FIFO or device.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::NotARegularFile`] when the path is a symlink
    /// or anything but a regular file, and [`PlatformError::FileTooLarge`]
    /// when it holds more than `max_len` bytes.
    fn read_file_limited(&self, path: &Path, max_len: u64) -> Result<Vec<u8>, PlatformError>;

    /// Replaces the file at `path` with `bytes` atomically: the bytes go to
    /// a temporary file with mode 0600 in the same directory, are synced,
    /// and the temporary file is renamed over the target before the
    /// directory is synced. On failure the temporary file is removed.
    ///
    /// # Errors
    ///
    /// Returns an error when any step fails; the old file is then intact.
    fn write_file_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), PlatformError>;
}

/// What a path is, without following a symlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link.
    Symlink,
    /// A FIFO, socket, or device.
    Other,
}

/// The identity of a file on its device, stable across the names that
/// reach it (symlinks, letter case, Unicode normalization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    /// The device the file lives on.
    pub device: u64,
    /// The inode number on that device.
    pub inode: u64,
}

/// Metadata of one path, read without following a symlink.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileInfo {
    /// What the path is.
    pub kind: FileKind,
    /// The size in bytes.
    pub len: u64,
    /// The device and inode.
    pub identity: FileIdentity,
    /// The owning user, when the OS has one.
    pub owner: Option<UserId>,
    /// The permission bits (`0o7777` mask), when the OS has them.
    pub mode: Option<u32>,
}

impl FileInfo {
    /// Bundles the metadata an adapter read.
    #[must_use]
    pub const fn new(
        kind: FileKind,
        len: u64,
        identity: FileIdentity,
        owner: Option<UserId>,
        mode: Option<u32>,
    ) -> Self {
        Self {
            kind,
            len,
            identity,
            owner,
            mode,
        }
    }

    /// Converts metadata that `std::fs::symlink_metadata` returned on a
    /// Unix system.
    #[cfg(unix)]
    #[must_use]
    pub fn from_unix_metadata(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;

        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            FileKind::Symlink
        } else if file_type.is_dir() {
            FileKind::Dir
        } else if file_type.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };
        Self::new(
            kind,
            metadata.len(),
            FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            Some(UserId::Unix(metadata.uid())),
            Some(metadata.mode() & 0o7777),
        )
    }
}

/// One entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirEntryInfo {
    /// The entry's file name, as stored on disk.
    pub name: OsString,
    /// The entry's metadata, read without following a symlink.
    pub info: FileInfo,
}

impl DirEntryInfo {
    /// Pairs an entry name with its metadata.
    #[must_use]
    pub const fn new(name: OsString, info: FileInfo) -> Self {
        Self { name, info }
    }
}

/// Returns true when `path` is `dir` or lies inside it.
///
/// Both paths must be canonical. Besides comparing them as text, this
/// compares the file identity of `dir` with every ancestor of `path` that is
/// at least as deep as `dir`, so another name for the same directory (a
/// different letter case on APFS, a different Unicode normalization, or a
/// firmlink) is still caught. Ancestors shallower than `dir` are never
/// inspected.
///
/// # Errors
///
/// Returns the error of a metadata read that fails for a reason other than
/// the path not existing.
pub fn is_same_or_inside(
    fs: &dyn Filesystem,
    path: &Path,
    dir: &Path,
) -> Result<bool, PlatformError> {
    if path.starts_with(dir) {
        return Ok(true);
    }
    let Some(dir_info) = fs.symlink_metadata(dir)? else {
        return Ok(false);
    };
    let depth = dir.components().count();
    for ancestor in path.ancestors() {
        if ancestor.components().count() < depth {
            break;
        }
        if let Some(info) = fs.symlink_metadata(ancestor)?
            && info.identity == dir_info.identity
        {
            return Ok(true);
        }
    }
    Ok(false)
}
