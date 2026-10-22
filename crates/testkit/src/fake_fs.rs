use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use stackly_platform::{
    AppendFile, DirEntryInfo, FileInfo, FileKind, Filesystem, PlatformError, REMOVE_TREE_MAX_DEPTH,
    ReadSeekFile, RemoveReport, TREE_USAGE_MAX_DEPTH, TreeUsage, TreeWriter, UsageLimits,
    VolumeFacts, VolumeSpace, remove_tree_names, tree_usage_root,
};

use crate::TestRoot;

mod handles;
mod tree_writer;

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
    /// [`Filesystem::remove_tree`].
    RemoveTree,
    /// [`Filesystem::open_read`].
    OpenRead,
    /// [`Filesystem::open_append`].
    OpenAppend,
    /// [`Filesystem::rename_no_replace`].
    RenameNoReplace,
    /// [`Filesystem::open_tree_writer`].
    OpenTreeWriter,
    /// [`Filesystem::volume_space`], and through it
    /// [`Filesystem::available_space`].
    VolumeSpace,
    /// [`Filesystem::volume_facts`].
    VolumeFacts,
    /// [`Filesystem::tree_usage`].
    TreeUsage,
    /// [`TreeWriter::create_dir`], matched against the root joined with
    /// the entry path.
    TreeCreateDir,
    /// [`TreeWriter::create_file`], matched like [`FsOp::TreeCreateDir`].
    TreeCreateFile,
    /// [`TreeWriter::create_symlink`], matched like
    /// [`FsOp::TreeCreateDir`].
    TreeCreateSymlink,
    /// [`TreeWriter::set_mode`], matched like [`FsOp::TreeCreateDir`].
    TreeSetMode,
    /// [`TreeWriter::sync`], matched against the root.
    TreeSync,
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
            Self::RemoveTree => "remove_tree",
            Self::OpenRead => "open_read",
            Self::OpenAppend => "open_append",
            Self::RenameNoReplace => "rename_no_replace",
            Self::OpenTreeWriter => "open_tree_writer",
            Self::VolumeSpace => "volume_space",
            Self::VolumeFacts => "volume_facts",
            Self::TreeUsage => "tree_usage",
            Self::TreeCreateDir => "create_dir",
            Self::TreeCreateFile => "create_file",
            Self::TreeCreateSymlink => "create_symlink",
            Self::TreeSetMode => "set_mode",
            Self::TreeSync => "sync",
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
    /// The operation: [`FsOp::CreateDirPrivate`],
    /// [`FsOp::WriteFileAtomic`], [`FsOp::RemoveTree`],
    /// [`FsOp::OpenAppend`], [`FsOp::RenameNoReplace`],
    /// [`FsOp::TreeCreateDir`], [`FsOp::TreeCreateFile`], or
    /// [`FsOp::TreeCreateSymlink`].
    pub op: FsOp,
    /// The path it created, replaced, or removed (the target of a
    /// removal, `root` joined with `relative`; the new path of a rename;
    /// the root joined with the entry path for a tree writer).
    pub path: PathBuf,
}

/// The size, the space left, and the kind that [`FakeFilesystem::volume_space`]
/// and [`FakeFilesystem::volume_facts`] answer for paths below one prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FakeVolume {
    /// The size of the volume, in bytes.
    pub total: u64,
    /// The bytes left, in bytes.
    pub available: u64,
    /// The filesystem type name [`Filesystem::volume_facts`] reports.
    pub fs_type: &'static str,
    /// Whether [`Filesystem::volume_facts`] reports the volume as local.
    pub local: bool,
}

impl FakeVolume {
    /// The volume every path lies on until a test sets another: a local
    /// `apfs` volume of 1 TiB, half of it free.
    pub const DEFAULT: Self = Self::new(1 << 40, 1 << 39);

    /// A local `apfs` volume of `total` bytes with `available` bytes left.
    #[must_use]
    pub const fn new(total: u64, available: u64) -> Self {
        Self {
            total,
            available,
            fs_type: "apfs",
            local: true,
        }
    }

    /// This volume with another filesystem type name and locality.
    #[must_use]
    pub const fn with_kind(mut self, fs_type: &'static str, local: bool) -> Self {
        self.fs_type = fs_type;
        self.local = local;
        self
    }
}

