//! The part of an extraction every format shares: name normalization,
//! `archiveRoot`, AppleDouble, the entry table, limits, modes, and the
//! symlinks written last. The format readers only decode members and hand
//! each to [`Extraction::add`].

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io::{self, Read, Write};

use stackly_domain::ArtifactFormat;
use stackly_platform::{EntryPath, FileMode, PlatformError, StopFlag, TreeWriter};

use super::limits::{COPY_BUFFER_LEN, SYMLINK_TARGET_MAX_LEN};
use super::names::{self, MARKER_NAME, Normalized};
use super::source::SourceError;
use super::symlinks::{self, Found};
use super::{ArchiveError, EntryKind, EntryRecord, ExtractReport, ExtractSpec, RejectReason};

/// The final mode of a folder.
const DIR_MODE: u32 = 0o755;
/// The final mode of a file the archive marks executable.
const EXECUTABLE_MODE: u32 = 0o555;
/// The final mode of any other file.
const FILE_MODE: u32 = 0o444;
const EXECUTE_BITS: u32 = 0o111;
const SPECIAL_BITS: u32 = 0o7000;

/// One member as a format reader decoded it.
pub(super) enum Member<'r> {
    /// A folder.
    Dir,
    /// A regular file of `size` bytes, whose contents `data` yields.
    File { size: u64, data: &'r mut dyn Read },
    /// A symlink with the raw target bytes.
    Symlink { target: Vec<u8> },
}

/// The permission bits a format gives a member, when it has them.
#[derive(Debug, Clone, Copy)]
pub(super) enum ArchiveMode {
    /// The bits as the archive stores them.
    Known(u32),
    /// The format carries none (a zip made on another OS, a binary).
    Unknown,
}

struct Node {
    record: EntryRecord,
    /// False for a folder only implied by the paths below it.
    explicit: bool,
    /// The next node whose path has the same hash.
    next: Option<usize>,
}

/// One extraction in progress.
pub(super) struct Extraction<'a> {
    spec: &'a ExtractSpec,
    root: Option<String>,
    sink: &'a mut dyn TreeWriter,
    stop: &'a StopFlag,
    /// True for `zip` and `binary`, whose executables get the execute bits
    /// whatever the archive says.
    executables_get_execute: bool,
    nodes: Vec<Node>,
    /// Path hash to the last node with that hash; the table keeps no
    /// second copy of any path.
    index: HashMap<u64, usize>,
    hasher: RandomState,
    symlinks: Vec<usize>,
    counted: u64,
    report: ExtractReport,
    buffer: Vec<u8>,
    source_error: SourceError,
}

impl<'a> Extraction<'a> {
    pub(super) fn new(
        format: ArtifactFormat,
        spec: &'a ExtractSpec,
        sink: &'a mut dyn TreeWriter,
        stop: &'a StopFlag,
    ) -> Result<Self, ArchiveError> {
        let root = match &spec.archive_root {
            None => None,
            Some(root) => match names::normalize(root.as_bytes(), true) {
                Ok(Normalized::Path(root)) => Some(root),
                _ => {
                    return Err(ArchiveError::InvalidSpec {
                        reason: "archiveRoot is not a relative folder path",
                    });
                }
            },
        };
        Ok(Self {
            spec,
            root,
            sink,
            stop,
            executables_get_execute: format != ArtifactFormat::TarGz,
            nodes: Vec::new(),
            index: HashMap::new(),
            hasher: RandomState::new(),
            symlinks: Vec::new(),
            counted: 0,
            report: ExtractReport::default(),
            buffer: vec![0; COPY_BUFFER_LEN],
            source_error: SourceError::default(),
        })
    }

    pub(super) fn spec(&self) -> &ExtractSpec {
        self.spec
    }

    /// The slot the reader's [`Tracked`](super::source::Tracked) file
    /// notes its I/O errors in.
    pub(super) fn source_error(&self) -> SourceError {
        self.source_error.clone()
    }

    pub(super) fn check_stop(&self) -> Result<(), ArchiveError> {
        if self.stop.is_stopped() {
            Err(ArchiveError::Stopped)
        } else {
            Ok(())
        }
    }

    /// The error of a member, named as the archive gives it.
    pub(super) fn reject(reason: RejectReason, raw: &[u8]) -> ArchiveError {
        ArchiveError::Rejected {
            reason,
            entry: names::entry_text(raw),
        }
    }

    /// Maps an error from reading or decoding the archive at member
    /// `raw`: an I/O error of the file itself, else a broken archive.
    pub(super) fn read_error(&self, raw: &[u8]) -> ArchiveError {
        read_error(&self.source_error, raw)
    }

