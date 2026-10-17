use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use stackly_platform::{DirEntryInfo, FileInfo, FileKind, Filesystem, PlatformError};

use crate::TestRoot;

/// Counter that makes each temporary file name unique within the process.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One [`Filesystem`] operation, for fault injection and the mutation log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FsOp {
    /// [`Filesystem::canonicalize`].
    Canonicalize,
    /// [`Filesystem::symlink_metadata`].
    SymlinkMetadata,
    /// [`Filesystem::create_dir_private`].
    CreateDirPrivate,
    /// [`Filesystem::read_dir`].
    ReadDir,
    /// [`Filesystem::read_file_limited`].
    ReadFileLimited,
    /// [`Filesystem::write_file_atomic`].
    WriteFileAtomic,
}

impl FsOp {
    /// Returns the method name, for example `create_dir_private`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Canonicalize => "canonicalize",
            Self::SymlinkMetadata => "symlink_metadata",
            Self::CreateDirPrivate => "create_dir_private",
            Self::ReadDir => "read_dir",
            Self::ReadFileLimited => "read_file_limited",
            Self::WriteFileAtomic => "write_file_atomic",
        }
    }
}

/// A failure [`FakeFilesystem`] injects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Fault {
    /// [`PlatformError::NoSpace`].
    NoSpace,
    /// [`PlatformError::PermissionDenied`].
    PermissionDenied,
    /// [`PlatformError::Io`] with kind `Other`.
    Io,
}

impl Fault {
    fn to_error(self, op: FsOp, path: &Path) -> PlatformError {
        let path = path.to_path_buf();
        match self {
            Self::NoSpace => PlatformError::NoSpace { path },
            Self::PermissionDenied => PlatformError::PermissionDenied { path },
            Self::Io => PlatformError::Io {
                op: op.as_code(),
                path,
                kind: io::ErrorKind::Other,
                message: "injected fault".to_owned(),
            },
        }
    }
}

/// One change [`FakeFilesystem`] made on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Mutation {
    /// The operation, [`FsOp::CreateDirPrivate`] or
    /// [`FsOp::WriteFileAtomic`].
    pub op: FsOp,
    /// The path it created or replaced.
    pub path: PathBuf,
}

#[derive(Debug, Default)]
struct State {
    next: Vec<(FsOp, Fault)>,
    when: Vec<(FsOp, PathBuf, Fault)>,
    mutations: Vec<Mutation>,
}

/// A real filesystem confined to the folders of one [`TestRoot`].
///
/// Every operation runs on the real disk through `std::fs`, but the
/// canonical parent of each path argument plus its last component as given
/// must lie inside the test root or its run folder, and so must the result
/// of [`canonicalize`](Filesystem::canonicalize). Anything else fails with
/// [`PlatformError::OutsideTestRoots`] and counts as a violation, so a path
/// that leaks out of a test fails that test instead of touching the user's
/// folders. A test that needs a symlink "to the outside" points it at
/// another folder inside the test root.
///
/// Faults can be injected per operation with [`fail_next`](Self::fail_next)
/// and [`fail_when`](Self::fail_when). A fault on
/// [`FsOp::WriteFileAtomic`] fires after the temporary file is written and
/// before the rename, so the cleanup path runs.
#[derive(Debug)]
pub struct FakeFilesystem {
    roots: Vec<PathBuf>,
    violations: AtomicUsize,
    state: Mutex<State>,
}

impl FakeFilesystem {
    /// Confines a filesystem to `root`'s test folder and run folder.
    #[must_use]
    pub fn new(root: &TestRoot) -> Self {
        Self {
            roots: vec![root.path().to_path_buf(), root.run_dir().to_path_buf()],
            violations: AtomicUsize::new(0),
            state: Mutex::new(State::default()),
        }
    }

    /// Returns how many calls touched a path outside the test roots.
    #[must_use]
    pub fn violations(&self) -> usize {
        self.violations.load(Ordering::SeqCst)
    }