#[derive(Debug, Default)]
struct State {
    next: Vec<(FsOp, Fault)>,
    when: Vec<(FsOp, PathBuf, Fault)>,
    mutations: Vec<Mutation>,
    volumes: Vec<(PathBuf, FakeVolume)>,
    delays: Vec<(FsOp, Duration)>,
    hooks: Vec<(FsOp, Hook)>,
}

/// A closure run once by the next call of an operation; see
/// [`FakeFilesystem::run_on_next`].
struct Hook(Box<dyn FnOnce() + Send>);

impl std::fmt::Debug for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hook(..)")
    }
}

/// The fault and mutation state, shared with the handles and tree writers
/// a [`FakeFilesystem`] hands out.
#[derive(Debug, Clone, Default)]
struct Shared(Arc<Mutex<State>>);

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn take_fault(&self, op: FsOp, path: &Path) -> Option<Fault> {
        let mut state = self.lock();
        if let Some(index) = state.next.iter().position(|(next_op, _)| *next_op == op) {
            return Some(state.next.remove(index).1);
        }
        state
            .when
            .iter()
            .find(|(when_op, suffix, _)| *when_op == op && path.ends_with(suffix))
            .map(|(_, _, fault)| *fault)
    }

    /// Fails with the fault injected for `op` on `path`, if any.
    fn check_fault(&self, op: FsOp, path: &Path) -> Result<(), PlatformError> {
        match self.take_fault(op, path) {
            Some(fault) => Err(fault.to_error(op, path)),
            None => Ok(()),
        }
    }

    fn record(&self, op: FsOp, path: &Path) {
        self.lock().mutations.push(Mutation {
            op,
            path: path.to_path_buf(),
        });
    }
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
/// before the rename, so the cleanup path runs. A fault on
/// [`FsOp::RemoveTree`] matches the target path and fires before anything
/// is removed.
///
/// [`remove_tree`](Filesystem::remove_tree) checks that its root and its
/// target are confined, then walks the tree with `symlink_metadata`,
/// `read_dir`, `remove_file`, and `remove_dir`, never following a symlink
/// and checking each entry's device. Unlike the macOS adapter it is not
/// safe against a folder swapped for a symlink during the walk, so race
/// tests run only against the adapter.
///
/// The same holds for [`open_read`](Filesystem::open_read),
/// [`open_append`](Filesystem::open_append),
/// [`rename_no_replace`](Filesystem::rename_no_replace), and the
/// [`TreeWriter`] from [`open_tree_writer`](Filesystem::open_tree_writer):
/// each checks a path with `symlink_metadata` and acts on it afterwards.
/// The tree writer's [`set_mode`](TreeWriter::set_mode) leaves a symlink
/// as it is, since `std` cannot change a link's own bits; it never touches
/// the target. [`volume_space`](Filesystem::volume_space) answers from a
/// table of volumes per path prefix ([`set_volume`](Self::set_volume)),
/// and [`FakeVolume::DEFAULT`] for any other path that exists.
#[derive(Debug)]
pub struct FakeFilesystem {
    roots: Vec<PathBuf>,
    violations: AtomicUsize,
    state: Shared,
    tree_usage_calls: AtomicUsize,
    tree_usage_active: AtomicUsize,
    tree_usage_peak: AtomicUsize,
}

impl FakeFilesystem {
    /// Confines a filesystem to `root`'s test folder and run folder.
    #[must_use]
    pub fn new(root: &TestRoot) -> Self {
        Self {
            roots: vec![root.path().to_path_buf(), root.run_dir().to_path_buf()],
            violations: AtomicUsize::new(0),
            state: Shared::default(),
            tree_usage_calls: AtomicUsize::new(0),
            tree_usage_active: AtomicUsize::new(0),
            tree_usage_peak: AtomicUsize::new(0),
        }
    }

    /// Returns how many [`tree_usage`](Filesystem::tree_usage) calls were
    /// made so far, the ones that failed included.
    #[must_use]
    pub fn tree_usage_calls(&self) -> usize {
        self.tree_usage_calls.load(Ordering::SeqCst)
    }

