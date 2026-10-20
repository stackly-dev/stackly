//! The runtime extractor (spec 0018, *Ekstraksi aman*).
//!
//! [`extract`] reads a `tar_gz`, `zip`, or `binary` artifact from a file
//! that already passed its digest check and writes it into an empty tree
//! through a [`TreeWriter`]: folders first as they appear, files with their
//! contents copied under the limits of [`ExtractLimits`], and symlinks last,
//! once every rule about where they point holds. [`scan`] runs the same
//! reader into a tree in memory that keeps no contents, which is how
//! `cargo xtask runtime pin` measures `unpacked_size` and `entry_count`.
//!
//! A member that breaks a rule stops the extraction with
//! [`ArchiveError::Rejected`] and a stable [`RejectReason`], which the
//! install pipeline reports as `runtime.archive_rejected`.

mod binary;
mod engine;
pub mod limits;
mod memory;
mod names;
mod source;
mod symlinks;
mod tar_gz;
mod zip_archive;

use std::fmt;
use std::io;

use stackly_domain::ArtifactFormat;
use stackly_platform::{EntryPath, FileMode, PlatformError, ReadSeekFile, StopFlag, TreeWriter};

pub use limits::ExtractLimits;

use engine::Extraction;
use memory::MemoryTree;

/// What to extract from one artifact (spec 0018, *Ekstraksi aman*).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExtractSpec {
    /// The one top level folder every kept member lies in, stripped before
    /// writing (`archiveRoot` in `versions.json`); `None` writes members
    /// at the root of the tree.
    pub archive_root: Option<String>,
    /// The artifact's executables. They must not be symlinks; in a `zip`
    /// or `binary` artifact they get the execute bits, and a `binary`
    /// artifact is written to the first one.
    pub executables: Vec<EntryPath>,
    /// The size and entry limits.
    pub limits: ExtractLimits,
}

impl ExtractSpec {
    /// Bundles what to extract.
    #[must_use]
    pub fn new(
        archive_root: Option<String>,
        executables: Vec<EntryPath>,
        limits: ExtractLimits,
    ) -> Self {
        Self {
            archive_root,
            executables,
            limits,
        }
    }
}

/// What one extraction wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExtractReport {
    /// Regular files written.
    pub files: u64,
    /// Folders created, those the archive only implies included.
    pub dirs: u64,
    /// Symlinks created.
    pub symlinks: u64,
    /// Bytes written to regular files.
    pub file_bytes: u64,
    /// AppleDouble members skipped (`__MACOSX/` and `._` names).
    pub skipped_metadata: u64,
    /// Files and folders whose setuid, setgid, or sticky bit was dropped.
    pub stripped_special_bits: u64,
    /// Every entry in the tree, in the order it was written, with its
    /// final mode, for the closure check and the final mode step.
    pub entries: Vec<EntryRecord>,
}

impl ExtractReport {
    /// Returns the count [`ExtractLimits::max_entries`] bounds and
    /// `runtime pin` records as `entry_count`: every entry plus every
    /// skipped AppleDouble member.
    #[must_use]
    pub const fn entry_count(&self) -> u64 {
        self.files + self.dirs + self.symlinks + self.skipped_metadata
    }
}

/// One entry of an extracted tree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EntryRecord {
    /// Where it lies, relative to the root of the tree.
    pub path: EntryPath,
    /// What it is.
    pub kind: EntryKind,
    /// The mode the final mode step gives it: 0755 for a folder, 0555 or
    /// 0444 for a file, and `None` for a symlink, whose bits are never set.
    pub mode: Option<FileMode>,
    /// The size of a file in bytes; 0 for a folder or symlink.
    pub size: u64,
    /// The target of a symlink, exactly as the archive gives it.
    pub target: Option<String>,
}

/// The kind of an [`EntryRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// A folder.
    Dir,
    /// A regular file.
    File,
    /// A symlink.
    Symlink,
}