    /// Makes the next call of `op` fail with `fault`, once.
    pub fn fail_next(&self, op: FsOp, fault: Fault) {
        self.state().next.push((op, fault));
    }

    /// Makes every call of `op` on a path that ends with `path_suffix`
    /// (compared by whole components) fail with `fault`, until
    /// [`clear_faults`](Self::clear_faults).
    pub fn fail_when(&self, op: FsOp, path_suffix: impl Into<PathBuf>, fault: Fault) {
        self.state().when.push((op, path_suffix.into(), fault));
    }

    /// Removes every pending fault.
    pub fn clear_faults(&self) {
        let mut state = self.state();
        state.next.clear();
        state.when.clear();
    }

    /// Returns every directory created and file written so far, in order.
    #[must_use]
    pub fn mutations(&self) -> Vec<Mutation> {
        self.state().mutations.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Checks confinement, then injected faults (except for writes, whose
    /// fault fires before the rename).
    fn enter(&self, op: FsOp, path: &Path) -> Result<(), PlatformError> {
        self.confine(path)?;
        if op == FsOp::WriteFileAtomic {
            return Ok(());
        }
        match self.take_fault(op, path) {
            Some(fault) => Err(fault.to_error(op, path)),
            None => Ok(()),
        }
    }

    fn confine(&self, path: &Path) -> Result<(), PlatformError> {
        let inside = resolve_last_as_given(path)
            .is_some_and(|resolved| self.roots.iter().any(|root| resolved.starts_with(root)));
        if inside {
            Ok(())
        } else {
            self.violations.fetch_add(1, Ordering::SeqCst);
            Err(PlatformError::OutsideTestRoots {
                path: path.to_path_buf(),
            })
        }
    }

    fn take_fault(&self, op: FsOp, path: &Path) -> Option<Fault> {
        let mut state = self.state();
        if let Some(index) = state.next.iter().position(|(next_op, _)| *next_op == op) {
            return Some(state.next.remove(index).1);
        }
        state
            .when
            .iter()
            .find(|(when_op, suffix, _)| *when_op == op && path.ends_with(suffix))
            .map(|(_, _, fault)| *fault)
    }

    fn record(&self, op: FsOp, path: &Path) {
        self.state().mutations.push(Mutation {
            op,
            path: path.to_path_buf(),
        });
    }
}

impl Filesystem for FakeFilesystem {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, PlatformError> {
        self.enter(FsOp::Canonicalize, path)?;
        let canonical = fs::canonicalize(path)
            .map_err(|err| PlatformError::from_io("canonicalize", path, &err))?;
        self.confine(&canonical)?;
        Ok(canonical)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Option<FileInfo>, PlatformError> {
        self.enter(FsOp::SymlinkMetadata, path)?;
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(Some(FileInfo::from_unix_metadata(&metadata))),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(PlatformError::from_io("symlink_metadata", path, &err)),
        }
    }

    fn create_dir_private(&self, path: &Path) -> Result<(), PlatformError> {
        self.enter(FsOp::CreateDirPrivate, path)?;
        DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|err| PlatformError::from_io("create_dir_private", path, &err))?;
        self.record(FsOp::CreateDirPrivate, path);
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntryInfo>, PlatformError> {
        self.enter(FsOp::ReadDir, path)?;
        let io_err = |err: io::Error| PlatformError::from_io("read_dir", path, &err);
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).map_err(io_err)? {
            let entry = entry.map_err(io_err)?;
            match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => entries.push(DirEntryInfo::new(
                    entry.file_name(),
                    FileInfo::from_unix_metadata(&metadata),
                )),
                // Removed between the listing and the stat: not an entry.
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(PlatformError::from_io("read_dir", &entry.path(), &err)),
            }
        }
        Ok(entries)
    }

