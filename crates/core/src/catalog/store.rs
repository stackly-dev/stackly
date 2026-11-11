//! Writing a provider package copy into a private folder of the data root:
//! the steps pinning a revision (spec 0017 K-1) and storing an imported
//! local package (spec 0037, *Store salinan*) share.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use stackly_domain::{ProviderRevisionKey, Sha256Digest};
use stackly_platform::{FileKind, Filesystem, Platform, PlatformError};

use super::local::package_sha256;
use crate::provider::{NameCheck, PackageFiles, VERSIONS_MAX_LEN, ValidatedPackage};
use crate::{CoreError, PreparedLayout, check_owned_path, ensure_private_dir};

/// The file `manifest_sha256` does not cover, so a pinned copy that
/// exists already is never compared on it: the first copy wins.
const VERSIONS_FILE: &str = "versions.json";
/// The most a stored file may hold when it is read back.
const MAX_READ_BACK_LEN: u64 = VERSIONS_MAX_LEN;
/// How often a file is read when another writer keeps replacing it. A
/// writer replaces a file only when it found the file missing, so each
/// file is replaced at most once per writer that started before the first
/// copy landed, and the engine runs at most 8 operations at once.
const READ_ATTEMPTS: usize = 16;

/// Which copy is written, and what its read back must match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode<'a> {
    /// A pinned provider revision: `versions.json` that exists already is
    /// not compared (the first copy wins), and the read back must validate
    /// with this key.
    Pinned(&'a ProviderRevisionKey),
    /// An imported local package: every file is compared, `versions.json`
    /// included, and the read back must have this package digest.
    Import(&'a Sha256Digest),
}

/// What [`write_package`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stored {
    /// Whether this call wrote a file. False when the folder held every
    /// file already.
    pub wrote_files: bool,
    /// Whether this call made the package folder itself, so a caller that
    /// undoes its work on failure owns it. Always false for
    /// [`StoreMode::Pinned`], which never undoes a folder.
    pub created_dir: bool,
}

/// Why a package copy could not be written. The reasons are those of
/// `catalog.pin_failed` and `provider.store_failed`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// A file in the folder of a pin differs from the package. It is left
    /// as it is.
    #[error("the stored copy of {} differs from the package", .path.display())]
    ExistingDiffers {
        /// The file.
        path: PathBuf,
    },
    /// The copy read back does not match the package.
    #[error("the stored copy does not have the digest of the package")]
    DigestMismatch,
    /// A symlink, a foreign entry, or a folder that is not private is in
    /// the way. Nothing is overwritten or removed.
    #[error("{} is not safe to write into", .path.display())]
    UnsafePath {
        /// The path.
        path: PathBuf,
    },
    /// The disk is full.
    #[error("the disk is full")]
    NoSpace,
    /// Any other failure of a port.
    #[error("{op} failed for {}: {message}", .path.display())]
    Io {
        /// What was being done.
        op: &'static str,
        /// The path.
        path: PathBuf,
        /// What the port said.
        message: String,
    },
}

impl StoreError {
    /// Returns the stable `reason` detail.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::ExistingDiffers { .. } => "existing_differs",
            Self::DigestMismatch => "digest_mismatch",
            Self::UnsafePath { .. } => "unsafe_path",
            Self::NoSpace => "no_space",
            Self::Io { .. } => "io",
        }
    }
}

fn platform_error(op: &'static str, path: &Path, error: &PlatformError) -> StoreError {
    match error {
        PlatformError::NoSpace { .. } => StoreError::NoSpace,
        PlatformError::SymlinkInPath { .. } | PlatformError::NotARegularFile { .. } => {
            StoreError::UnsafePath {
                path: path.to_path_buf(),
            }
        }
        other => StoreError::Io {
            op,
            path: path.to_path_buf(),
            message: other.to_string(),
        },
    }
}

pub(super) fn core_error(op: &'static str, path: &Path, error: &CoreError) -> StoreError {
    match error {
        CoreError::RootNotPrivate { .. }
        | CoreError::NotNormalized { .. }
        | CoreError::OutsideOwnedRoots { .. }
        | CoreError::IsOwnedRoot { .. } => StoreError::UnsafePath {
            path: path.to_path_buf(),
        },
        CoreError::Platform(error) => platform_error(op, path, error),
        other => StoreError::Io {
            op,
            path: path.to_path_buf(),
            message: other.to_string(),
        },
    }
}

