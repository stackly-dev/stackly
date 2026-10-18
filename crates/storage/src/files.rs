//! Every file operation on the files the storage adapter owns: the
//! temporary create file and its lock, publishing with `hard_link`,
//! removing, syncing, modes, and hashing.
//!
//! The port `Filesystem` has no lock, link, or remove, so these go through
//! `std::fs` here, and nowhere else in the crate. Each operation refuses a
//! path outside the layout's registry folder.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::StorageError;

/// Size of the chunks a file is hashed in.
const HASH_CHUNK: usize = 64 * 1024;

/// The registry folder, the only place these operations may touch.
#[derive(Debug, Clone)]
pub(crate) struct RegistryFiles {
    dir: PathBuf,
}

/// The result of trying to lock an existing file.
#[derive(Debug)]
pub(crate) enum LockAttempt {
    /// The lock is ours until the file is dropped.
    Acquired(File),
    /// Another open file holds the lock.
    Busy,
    /// Nothing exists at the path.
    Missing,
}

/// The result of publishing a file under a second name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkOutcome {
    /// The new name now points at the file.
    Linked,
    /// Something already exists under the new name; it was left alone.
    TargetExists,
    /// The source name no longer exists.
    SourceMissing,
}

impl RegistryFiles {
    /// Confines the operations to `dir`, the layout's registry folder.
    pub(crate) const fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Creates `path` as a new, empty file with mode 0600 and locks it
    /// exclusively. Returns `None` when something already exists there,
    /// another open file holds the lock, or the name no longer points at
    /// the new file once it is locked.
    ///
    /// The last check closes the gap between creating and locking: in it,
    /// a cleaner may take the free lock, judge the file a leftover, and
    /// remove it. Once the lock is held and the name still points at the
    /// file, nobody else can remove it.
    pub(crate) fn create_locked(&self, path: &Path) -> Result<Option<File>, StorageError> {
        self.guard("create", path)?;
        let file = match private_create_new(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => return Ok(None),
            Err(err) => return Err(io_error("create", path, &err)),
        };
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(err)) => return Err(io_error("lock", path, &err)),
        }
        if self.is_same_file(&file, path)? {
            Ok(Some(file))
        } else {
            Ok(None)
        }
    }

    /// Opens an existing regular file and tries to lock it exclusively
    /// without waiting.
    pub(crate) fn try_lock(&self, path: &Path) -> Result<LockAttempt, StorageError> {
        self.guard("lock", path)?;
        let file = match open_regular(path, true) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(LockAttempt::Missing),
            // Replaced while it was opened: someone else is creating it now.
            Err(err) if err.kind() == io::ErrorKind::ResourceBusy => return Ok(LockAttempt::Busy),
            Err(err) => return Err(io_error("open", path, &err)),
        };
        match file.try_lock() {
            Ok(()) => Ok(LockAttempt::Acquired(file)),
            Err(fs::TryLockError::WouldBlock) => Ok(LockAttempt::Busy),
            Err(fs::TryLockError::Error(err)) => Err(io_error("lock", path, &err)),
        }
    }

    /// Whether `file` is still the file named `path`, compared by device
    /// and inode.
    pub(crate) fn is_same_file(&self, file: &File, path: &Path) -> Result<bool, StorageError> {
        self.guard("stat", path)?;
        let named = match fs::symlink_metadata(path) {
            Ok(named) => named,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(io_error("stat", path, &err)),
        };
        let open = file
            .metadata()
            .map_err(|err| io_error("stat", path, &err))?;
        Ok(named.file_type().is_file() && same_identity(&named, &open))
    }

    /// Gives the file at `from` the second name `to`. Never replaces
    /// anything that exists at `to`.
    pub(crate) fn hard_link(&self, from: &Path, to: &Path) -> Result<LinkOutcome, StorageError> {
        self.guard("hard_link", from)?;
        self.guard("hard_link", to)?;
        match fs::hard_link(from, to) {
            Ok(()) => Ok(LinkOutcome::Linked),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(LinkOutcome::TargetExists),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(LinkOutcome::SourceMissing),
            Err(err) => Err(io_error("hard_link", to, &err)),
        }
    }

    /// Removes one file. A file that is already gone is not an error.
    pub(crate) fn remove(&self, path: &Path) -> Result<(), StorageError> {
        self.guard("remove", path)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(io_error("remove", path, &err)),
        }
    }

    /// Flushes a regular file to stable storage (`F_FULLFSYNC` on macOS).
    pub(crate) fn sync_file(&self, path: &Path) -> Result<(), StorageError> {
        self.guard("sync", path)?;
        open_regular(path, false)
            .and_then(|file| file.sync_all())
            .map_err(|err| io_error("sync", path, &err))
    }

    /// Flushes a folder's entries to stable storage, after a file in it was
    /// created, linked, or removed.
    pub(crate) fn sync_dir(&self, path: &Path) -> Result<(), StorageError> {
        self.guard("sync_dir", path)?;
        sync_dir(path).map_err(|err| io_error("sync_dir", path, &err))
    }

    /// Sets a regular file's mode to 0600.
    pub(crate) fn make_private(&self, path: &Path) -> Result<(), StorageError> {
        self.guard("chmod", path)?;
        set_private(path).map_err(|err| io_error("chmod", path, &err))
    }

    /// Reads at most `len` bytes from the start of a regular file.
    pub(crate) fn read_prefix(&self, path: &Path, len: usize) -> Result<Vec<u8>, io::Error> {
        self.guard("read", path)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;
        let file = open_regular(path, false)?;
        let mut prefix = Vec::with_capacity(len);
        file.take(len as u64).read_to_end(&mut prefix)?;
        Ok(prefix)
    }

    /// Returns the size and the SHA-256 (64 lowercase hex characters) of a
    /// regular file.
    pub(crate) fn sha256(&self, path: &Path) -> Result<(u64, String), io::Error> {
        self.guard("read", path)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;
        let mut file = open_regular(path, false)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; HASH_CHUNK];
        let mut len: u64 = 0;
        loop {
            let read = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            };
            hasher.update(&buffer[..read]);
            len += read as u64;
        }
        Ok((len, hex(&hasher.finalize())))
    }

    fn guard(&self, op: &'static str, path: &Path) -> Result<(), StorageError> {
        let plain = path
            .components()
            .all(|part| !matches!(part, Component::CurDir | Component::ParentDir));
        if plain && path.starts_with(&self.dir) {
            Ok(())
        } else {
            Err(StorageError::Io {
                op,
                path: path.to_path_buf(),
                detail: format!("outside the registry folder {}", self.dir.display()),
            })
        }
    }
}

