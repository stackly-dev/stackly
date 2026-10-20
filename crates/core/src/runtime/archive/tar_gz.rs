//! `tar_gz`: exactly one gzip member holding a tar stream (spec 0018,
//! *Format*).
//!
//! The tar crate iterates raw headers, so the PAX and GNU extension
//! headers come here first and are held to
//! [`EXTENSION_HEADER_MAX_LEN`] before anything reads them. Only `path`,
//! `linkpath`, and `size` of a PAX header are used; extended attributes
//! and every other key are ignored.

use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};

use flate2::bufread::GzDecoder;
use stackly_platform::ReadSeekFile;
use tar::{Archive, Entry, EntryType, PaxExtensions};

use super::engine::{ArchiveMode, Extraction, Member};
use super::limits::{COPY_BUFFER_LEN, EXTENSION_HEADER_MAX_LEN, TAR_PADDING_MAX_LEN};
use super::source::Tracked;
use super::{ArchiveError, RejectReason};

/// What the extension headers before a member set for it.
#[derive(Debug, Default)]
struct Pending {
    path: Option<Vec<u8>>,
    link: Option<Vec<u8>>,
    size: Option<u64>,
    sparse: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.path.is_none() && self.link.is_none() && self.size.is_none() && !self.sparse
    }
}

/// Reads a `tar_gz` artifact into `run`.
pub(super) fn read(
    input: &mut dyn ReadSeekFile,
    run: &mut Extraction<'_>,
) -> Result<(), ArchiveError> {
    let mut file = Tracked::new(input, run.source_error());
    file.seek(SeekFrom::Start(0))
        .map_err(|_| run.read_error(b""))?;
    let mut archive = Archive::new(GzDecoder::new(BufReader::with_capacity(
        COPY_BUFFER_LEN,
        file,
    )));
    read_members(&mut archive, run)?;
    let mut gzip = archive.into_inner();
    check_padding(&mut gzip, run)?;
    // The gzip member is fully read and its trailer checked; nothing may
    // follow it, not even a second member.
    let mut rest = gzip.into_inner();
    match rest.fill_buf() {
        Ok([]) => Ok(()),
        Ok(_) => Err(Extraction::reject(RejectReason::TrailingData, b"")),
        Err(_) => Err(run.read_error(b"")),
    }
}

fn read_members<R: Read>(
    archive: &mut Archive<R>,
    run: &mut Extraction<'_>,
) -> Result<(), ArchiveError> {
    let entries = archive
        .entries()
        .map_err(|_| run.read_error(b""))?
        .raw(true);
    let mut pending = Pending::default();
    for entry in entries {
        let mut entry = entry.map_err(|_| run.read_error(b""))?;
        run.check_stop()?;
        let kind = entry.header().entry_type();
        let header_name = entry.header().path_bytes().into_owned();
        match kind {
            EntryType::XHeader => {
                let data = read_extension(&mut entry, &header_name, run)?;
                apply_pax(&data, &mut pending, &header_name)?;
                continue;
            }
            EntryType::XGlobalHeader => {
                read_extension(&mut entry, &header_name, run)?;
                continue;
            }
            EntryType::GNULongName | EntryType::GNULongLink => {
                let data = trim_nul(read_extension(&mut entry, &header_name, run)?);
                let slot = if kind == EntryType::GNULongName {
                    &mut pending.path
                } else {
                    &mut pending.link
                };
                if slot.replace(data).is_some() {
                    return Err(Extraction::reject(RejectReason::Corrupt, &header_name));
                }
                continue;
            }
            _ => {}
        }
        let current = std::mem::take(&mut pending);
        let name = current.path.unwrap_or(header_name);
        let size = entry.size();
        if current.sparse || current.size.is_some_and(|declared| declared != size) {
            return Err(Extraction::reject(RejectReason::UnsupportedEntry, &name));
        }
        let mode = entry
            .header()
            .mode()
            .map_err(|_| Extraction::reject(RejectReason::Corrupt, &name))?;
        let mode = ArchiveMode::Known(mode);
        match kind {
            EntryType::Regular | EntryType::Continuous => {
                run.add(
                    &name,
                    mode,
                    Member::File {
                        size,
                        data: &mut entry,
                    },
                )?;
            }
            EntryType::Directory | EntryType::Symlink if size != 0 => {
                return Err(Extraction::reject(RejectReason::SizeMismatch, &name));
            }
            EntryType::Directory => run.add(&name, mode, Member::Dir)?,
            EntryType::Symlink => {
                let target = current
                    .link
                    .or_else(|| {
                        entry
                            .header()
                            .link_name_bytes()
                            .map(|link| link.into_owned())
                    })
                    .unwrap_or_default();
                run.add(&name, mode, Member::Symlink { target })?;
            }
            EntryType::Link => {
                return Err(Extraction::reject(RejectReason::HardLink, &name));
            }
            EntryType::Char | EntryType::Block | EntryType::Fifo => {
                return Err(Extraction::reject(RejectReason::SpecialFile, &name));
            }
            _ => return Err(Extraction::reject(RejectReason::UnsupportedEntry, &name)),
        }
    }
    if pending.is_empty() {
        Ok(())
    } else {
        Err(Extraction::reject(RejectReason::Corrupt, b""))
    }
}

/// Reads the data of an extension header, at most
/// [`EXTENSION_HEADER_MAX_LEN`] bytes.
fn read_extension<R: Read>(
    entry: &mut Entry<'_, R>,
    name: &[u8],
    run: &Extraction<'_>,
) -> Result<Vec<u8>, ArchiveError> {
    if entry.size() > EXTENSION_HEADER_MAX_LEN {
        return Err(Extraction::reject(RejectReason::HeaderTooLarge, name));
    }
    let mut data = Vec::new();
    entry
        .read_to_end(&mut data)
        .map_err(|_| run.read_error(name))?;
    Ok(data)
}

/// Takes `path`, `linkpath`, and `size` from a PAX header, and notes a
/// GNU sparse map; every other key is ignored.
fn apply_pax(data: &[u8], pending: &mut Pending, name: &[u8]) -> Result<(), ArchiveError> {
    let corrupt = || Extraction::reject(RejectReason::Corrupt, name);
    for extension in PaxExtensions::new(data) {
        let extension = extension.map_err(|_| corrupt())?;
        let key = extension.key_bytes();
        match key {
            b"path" => pending.path = Some(extension.value_bytes().to_vec()),
            b"linkpath" => pending.link = Some(extension.value_bytes().to_vec()),
            b"size" => {
                let size = extension
                    .value()
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(corrupt)?;
                pending.size = Some(size);
            }
            _ if key.starts_with(b"GNU.sparse.") => pending.sparse = true,
            _ => {}
        }
    }
    Ok(())
}

/// Drops the NUL bytes a GNU long name ends with.
fn trim_nul(mut data: Vec<u8>) -> Vec<u8> {
    while data.last() == Some(&0) {
        data.pop();
    }
    data
}

/// Reads the rest of the tar stream after its end marker: zero bytes only,
/// at most [`TAR_PADDING_MAX_LEN`] of them.
fn check_padding(stream: &mut impl Read, run: &Extraction<'_>) -> Result<(), ArchiveError> {
    let mut buffer = [0_u8; 8192];
    let mut total = 0_u64;
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(run.read_error(b"")),
        };
        total += read as u64;
        if total > TAR_PADDING_MAX_LEN || buffer[..read].iter().any(|byte| *byte != 0) {
            return Err(Extraction::reject(RejectReason::TrailingData, b""));
        }
        run.check_stop()?;
    }
}