/// Writes `files` into `dir`, a folder under `base`, and reads the copy
/// back.
///
/// `dir` is checked with `check_owned_path` before any IO. `base` and every
/// folder from it down to `dir` are made with mode 0700 and, when one
/// exists already, accepted only if it is a private folder of the user, not
/// a symlink. A folder holding an entry the package does not is refused. A
/// file that is not there is written atomically with mode 0600; one that is
/// there is compared byte for byte (in [`StoreMode::Pinned`], except
/// `versions.json`). In [`StoreMode::Import`] a file that differs is
/// written again atomically, since its folder is named by the digest of
/// the package and the difference is a change made afterwards; in
/// [`StoreMode::Pinned`] it is refused. The copy is then read back file by
/// file and must match the mode's key or digest.
///
/// It never deletes a file, and overwrites one only in an import as said.
/// Concurrent writers of the same bytes write the same bytes and all
/// succeed.
///
/// # Errors
///
/// Returns [`StoreError::UnsafePath`] for a path outside the owned roots, a
/// symlink, a foreign entry, or a folder that is not private;
/// [`StoreError::ExistingDiffers`] when a file of a pin differs; [`StoreError::NoSpace`]
/// and [`StoreError::Io`] when a port fails; and
/// [`StoreError::DigestMismatch`] when the copy reads back otherwise.
pub fn write_package(
    platform: &Platform,
    prepared: &PreparedLayout,
    base: &Path,
    dir: &Path,
    files: &PackageFiles,
    mode: StoreMode<'_>,
) -> Result<Stored, StoreError> {
    check_owned_path(platform, prepared, dir).map_err(|e| core_error("check path", dir, &e))?;
    let fs = platform.fs();
    let user = platform
        .identity()
        .current_user()
        .map_err(|e| platform_error("current user", dir, &e))?;
    let relative = dir.strip_prefix(base).map_err(|_| StoreError::UnsafePath {
        path: dir.to_path_buf(),
    })?;
    // Only an import undoes its folder, so only an import looks first; a
    // pin keeps the calls it always made.
    let created_dir = match mode {
        StoreMode::Pinned(_) => false,
        StoreMode::Import(_) => fs
            .symlink_metadata(dir)
            .map_err(|e| platform_error("inspect folder", dir, &e))?
            .is_none(),
    };
    let mut folder = base.to_path_buf();
    ensure_private_dir(fs, &folder, &user).map_err(|e| core_error("create folder", &folder, &e))?;
    for part in relative.components() {
        folder.push(part);
        ensure_private_dir(fs, &folder, &user)
            .map_err(|e| core_error("create folder", &folder, &e))?;
    }

    check_no_foreign_entries(fs, files, dir)?;
    let mut wrote_files = false;
    for (path, bytes) in files.iter() {
        let target = dir.join(path);
        if let Some(parent) = target.parent()
            && parent != dir
        {
            ensure_private_dir(fs, parent, &user)
                .map_err(|e| core_error("create folder", parent, &e))?;
        }
        match fs
            .symlink_metadata(&target)
            .map_err(|e| platform_error("inspect file", &target, &e))?
        {
            None => {
                fs.write_file_atomic(&target, bytes)
                    .map_err(|e| platform_error("write file", &target, &e))?;
                wrote_files = true;
            }
            Some(info) if info.kind != FileKind::File => {
                return Err(StoreError::UnsafePath { path: target });
            }
            Some(_) if path == VERSIONS_FILE && matches!(mode, StoreMode::Pinned(_)) => {}
            Some(info) => {
                let max = info.len.max(bytes.len() as u64);
                let stored = read_stored(fs, "read file", &target, max)?;
                if stored != bytes {
                    match mode {
                        StoreMode::Pinned(_) => {
                            return Err(StoreError::ExistingDiffers { path: target });
                        }
                        // The folder of an import is named by the digest of
                        // these bytes, so a file that differs was changed
                        // after the import: an import of the same bytes
                        // writes it again (spec 0037 AC-3).
                        StoreMode::Import(_) => {
                            fs.write_file_atomic(&target, bytes)
                                .map_err(|e| platform_error("write file", &target, &e))?;
                            wrote_files = true;
                        }
                    }
                }
            }
        }
    }

    // Read back file by file: a folder listing could meet the temporary
    // file of a writer running at the same time.
    let mut stored = BTreeMap::new();
    for (path, bytes) in files.iter() {
        let target = dir.join(path);
        let max = (bytes.len() as u64).max(MAX_READ_BACK_LEN);
        let read = read_stored(fs, "read back file", &target, max)?;
        stored.insert(path.to_owned(), read);
    }
    let read_back = PackageFiles::new(files.folder().to_owned(), stored);
    let matches = match mode {
        StoreMode::Pinned(key) => ValidatedPackage::from_files(read_back, NameCheck::Skip)
            .is_ok_and(|package| package.key() == key),
        StoreMode::Import(digest) => package_sha256(&read_back) == *digest,
    };
    if !matches {
        return Err(StoreError::DigestMismatch);
    }
    Ok(Stored {
        wrote_files,
        created_dir,
    })
}

