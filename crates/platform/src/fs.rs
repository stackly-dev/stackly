use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::PlatformError;
use crate::identity::UserId;

/// The deepest directory nesting [`Filesystem::remove_tree`] walks, with
/// the target counted as the first level.
pub const REMOVE_TREE_MAX_DEPTH: usize = 128;

/// The filesystem operations Stackly's use cases need, on paths the caller
/// already derived from the layout.
///
/// No method follows a symlink in the last path component except
/// [`canonicalize`](Self::canonicalize), and no method deletes anything
/// except [`remove_tree`](Self::remove_tree) and the temporary file of a
/// failed atomic write.
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

    /// Removes the entry at `root` joined with `relative`, and everything
    /// under it when it is a directory, without ever following a symlink.
    ///
    /// `root` is a canonical owned root, and `relative` names at least one
    /// entry below it with plain names only (see [`remove_tree_names`]).
    /// Every component of `relative` but the last must be a real directory
    /// on the device of `root`; otherwise nothing is removed. A symlink
    /// found as the target or anywhere in the tree is removed as itself,
    /// never its target. Removal stops at an entry on another device than
    /// `root` before touching that device, and at a directory nested more
    /// than [`REMOVE_TREE_MAX_DEPTH`] levels deep. Entries removed before an
    /// error stay removed.
    ///
    /// A target that does not exist, or whose parent does not, yields a
    /// report with [`missing`](RemoveReport::missing) set, so removing the
    /// same tree twice succeeds both times.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::SymlinkInPath`] or
    /// [`PlatformError::NotADirectory`] for a component on the way to the
    /// target that is a symlink or not a directory,
    /// [`PlatformError::CrossesDevice`] for an entry on another device,
    /// [`PlatformError::TooDeep`] past the depth limit,
    /// [`PlatformError::Io`] with kind `InvalidInput` for an invalid
    /// argument, and other variants when an entry cannot be read or
    /// removed.
    fn remove_tree(&self, root: &Path, relative: &Path) -> Result<RemoveReport, PlatformError>;
}

/// What one [`Filesystem::remove_tree`] call removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RemoveReport {
    /// Entries removed that were not directories: regular files, symlinks
    /// (never their targets), FIFOs, sockets, and devices.
    pub files: u64,
    /// Directories removed, the target included when it was one.
    pub dirs: u64,
    /// True when the target or a directory on the way to it did not exist,
    /// so nothing was removed.
    pub missing: bool,
}

impl RemoveReport {
    /// The report of a target that was found and removed.
    #[must_use]
    pub const fn removed(files: u64, dirs: u64) -> Self {
        Self {
            files,
            dirs,
            missing: false,
        }
    }

    /// The report of a target that did not exist.
    #[must_use]
    pub const fn missing() -> Self {
        Self {
            files: 0,
            dirs: 0,
            missing: true,
        }
    }
}

/// Checks the arguments of [`Filesystem::remove_tree`] and returns the
/// names `relative` walks through, the target's last.
///
/// `root` must be absolute, and `relative` must hold at least one
/// component, every one a plain name: no root, prefix, `.`, or `..`.
///
/// # Errors
///
/// Returns [`PlatformError::Io`] with kind `InvalidInput` when an argument
/// breaks these rules.
pub fn remove_tree_names<'a>(
    root: &Path,
    relative: &'a Path,
) -> Result<Vec<&'a OsStr>, PlatformError> {
    let invalid = |message: &str| PlatformError::Io {
        op: "remove_tree",
        path: root.join(relative),
        kind: io::ErrorKind::InvalidInput,
        message: message.to_owned(),
    };
    if !root.is_absolute() {
        return Err(invalid("the root must be an absolute path"));
    }
    let names = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => Ok(name),
            _ => Err(invalid(
                "the relative path must hold plain names only, with no root, `.`, or `..`",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if names.is_empty() {
        return Err(invalid(
            "the relative path must name an entry below the root",
        ));
    }
    Ok(names)
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

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    fn is_invalid_input<T: std::fmt::Debug>(result: Result<T, PlatformError>) -> bool {
        matches!(
            result,
            Err(PlatformError::Io {
                op: "remove_tree",
                kind: io::ErrorKind::InvalidInput,
                ..
            })
        )
    }

    #[test]
    fn remove_tree_names_splits_a_relative_path_into_its_names() {
        let names = remove_tree_names(Path::new("/root"), Path::new("instances/abc/")).unwrap();

        assert_eq!(names, [OsStr::new("instances"), OsStr::new("abc")]);
    }

    #[test]
    fn remove_tree_names_rejects_an_empty_absolute_or_dotted_relative_path() {
        for relative in [
            "",
            "/instances",
            "../instances",
            "./instances",
            "a/../b",
            ".",
        ] {
            assert!(
                is_invalid_input(remove_tree_names(Path::new("/root"), Path::new(relative))),
                "{relative:?}"
            );
        }
    }

    #[test]
    fn remove_tree_names_needs_an_absolute_root() {
        let result = remove_tree_names(Path::new("root"), Path::new("instances"));

        assert!(is_invalid_input(result));
    }

    #[test]
    fn a_remove_report_says_whether_the_target_was_there() {
        assert_eq!(
            RemoveReport::missing(),
            RemoveReport {
                files: 0,
                dirs: 0,
                missing: true
            }
        );
        assert!(!RemoveReport::removed(2, 1).missing);
        assert_eq!(RemoveReport::default(), RemoveReport::removed(0, 0));
    }
}
