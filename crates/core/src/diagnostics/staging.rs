//! The staged previews of the diagnostic bundle (spec 0034, *Staging
//! preview*, AC-12).
//!
//! A preview is one zip in `<cache>/diagnostics/<previewId>.zip`, mode
//! 0600, in a folder of mode 0700 that the daemon empties at every start.
//! The daemon keeps at most [`PREVIEWS_MAX`] previews, each with the file
//! open: expiry is checked at every lookup and every new preview, and a
//! preview that expired or was pushed out by a newer one is dropped from
//! the map and unlinked, while an export already copying from its open file
//! still finishes. Nothing here outlives the daemon.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use stackly_platform::{FileKind, Filesystem, PlatformError, ReadSeekFile};
use uuid::Uuid;

use super::bundle::{BuiltBundle, hex};

/// How long a preview can be exported.
pub const PREVIEW_TTL: SignedDuration = SignedDuration::from_mins(15);
/// The most previews the daemon keeps; a newer one pushes out the oldest.
pub const PREVIEWS_MAX: usize = 2;
/// The name of the staging folder under the cache root.
const STAGING_DIR: &str = "diagnostics";

/// Why a write of the bundle failed, the `reason` of
/// `diagnostics.bundle_failed` and `diagnostics.write_failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WriteReason {
    /// The volume is full.
    NoSpace,
    /// The folder or file cannot be written by this user.
    Permission,
    /// Any other error.
    Io,
}

impl WriteReason {
    /// The reason of a filesystem error.
    #[must_use]
    pub const fn of(error: &PlatformError) -> Self {
        match error {
            PlatformError::NoSpace { .. } => Self::NoSpace,
            PlatformError::PermissionDenied { .. } => Self::Permission,
            _ => Self::Io,
        }
    }

    /// The reason of an I/O error.
    #[must_use]
    pub fn of_io(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::StorageFull => Self::NoSpace,
            io::ErrorKind::PermissionDenied => Self::Permission,
            _ => Self::Io,
        }
    }

    /// The wire code, such as `no_space`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NoSpace => "no_space",
            Self::Permission => "permission",
            Self::Io => "io",
        }
    }
}

/// A preview could not be staged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the bundle preview cannot be staged ({}): {message}", reason.as_code())]
#[non_exhaustive]
pub struct StagingError {
    /// Why.
    pub reason: WriteReason,
    /// What failed.
    pub message: String,
}

impl StagingError {
    fn of(error: &PlatformError) -> Self {
        Self {
            reason: WriteReason::of(error),
            message: error.to_string(),
        }
    }

    fn io(path: &Path, error: &io::Error) -> Self {
        Self {
            reason: WriteReason::of_io(error),
            message: format!("{}: {error}", path.display()),
        }
    }
}

/// One staged preview: the zip open for reading, its digest, and its
/// times.
pub struct StagedPreview {
    /// The preview ID, a UUID v7.
    pub id: Uuid,
    /// When it was built.
    pub created_at: Timestamp,
    /// When it can no longer be exported.
    pub expires_at: Timestamp,
    /// The sha256 of the zip, lowercase hex.
    pub sha256: String,
    /// The size of the zip.
    pub bytes: u64,
    path: PathBuf,
    file: Mutex<Box<dyn ReadSeekFile>>,
}

impl std::fmt::Debug for StagedPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagedPreview")
            .field("id", &self.id)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("sha256", &self.sha256)
            .field("bytes", &self.bytes)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl StagedPreview {
    /// Copies the whole zip from its open file into `out` and returns its
    /// size and sha256. Two copies of one preview take turns on the file.
    ///
    /// # Errors
    ///
    /// Returns the I/O error of the read or of `out`.
    pub fn copy_to(&self, out: &mut dyn Write) -> io::Result<(u64, String)> {
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        file.seek(SeekFrom::Start(0))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut copied = 0u64;
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let chunk = &buffer[..read];
            hasher.update(chunk);
            out.write_all(chunk)?;
            copied += u64::try_from(read).unwrap_or(u64::MAX);
        }
        Ok((copied, hex(&hasher.finalize())))
    }

    /// The staged file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the preview can no longer be exported at `now`.
    #[must_use]
    pub fn expired(&self, now: Timestamp) -> bool {
        now >= self.expires_at
    }
}

/// The staged previews of one daemon.
#[derive(Debug)]
pub struct Staging {
    cache_root: PathBuf,
    previews: Mutex<Vec<Arc<StagedPreview>>>,
}