/// Bytes as lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// SHA-256 of `bytes` as 64 lowercase hex characters.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn io_error(op: &'static str, path: &Path, err: &io::Error) -> StorageError {
    StorageError::Io {
        op,
        path: path.to_path_buf(),
        detail: err.to_string(),
    }
}

/// Opens `path` only when it is a regular file, and checks that the file
/// opened is the one inspected, so a symlink swapped in between is never
/// followed.
fn open_regular(path: &Path, write: bool) -> io::Result<File> {
    let named = fs::symlink_metadata(path)?;
    if !named.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let file = fs::OpenOptions::new().read(true).write(write).open(path)?;
    if !same_identity(&named, &file.metadata()?) {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "the file changed while it was opened",
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn same_identity(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_identity(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.len() == b.len() && a.file_type() == b.file_type()
}

#[cfg(unix)]
fn private_create_new(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_create_new(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn set_private(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let named = fs::symlink_metadata(path)?;
    if !named.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use stackly_testkit::TestRoot;

    use super::*;

    fn files(root: &TestRoot) -> (RegistryFiles, PathBuf) {
        let dir = root.path().join("registry");
        fs::create_dir(&dir).unwrap();
        (RegistryFiles::new(dir.clone()), dir)
    }

    #[test]
    fn create_locked_makes_a_private_file_and_holds_its_lock() {
        let root = TestRoot::new().unwrap();
        let (files, dir) = files(&root);
        let path = dir.join(".stackly.sqlite.creating");

        let held = files.create_locked(&path).unwrap().unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(matches!(files.try_lock(&path).unwrap(), LockAttempt::Busy));
        assert!(files.create_locked(&path).unwrap().is_none());
        assert!(files.is_same_file(&held, &path).unwrap());
        drop(held);
        assert!(matches!(
            files.try_lock(&path).unwrap(),
            LockAttempt::Acquired(_)
        ));
    }

    #[test]
    fn hard_link_never_replaces_an_existing_file() {
        let root = TestRoot::new().unwrap();
        let (files, dir) = files(&root);
        let (from, to) = (dir.join("a"), dir.join("b"));
        fs::write(&from, b"new").unwrap();
        fs::write(&to, b"old").unwrap();

        assert_eq!(
            files.hard_link(&from, &to).unwrap(),
            LinkOutcome::TargetExists
        );
        assert_eq!(fs::read(&to).unwrap(), b"old");
        assert_eq!(
            files
                .hard_link(&dir.join("missing"), &dir.join("c"))
                .unwrap(),
            LinkOutcome::SourceMissing
        );
    }

    #[test]
    fn paths_outside_the_registry_folder_are_refused() {
        let root = TestRoot::new().unwrap();
        let (files, dir) = files(&root);
        let outside = root.path().join("outside");
        fs::write(&outside, b"keep").unwrap();

        for path in [outside.clone(), dir.join("..").join("outside")] {
            assert!(matches!(
                files.remove(&path),
                Err(StorageError::Io { op: "remove", .. })
            ));
        }
        assert!(outside.exists());
    }

    #[test]
    fn hashing_and_reading_refuse_a_symlink() {
        let root = TestRoot::new().unwrap();
        let (files, dir) = files(&root);
        let target = dir.join("target");
        fs::write(&target, b"abc").unwrap();
        symlink(&target, dir.join("link")).unwrap();

        let (len, sha) = files.sha256(&target).unwrap();
        assert_eq!(len, 3);
        assert_eq!(
            sha,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(files.sha256(&dir.join("link")).is_err());
        assert!(files.read_prefix(&dir.join("link"), 16).is_err());
        assert!(files.make_private(&dir.join("link")).is_err());
    }
}
