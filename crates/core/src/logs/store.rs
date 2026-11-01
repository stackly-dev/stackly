//! One instance's durable log store, `<logs>/instances/<id>/captured/`
//! (spec 0027, *Model data*): the header `store.json` with the `storeId`,
//! and up to [`LOG_SEGMENTS_PER_STREAM`](super::LOG_SEGMENTS_PER_STREAM)
//! segments per stream.
//!
//! [`OpenStore`] is the writer's handle: opening it recovers the store once
//! per daemon run (AC-13), and appending rotates at a record boundary
//! (AC-2). Only the writer thread holds one.

use std::fmt;
use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use stackly_platform::{FileKind, Filesystem, OutputStream, PlatformError};
use uuid::Uuid;

use super::DurableLogConfig;
use super::record::{Record, RecordKind, RecordSource};
use super::scan::{Backward, Segment, list_segments, segment_file_name, slot};

/// The header file of a store.
pub(crate) const HEADER_FILE: &str = "store.json";

/// The largest header the reader accepts.
const HEADER_MAX: u64 = 4096;

/// The format version this code writes.
const FORMAT_VERSION: u32 = 1;

/// The identity of one store. A new store, or one whose header was lost,
/// gets a new one, so a cursor of the old store is recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LogStoreId(Uuid);

impl LogStoreId {
    /// Returns a new UUID v7.
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Wraps a UUID read from a cursor.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for LogStoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A position in one store: the record with this `seq`, which may no
/// longer exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LogCursor {
    /// The store the `seq` belongs to.
    pub store: LogStoreId,
    /// The `seq`.
    pub seq: u64,
}

/// Why a store could not be opened or written; also the `reason` of the
/// `instance_logs` health and of `instance.logs_unreadable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StoreFailure {
    /// The volume is full or over its quota.
    NoSpace,
    /// The folder or a file may not be written or read.
    PermissionDenied,
    /// `captured/` or a part of its path is not a folder.
    NotADirectory,
    /// Any other IO failure.
    Io,
}