    fn read_file_limited(&self, path: &Path, max_len: u64) -> Result<Vec<u8>, PlatformError> {
        self.enter(FsOp::ReadFileLimited, path)?;
        let io_err = |err: io::Error| PlatformError::from_io("read_file_limited", path, &err);
        let not_regular = || PlatformError::NotARegularFile {
            path: path.to_path_buf(),
        };
        // Without libc the fake cannot open with O_NOFOLLOW, so it checks
        // the path first and the opened file after.
        let before = fs::symlink_metadata(path).map_err(io_err)?;
        if FileInfo::from_unix_metadata(&before).kind != FileKind::File {
            return Err(not_regular());
        }
        let file = File::open(path).map_err(io_err)?;
        let opened = FileInfo::from_unix_metadata(&file.metadata().map_err(io_err)?);
        if opened.kind != FileKind::File
            || opened.identity != FileInfo::from_unix_metadata(&before).identity
        {
            return Err(not_regular());
        }
        read_at_most(file, path, max_len)
    }

    fn write_file_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), PlatformError> {
        self.enter(FsOp::WriteFileAtomic, path)?;
        let (dir, temp) = temp_path_for(path)?;
        let written = write_temp(&temp, bytes);
        let renamed = written.and_then(|()| {
            if let Some(fault) = self.take_fault(FsOp::WriteFileAtomic, path) {
                return Err(fault.to_error(FsOp::WriteFileAtomic, path));
            }
            fs::rename(&temp, path).map_err(|err| PlatformError::from_io("rename", path, &err))
        });
        if let Err(err) = renamed {
            remove_own_temp(&temp, &err);
            return Err(err);
        }
        File::open(dir)
            .and_then(|dir| dir.sync_all())
            .map_err(|err| PlatformError::from_io("sync directory", dir, &err))?;
        self.record(FsOp::WriteFileAtomic, path);
        Ok(())
    }
}

/// The canonical form of the path's parent joined with its last component
/// as given, or `None` when the path is relative or has no last component.
/// A parent that does not exist yet resolves through its nearest existing
/// ancestor.
fn resolve_last_as_given(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let name = path.file_name()?;
    let mut missing: Vec<&std::ffi::OsStr> = vec![name];
    let mut current = path.parent()?;
    loop {
        if let Ok(mut resolved) = fs::canonicalize(current) {
            resolved.extend(missing.iter().rev());
            return Some(resolved);
        }
        missing.push(current.file_name()?);
        current = current.parent()?;
    }
}

/// Returns the directory and the temporary file name for an atomic write
/// to `path`: `.<file name>.tmp-<pid>-<counter>` next to it.
fn temp_path_for(path: &Path) -> Result<(&Path, PathBuf), PlatformError> {
    let no_name = || PlatformError::Io {
        op: "write_file_atomic",
        path: path.to_path_buf(),
        kind: io::ErrorKind::InvalidInput,
        message: "path has no file name".to_owned(),
    };
    let dir = path.parent().ok_or_else(no_name)?;
    let name = path.file_name().ok_or_else(no_name)?;
    let mut temp = OsString::from(".");
    temp.push(name);
    temp.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    Ok((dir, dir.join(temp)))
}

fn write_temp(temp: &Path, bytes: &[u8]) -> Result<(), PlatformError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temp)
        .map_err(|err| PlatformError::from_io("create temporary file", temp, &err))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|err| PlatformError::from_io("write temporary file", temp, &err))
}

/// Removes the temporary file after a failed write, unless the failure was
/// that the file could not be created (then it is not ours to remove).
fn remove_own_temp(temp: &Path, err: &PlatformError) {
    let created = !matches!(err, PlatformError::AlreadyExists { path } if path == temp)
        && fs::symlink_metadata(temp).is_ok();
    if created {
        // Best effort: the write already failed, and that error is returned.
        let _ = fs::remove_file(temp);
    }
}

fn read_at_most(file: File, path: &Path, max_len: u64) -> Result<Vec<u8>, PlatformError> {
    let mut bytes = Vec::new();
    file.take(max_len.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| PlatformError::from_io("read_file_limited", path, &err))?;
    if bytes.len() as u64 > max_len {
        return Err(PlatformError::FileTooLarge {
            path: path.to_path_buf(),
            max: max_len,
        });
    }
    Ok(bytes)
}
