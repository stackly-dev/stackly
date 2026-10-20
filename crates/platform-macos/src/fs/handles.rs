use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use stackly_platform::{AppendFile, FileInfo, FileKind, PlatformError, ReadSeekFile};

/// Flags every open here adds: never follow a symlink in the last
/// component, and never block on a FIFO.
const OPEN_FLAGS: libc::c_int = libc::O_NOFOLLOW | libc::O_NONBLOCK;

/// [`Filesystem::open_read`](stackly_platform::Filesystem::open_read).
pub(super) fn open_read(path: &Path) -> Result<Box<dyn ReadSeekFile>, PlatformError> {
    let (file, len) = open_regular(OpenOptions::new().read(true), "open_read", path)?;
    Ok(Box::new(MacosReadFile { file, len }))
}

/// [`Filesystem::open_append`](stackly_platform::Filesystem::open_append).
pub(super) fn open_append(path: &Path) -> Result<Box<dyn AppendFile>, PlatformError> {
    let (file, len) = open_regular(
        OpenOptions::new().append(true).create(true).mode(0o600),
        "open_append",
        path,
    )?;
    Ok(Box::new(MacosAppendFile {
        file,
        len,
        path: path.to_path_buf(),
    }))
}

/// Opens `path` with `options` and checks through `fstat` that what opened
/// is a regular file, returning it with its size.
fn open_regular(
    options: &mut OpenOptions,
    op: &'static str,
    path: &Path,
) -> Result<(File, u64), PlatformError> {
    let not_regular = || PlatformError::NotARegularFile {
        path: path.to_path_buf(),
    };
    let file = match options.custom_flags(OPEN_FLAGS).open(path) {
        Ok(file) => file,
        // O_NOFOLLOW on a symlink fails with ELOOP, a directory opened for
        // writing with EISDIR, and a FIFO without a reader opened for
        // writing without blocking with ENXIO.
        Err(err)
            if matches!(
                err.raw_os_error(),
                Some(libc::ELOOP | libc::EISDIR | libc::ENXIO)
            ) =>
        {
            return Err(not_regular());
        }
        Err(err) => return Err(PlatformError::from_io(op, path, &err)),
    };
    let metadata = file
        .metadata()
        .map_err(|err| PlatformError::from_io(op, path, &err))?;
    if FileInfo::from_unix_metadata(&metadata).kind != FileKind::File {
        return Err(not_regular());
    }
    Ok((file, metadata.len()))
}

/// A regular file open for reading.
struct MacosReadFile {
    file: File,
    len: u64,
}

impl Read for MacosReadFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Seek for MacosReadFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

impl ReadSeekFile for MacosReadFile {
    fn len(&self) -> u64 {
        self.len
    }
}

/// A regular file open with `O_APPEND`, so every write lands at its end.
struct MacosAppendFile {
    file: File,
    len: u64,
    path: PathBuf,
}

impl Write for MacosAppendFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.file.write(buf)?;
        self.len += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl AppendFile for MacosAppendFile {
    fn len(&self) -> u64 {
        self.len
    }

    fn truncate(&mut self, len: u64) -> Result<(), PlatformError> {
        if len > self.len {
            return Err(PlatformError::Io {
                op: "truncate",
                path: self.path.clone(),
                kind: io::ErrorKind::InvalidInput,
                message: format!("cannot grow a {} byte file to {len} bytes", self.len),
            });
        }
        self.file
            .set_len(len)
            .map_err(|err| PlatformError::from_io("truncate", &self.path, &err))?;
        self.len = len;
        Ok(())
    }

    /// On macOS `sync_all` issues `F_FULLFSYNC`, so the bytes reach the
    /// disk, not only its cache.
    fn sync(&mut self) -> Result<(), PlatformError> {
        self.file
            .sync_all()
            .map_err(|err| PlatformError::from_io("sync", &self.path, &err))
    }
}
