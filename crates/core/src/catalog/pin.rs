//! Pinning a provider revision (spec 0017 K-1, built by spec 0020): the
//! files of a catalog entry's package are copied into the data folder, so
//! an instance keeps running with the exact package it was created with
//! even when the catalog changes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use stackly_domain::{ProviderRevisionRecord, ProviderSourceId};
use stackly_platform::{FileKind, Platform, PlatformError};

use super::model::ProviderEntry;
use crate::provider::{NameCheck, PackageFiles, VERSIONS_MAX_LEN, ValidatedPackage};
use crate::{CoreError, PreparedLayout, check_owned_path, ensure_private_dir};

/// The file `manifest_sha256` does not cover, so a copy that exists already
/// is never compared: the first copy wins.
const VERSIONS_FILE: &str = "versions.json";
/// The most a pinned `versions.json` may hold when it is read back.
const MAX_VERSIONS_LEN: u64 = VERSIONS_MAX_LEN;
/// The `apiVersion` the pinned record names.
const SCHEMA_VERSION: &str = "stackly/v1alpha1";
/// The publisher of every package in the catalogs this build loads.
const PUBLISHER: &str = "Stackly";

/// What [`pin_revision`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedRevision {
    /// The record to commit with `InsertProviderRevision`, when the registry
    /// does not hold the revision yet.
    pub record: ProviderRevisionRecord,
    /// Whether this call wrote a file. False when the folder held every
    /// file already.
    pub wrote_files: bool,
}

/// Why a package could not be pinned. The wire code is
/// `catalog.pin_failed` with the `reason` detail of [`reason`](Self::reason).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PinError {
    /// A file in the pinned folder differs from the one in the catalog. It
    /// is left as it is.
    #[error("the pinned copy of {} differs from the catalog", .path.display())]
    ExistingDiffers {
        /// The file.
        path: PathBuf,
    },
    /// The copy read back has another digest than the revision key.
    #[error("the pinned copy does not have the digest of the revision")]
    DigestMismatch,
    /// A symlink, a foreign entry, or a folder that is not private is in
    /// the way. Nothing is overwritten or removed.
    #[error("{} is not safe to pin into", .path.display())]
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
    /// The catalog source has no row in the registry, so nothing was
    /// written.
    #[error("the catalog source is not recorded")]
    SourceUnrecorded,
}

impl PinError {
    /// Returns the stable `reason` detail of `catalog.pin_failed`.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::ExistingDiffers { .. } => "existing_differs",
            Self::DigestMismatch => "digest_mismatch",
            Self::UnsafePath { .. } => "unsafe_path",
            Self::NoSpace => "no_space",
            Self::Io { .. } => "io",
            Self::SourceUnrecorded => "source_unrecorded",
        }
    }
}

fn platform_error(op: &'static str, path: &Path, error: &PlatformError) -> PinError {
    match error {
        PlatformError::NoSpace { .. } => PinError::NoSpace,
        PlatformError::SymlinkInPath { .. } | PlatformError::NotARegularFile { .. } => {
            PinError::UnsafePath {
                path: path.to_path_buf(),
            }
        }
        other => PinError::Io {
            op,
            path: path.to_path_buf(),
            message: other.to_string(),
        },
    }
}

fn core_error(op: &'static str, path: &Path, error: &CoreError) -> PinError {
    match error {
        CoreError::RootNotPrivate { .. }
        | CoreError::NotNormalized { .. }
        | CoreError::OutsideOwnedRoots { .. }
        | CoreError::IsOwnedRoot { .. } => PinError::UnsafePath {
            path: path.to_path_buf(),
        },
        CoreError::Platform(error) => platform_error(op, path, error),
        other => PinError::Io {
            op,
            path: path.to_path_buf(),
            message: other.to_string(),
        },
    }
}