    /// Writes one member, or skips it when it is AppleDouble metadata, the
    /// root itself, or `archiveRoot` itself.
    pub(super) fn add(
        &mut self,
        raw: &[u8],
        mode: ArchiveMode,
        member: Member<'_>,
    ) -> Result<(), ArchiveError> {
        self.check_stop()?;
        let is_dir = matches!(member, Member::Dir);
        let reject = |reason| Self::reject(reason, raw);
        let name = match names::normalize(raw, is_dir).map_err(reject)? {
            Normalized::Root => return Ok(()),
            Normalized::Path(name) => name,
        };
        if names::is_apple_double(&name) {
            self.claim_room(raw)?;
            self.report.skipped_metadata += 1;
            return Ok(());
        }
        let relative = match &self.root {
            None => name.as_str(),
            Some(root) if name == *root => {
                return if is_dir {
                    Ok(())
                } else {
                    Err(reject(RejectReason::UnexpectedTopLevel))
                };
            }
            Some(root) => name
                .strip_prefix(root.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .ok_or_else(|| reject(RejectReason::UnexpectedTopLevel))?,
        };
        let path = names::entry_path(relative).map_err(reject)?;
        if path.as_str() == MARKER_NAME {
            return Err(reject(RejectReason::ReservedName));
        }
        self.ensure_parents(&path, raw)?;
        if let Some(found) = self.find(path.as_str()) {
            // A folder named again (the MySQL tarballs repeat `bin/`) or
            // named after the paths below it overwrites nothing; any other
            // second member with the same name is a duplicate.
            let node = &mut self.nodes[found];
            if is_dir && node.record.kind == EntryKind::Dir {
                if !std::mem::replace(&mut node.explicit, true) {
                    self.note_special_bits(mode);
                }
                return Ok(());
            }
            return Err(reject(RejectReason::DuplicateEntry));
        }
        match member {
            Member::Dir => {
                self.claim_room(raw)?;
                self.sink
                    .create_dir(&path)
                    .map_err(|err| write_error(err, raw))?;
                self.note_special_bits(mode);
                self.report.dirs += 1;
                self.push(path, EntryKind::Dir, Some(DIR_MODE), 0, None, true);
            }
            Member::File { size, data } => {
                self.claim_room(raw)?;
                let final_mode = self.file_mode(&path, mode);
                self.write_file(&path, size, data, raw)?;
                self.note_special_bits(mode);
                self.report.files += 1;
                self.push(path, EntryKind::File, Some(final_mode), size, None, true);
            }
            Member::Symlink { target } => {
                self.claim_room(raw)?;
                let target = check_target(target).map_err(reject)?;
                let index = self.push(path, EntryKind::Symlink, None, 0, Some(target), true);
                self.symlinks.push(index);
            }
        }
        Ok(())
    }

    /// Checks the symlinks against the finished tree, then the
    /// executables, and creates the symlinks.
    pub(super) fn finish(mut self) -> Result<ExtractReport, ArchiveError> {
        for &index in &self.symlinks {
            let record = &self.nodes[index].record;
            let target = record
                .target
                .as_deref()
                .expect("a symlink record carries its target");
            symlinks::check(&|path| self.found(path), record.path.as_str(), target)
                .map_err(|reason| Self::reject(reason, record.path.as_str().as_bytes()))?;
        }
        for executable in &self.spec.executables {
            if let Some(index) = self.find(executable.as_str())
                && self.nodes[index].record.kind != EntryKind::File
            {
                return Err(Self::reject(
                    RejectReason::ExecutableNotRegular,
                    executable.as_str().as_bytes(),
                ));
            }
        }
        for &index in &self.symlinks {
            self.check_stop()?;
            let record = &self.nodes[index].record;
            let target = record
                .target
                .as_deref()
                .expect("a symlink record carries its target");
            self.sink
                .create_symlink(&record.path, target)
                .map_err(|err| write_error(err, record.path.as_str().as_bytes()))?;
            self.report.symlinks += 1;
        }
        self.report.entries = self.nodes.into_iter().map(|node| node.record).collect();
        Ok(self.report)
    }

    /// Creates every folder on the way to `path` that the archive has not
    /// created yet, and refuses a symlink or file on the way.
    fn ensure_parents(&mut self, path: &EntryPath, raw: &[u8]) -> Result<(), ArchiveError> {
        let Some(parent) = path.parent() else {
            return Ok(());
        };
        let mut prefix = String::new();
        for component in parent.components() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            match self
                .find(&prefix)
                .map(|index| self.nodes[index].record.kind)
            {
                Some(EntryKind::Dir) => {}
                Some(EntryKind::Symlink) => {
                    return Err(Self::reject(RejectReason::EntryThroughSymlink, raw));
                }
                Some(EntryKind::File) => {
                    return Err(Self::reject(RejectReason::DuplicateEntry, raw));
                }
                None => {
                    self.claim_room(raw)?;
                    let dir = EntryPath::new(&prefix)
                        .map_err(|_| Self::reject(RejectReason::NameInvalid, raw))?;
                    self.sink
                        .create_dir(&dir)
                        .map_err(|err| write_error(err, raw))?;
                    self.report.dirs += 1;
                    self.push(dir, EntryKind::Dir, Some(DIR_MODE), 0, None, false);
                }
            }
        }
        Ok(())
    }

