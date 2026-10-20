use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::PlatformError;
use crate::identity::UserId;
use crate::tree::TreeWriter;

/// The deepest directory nesting [`Filesystem::remove_tree`] walks, with
/// the target counted as the first level.
pub const REMOVE_TREE_MAX_DEPTH: usize = 128;

/// The filesystem operations Stackly's use cases need, on paths the caller
/// already derived from the layout.
///
/// No method follows a symlink in the last path component except
/// [`canonicalize`](Self::canonicalize) and the volume queries, and no
/// method deletes anything except [`remove_tree`](Self::remove_tree), the
/// temporary file of a failed atomic write, and the bytes an
/// [`AppendFile::truncate`] cuts off.
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

    /// Opens a regular file for reading and seeking, without following a
    /// symlink in the last component and without ever opening a FIFO or
    /// device for reading.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::NotARegularFile`] when the path is a symlink
    /// or anything but a regular file, [`PlatformError::NotFound`] when it
    /// does not exist, and other variants when it cannot be opened.
    fn open_read(&self, path: &Path) -> Result<Box<dyn ReadSeekFile>, PlatformError>;

    /// Opens a regular file for appending, creating it with mode 0600 when
    /// it does not exist, without following a symlink in the last
    /// component (spec 0018, *Download dan cache*).
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::NotARegularFile`] when the path is a symlink
    /// or anything but a regular file, [`PlatformError::NotFound`] when the
    /// parent does not exist, and other variants when it cannot be opened.
    fn open_append(&self, path: &Path) -> Result<Box<dyn AppendFile>, PlatformError>;

    /// Moves the entry at `from` to `to` in one atomic step and syncs both
    /// parent directories. It never replaces or deletes anything: `to` must
    /// not exist, not even as a symlink, and a symlink at `from` moves as
    /// itself.
    ///
    /// The port does not check ownership. Its caller in core checks both
    /// paths with `check_owned_path` and that they share one owned root
    /// first (spec 0018, *Commit atomik*).
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::AlreadyExists`] when `to` exists,
    /// [`PlatformError::CrossesDevice`] when the paths lie on different
    /// volumes, [`PlatformError::NotFound`] when `from` or the parent of
    /// `to` does not exist, and other variants when the move fails.
    fn rename_no_replace(&self, from: &Path, to: &Path) -> Result<(), PlatformError>;

    /// Returns how many bytes an unprivileged user can still write on the
    /// volume that holds `path`, which is
    /// [`VolumeSpace::available`] of [`volume_space`](Self::volume_space).
    ///
    /// # Errors
    ///
    /// Returns the errors of [`volume_space`](Self::volume_space).
    fn available_space(&self, path: &Path) -> Result<u64, PlatformError> {
        Ok(self.volume_space(path)?.available)
    }

    /// Returns a [`TreeWriter`] that builds a new tree inside `root`, which
    /// must be an empty directory with mode 0700 and not a symlink
    /// (spec 0018, *Ekstraksi aman*).
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::SymlinkInPath`] when `root` is a symlink,
    /// [`PlatformError::NotADirectory`] when it is not a directory,
    /// [`PlatformError::Io`] with kind `DirectoryNotEmpty` when it holds an
    /// entry or `InvalidInput` when its mode is not 0700, and other
    /// variants when it cannot be opened.
    fn open_tree_writer(&self, root: &Path) -> Result<Box<dyn TreeWriter>, PlatformError>;

    /// Returns the size and the space left of the volume that holds
    /// `path`, following symlinks to find it.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::NotFound`] when the path does not exist,
    /// and other variants when the volume cannot be read.
    fn volume_space(&self, path: &Path) -> Result<VolumeSpace, PlatformError>;
}

/// A regular file opened by [`Filesystem::open_read`].
pub trait ReadSeekFile: io::Read + io::Seek + Send {
    /// Returns the size of the file in bytes when it was opened.
    fn len(&self) -> u64;

    /// Returns true when the file was empty when it was opened.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A regular file opened by [`Filesystem::open_append`]. Every write goes
/// to its end.
pub trait AppendFile: io::Write + Send {
    /// Returns the size of the file in bytes, counting every write so far.
    fn len(&self) -> u64;

    /// Returns true when the file holds no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Cuts the file down to `len` bytes; later writes continue from there.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Io`] with kind `InvalidInput` when `len` is
    /// above [`len`](Self::len), since the file never grows this way, and
    /// other variants when the file cannot be cut.
    fn truncate(&mut self, len: u64) -> Result<(), PlatformError>;

    /// Makes every byte written so far durable.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be synced.
    fn sync(&mut self) -> Result<(), PlatformError>;
}

/// The size and the space left of one volume, in bytes
/// ([`Filesystem::volume_space`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct VolumeSpace {
    /// The size of the volume.
    pub total: u64,
    /// The bytes an unprivileged user can still write.
    pub available: u64,
}

impl VolumeSpace {
    /// Bundles the two sizes an adapter read.
    #[must_use]
    pub const fn new(total: u64, available: u64) -> Self {
        Self { total, available }
    }
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