/// Why the extractor refused an archive, the `reason` detail of
/// `runtime.archive_rejected` (spec 0018, *Ekstraksi aman*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RejectReason {
    /// A member name starts with `/`.
    AbsolutePath,
    /// A member name holds a `..` component.
    PathTraversal,
    /// A member name is not UTF-8, holds a NUL byte or a backslash, or has
    /// an empty or `.` component; also an empty or malformed symlink
    /// target.
    NameInvalid,
    /// A member name or symlink target breaks a length or depth limit.
    NameTooLong,
    /// A member lies outside `archiveRoot`.
    UnexpectedTopLevel,
    /// A member is named `.stackly-runtime.json` at the root of the tree.
    ReservedName,
    /// A file holds more or fewer bytes than its header says, or a folder
    /// or symlink carries data.
    SizeMismatch,
    /// A member is a hard link.
    HardLink,
    /// A member is a device, FIFO, or socket.
    SpecialFile,
    /// A member has a type the extractor does not write, a sparse file
    /// included.
    UnsupportedEntry,
    /// A symlink target is absolute or leads out of the tree.
    SymlinkEscape,
    /// A symlink target, or a component on the way to it, is missing or
    /// is a file where a folder is needed.
    SymlinkDangling,
    /// A symlink leads through another symlink on the way, through a chain
    /// longer than 8, or around a cycle.
    SymlinkChain,
    /// A member lies below a symlink.
    EntryThroughSymlink,
    /// An executable of the artifact is a symlink or folder.
    ExecutableNotRegular,
    /// Two members name the same entry, also when they differ only in
    /// letter case on a volume that ignores it. A folder named twice is
    /// one folder, not a duplicate.
    DuplicateEntry,
    /// Bytes follow the end of the archive.
    TrailingData,
    /// Bytes precede the first entry of a zip.
    PrefixData,
    /// A zip entry is encrypted.
    Encrypted,
    /// A zip entry uses a method other than stored and deflate.
    UnsupportedCompression,
    /// The data of two zip entries overlap.
    OverlappingEntries,
    /// The local header of a zip entry names another entry than the
    /// central directory.
    NameMismatch,
    /// The archive holds more entries than the limit.
    TooManyEntries,
    /// The files of the archive hold more bytes than the limit.
    TooLarge,
    /// A PAX extended header or GNU long name is longer than 64 KiB.
    HeaderTooLarge,
    /// The archive data cannot be decoded: a broken gzip, deflate, or tar
    /// stream, a checksum that does not match, or a malformed zip.
    Corrupt,
}