    /// Returns the most [`tree_usage`](Filesystem::tree_usage) calls that
    /// ran at the same time so far, a call counting from its start until it
    /// returns, delay included.
    #[must_use]
    pub fn tree_usage_peak(&self) -> usize {
        self.tree_usage_peak.load(Ordering::SeqCst)
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

    /// Makes every call of `op` sleep for `delay` before it does anything
    /// else, until another call sets a new delay for `op`; a zero `delay`
    /// removes it. [`clear_faults`](Self::clear_faults) leaves delays alone.
    pub fn set_delay(&self, op: FsOp, delay: Duration) {
        let mut state = self.state();
        state.delays.retain(|(existing, _)| *existing != op);
        if !delay.is_zero() {
            state.delays.push((op, delay));
        }
    }

    /// Runs `hook` once, on the calling thread of the next call of `op`,
    /// before the call does anything else (its delay included, so the hook
    /// has run by the time a delayed read touches the volume). The hook runs
    /// without any lock of this filesystem held, so it may use it. Use it to
    /// move a [`FakeClock`](crate::FakeClock) or flip a flag at an exact
    /// point of a read, with no wall clock sequencing.
    pub fn run_on_next(&self, op: FsOp, hook: impl FnOnce() + Send + 'static) {
        self.state().hooks.push((op, Hook(Box::new(hook))));
    }

    /// Returns every directory created, file written, and tree removed so
    /// far, in order. A removal counts when it removed at least one entry,
    /// even if it then failed. An append file counts once, when it is
    /// opened, a rename once it moved the entry, and a tree writer once per
    /// directory, file, and symlink it created.
    #[must_use]
    pub fn mutations(&self) -> Vec<Mutation> {
        self.state().mutations.clone()
    }

    /// Makes [`volume_space`](Filesystem::volume_space) answer `volume`
    /// for every path below `prefix` (compared by whole components); the
    /// longest matching prefix wins.
    pub fn set_volume(&self, prefix: impl Into<PathBuf>, volume: FakeVolume) {
        let prefix = prefix.into();
        let mut state = self.state();
        state.volumes.retain(|(existing, _)| *existing != prefix);
        state.volumes.push((prefix, volume));
    }

    /// The volume table entry for `path`, which must exist: the longest
    /// matching prefix, else [`FakeVolume::DEFAULT`].
    fn volume_of(&self, op: &'static str, path: &Path) -> Result<FakeVolume, PlatformError> {
        fs::metadata(path).map_err(|err| PlatformError::from_io(op, path, &err))?;
        let state = self.state();
        Ok(state
            .volumes
            .iter()
            .filter(|(prefix, _)| path.starts_with(prefix))
            .max_by_key(|(prefix, _)| prefix.components().count())
            .map_or(FakeVolume::DEFAULT, |(_, volume)| *volume))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock()
    }

    /// Checks confinement, then injected faults (except for writes, whose
    /// fault fires before the rename).
    fn enter(&self, op: FsOp, path: &Path) -> Result<(), PlatformError> {
        let hook = {
            let mut state = self.state();
            let index = state.hooks.iter().position(|(hooked, _)| *hooked == op);
            index.map(|index| state.hooks.remove(index).1)
        };
        if let Some(Hook(hook)) = hook {
            hook();
        }
        let delay = self
            .state()
            .delays
            .iter()
            .find(|(delayed, _)| *delayed == op)
            .map(|(_, delay)| *delay);
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
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
        self.state.take_fault(op, path)
    }

    fn record(&self, op: FsOp, path: &Path) {
        self.state.record(op, path);
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

    fn remove_tree(&self, root: &Path, relative: &Path) -> Result<RemoveReport, PlatformError> {
        let names = remove_tree_names(root, relative)?;
        let target = root.join(relative);
        self.confine(root)?;
        self.enter(FsOp::RemoveTree, &target)?;
        let mut report = RemoveReport::default();
        let result = remove_tree_portable(root, &names, &mut report);
        if report.files + report.dirs > 0 {
            self.record(FsOp::RemoveTree, &target);
        }
        result.map(|()| report)
    }

    fn open_read(&self, path: &Path) -> Result<Box<dyn ReadSeekFile>, PlatformError> {
        self.enter(FsOp::OpenRead, path)?;
        handles::open_read(path)
    }

    fn open_append(&self, path: &Path) -> Result<Box<dyn AppendFile>, PlatformError> {
        self.enter(FsOp::OpenAppend, path)?;
        let file = handles::open_append(path)?;
        self.record(FsOp::OpenAppend, path);
        Ok(file)
    }

    fn rename_no_replace(&self, from: &Path, to: &Path) -> Result<(), PlatformError> {
        self.confine(from)?;
        self.enter(FsOp::RenameNoReplace, to)?;
        handles::rename_no_replace(from, to)?;
        self.record(FsOp::RenameNoReplace, to);
        Ok(())
    }

    fn open_tree_writer(&self, root: &Path) -> Result<Box<dyn TreeWriter>, PlatformError> {
        self.enter(FsOp::OpenTreeWriter, root)?;
        tree_writer::open(root, self.state.clone())
    }

    fn volume_space(&self, path: &Path) -> Result<VolumeSpace, PlatformError> {
        self.enter(FsOp::VolumeSpace, path)?;
        let volume = self.volume_of("volume_space", path)?;
        Ok(VolumeSpace::new(volume.total, volume.available))
    }

    fn volume_facts(&self, path: &Path) -> Result<VolumeFacts, PlatformError> {
        self.enter(FsOp::VolumeFacts, path)?;
        let volume = self.volume_of("volume_facts", path)?;
        Ok(VolumeFacts::new(volume.fs_type, volume.local))
    }

    fn tree_usage(&self, root: &Path, limits: UsageLimits) -> Result<TreeUsage, PlatformError> {
        self.tree_usage_calls.fetch_add(1, Ordering::SeqCst);
        let active = self.tree_usage_active.fetch_add(1, Ordering::SeqCst) + 1;
        self.tree_usage_peak.fetch_max(active, Ordering::SeqCst);
        let _running = ActiveCall(&self.tree_usage_active);
        tree_usage_root(root)?;
        // The delay of `enter` runs before the deadline starts.
        self.enter(FsOp::TreeUsage, root)?;
        tree_usage_portable(root, limits)
    }
}

/// Counts one running `tree_usage` call down when it returns.
struct ActiveCall<'a>(&'a AtomicUsize);

impl Drop for ActiveCall<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One open folder of the portable `tree_usage` walk.
struct UsageLevel {
    entries: fs::ReadDir,
    path: PathBuf,
}

/// The portable walk of [`Filesystem::tree_usage`] with the rules of the
/// macOS adapter, on `std::fs`: one open folder per level, names read one at
/// a time, and every check reads a path with `symlink_metadata` semantics
/// and acts on it afterwards, so it is not safe against a folder swapped
/// during the walk.
fn tree_usage_portable(root: &Path, limits: UsageLimits) -> Result<TreeUsage, PlatformError> {
    let deadline = Instant::now().checked_add(limits.deadline);
    let io_err = |path: &Path, err: &io::Error| PlatformError::from_io("tree_usage", path, err);
    let root_info = fs::symlink_metadata(root).map_err(|err| io_err(root, &err))?;
    if root_info.file_type().is_symlink() {
        return Err(PlatformError::SymlinkInPath {
            path: root.to_path_buf(),
        });
    }
    if !root_info.is_dir() {
        return Err(PlatformError::NotADirectory {
            path: root.to_path_buf(),
        });
    }
    let device = root_info.dev();
    let mut bytes = root_info.blocks().saturating_mul(512);
    let mut entries = 0_u64;
    let mut complete = true;
    let mut linked = std::collections::HashSet::new();
    let mut stack = vec![UsageLevel {
        entries: fs::read_dir(root).map_err(|err| io_err(root, &err))?,
        path: root.to_path_buf(),
    }];
    while let Some(level) = stack.last_mut() {
        let Some(entry) = level.entries.next() else {
            stack.pop();
            continue;
        };
        let entry = entry.map_err(|err| io_err(&level.path, &err))?;
        entries += 1;
        if entries > limits.max_entries || deadline.is_some_and(|at| Instant::now() >= at) {
            complete = false;
            break;
        }
        let path = entry.path();
        let info = match fs::symlink_metadata(&path) {
            Ok(info) => info,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(io_err(&path, &err)),
        };
        if info.dev() != device {
            continue;
        }
        if !info.is_dir() {
            if info.nlink() <= 1 || linked.insert((info.dev(), info.ino())) {
                bytes = bytes.saturating_add(info.blocks().saturating_mul(512));
            }
            continue;
        }
        bytes = bytes.saturating_add(info.blocks().saturating_mul(512));
        if stack.len() >= TREE_USAGE_MAX_DEPTH {
            complete = false;
            continue;
        }
        match fs::read_dir(&path) {
            Ok(entries) => stack.push(UsageLevel { entries, path }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                bytes = bytes.saturating_sub(info.blocks().saturating_mul(512));
            }
            Err(err) if err.kind() == io::ErrorKind::NotADirectory => {}
            Err(err) => return Err(io_err(&path, &err)),
        }
    }
    Ok(TreeUsage::new(bytes, complete))
}

/// Removes `root` joined with `names` the way [`Filesystem::remove_tree`]
/// describes, counting what it removed in `report` as it goes. Every check
/// reads a path with `symlink_metadata` and acts on it afterwards, so a
/// concurrent swap between the two can redirect it.
fn remove_tree_portable(
    root: &Path,
    names: &[&OsStr],
    report: &mut RemoveReport,
) -> Result<(), PlatformError> {
    let root_info = fs::symlink_metadata(root)
        .map_err(|err| PlatformError::from_io("remove_tree", root, &err))?;
    expect_dir_on_the_way(root, &root_info)?;
    let device = root_info.dev();
    let (last, parents) = names
        .split_last()
        .expect("remove_tree_names returns at least one name");
    let mut path = root.to_path_buf();
    for name in parents {
        path.push(name);
        let Some(info) = stat_entry(&path)? else {
            *report = RemoveReport::missing();
            return Ok(());
        };
        expect_dir_on_the_way(&path, &info)?;
        expect_device(&path, &info, device)?;
    }
    path.push(last);
    let Some(info) = stat_entry(&path)? else {
        *report = RemoveReport::missing();
        return Ok(());
    };
    expect_device(&path, &info, device)?;
    if !info.is_dir() {
        if remove_entry(&path)? {
            report.files += 1;
        } else {
            *report = RemoveReport::missing();
        }
        return Ok(());
    }
    let mut stack = vec![(list_entries(&path)?, path)];
    while let Some((pending, dir)) = stack.last_mut() {
        if let Some(entry) = pending.pop() {
            let Some(info) = stat_entry(&entry)? else {
                continue;
            };
            expect_device(&entry, &info, device)?;
            if info.is_dir() {
                if stack.len() >= REMOVE_TREE_MAX_DEPTH {
                    return Err(PlatformError::TooDeep {
                        path: entry,
                        max: REMOVE_TREE_MAX_DEPTH,
                    });
                }
                stack.push((list_entries(&entry)?, entry));
            } else if remove_entry(&entry)? {
                report.files += 1;
            }
        } else {
            let dir = std::mem::take(dir);
            stack.pop();
            match fs::remove_dir(&dir) {
                Ok(()) => report.dirs += 1,
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(PlatformError::from_io("remove_tree", &dir, &err)),
            }
        }
    }
    Ok(())
}

/// Reads an entry without following a symlink; `None` when it is gone.
fn stat_entry(path: &Path) -> Result<Option<Metadata>, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(info) => Ok(Some(info)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(PlatformError::from_io("remove_tree", path, &err)),
    }
}