/// Copies the package of `entry` into its folder under `providers/` and
/// returns the record of the revision, naming `source`.
///
/// The folder is `DirectoryLayout::provider_revision_dir(key)`, checked
/// with `check_owned_path` before any IO. Folders are made with mode 0700
/// and, when one exists already, accepted only if it is a private folder of
/// the user, not a symlink. A file that is not there is written atomically
/// with mode 0600; one that is there is compared byte for byte, except
/// `versions.json`, which the digest does not cover (the first copy wins).
/// The folder is read back and must have the digest of the key.
///
/// Pinning never deletes a file, never overwrites one, and registers
/// nothing for compensation: a folder nobody refers to any more is for
/// garbage collection. Concurrent pins of one revision write the same bytes
/// and all succeed.
///
/// # Errors
///
/// Returns [`PinError::UnsafePath`] for a path outside the owned roots, a
/// symlink, a foreign entry, or a folder that is not private;
/// [`PinError::ExistingDiffers`] when a file differs; [`PinError::NoSpace`]
/// and [`PinError::Io`] when a port fails; and [`PinError::DigestMismatch`]
/// when the copy reads back with another digest.
pub fn pin_revision(
    platform: &Platform,
    prepared: &PreparedLayout,
    entry: &ProviderEntry,
    source: ProviderSourceId,
) -> Result<PinnedRevision, PinError> {
    let package = &entry.package;
    let key = package.key();
    let layout = prepared.layout();
    let dir = layout.provider_revision_dir(key);
    check_owned_path(platform, prepared, &dir).map_err(|e| core_error("check path", &dir, &e))?;

    let fs = platform.fs();
    let user = platform
        .identity()
        .current_user()
        .map_err(|e| platform_error("current user", &dir, &e))?;
    let providers = layout.providers_dir();
    let relative = dir
        .strip_prefix(&providers)
        .map_err(|_| PinError::UnsafePath { path: dir.clone() })?;
    let mut folder = providers;
    ensure_private_dir(fs, &folder, &user).map_err(|e| core_error("create folder", &folder, &e))?;
    for part in relative.components() {
        folder.push(part);
        ensure_private_dir(fs, &folder, &user)
            .map_err(|e| core_error("create folder", &folder, &e))?;
    }

    check_no_foreign_entries(platform, package, &dir)?;
    let mut wrote_files = false;
    for (path, bytes) in package.files().iter() {
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
                return Err(PinError::UnsafePath { path: target });
            }
            Some(_) if path == VERSIONS_FILE => {}
            Some(info) => {
                let max = info.len.max(bytes.len() as u64);
                let stored = fs
                    .read_file_limited(&target, max)
                    .map_err(|e| platform_error("read file", &target, &e))?;
                if stored != bytes {
                    return Err(PinError::ExistingDiffers { path: target });
                }
            }
        }
    }

    // Read back file by file: a folder listing could meet the temporary
    // file of a pin running at the same time.
    let mut stored = BTreeMap::new();
    for (path, bytes) in package.files().iter() {
        let target = dir.join(path);
        let max = (bytes.len() as u64).max(MAX_VERSIONS_LEN);
        let read = fs
            .read_file_limited(&target, max)
            .map_err(|e| platform_error("read back file", &target, &e))?;
        stored.insert(path.to_owned(), read);
    }
    let read_back = ValidatedPackage::from_files(
        PackageFiles::new(package.files().folder().to_owned(), stored),
        NameCheck::Skip,
    )
    .map_err(|_| PinError::DigestMismatch)?;
    if read_back.key() != key {
        return Err(PinError::DigestMismatch);
    }
    let record = ProviderRevisionRecord::new(
        package.provider_revision().clone(),
        Some(SCHEMA_VERSION.to_owned()),
        Some(PUBLISHER.to_owned()),
        None,
    )
    .with_source_id(source);
    Ok(PinnedRevision {
        record,
        wrote_files,
    })
}

/// Refuses a pinned folder that holds an entry the package does not: a
/// symlink, a file or folder of another name. Nothing is touched.
fn check_no_foreign_entries(
    platform: &Platform,
    package: &ValidatedPackage,
    dir: &Path,
) -> Result<(), PinError> {
    let fs = platform.fs();
    let mut expected: BTreeSet<PathBuf> = BTreeSet::new();
    for (path, _) in package.files().iter() {
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
                return Err(PinError::UnsafePath { path });
            }
            if entry.info.kind == FileKind::Dir {
                folders.push(path);
            }
        }
    }
    Ok(())
}

/// Whether `path` is the temporary file `write_file_atomic` makes beside an
/// expected file, `.<name>.tmp-<pid>-<counter>`: a pin running at the same
/// time owns it.
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
