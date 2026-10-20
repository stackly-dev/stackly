//! `zip`: entries from the central directory through `zip::ZipArchive`
//! (spec 0018, *Format*).
//!
//! Before the crate parses anything, the end of central directory record
//! is located here and must end the file, and its entry count is held to
//! the entry limit. The crate then parses the central directory and hands
//! out the raw data of each entry, which is inflated and CRC checked here,
//! so the real size is counted even when the headers lie about it. The
//! local header of each entry is read here through a second handle to the
//! same file, since the crate keeps one entry per name and reports a name
//! that differs from its local header only as free text.

use std::cell::RefCell;
use std::io::{self, Read, Seek, SeekFrom};
use std::rc::Rc;

use flate2::Crc;
use flate2::read::DeflateDecoder;
use stackly_platform::ReadSeekFile;
use zip::result::ZipError;
use zip::{CompressionMethod, System, ZipArchive};

use super::engine::{ArchiveMode, Extraction, Member};
use super::limits::SYMLINK_TARGET_MAX_LEN;
use super::source::Tracked;
use super::{ArchiveError, RejectReason};

const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const EOCD_LEN: usize = 22;
const ZIP64_LOCATOR_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
const ZIP64_LOCATOR_LEN: u64 = 20;
const ZIP64_EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
const ZIP64_EOCD_LEN: usize = 56;
const LOCAL_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const LOCAL_HEADER_LEN: usize = 30;
const MAX_COMMENT_LEN: usize = 0xffff;
const FILE_TYPE_MASK: u32 = 0o170_000;
const TYPE_FILE: u32 = 0o100_000;
const TYPE_DIR: u32 = 0o040_000;
const TYPE_SYMLINK: u32 = 0o120_000;

/// The artifact file, shared by the zip crate and the local header checks.
#[derive(Clone)]
struct Shared<'a>(Rc<RefCell<Tracked<'a>>>);

impl Read for Shared<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.borrow_mut().read(buf)
    }
}

impl Seek for Shared<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.borrow_mut().seek(pos)
    }
}

impl Shared<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.seek(SeekFrom::Start(offset))?;
        self.read_exact(buf)
    }
}

/// What one central directory entry says, copied out of the crate.
struct EntryInfo {
    name: Vec<u8>,
    header_start: u64,
    compression: CompressionMethod,
    encrypted: bool,
    size: u64,
    crc: u32,
    unix: bool,
    mode: u32,
}

/// What a zip entry turns out to be.
enum Kind {
    Dir,
    File,
    Symlink,
    Special,
}

/// Reads a `zip` artifact into `run`.
pub(super) fn read(
    input: &mut dyn ReadSeekFile,
    run: &mut Extraction<'_>,
) -> Result<(), ArchiveError> {
    let mut file = Shared(Rc::new(RefCell::new(Tracked::new(
        input,
        run.source_error(),
    ))));
    let len = file.0.borrow().len();
    let declared = read_end_record(&mut file, len, run)?;
    if declared > run.spec().limits.max_entries {
        return Err(Extraction::reject(RejectReason::TooManyEntries, b""));
    }
    let mut archive = ZipArchive::new(file.clone()).map_err(|err| zip_error(&err, b"", run))?;
    if archive.offset() != 0 {
        return Err(Extraction::reject(RejectReason::PrefixData, b""));
    }
    if archive.len() as u64 != declared {
        // The crate keeps one entry per name.
        return Err(Extraction::reject(RejectReason::DuplicateEntry, b""));
    }
    if archive
        .has_overlapping_entries()
        .map_err(|err| zip_error(&err, b"", run))?
    {
        return Err(Extraction::reject(RejectReason::OverlappingEntries, b""));
    }
    let entries = (0..archive.len())
        .map(|index| entry_info(&archive, index, run))
        .collect::<Result<Vec<_>, _>>()?;
    let first = entries
        .iter()
        .map(|entry| entry.header_start)
        .min()
        .unwrap_or_else(|| archive.central_directory_start());
    if first != 0 {
        return Err(Extraction::reject(RejectReason::PrefixData, b""));
    }
    for (index, entry) in entries.iter().enumerate() {
        run.check_stop()?;
        read_entry(&mut archive, &mut file, index, entry, run)?;
    }
    Ok(())
}

