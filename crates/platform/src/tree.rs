use std::fmt;
use std::io;
use std::path::Path;

use crate::error::PlatformError;

/// The longest [`EntryPath`], in bytes.
pub const ENTRY_PATH_MAX_LEN: usize = 1024;

/// The longest component of an [`EntryPath`], in bytes.
pub const ENTRY_NAME_MAX_LEN: usize = 255;

/// The most components an [`EntryPath`] holds.
pub const ENTRY_PATH_MAX_DEPTH: usize = 32;

/// A relative path inside a tree that a [`TreeWriter`] builds: 1 to
/// [`ENTRY_PATH_MAX_DEPTH`] components joined by `/`, each a plain name of
/// at most [`ENTRY_NAME_MAX_LEN`] bytes, at most [`ENTRY_PATH_MAX_LEN`]
/// bytes in all, with no NUL byte, no backslash, and no empty, `.`, or `..`
/// component (spec 0018, *Ekstraksi aman*).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryPath(String);

impl EntryPath {
    /// Checks `path` against the rules above.
    ///
    /// # Errors
    ///
    /// Returns the first [`EntryPathError`] the path breaks.
    pub fn new(path: &str) -> Result<Self, EntryPathError> {
        if path.is_empty() {
            return Err(EntryPathError::Empty);
        }
        if path.bytes().any(|byte| byte == 0 || byte == b'\\') {
            return Err(EntryPathError::InvalidByte);
        }
        if path.starts_with('/') {
            return Err(EntryPathError::Absolute);
        }
        let mut depth = 0;
        for name in path.split('/') {
            match name {
                ".." => return Err(EntryPathError::Traversal),
                "" | "." => return Err(EntryPathError::EmptyComponent),
                _ if name.len() > ENTRY_NAME_MAX_LEN => return Err(EntryPathError::NameTooLong),
                _ => depth += 1,
            }
        }
        if path.len() > ENTRY_PATH_MAX_LEN {
            return Err(EntryPathError::TooLong);
        }
        if depth > ENTRY_PATH_MAX_DEPTH {
            return Err(EntryPathError::TooDeep);
        }
        Ok(Self(path.to_owned()))
    }

    /// Returns the path as text, components joined by `/`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the path for joining onto the tree's root.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    /// Returns the components, first to last.
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.0.split('/')
    }

    /// Returns how many components the path has, at least 1.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.components().count()
    }

    /// Returns the last component.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.rsplit_once('/').map_or(&self.0, |(_, name)| name)
    }

    /// Returns the path without its last component, or `None` for a path
    /// with one component, whose parent is the tree's root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _)| Self(parent.to_owned()))
    }
}

