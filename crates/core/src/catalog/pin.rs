//! Pinning a provider revision (spec 0017 K-1, built by spec 0020): the
//! files of a catalog entry's package are copied into the data folder, so
//! an instance keeps running with the exact package it was created with
//! even when the catalog changes.

use std::path::PathBuf;

use jiff::Timestamp;
use stackly_domain::{ProviderRevisionRecord, ProviderSourceId, TrustDecision};
use stackly_platform::Platform;

use super::model::ProviderEntry;
use super::store::{StoreError, StoreMode, write_package};
use crate::PreparedLayout;

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

impl From<StoreError> for PinError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::ExistingDiffers { path } => Self::ExistingDiffers { path },
            StoreError::DigestMismatch => Self::DigestMismatch,
            StoreError::UnsafePath { path } => Self::UnsafePath { path },
            StoreError::NoSpace => Self::NoSpace,
            StoreError::Io { op, path, message } => Self::Io { op, path, message },
        }
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
    pin_revision_with(platform, prepared, entry, source, PinMeta::catalog())
}

/// The registry columns a pin records beside the revision (spec 0037,
/// *Value sourcing*): `Stackly` without an approval for a catalog entry,
/// the namespace with its approval for an imported local entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PinMeta {
    /// The publisher column.
    pub publisher: String,
    /// The approval column.
    pub approval: Option<TrustDecision>,
    /// When the approval was decided.
    pub approved_at: Option<Timestamp>,
}

impl PinMeta {
    /// The columns of a catalog entry: publisher `Stackly`, no approval.
    #[must_use]
    pub fn catalog() -> Self {
        Self {
            publisher: PUBLISHER.to_owned(),
            approval: None,
            approved_at: None,
        }
    }

    /// The columns of an approved local entry of namespace `namespace`,
    /// approved at `approved_at`.
    #[must_use]
    pub fn local(namespace: &str, approved_at: Timestamp) -> Self {
        Self {
            publisher: namespace.to_owned(),
            approval: Some(TrustDecision::Approved),
            approved_at: Some(approved_at),
        }
    }
}

/// [`pin_revision`] with the registry columns of `meta`.
///
/// # Errors
///
/// The errors of [`pin_revision`].
pub fn pin_revision_with(
    platform: &Platform,
    prepared: &PreparedLayout,
    entry: &ProviderEntry,
    source: ProviderSourceId,
    meta: PinMeta,
) -> Result<PinnedRevision, PinError> {
    let package = &entry.package;
    let key = package.key();
    let layout = prepared.layout();
    let dir = layout.provider_revision_dir(key);
    let stored = write_package(
        platform,
        prepared,
        &layout.providers_dir(),
        &dir,
        package.files(),
        StoreMode::Pinned(key),
    )?;
    let record = ProviderRevisionRecord::new(
        package.provider_revision().clone(),
        Some(SCHEMA_VERSION.to_owned()),
        Some(meta.publisher),
        meta.approval,
    )
    .with_source_id(source);
    let record = match meta.approved_at {
        Some(at) => record.with_approved_at(at),
        None => record,
    };
    Ok(PinnedRevision {
        record,
        wrote_files: stored.wrote_files,
    })
}
