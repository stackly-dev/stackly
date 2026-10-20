use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use stackly_platform::{AppendFile, FileInfo, FileKind, PlatformError, ReadSeekFile};

/// [`Filesystem::open_read`](stackly_platform::Filesystem::open_read)
/// through `std::fs`: the path is checked first and the opened file after.
pub(super) fn open_read(path: &Path) -> Result<Box<dyn ReadSeekFile>, PlatformError> {
    let before = expect_regular("open_read", path)?;
    let file = File::open(path).map_err(|err| PlatformError::from_io("open_read", path, &err))?;
    let len = expect_same("open_read", path, &file, &before)?;
    Ok(Box::new(FakeReadFile { file, len }))
}

/// [`Filesystem::open_append`](stackly_platform::Filesystem::open_append)
/// through `std::fs`: an existing path is checked first and the opened
/// file after; a missing one is created with `create_new`.
pub(super) fn open_append(path: &Path) -> Result<Box<dyn AppendFile>, PlatformError> {
    let io_err = |err: io::Error| PlatformError::from_io("open_append", path, &err);
    let (file, len) = match fs::symlink_metadata(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let file = OpenOptions::new()
                .append(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(io_err)?;
            (file, 0)
        }
        Err(err) => return Err(io_err(err)),
        Ok(_) => {
            let before = expect_regular("open_append", path)?;
            let file = OpenOptions::new().append(true).open(path).map_err(io_err)?;
            let len = expect_same("open_append", path, &file, &before)?;
            (file, len)
        }
    };
    Ok(Box::new(FakeAppendFile {
        file,
        len,
        path: path.to_path_buf(),
    }))
}

/// [`Filesystem::rename_no_replace`](stackly_platform::Filesystem::rename_no_replace)
/// through `std::fs`: the target is checked to be missing, then renamed
/// onto, so a target created in between would be replaced.
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> Result<(), PlatformError> {
    const OP: &str = "rename_no_replace";
    let no_parent = |path: &Path| PlatformError::Io {
        op: OP,
        path: path.to_path_buf(),
        kind: io::ErrorKind::InvalidInput,
        message: "the path has no parent directory".to_owned(),
    };
    let from_parent = from.parent().ok_or_else(|| no_parent(from))?;
    let to_parent = to.parent().ok_or_else(|| no_parent(to))?;
    let source =
        fs::symlink_metadata(from).map_err(|err| PlatformError::from_io(OP, from, &err))?;
    match fs::symlink_metadata(to) {
        Ok(_) => {
            return Err(PlatformError::AlreadyExists {
                path: to.to_path_buf(),
            });
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(PlatformError::from_io(OP, to, &err)),
    }
    let target_dir =
        fs::metadata(to_parent).map_err(|err| PlatformError::from_io(OP, to_parent, &err))?;
    if target_dir.dev() != source.dev() {
        return Err(PlatformError::CrossesDevice {
            path: to.to_path_buf(),
        });
    }
    fs::rename(from, to).map_err(|err| PlatformError::from_io(OP, from, &err))?;
    for dir in [from_parent, to_parent] {
        File::open(dir)
            .and_then(|dir| dir.sync_all())
            .map_err(|err| PlatformError::from_io(OP, dir, &err))?;
    }
    Ok(())
}

/// Fails unless the path is a regular file, read without following a
/// symlink.
fn expect_regular(op: &'static str, path: &Path) -> Result<FileInfo, PlatformError> {
    let before =
        fs::symlink_metadata(path).map_err(|err| PlatformError::from_io(op, path, &err))?;
    let info = FileInfo::from_unix_metadata(&before);
    if info.kind == FileKind::File {
        Ok(info)
    } else {
        Err(PlatformError::NotARegularFile {
            path: path.to_path_buf(),
        })
    }
}

/// Fails unless the opened file is the regular file checked before, and
/// returns its size.
fn expect_same(
    op: &'static str,
    path: &Path,
    file: &File,
    before: &FileInfo,
) -> Result<u64, PlatformError> {
    let opened = FileInfo::from_unix_metadata(
        &file
            .metadata()
            .map_err(|err| PlatformError::from_io(op, path, &err))?,
    );
    if opened.kind != FileKind::File || opened.identity != before.identity {
        return Err(PlatformError::NotARegularFile {
            path: path.to_path_buf(),
        });
    }
    Ok(opened.len)
}

struct FakeReadFile {
    file: File,
    len: u64,
}

impl Read for FakeReadFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Seek for FakeReadFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

impl ReadSeekFile for FakeReadFile {
    fn len(&self) -> u64 {
        self.len
    }
}

struct FakeAppendFile {
    file: File,
    len: u64,
    path: PathBuf,
}

impl Write for FakeAppendFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.file.write(buf)?;
        self.len += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl AppendFile for FakeAppendFile {
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

    fn sync(&mut self) -> Result<(), PlatformError> {
        self.file
            .sync_all()
            .map_err(|err| PlatformError::from_io("sync", &self.path, &err))
    }
}