/// Finds the end of central directory record, which must end the file
/// right after its comment, and returns the entry count it declares (from
/// the zip64 record when the classic one is saturated).
fn read_end_record(
    file: &mut Shared<'_>,
    len: u64,
    run: &Extraction<'_>,
) -> Result<u64, ArchiveError> {
    let tail_len = len.min((EOCD_LEN + MAX_COMMENT_LEN) as u64);
    let tail_start = len - tail_len;
    let mut tail = vec![0_u8; usize::try_from(tail_len).unwrap_or(0)];
    file.read_at(tail_start, &mut tail)
        .map_err(|_| run.read_error(b""))?;
    let mut seen_signature = false;
    let mut found = None;
    for start in (0..=tail.len().saturating_sub(EOCD_LEN)).rev() {
        if tail.len() < EOCD_LEN || tail[start..start + 4] != EOCD_SIGNATURE {
            continue;
        }
        seen_signature = true;
        let comment_len = usize::from(u16::from_le_bytes([tail[start + 20], tail[start + 21]]));
        if start + EOCD_LEN + comment_len == tail.len() {
            found = Some(start);
            break;
        }
    }
    let Some(start) = found else {
        let reason = if seen_signature {
            RejectReason::TrailingData
        } else {
            RejectReason::Corrupt
        };
        return Err(Extraction::reject(reason, b""));
    };
    let record = &tail[start..start + EOCD_LEN];
    let entries = u16::from_le_bytes([record[10], record[11]]);
    let offset = u32::from_le_bytes([record[16], record[17], record[18], record[19]]);
    if entries != u16::MAX && offset != u32::MAX {
        return Ok(u64::from(entries));
    }
    zip64_entries(file, tail_start + start as u64, run)
}

/// Reads the entry count of the zip64 end of central directory record
/// that the locator before the classic record points to.
fn zip64_entries(
    file: &mut Shared<'_>,
    eocd_at: u64,
    run: &Extraction<'_>,
) -> Result<u64, ArchiveError> {
    let corrupt = || Extraction::reject(RejectReason::Corrupt, b"");
    let locator_at = eocd_at.checked_sub(ZIP64_LOCATOR_LEN).ok_or_else(corrupt)?;
    let mut locator = [0_u8; 20];
    file.read_at(locator_at, &mut locator)
        .map_err(|_| run.read_error(b""))?;
    if locator[..4] != ZIP64_LOCATOR_SIGNATURE {
        return Err(corrupt());
    }
    let record_at = u64::from_le_bytes(locator[8..16].try_into().map_err(|_| corrupt())?);
    let mut record = [0_u8; ZIP64_EOCD_LEN];
    file.read_at(record_at, &mut record)
        .map_err(|_| run.read_error(b""))?;
    if record[..4] != ZIP64_EOCD_SIGNATURE {
        return Err(corrupt());
    }
    Ok(u64::from_le_bytes(
        record[32..40].try_into().map_err(|_| corrupt())?,
    ))
}

fn entry_info(
    archive: &ZipArchive<Shared<'_>>,
    index: usize,
    run: &Extraction<'_>,
) -> Result<EntryInfo, ArchiveError> {
    let entry = archive
        .by_index_data(index)
        .map_err(|err| zip_error(&err, b"", run))?;
    Ok(EntryInfo {
        name: entry.name_raw().to_vec(),
        header_start: entry.header_start(),
        compression: entry.compression(),
        encrypted: entry.encrypted(),
        size: entry.size(),
        crc: entry.crc32(),
        unix: entry.system() == System::Unix,
        mode: entry.external_attributes() >> 16,
    })
}

