//! Log spools (spec 0010, *Spool log* and *Adapter macOS*, row Spool).
//!
//! A main process appends to its spools, which outlive the daemon. The
//! reader opens a spool `O_RDWR` without `O_CREAT`, `O_TRUNC`, or
//! `O_APPEND`, because macOS 27 refuses `F_PUNCHHOLE` on an `O_RDONLY`
//! descriptor, and never writes through it. It frees only whole punch
//! units of one VM page (or one filesystem block, when that is larger):
//! a punched block inside a page the child still writes would come back
//! as zeros and leave `SEEK_DATA` stale.

use std::fs::{File, OpenOptions};
use std::io;
use std::mem::{self, MaybeUninit};
use std::num::NonZeroU64;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use stackly_platform::{FileIdentity, ProcessError, SpoolReader, SpoolWatermark, SpoolWriter};

use super::lock::identity;
use crate::identity::current_uid;

/// Creates a new spool: append only, mode 0600, never through a symlink,
/// failing when the path exists.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when the file exists or cannot be created.
pub(super) fn create_spool(path: &Path) -> Result<SpoolWriter, ProcessError> {
    let io_error = |error: io::Error| ProcessError::io("create_spool", Some(path), &error);
    let file = OpenOptions::new()
        .append(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    Ok(SpoolWriter::new(
        file,
        path.to_path_buf(),
        identity(&metadata),
    ))
}

/// Opens an existing spool to read and free it, or `None` when there is
/// none.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when the spool cannot be opened, is not a
/// regular file of the current user, or the punch unit cannot be
/// measured.
pub(super) fn open_spool(path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError> {
    let io_error = |op, error: io::Error| ProcessError::io(op, Some(path), &error);
    // O_NONBLOCK keeps a FIFO put in the spool's place from blocking the
    // open; the check below refuses it.
    let opened = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path);
    let file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            return Err(not_a_spool(path));
        }
        Err(error) => return Err(io_error("open_spool", error)),
    };
    let metadata = file.metadata().map_err(|error| io_error("fstat", error))?;
    if !metadata.is_file() || metadata.uid() != current_uid() {
        return Err(not_a_spool(path));
    }
    let unit = punch_unit(&file, path)?;
    let mut reader = MacosSpoolReader {
        file,
        path: path.to_path_buf(),
        identity: identity(&metadata),
        watermark: SpoolWatermark::new(unit, None, 0),
    };
    let first = reader.first_data_offset()?;
    reader.watermark = SpoolWatermark::new(unit, first, metadata.len());
    Ok(Some(Box::new(reader)))
}

fn not_a_spool(path: &Path) -> ProcessError {
    ProcessError::Io {
        op: "open_spool",
        path: Some(path.to_path_buf()),
        kind: io::ErrorKind::InvalidInput,
        message: "the spool is not a regular file of the current user".to_owned(),
    }
}

/// `max(sysconf(_SC_PAGESIZE), f_bsize)`, which must be a whole number of
/// filesystem blocks.
fn punch_unit(file: &File, path: &Path) -> Result<NonZeroU64, ProcessError> {
    let invalid = |message: String| ProcessError::Io {
        op: "punch unit",
        path: Some(path.to_path_buf()),
        kind: io::ErrorKind::Unsupported,
        message,
    };
    // SAFETY: sysconf takes an integer and touches no memory of ours.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page = u64::try_from(page)
        .ok()
        .filter(|page| *page > 0)
        .ok_or_else(|| invalid("sysconf(_SC_PAGESIZE) failed".to_owned()))?;
    let mut stats = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: fstatfs fills the statfs it gets for an open descriptor.
    if unsafe { libc::fstatfs(file.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
        return Err(ProcessError::io(
            "fstatfs",
            Some(path),
            &io::Error::last_os_error(),
        ));
    }
    // SAFETY: fstatfs succeeded, so it filled the struct.
    let block = u64::from(unsafe { stats.assume_init() }.f_bsize);
    let unit = page.max(block);
    if block == 0 || unit % block != 0 {
        return Err(invalid(format!(
            "a page of {page} bytes is not a whole number of {block} byte blocks"
        )));
    }
    NonZeroU64::new(unit).ok_or_else(|| invalid("the punch unit is 0".to_owned()))
}

/// The reader of one spool.
#[derive(Debug)]
struct MacosSpoolReader {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
    watermark: SpoolWatermark,
}

impl MacosSpoolReader {
    fn io_error(&self, op: &'static str, error: &io::Error) -> ProcessError {
        ProcessError::io(op, Some(&self.path), error)
    }
}

impl SpoolReader for MacosSpoolReader {
    fn len(&self) -> Result<u64, ProcessError> {
        self.file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|error| self.io_error("fstat", &error))
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, ProcessError> {
        self.file
            .read_at(buf, offset)
            .map_err(|error| self.io_error("pread", &error))
    }

    fn first_data_offset(&self) -> Result<Option<u64>, ProcessError> {
        // SAFETY: lseek takes integers and touches no memory of ours; the
        // reader only uses pread, so moving the offset changes nothing.
        let offset = unsafe { libc::lseek(self.file.as_raw_fd(), 0, libc::SEEK_DATA) };
        if offset >= 0 {
            return Ok(u64::try_from(offset).ok());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENXIO) {
            Ok(None)
        } else {
            Err(self.io_error("lseek SEEK_DATA", &error))
        }
    }

    fn punch_unit(&self) -> NonZeroU64 {
        self.watermark.unit()
    }

    fn release_below(&mut self, offset: u64) -> Result<(), ProcessError> {
        let len = self.len()?;
        let Some(range) = self.watermark.next_release(offset, len) else {
            return Ok(());
        };
        let out_of_range = || ProcessError::Io {
            op: "F_PUNCHHOLE",
            path: Some(self.path.clone()),
            kind: io::ErrorKind::InvalidInput,
            message: "the range does not fit an off_t".to_owned(),
        };
        let start = libc::off_t::try_from(range.start).map_err(|_| out_of_range())?;
        let length = libc::off_t::try_from(range.end - range.start).map_err(|_| out_of_range())?;
        // SAFETY: fpunchhole_t is plain integers, for which all zero bytes
        // are a valid value.
        let mut hole: libc::fpunchhole_t = unsafe { mem::zeroed() };
        hole.fp_offset = start;
        hole.fp_length = length;
        // SAFETY: F_PUNCHHOLE reads the fpunchhole_t it gets during the
        // call; the descriptor is open for writing.
        let result =
            unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_PUNCHHOLE, &raw const hole) };
        if result == -1 {
            return Err(self.io_error("F_PUNCHHOLE", &io::Error::last_os_error()));
        }
        self.watermark.advance(range.end);
        Ok(())
    }

    fn file_identity(&self) -> FileIdentity {
        self.identity
    }
}