impl StoreFailure {
    /// Returns the `reason` code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NoSpace => "no_space",
            Self::PermissionDenied => "permission_denied",
            Self::NotADirectory => "not_a_directory",
            Self::Io => "io",
        }
    }

    /// Maps a platform error.
    #[must_use]
    pub fn of(error: &PlatformError) -> Self {
        match error {
            PlatformError::NoSpace { .. } => Self::NoSpace,
            PlatformError::PermissionDenied { .. } => Self::PermissionDenied,
            PlatformError::NotADirectory { .. } => Self::NotADirectory,
            PlatformError::Io { kind, .. } => Self::of_kind(*kind),
            _ => Self::Io,
        }
    }

    /// Maps an IO error kind.
    #[must_use]
    pub fn of_kind(kind: io::ErrorKind) -> Self {
        match kind {
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Self::NoSpace,
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            io::ErrorKind::NotADirectory => Self::NotADirectory,
            _ => Self::Io,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    store_id: Uuid,
    format_version: u32,
    created_at: Timestamp,
}

/// Reads the `storeId` of the header in `dir`. A header that is missing or
/// cannot be parsed reads as `None`.
///
/// # Errors
///
/// Returns the platform error when the file exists but cannot be read.
pub(crate) fn read_store_id(
    fs: &dyn Filesystem,
    dir: &Path,
) -> Result<Option<LogStoreId>, PlatformError> {
    match fs.read_file_limited(&dir.join(HEADER_FILE), HEADER_MAX) {
        Ok(bytes) => Ok(serde_json::from_slice::<Header>(&bytes)
            .ok()
            .map(|header| LogStoreId(header.store_id))),
        Err(PlatformError::NotFound { .. } | PlatformError::FileTooLarge { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// What the store folder of an instance is.
pub(crate) enum Folder {
    /// It does not exist: an empty store.
    Missing,
    /// It is a folder.
    Present,
}

/// Checks the store folder without creating it.
///
/// # Errors
///
/// Returns [`PlatformError::NotADirectory`] when something else is there,
/// and other errors when it cannot be read.
pub(crate) fn check_folder(fs: &dyn Filesystem, dir: &Path) -> Result<Folder, PlatformError> {
    match fs.symlink_metadata(dir)? {
        None => Ok(Folder::Missing),
        Some(info) if info.kind == FileKind::Dir => Ok(Folder::Present),
        Some(_) => Err(PlatformError::NotADirectory {
            path: dir.to_path_buf(),
        }),
    }
}

/// The writing side of one stream.
struct StreamFiles {
    stream: OutputStream,
    segments: Vec<Segment>,
    active: Option<File>,
    /// The `src` of the last line stored from a spool.
    last_src: Option<RecordSource>,
}

/// One instance's store, open for writing.
pub(crate) struct OpenStore {
    dir: PathBuf,
    id: LogStoreId,
    next_seq: u64,
    head: Option<u64>,
    streams: [StreamFiles; 2],
    config: DurableLogConfig,
}

impl OpenStore {
    /// Opens the store in `dir`, creating the folder and the header when
    /// they are missing, and recovers it: the cut last line of each
    /// stream's newest segment is trimmed, the next `seq` follows the last
    /// whole record, and the last `src` of each stream is read back.
    ///
    /// # Errors
    ///
    /// Returns the platform error of the step that failed.
    pub(crate) fn open(
        fs: &dyn Filesystem,
        now: Timestamp,
        dir: &Path,
        config: DurableLogConfig,
    ) -> Result<Self, PlatformError> {
        if let Folder::Missing = check_folder(fs, dir)? {
            fs.create_dir_private(dir)?;
        }
        let listed = list_segments(fs, dir)?;
        let id = match read_store_id(fs, dir)? {
            Some(id) => id,
            None => {
                let id = LogStoreId::generate();
                let header = Header {
                    store_id: id.0,
                    format_version: FORMAT_VERSION,
                    created_at: now,
                };
                let bytes = serde_json::to_vec(&header)
                    .expect("a header of a UUID, a number, and a time always serializes");
                fs.write_file_atomic(&dir.join(HEADER_FILE), &bytes)?;
                id
            }
        };
        let [stdout, stderr] = listed;
        let mut store = Self {
            dir: dir.to_path_buf(),
            id,
            next_seq: 1,
            head: None,
            streams: [
                StreamFiles::new(OutputStream::Stdout, stdout),
                StreamFiles::new(OutputStream::Stderr, stderr),
            ],
            config,
        };
        for index in 0..2 {
            store.recover(fs, index)?;
        }
        Ok(store)
    }

    fn recover(&mut self, fs: &dyn Filesystem, index: usize) -> Result<(), PlatformError> {
        let files = &mut self.streams[index];
        let Some(newest) = files.segments.last_mut() else {
            return Ok(());
        };
        let mut file = fs.open_append_private(&newest.path)?;
        let io = |error: io::Error| PlatformError::from_io("recover", &newest.path, &error);
        let len = file.seek(SeekFrom::End(0)).map_err(io)?;
        let whole = whole_len(&mut file, len).map_err(io)?;
        if whole < len {
            file.set_len(whole).map_err(io)?;
        }
        newest.len = whole;
        self.next_seq = self.next_seq.max(newest.first_seq);
        let (last, src) = last_records(&mut file, whole).map_err(io)?;
        files.active = Some(file);
        let mut last_seq = last;
        let mut last_src = src;
        if (last_seq.is_none() || last_src.is_none()) && files.segments.len() > 1 {
            let previous = &files.segments[files.segments.len() - 2];
            match fs.open_read(&previous.path) {
                Ok(mut file) => {
                    let len = file.len();
                    let (seq, src) = last_records(&mut file, len).map_err(|error| {
                        PlatformError::from_io("recover", &previous.path, &error)
                    })?;
                    last_seq = last_seq.or(seq);
                    last_src = last_src.or(src);
                }
                Err(PlatformError::NotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        files.last_src = last_src;
        if let Some(seq) = last_seq {
            self.next_seq = self.next_seq.max(seq + 1);
            self.head = Some(self.head.map_or(seq, |head| head.max(seq)));
        }
        Ok(())
    }

    /// Returns the store's identity.
    pub(crate) const fn id(&self) -> LogStoreId {
        self.id
    }

    /// Returns the `seq` of the newest record written, if any.
    pub(crate) const fn head(&self) -> Option<u64> {
        self.head
    }

    /// Takes the next `seq`.
    pub(crate) const fn take_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    /// Returns the `src` of the last line stored from a spool for `stream`.
    pub(crate) const fn last_src(&self, stream: OutputStream) -> Option<RecordSource> {
        self.streams[slot(stream)].last_src
    }

    /// Remembers the `src` of a line about to be stored.
    pub(crate) const fn set_last_src(&mut self, stream: OutputStream, src: RecordSource) {
        self.streams[slot(stream)].last_src = Some(src);
    }

    /// Appends `records` to the segments of `stream`, oldest first,
    /// rotating at record boundaries. Returns how many were written; on an
    /// error the bytes of the failed write are cut off again (best effort)
    /// and the count says how many records before it are stored.
    pub(crate) fn append(
        &mut self,
        fs: &dyn Filesystem,
        stream: OutputStream,
        records: &[Record],
    ) -> (usize, Result<(), PlatformError>) {
        let mut written = 0;
        let mut buf = Vec::new();
        let mut pending = 0;
        for record in records {
            let needs_new = {
                let files = &self.streams[slot(stream)];
                files.active.is_none()
                    || files
                        .segments
                        .last()
                        .is_none_or(|segment| segment.len >= self.config.segment_max_bytes)
            };
            if needs_new {
                if let Err(error) = self.flush(stream, &mut buf) {
                    return (written, Err(error));
                }
                written += pending;
                pending = 0;
                if let Err(error) = self.rotate(fs, stream, record.seq) {
                    return (written, Err(error));
                }
            }
            let bytes = record.encode();
            if let Some(segment) = self.streams[slot(stream)].segments.last_mut() {
                // Counted ahead so the rotation check sees the bytes not
                // flushed yet.
                segment.len += bytes.len() as u64;
            }
            buf.extend_from_slice(&bytes);
            pending += 1;
        }
        if let Err(error) = self.flush(stream, &mut buf) {
            return (written, Err(error));
        }
        written += pending;
        if let Some(last) = records.last() {
            self.head = Some(self.head.map_or(last.seq, |head| head.max(last.seq)));
        }
        (written, Ok(()))
    }

    /// Writes `buf` to the active segment of `stream` in one `write_all`.
    fn flush(&mut self, stream: OutputStream, buf: &mut Vec<u8>) -> Result<(), PlatformError> {
        if buf.is_empty() {
            return Ok(());
        }
        let files = &mut self.streams[slot(stream)];
        let segment = files
            .segments
            .last_mut()
            .expect("an active segment exists once a record was counted into it");
        let file = files
            .active
            .as_mut()
            .expect("an active segment always has its file open");
        let before = segment.len - buf.len() as u64;
        let result = file.write_all(buf);
        buf.clear();
        result.map_err(|error| {
            segment.len = before;
            // The failed write may have left part of a record behind.
            let _ = file.set_len(before);
            PlatformError::from_io("append", &segment.path, &error)
        })
    }

    /// Closes the active segment of `stream` (synced), opens a new one
    /// named after `first_seq`, and deletes the oldest past the limit.
    fn rotate(
        &mut self,
        fs: &dyn Filesystem,
        stream: OutputStream,
        first_seq: u64,
    ) -> Result<(), PlatformError> {
        let dir = self.dir.clone();
        let limit = self.config.segments_per_stream.max(1);
        let files = &mut self.streams[slot(stream)];
        if let Some(file) = files.active.take()
            && let Err(error) = file.sync_all()
        {
            tracing::warn!(stream = stream.as_code(), %error, "a log segment could not be synced");
        }
        let name = segment_file_name(stream, first_seq);
        let path = dir.join(&name);
        // A segment of this name is left only by a crash right after it was
        // created: it holds nothing newer, so writing goes on in it.
        let file = match fs.create_file_private(&path) {
            Ok(file) => file,
            Err(PlatformError::AlreadyExists { .. }) => fs.open_append_private(&path)?,
            Err(error) => return Err(error),
        };
        let len = file
            .metadata()
            .map_err(|error| PlatformError::from_io("rotate", &path, &error))?
            .len();
        if files.segments.last().is_none_or(|last| last.path != path) {
            files.segments.push(Segment {
                first_seq,
                path,
                len,
            });
        }
        files.active = Some(file);
        while files.segments.len() > limit {
            let oldest = files.segments.remove(0);
            let name = oldest
                .path
                .file_name()
                .map(PathBuf::from)
                .expect("a segment path always ends in its file name");
            if let Err(error) = fs.remove_tree(&dir, &name) {
                tracing::warn!(stream = stream.as_code(), %error, "a rotated log segment could not be deleted");
            }
        }
        Ok(())
    }

    /// Syncs and closes every open segment.
    pub(crate) fn close(&mut self) {
        for files in &mut self.streams {
            if let Some(file) = files.active.take()
                && let Err(error) = file.sync_all()
            {
                tracing::warn!(stream = files.stream.as_code(), %error, "a log segment could not be synced");
            }
        }
    }
}

impl StreamFiles {
    const fn new(stream: OutputStream, segments: Vec<Segment>) -> Self {
        Self {
            stream,
            segments,
            active: None,
            last_src: None,
        }
    }
}

/// Returns the length of the file up to and with its last `\n`.
fn whole_len(file: &mut File, len: u64) -> io::Result<u64> {
    let mut backward = Backward::new(&mut *file, len);
    Ok(backward.next_line()?.map_or(0, |line| line.end()))
}

/// Returns the `seq` of the last whole record of the file and the `src` of
/// its last line from a spool, reading backward until both are found.
fn last_records<R: io::Read + io::Seek>(
    file: &mut R,
    len: u64,
) -> io::Result<(Option<u64>, Option<RecordSource>)> {
    let mut backward = Backward::new(file, len);
    let mut seq = None;
    while let Some(line) = backward.next_line()? {
        let Some(record) = Record::decode(&line.bytes) else {
            continue;
        };
        seq = seq.or(Some(record.seq));
        if let RecordKind::Line(line) = &record.kind
            && let Some(src) = line.src
        {
            return Ok((seq, Some(src)));
        }
    }
    Ok((seq, None))
}
