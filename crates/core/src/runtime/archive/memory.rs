//! The tree [`scan`](super::scan) writes into: entries in memory, file
//! contents discarded, and names compared without letter case, as on the
//! default APFS volume.

use std::collections::HashMap;
use std::io::{self, Write};

use stackly_platform::{EntryPath, FileMode, PlatformError, TreeFile, TreeWriter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Dir,
    File,
    Symlink,
}

/// A [`TreeWriter`] that keeps only the kind of each entry.
#[derive(Debug, Default)]
pub(crate) struct MemoryTree {
    entries: HashMap<String, Kind>,
}

impl MemoryTree {
    /// Checks the way to `path` and that `path` itself is free.
    fn check(&self, path: &EntryPath) -> Result<String, PlatformError> {
        let key = path.as_str().to_lowercase();
        if let Some(parent) = path.parent() {
            let mut prefix = String::new();
            for component in parent.components() {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(&component.to_lowercase());
                let on_the_way = || prefix.clone().into();
                match self.entries.get(&prefix) {
                    Some(Kind::Dir) => {}
                    Some(Kind::Symlink) => {
                        return Err(PlatformError::SymlinkInPath { path: on_the_way() });
                    }
                    Some(Kind::File) => {
                        return Err(PlatformError::NotADirectory { path: on_the_way() });
                    }
                    None => return Err(PlatformError::NotFound { path: on_the_way() }),
                }
            }
        }
        if self.entries.contains_key(&key) {
            return Err(PlatformError::AlreadyExists {
                path: path.as_path().to_path_buf(),
            });
        }
        Ok(key)
    }

    fn create(&mut self, path: &EntryPath, kind: Kind) -> Result<(), PlatformError> {
        let key = self.check(path)?;
        self.entries.insert(key, kind);
        Ok(())
    }
}

impl TreeWriter for MemoryTree {
    fn create_dir(&mut self, path: &EntryPath) -> Result<(), PlatformError> {
        self.create(path, Kind::Dir)
    }

    fn create_file(&mut self, path: &EntryPath) -> Result<Box<dyn TreeFile + '_>, PlatformError> {
        self.create(path, Kind::File)?;
        Ok(Box::new(Discard(0)))
    }

    fn create_symlink(&mut self, path: &EntryPath, target: &str) -> Result<(), PlatformError> {
        if target.is_empty() || target.contains('\0') {
            return Err(PlatformError::Io {
                op: "create_symlink",
                path: path.as_path().to_path_buf(),
                kind: io::ErrorKind::InvalidInput,
                message: "the symlink target is empty or holds a NUL byte".to_owned(),
            });
        }
        self.create(path, Kind::Symlink)
    }

    fn set_mode(&mut self, path: &EntryPath, _mode: FileMode) -> Result<(), PlatformError> {
        if self.entries.contains_key(&path.as_str().to_lowercase()) {
            Ok(())
        } else {
            Err(PlatformError::NotFound {
                path: path.as_path().to_path_buf(),
            })
        }
    }

    fn sync(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// A file whose bytes are counted and dropped.
struct Discard(u64);

impl Write for Discard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl TreeFile for Discard {
    fn finish(self: Box<Self>) -> Result<u64, PlatformError> {
        Ok(self.0)
    }
}