impl RejectReason {
    /// Every reason, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::AbsolutePath,
        Self::PathTraversal,
        Self::NameInvalid,
        Self::NameTooLong,
        Self::UnexpectedTopLevel,
        Self::ReservedName,
        Self::SizeMismatch,
        Self::HardLink,
        Self::SpecialFile,
        Self::UnsupportedEntry,
        Self::SymlinkEscape,
        Self::SymlinkDangling,
        Self::SymlinkChain,
        Self::EntryThroughSymlink,
        Self::ExecutableNotRegular,
        Self::DuplicateEntry,
        Self::TrailingData,
        Self::PrefixData,
        Self::Encrypted,
        Self::UnsupportedCompression,
        Self::OverlappingEntries,
        Self::NameMismatch,
        Self::TooManyEntries,
        Self::TooLarge,
        Self::HeaderTooLarge,
        Self::Corrupt,
    ];

    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::AbsolutePath => "absolute_path",
            Self::PathTraversal => "path_traversal",
            Self::NameInvalid => "name_invalid",
            Self::NameTooLong => "name_too_long",
            Self::UnexpectedTopLevel => "unexpected_top_level",
            Self::ReservedName => "reserved_name",
            Self::SizeMismatch => "size_mismatch",
            Self::HardLink => "hard_link",
            Self::SpecialFile => "special_file",
            Self::UnsupportedEntry => "unsupported_entry",
            Self::SymlinkEscape => "symlink_escape",
            Self::SymlinkDangling => "symlink_dangling",
            Self::SymlinkChain => "symlink_chain",
            Self::EntryThroughSymlink => "entry_through_symlink",
            Self::ExecutableNotRegular => "executable_not_regular",
            Self::DuplicateEntry => "duplicate_entry",
            Self::TrailingData => "trailing_data",
            Self::PrefixData => "prefix_data",
            Self::Encrypted => "encrypted",
            Self::UnsupportedCompression => "unsupported_compression",
            Self::OverlappingEntries => "overlapping_entries",
            Self::NameMismatch => "name_mismatch",
            Self::TooManyEntries => "too_many_entries",
            Self::TooLarge => "too_large",
            Self::HeaderTooLarge => "header_too_large",
            Self::Corrupt => "corrupt",
        }
    }
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Why an extraction stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ArchiveError {
    /// A member broke a rule; `entry` is its name as the archive gives it,
    /// cut to [`limits::ENTRY_TEXT_MAX_CHARS`] characters, or empty when
    /// the archive as a whole is refused.
    #[error("archive rejected ({reason}) at {entry:?}")]
    Rejected {
        /// The rule that was broken.
        reason: RejectReason,
        /// The member that broke it.
        entry: String,
    },
    /// The stop flag was raised.
    #[error("the extraction was stopped")]
    Stopped,
    /// The [`ExtractSpec`] does not fit the format, for example a `binary`
    /// artifact without an executable.
    #[error("invalid extract spec: {reason}")]
    InvalidSpec {
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The tree writer failed, for example with
    /// [`PlatformError::NoSpace`].
    #[error("writing the runtime tree failed: {0}")]
    Write(PlatformError),
    /// Reading the artifact file itself failed, not decoding its contents.
    #[error("reading the artifact failed: {message}")]
    Read {
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// The OS error message.
        message: String,
    },
}

/// Extracts the artifact in `input`, read from its start, into `sink`.
///
/// `sink` writes into an empty tree; on error the entries written so far
/// stay for the caller to remove with the tree. `stop` is checked before
/// each member and each 64 KiB chunk.
///
/// # Errors
///
/// Returns [`ArchiveError::Rejected`] for a member or archive that breaks
/// a rule, [`ArchiveError::Stopped`] once `stop` is raised,
/// [`ArchiveError::InvalidSpec`] for a spec that does not fit the format,
/// [`ArchiveError::Write`] when `sink` fails otherwise, and
/// [`ArchiveError::Read`] when `input` cannot be read.
pub fn extract(
    format: ArtifactFormat,
    input: &mut dyn ReadSeekFile,
    spec: &ExtractSpec,
    sink: &mut dyn TreeWriter,
    stop: &StopFlag,
) -> Result<ExtractReport, ArchiveError> {
    let mut run = Extraction::new(format, spec, sink, stop)?;
    match format {
        ArtifactFormat::TarGz => tar_gz::read(input, &mut run)?,
        ArtifactFormat::Zip => zip_archive::read(input, &mut run)?,
        ArtifactFormat::Binary => binary::read(input, &mut run)?,
    }
    run.finish()
}

/// Runs [`extract`] into a tree in memory that keeps no file contents and
/// treats names that differ only in letter case as one entry, as the
/// default APFS volume does. `cargo xtask runtime pin` takes
/// `unpacked_size` and `entry_count` from the report.
///
/// # Errors
///
/// Returns the errors of [`extract`], never [`ArchiveError::Stopped`].
pub fn scan(
    format: ArtifactFormat,
    input: &mut dyn ReadSeekFile,
    spec: &ExtractSpec,
) -> Result<ExtractReport, ArchiveError> {
    extract(
        format,
        input,
        spec,
        &mut MemoryTree::default(),
        &StopFlag::new(),
    )
}

#[cfg(test)]
mod tests;
