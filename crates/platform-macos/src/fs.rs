use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use stackly_platform::{
    AppendFile, DirEntryInfo, FileInfo, FileKind, Filesystem, PlatformError, ReadSeekFile,
    RemoveReport, TreeWriter, VolumeSpace,
};

mod fd;
mod handles;
mod remove_tree;
mod rename;
mod tree_writer;
mod volume;

/// Counter that makes each temporary file name unique within the process.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The macOS filesystem, through `std::fs` plus `O_NOFOLLOW` and
/// `O_NONBLOCK` for reads, so a symlink is never followed and a FIFO never
/// blocks. [`remove_tree`](Filesystem::remove_tree) works on file
/// descriptors (`openat`, `fstatat`, `unlinkat`, `fdopendir`), and so does
/// the [`TreeWriter`] from [`open_tree_writer`](Filesystem::open_tree_writer)
/// (`openat`, `mkdirat`, `symlinkat`, `fchmodat`), so a folder swapped for
/// a symlink while they run is never followed.
/// [`rename_no_replace`](Filesystem::rename_no_replace) uses
/// `renamex_np(RENAME_EXCL)`, and [`volume_space`](Filesystem::volume_space)
/// reads `statfs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacosFilesystem;

impl Filesystem for MacosFilesystem {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, PlatformError> {
        fs::canonicalize(path).map_err(|err| PlatformError::from_io("canonicalize", path, &err))
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Option<FileInfo>, PlatformError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(Some(FileInfo::from_unix_metadata(&metadata))),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(PlatformError::from_io("symlink_metadata", path, &err)),
        }
    }

    fn create_dir_private(&self, path: &Path) -> Result<(), PlatformError> {
        DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|err| PlatformError::from_io("create_dir_private", path, &err))
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntryInfo>, PlatformError> {
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
        let not_regular = || PlatformError::NotARegularFile {
            path: path.to_path_buf(),
        };
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            // O_NOFOLLOW on a symlink fails with ELOOP.
            Err(err) if err.raw_os_error() == Some(libc::ELOOP) => return Err(not_regular()),
            Err(err) => return Err(PlatformError::from_io("read_file_limited", path, &err)),
        };
        let metadata = file
            .metadata()
            .map_err(|err| PlatformError::from_io("read_file_limited", path, &err))?;
        if FileInfo::from_unix_metadata(&metadata).kind != FileKind::File {
            return Err(not_regular());
        }
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

    fn write_file_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), PlatformError> {
        let (dir, temp) = temp_path_for(path)?;
        let renamed = write_temp(&temp, bytes).and_then(|()| {
            fs::rename(&temp, path).map_err(|err| PlatformError::from_io("rename", path, &err))
        });
        if let Err(err) = renamed {
            remove_own_temp(&temp, &err);
            return Err(err);
        }
        File::open(dir)
            .and_then(|dir| dir.sync_all())
            .map_err(|err| PlatformError::from_io("sync directory", dir, &err))
    }

    fn remove_tree(&self, root: &Path, relative: &Path) -> Result<RemoveReport, PlatformError> {
        remove_tree::remove_tree(root, relative)
    }

    fn open_read(&self, path: &Path) -> Result<Box<dyn ReadSeekFile>, PlatformError> {
        handles::open_read(path)
    }

    fn open_append(&self, path: &Path) -> Result<Box<dyn AppendFile>, PlatformError> {
        handles::open_append(path)
    }

    fn rename_no_replace(&self, from: &Path, to: &Path) -> Result<(), PlatformError> {
        rename::rename_no_replace(from, to)
    }

    fn open_tree_writer(&self, root: &Path) -> Result<Box<dyn TreeWriter>, PlatformError> {
        tree_writer::open_tree_writer(root)
    }

    fn volume_space(&self, path: &Path) -> Result<VolumeSpace, PlatformError> {
        volume::volume_space(path)
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

/// Writes and syncs the temporary file. On macOS `sync_all` issues
/// `F_FULLFSYNC`, so the bytes reach the disk, not only its cache.
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