/// Reads one stored file.
///
/// Another writer of the same package that found the file missing renames
/// its own copy over it, with the same bytes and a new inode, and a
/// `Filesystem` may report a read that meets that replace as
/// [`PlatformError::NotARegularFile`]. When the path still holds a regular
/// file afterwards, the read runs again, at most [`READ_ATTEMPTS`] times;
/// a symlink or anything else there is [`StoreError::UnsafePath`], and a
/// file that never settles is [`StoreError::Io`].
fn read_stored(
    fs: &dyn Filesystem,
    op: &'static str,
    path: &Path,
    max: u64,
) -> Result<Vec<u8>, StoreError> {
    for _ in 0..READ_ATTEMPTS {
        match fs.read_file_limited(path, max) {
            Ok(bytes) => return Ok(bytes),
            Err(PlatformError::NotARegularFile { .. }) => match fs.symlink_metadata(path) {
                Ok(Some(info)) if info.kind == FileKind::File => {}
                Ok(_) => {
                    return Err(StoreError::UnsafePath {
                        path: path.to_path_buf(),
                    });
                }
                Err(error) => return Err(platform_error(op, path, &error)),
            },
            Err(error) => return Err(platform_error(op, path, &error)),
        }
    }
    Err(StoreError::Io {
        op,
        path: path.to_path_buf(),
        message: "the file kept being replaced while it was read".to_owned(),
    })
}

/// Refuses a folder that holds an entry the package does not: a symlink, a
/// file or folder of another name. Nothing is touched.
fn check_no_foreign_entries(
    fs: &dyn Filesystem,
    files: &PackageFiles,
    dir: &Path,
) -> Result<(), StoreError> {
    let mut expected: BTreeSet<PathBuf> = BTreeSet::new();
    for (path, _) in files.iter() {
        let mut so_far = dir.to_path_buf();
        for part in Path::new(path).components() {
            so_far.push(part);
            expected.insert(so_far.clone());
        }
    }
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = match fs.read_dir(&folder) {
            Ok(entries) => entries,
            Err(PlatformError::NotFound { .. }) => continue,
            Err(error) => return Err(platform_error("list folder", &folder, &error)),
        };
        for entry in entries {
            let path = folder.join(&entry.name);
            if entry.info.kind == FileKind::File && is_temp_of(&path, &expected) {
                continue;
            }
            if entry.info.kind == FileKind::Symlink
                || !expected.contains(&path)
                || (entry.info.kind != FileKind::Dir && entry.info.kind != FileKind::File)
            {
                return Err(StoreError::UnsafePath { path });
            }
            if entry.info.kind == FileKind::Dir {
                folders.push(path);
            }
        }
    }
    Ok(())
}

/// Whether `path` is the temporary file `write_file_atomic` makes beside an
/// expected file, `.<name>.tmp-<pid>-<counter>`: a writer running at the
/// same time owns it.
fn is_temp_of(path: &Path, expected: &BTreeSet<PathBuf>) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return false;
    };
    let Some(rest) = name.strip_prefix('.') else {
        return false;
    };
    rest.split_once(".tmp-")
        .is_some_and(|(base, _)| expected.contains(&parent.join(base)))
}