fn read_entry(
    archive: &mut ZipArchive<Shared<'_>>,
    file: &mut Shared<'_>,
    index: usize,
    entry: &EntryInfo,
    run: &mut Extraction<'_>,
) -> Result<(), ArchiveError> {
    let name = entry.name.as_slice();
    let reject = |reason| Extraction::reject(reason, name);
    if entry.encrypted {
        return Err(reject(RejectReason::Encrypted));
    }
    if !matches!(
        entry.compression,
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(reject(RejectReason::UnsupportedCompression));
    }
    if local_name(file, entry.header_start, run, name)? != entry.name {
        return Err(reject(RejectReason::NameMismatch));
    }
    let mode = if entry.unix {
        ArchiveMode::Known(entry.mode)
    } else {
        ArchiveMode::Unknown
    };
    match kind(entry).ok_or_else(|| reject(RejectReason::NameInvalid))? {
        Kind::Special => Err(reject(RejectReason::SpecialFile)),
        Kind::Dir if entry.size != 0 => Err(reject(RejectReason::SizeMismatch)),
        Kind::Dir => run.add(name, mode, Member::Dir),
        Kind::Symlink => {
            if entry.size > SYMLINK_TARGET_MAX_LEN as u64 {
                return Err(reject(RejectReason::NameTooLong));
            }
            let raw = archive
                .by_index_raw(index)
                .map_err(|err| zip_error(&err, name, run))?;
            let mut target = Vec::new();
            Checked::new(raw, entry)
                .take(SYMLINK_TARGET_MAX_LEN as u64 + 1)
                .read_to_end(&mut target)
                .map_err(|_| run.read_error(name))?;
            if target.len() as u64 != entry.size {
                return Err(reject(RejectReason::SizeMismatch));
            }
            run.add(name, mode, Member::Symlink { target })
        }
        Kind::File => {
            let raw = archive
                .by_index_raw(index)
                .map_err(|err| zip_error(&err, name, run))?;
            run.add(
                name,
                mode,
                Member::File {
                    size: entry.size,
                    data: &mut Checked::new(raw, entry),
                },
            )
        }
    }
}

/// The contents of one entry: its raw data, inflated for deflate, with
/// the CRC of everything read checked at the end.
struct Checked<R> {
    data: Decoded<R>,
    crc: Crc,
    expected: u32,
}

enum Decoded<R> {
    Stored(R),
    Deflated(DeflateDecoder<R>),
}

impl<R: Read> Checked<R> {
    fn new(raw: R, entry: &EntryInfo) -> Self {
        let data = if entry.compression == CompressionMethod::Deflated {
            Decoded::Deflated(DeflateDecoder::new(raw))
        } else {
            Decoded::Stored(raw)
        };
        Self {
            data,
            crc: Crc::new(),
            expected: entry.crc,
        }
    }
}

impl<R: Read> Read for Checked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = match &mut self.data {
            Decoded::Stored(raw) => raw.read(buf)?,
            Decoded::Deflated(decoder) => decoder.read(buf)?,
        };
        if read == 0 && !buf.is_empty() && self.crc.sum() != self.expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the entry's CRC does not match the central directory",
            ));
        }
        self.crc.update(&buf[..read]);
        Ok(read)
    }
}

/// Classifies an entry: the file type bits count only in a zip made on
/// Unix, and a name ending in `/` is a folder. `None` when the two
/// disagree.
fn kind(entry: &EntryInfo) -> Option<Kind> {
    let slash = entry.name.ends_with(b"/");
    if !entry.unix {
        return Some(if slash { Kind::Dir } else { Kind::File });
    }
    match entry.mode & FILE_TYPE_MASK {
        TYPE_DIR => Some(Kind::Dir),
        0 if slash => Some(Kind::Dir),
        _ if slash => None,
        0 | TYPE_FILE => Some(Kind::File),
        TYPE_SYMLINK => Some(Kind::Symlink),
        _ => Some(Kind::Special),
    }
}

/// Reads the file name of the local header at `offset`.
fn local_name(
    file: &mut Shared<'_>,
    offset: u64,
    run: &Extraction<'_>,
    name: &[u8],
) -> Result<Vec<u8>, ArchiveError> {
    let mut header = [0_u8; LOCAL_HEADER_LEN];
    file.read_at(offset, &mut header)
        .map_err(|_| run.read_error(name))?;
    if header[..4] != LOCAL_SIGNATURE {
        return Err(Extraction::reject(RejectReason::Corrupt, name));
    }
    let len = usize::from(u16::from_le_bytes([header[26], header[27]]));
    let mut local = vec![0_u8; len];
    file.read_exact(&mut local)
        .map_err(|_| run.read_error(name))?;
    Ok(local)
}

/// Maps a zip crate error: an I/O error goes through the file's own error
/// slot, anything else is a broken archive.
fn zip_error(err: &ZipError, name: &[u8], run: &Extraction<'_>) -> ArchiveError {
    match err {
        ZipError::Io(_) => run.read_error(name),
        ZipError::CompressionMethodNotSupported(_) => {
            Extraction::reject(RejectReason::UnsupportedCompression, name)
        }
        ZipError::InvalidPassword => Extraction::reject(RejectReason::Encrypted, name),
        _ => Extraction::reject(RejectReason::Corrupt, name),
    }
}