impl Staging {
    /// The staging of the canonical cache root `cache_root`; its folder is
    /// `<cache>/diagnostics`.
    #[must_use]
    pub const fn new(cache_root: PathBuf) -> Self {
        Self {
            cache_root,
            previews: Mutex::new(Vec::new()),
        }
    }

    /// The staging folder.
    #[must_use]
    pub fn dir(&self) -> PathBuf {
        self.cache_root.join(STAGING_DIR)
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Arc<StagedPreview>>> {
        self.previews.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Removes the staging folder and everything in it, as the daemon does
    /// at every start; nothing there belongs to a running daemon then.
    ///
    /// # Errors
    ///
    /// Returns the error of the removal.
    pub fn clear(&self, fs: &dyn Filesystem) -> Result<(), PlatformError> {
        fs.remove_tree(&self.cache_root, Path::new(STAGING_DIR))
            .map(drop)
    }

    /// Stages `bundle` as preview `id`, built at `created_at`: drops the
    /// previews that expired, pushes out the oldest past
    /// [`PREVIEWS_MAX`], writes the zip with mode 0600, and keeps it open.
    ///
    /// # Errors
    ///
    /// Returns [`StagingError`] when the folder or the file cannot be
    /// written; nothing of the new preview is kept then.
    pub fn stage(
        &self,
        fs: &dyn Filesystem,
        id: Uuid,
        created_at: Timestamp,
        bundle: &BuiltBundle,
    ) -> Result<Arc<StagedPreview>, StagingError> {
        self.prune(fs, created_at);
        let dir = self.dir();
        match fs
            .symlink_metadata(&dir)
            .map_err(|error| StagingError::of(&error))?
        {
            None => fs
                .create_dir_private(&dir)
                .map_err(|error| StagingError::of(&error))?,
            Some(info) if info.kind == FileKind::Dir => {}
            Some(_) => {
                return Err(StagingError {
                    reason: WriteReason::Io,
                    message: format!("{} is not a folder", dir.display()),
                });
            }
        }
        let path = dir.join(format!("{id}.zip"));
        let written = fs
            .create_file_private(&path)
            .map_err(|error| StagingError::of(&error))
            .and_then(|mut file| {
                file.write_all(&bundle.bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| StagingError::io(&path, &error))
            })
            .and_then(|()| {
                fs.open_read(&path)
                    .map_err(|error| StagingError::of(&error))
            });
        let file = match written {
            Ok(file) => file,
            Err(error) => {
                let _ = fs.remove_file(&path);
                return Err(error);
            }
        };
        let preview = Arc::new(StagedPreview {
            id,
            created_at,
            expires_at: created_at.saturating_add(PREVIEW_TTL).unwrap_or(created_at),
            sha256: bundle.sha256.clone(),
            bytes: u64::try_from(bundle.bytes.len()).unwrap_or(u64::MAX),
            path,
            file: Mutex::new(file),
        });
        let evicted: Vec<Arc<StagedPreview>> = {
            let mut previews = self.lock();
            previews.push(Arc::clone(&preview));
            let over = previews.len().saturating_sub(PREVIEWS_MAX);
            previews.drain(..over).collect()
        };
        for old in evicted {
            let _ = fs.remove_file(old.path());
        }
        Ok(preview)
    }

    /// The preview `id`, unless it expired at `now` or was pushed out.
    #[must_use]
    pub fn get(&self, fs: &dyn Filesystem, id: Uuid, now: Timestamp) -> Option<Arc<StagedPreview>> {
        self.prune(fs, now);
        self.lock()
            .iter()
            .find(|preview| preview.id == id)
            .map(Arc::clone)
    }

    /// The IDs of the previews kept now, oldest first.
    #[must_use]
    pub fn ids(&self) -> Vec<Uuid> {
        self.lock().iter().map(|preview| preview.id).collect()
    }

    /// Drops and unlinks every preview that expired at `now`.
    fn prune(&self, fs: &dyn Filesystem, now: Timestamp) {
        let expired: Vec<Arc<StagedPreview>> = {
            let mut previews = self.lock();
            let (gone, kept): (Vec<_>, Vec<_>) =
                previews.drain(..).partition(|preview| preview.expired(now));
            *previews = kept;
            gone
        };
        for preview in expired {
            let _ = fs.remove_file(preview.path());
        }
    }
}