impl fmt::Display for EntryPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a text is not an [`EntryPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum EntryPathError {
    /// The text is empty.
    #[error("the path is empty")]
    Empty,
    /// The text holds a NUL byte or a backslash.
    #[error("the path holds a NUL byte or a backslash")]
    InvalidByte,
    /// The text starts with `/`.
    #[error("the path is absolute")]
    Absolute,
    /// A component is `..`.
    #[error("the path holds a `..` component")]
    Traversal,
    /// A component is empty or `.`.
    #[error("the path holds an empty or `.` component")]
    EmptyComponent,
    /// A component is longer than [`ENTRY_NAME_MAX_LEN`] bytes.
    #[error("a path component is longer than {ENTRY_NAME_MAX_LEN} bytes")]
    NameTooLong,
    /// The text is longer than [`ENTRY_PATH_MAX_LEN`] bytes.
    #[error("the path is longer than {ENTRY_PATH_MAX_LEN} bytes")]
    TooLong,
    /// The path has more than [`ENTRY_PATH_MAX_DEPTH`] components.
    #[error("the path has more than {ENTRY_PATH_MAX_DEPTH} components")]
    TooDeep,
}

/// Permission bits for [`TreeWriter::set_mode`], at most `0o777`: the
/// setuid, setgid, and sticky bits can never be set through it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileMode(u32);

impl FileMode {
    /// The highest mode accepted.
    pub const MAX: u32 = 0o777;

    /// Returns the mode, or `None` when `bits` is above [`Self::MAX`].
    #[must_use]
    pub const fn new(bits: u32) -> Option<Self> {
        if bits <= Self::MAX {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Returns the permission bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for FileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FileMode({:#o})", self.0)
    }
}

/// Builds a new tree below one root, entry by entry, without following a
/// symlink anywhere and without replacing anything
/// ([`Filesystem::open_tree_writer`](crate::Filesystem::open_tree_writer)).
///
/// The parent of every path must already exist as a directory this writer
/// created; a component on the way that is a symlink fails with
/// [`PlatformError::SymlinkInPath`], one that is not a directory with
/// [`PlatformError::NotADirectory`], and a missing one with
/// [`PlatformError::NotFound`]. An entry that already exists, a symlink
/// included, fails with [`PlatformError::AlreadyExists`]; on a volume that
/// ignores letter case, `Foo` and `foo` are the same entry.
pub trait TreeWriter: Send {
    /// Creates a directory with mode 0700.
    ///
    /// # Errors
    ///
    /// Returns the errors listed on the trait, and others when the
    /// directory cannot be created.
    fn create_dir(&mut self, path: &EntryPath) -> Result<(), PlatformError>;

    /// Creates a regular file with mode 0600 and returns it for writing
    /// its contents.
    ///
    /// # Errors
    ///
    /// Returns the errors listed on the trait, and others when the file
    /// cannot be created.
    fn create_file(&mut self, path: &EntryPath) -> Result<Box<dyn TreeFile + '_>, PlatformError>;

    /// Creates a symlink whose target is `target` exactly as given. The
    /// writer does not check where the target points; its caller does.
    ///
    /// # Errors
    ///
    /// Returns the errors listed on the trait,
    /// [`PlatformError::Io`] with kind `InvalidInput` for an empty target
    /// or one holding a NUL byte, and others when the symlink cannot be
    /// created.
    fn create_symlink(&mut self, path: &EntryPath, target: &str) -> Result<(), PlatformError>;

    /// Sets the permission bits of an entry without following a symlink:
    /// on a symlink it changes the link's own bits, never its target's.
    ///
    /// # Errors
    ///
    /// Returns the errors listed on the trait, and others when the bits
    /// cannot be set.
    fn set_mode(&mut self, path: &EntryPath, mode: FileMode) -> Result<(), PlatformError>;

    /// Makes every directory created so far, and the root, durable.
    ///
    /// # Errors
    ///
    /// Returns an error when a directory cannot be reopened or synced.
    fn sync(&mut self) -> Result<(), PlatformError>;
}

/// The contents of one file a [`TreeWriter`] created.
pub trait TreeFile: io::Write {
    /// Syncs the file and returns how many bytes were written to it.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be synced.
    fn finish(self: Box<Self>) -> Result<u64, PlatformError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_path_accepts_normal_relative_paths() {
        let path = EntryPath::new("pgsql/bin/postgres").unwrap();
        assert_eq!(path.as_str(), "pgsql/bin/postgres");
        assert_eq!(path.depth(), 3);
        assert_eq!(path.file_name(), "postgres");
        assert_eq!(path.parent(), Some(EntryPath::new("pgsql/bin").unwrap()));
        assert_eq!(EntryPath::new("nats-server").unwrap().parent(), None);
        assert_eq!(
            path.components().collect::<Vec<_>>(),
            ["pgsql", "bin", "postgres"]
        );
    }

    #[test]
    fn entry_path_rejects_each_broken_rule() {
        let cases = [
            ("", EntryPathError::Empty),
            ("a\0b", EntryPathError::InvalidByte),
            ("a\\b", EntryPathError::InvalidByte),
            ("/etc/passwd", EntryPathError::Absolute),
            ("a/../b", EntryPathError::Traversal),
            ("..", EntryPathError::Traversal),
            ("a//b", EntryPathError::EmptyComponent),
            ("a/./b", EntryPathError::EmptyComponent),
            ("a/", EntryPathError::EmptyComponent),
            (".", EntryPathError::EmptyComponent),
        ];
        for (text, expected) in cases {
            assert_eq!(EntryPath::new(text), Err(expected), "{text:?}");
        }
    }

    #[test]
    fn entry_path_enforces_the_length_and_depth_limits() {
        let name = "n".repeat(ENTRY_NAME_MAX_LEN);
        assert!(EntryPath::new(&name).is_ok());
        assert_eq!(
            EntryPath::new(&format!("{name}n")),
            Err(EntryPathError::NameTooLong)
        );

        let deep = vec!["d"; ENTRY_PATH_MAX_DEPTH].join("/");
        assert!(EntryPath::new(&deep).is_ok());
        assert_eq!(
            EntryPath::new(&format!("{deep}/d")),
            Err(EntryPathError::TooDeep)
        );

        let long = vec!["x".repeat(100); 11].join("/");
        assert!(long.len() > ENTRY_PATH_MAX_LEN);
        assert_eq!(EntryPath::new(&long), Err(EntryPathError::TooLong));
    }

    #[test]
    fn file_mode_accepts_permission_bits_only() {
        assert_eq!(FileMode::new(0o755).map(FileMode::bits), Some(0o755));
        assert_eq!(FileMode::new(0o777).map(FileMode::bits), Some(0o777));
        assert_eq!(FileMode::new(0o4755), None);
        assert_eq!(FileMode::new(0o1777), None);
        assert_eq!(
            format!("{:?}", FileMode::new(0o644).unwrap()),
            "FileMode(0o644)"
        );
    }
}