/// Fails unless a component on the way to the target is a real directory.
fn expect_dir_on_the_way(path: &Path, info: &Metadata) -> Result<(), PlatformError> {
    let path = path.to_path_buf();
    if info.file_type().is_symlink() {
        Err(PlatformError::SymlinkInPath { path })
    } else if info.is_dir() {
        Ok(())
    } else {
        Err(PlatformError::NotADirectory { path })
    }
}

/// Fails when an entry lies on another device than the root.
fn expect_device(path: &Path, info: &Metadata, device: u64) -> Result<(), PlatformError> {
    if info.dev() == device {
        Ok(())
    } else {
        Err(PlatformError::CrossesDevice {
            path: path.to_path_buf(),
        })
    }
}

/// Lists the full paths of a directory's entries.
fn list_entries(dir: &Path) -> Result<Vec<PathBuf>, PlatformError> {
    let io_err = |err: io::Error| PlatformError::from_io("remove_tree", dir, &err);
    fs::read_dir(dir)
        .map_err(io_err)?
        .map(|entry| entry.map(|entry| entry.path()).map_err(io_err))
        .collect()
}

/// Unlinks an entry that is not a directory; false when it is already gone.
fn remove_entry(path: &Path) -> Result<bool, PlatformError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(PlatformError::from_io("remove_tree", path, &err)),
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