    /// Copies `size` bytes of `data` into a new file, refusing more or
    /// fewer bytes and any byte past the total limit.
    fn write_file(
        &mut self,
        path: &EntryPath,
        size: u64,
        data: &mut dyn Read,
        raw: &[u8],
    ) -> Result<(), ArchiveError> {
        if self.report.file_bytes.saturating_add(size) > self.spec.limits.max_total_file_bytes {
            return Err(Self::reject(RejectReason::TooLarge, raw));
        }
        let mut file = self
            .sink
            .create_file(path)
            .map_err(|err| write_error(err, raw))?;
        let mut written = 0_u64;
        loop {
            if self.stop.is_stopped() {
                return Err(ArchiveError::Stopped);
            }
            let read = match data.read(&mut self.buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(read_error(&self.source_error, raw)),
            };
            let chunk = read as u64;
            if written + chunk > size {
                return Err(Self::reject(RejectReason::SizeMismatch, raw));
            }
            file.write_all(&self.buffer[..read]).map_err(|err| {
                ArchiveError::Write(PlatformError::from_io("write", path.as_path(), &err))
            })?;
            written += chunk;
            self.report.file_bytes += chunk;
        }
        if written != size {
            return Err(Self::reject(RejectReason::SizeMismatch, raw));
        }
        file.finish().map_err(|err| write_error(err, raw))?;
        Ok(())
    }

    /// Counts one more entry, refusing it past the entry limit.
    fn claim_room(&mut self, raw: &[u8]) -> Result<(), ArchiveError> {
        if self.counted >= self.spec.limits.max_entries {
            return Err(Self::reject(RejectReason::TooManyEntries, raw));
        }
        self.counted += 1;
        Ok(())
    }

    fn file_mode(&self, path: &EntryPath, mode: ArchiveMode) -> u32 {
        let marked = matches!(mode, ArchiveMode::Known(bits) if bits & EXECUTE_BITS != 0);
        let listed = self.executables_get_execute && self.spec.executables.contains(path);
        if marked || listed {
            EXECUTABLE_MODE
        } else {
            FILE_MODE
        }
    }

    fn note_special_bits(&mut self, mode: ArchiveMode) {
        if matches!(mode, ArchiveMode::Known(bits) if bits & SPECIAL_BITS != 0) {
            self.report.stripped_special_bits += 1;
        }
    }

    fn hash(&self, path: &str) -> u64 {
        self.hasher.hash_one(path)
    }

    fn find(&self, path: &str) -> Option<usize> {
        let mut cursor = self.index.get(&self.hash(path)).copied();
        while let Some(index) = cursor {
            let node = &self.nodes[index];
            if node.record.path.as_str() == path {
                return Some(index);
            }
            cursor = node.next;
        }
        None
    }

    fn found(&self, path: &str) -> Option<Found<'_>> {
        let record = &self.nodes[self.find(path)?].record;
        Some(match record.kind {
            EntryKind::Dir => Found::Dir,
            EntryKind::File => Found::File,
            EntryKind::Symlink => Found::Symlink(record.target.as_deref().unwrap_or_default()),
        })
    }

    fn push(
        &mut self,
        path: EntryPath,
        kind: EntryKind,
        mode: Option<u32>,
        size: u64,
        target: Option<String>,
        explicit: bool,
    ) -> usize {
        let index = self.nodes.len();
        let next = self.index.insert(self.hash(path.as_str()), index);
        let mode = mode.map(|bits| {
            FileMode::new(bits).expect("the extractor's final modes are at most 0o777")
        });
        self.nodes.push(Node {
            record: EntryRecord {
                path,
                kind,
                mode,
                size,
                target,
            },
            explicit,
            next,
        });
        index
    }
}

/// See [`Extraction::read_error`].
fn read_error(source: &SourceError, raw: &[u8]) -> ArchiveError {
    match &*source.borrow() {
        Some((kind, message)) => ArchiveError::Read {
            kind: *kind,
            message: message.clone(),
        },
        None => Extraction::reject(RejectReason::Corrupt, raw),
    }
}

/// Maps a tree writer error: an existing entry or a file on the way is a
/// duplicate, a symlink on the way an entry through a symlink, anything
/// else a write failure.
fn write_error(err: PlatformError, raw: &[u8]) -> ArchiveError {
    match err {
        PlatformError::AlreadyExists { .. } | PlatformError::NotADirectory { .. } => {
            Extraction::reject(RejectReason::DuplicateEntry, raw)
        }
        PlatformError::SymlinkInPath { .. } => {
            Extraction::reject(RejectReason::EntryThroughSymlink, raw)
        }
        other => ArchiveError::Write(other),
    }
}

/// Checks a symlink target: UTF-8, not empty, no NUL, at most
/// [`SYMLINK_TARGET_MAX_LEN`] bytes, and relative.
fn check_target(target: Vec<u8>) -> Result<String, RejectReason> {
    let target = String::from_utf8(target).map_err(|_| RejectReason::NameInvalid)?;
    if target.is_empty() || target.contains('\0') {
        return Err(RejectReason::NameInvalid);
    }
    if target.len() > SYMLINK_TARGET_MAX_LEN {
        return Err(RejectReason::NameTooLong);
    }
    if target.starts_with('/') {
        return Err(RejectReason::SymlinkEscape);
    }
    Ok(target)
}
