//! The rules a folder must pass to become a storage location (spec 0032,
//! *Aturan lokasi*): [`gather_location_facts`] reads what the rules need
//! and [`check_location`] judges it, a pure function that reports the first
//! rule that fails, in the order of the table.
//!
//! The twelfth rule, `not_writable`, is not a fact: it is what creating the
//! folders ends with ([`prepare_location`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use stackly_domain::{LocationKind, LocationRule, StorageLocationId};
use stackly_platform::{
    FileKind, Filesystem, Platform, PlatformError, VolumeFacts, check_private_dir,
    is_same_or_inside,
};

use crate::layout::ensure_private_dir;

/// The folder a location keeps inside the one the person chose.
pub const LOCATION_DIR: &str = "Stackly";

/// The marker file inside `<folder>/Stackly` that names the location.
pub const MARKER_FILE: &str = ".stackly-location";

/// The most bytes read from a marker file.
const MARKER_MAX: u64 = 4096;

/// Folders of the home folder macOS may keep the daemon out of.
const PROTECTED: [&[&str]; 5] = [
    &["Desktop"],
    &["Documents"],
    &["Downloads"],
    &["Library", "Mobile Documents"],
    &["Library", "CloudStorage"],
];

/// The content of `.stackly-location`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    /// The location this folder belongs to.
    pub id: StorageLocationId,
    /// What it holds.
    pub kind: String,
}

/// What `<folder>/Stackly` is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderFact {
    /// Nothing is there.
    Absent,
    /// A marker names a location.
    Marked(Marker),
    /// Something is there and no readable marker names a location.
    Unmarked,
}

/// What the rules need to know about one candidate folder.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LocationFacts {
    /// The path as given.
    pub path: PathBuf,
    /// Whether anything exists there, a symlink included.
    pub exists: bool,
    /// Whether it is a folder, or a symlink to one.
    pub is_dir: bool,
    /// Whether the path or a folder above it is a symlink, apart from the
    /// links the system itself keeps (`/var`, `/tmp`, `/etc`).
    pub symlink_on_path: bool,
    /// Whether the current user owns it.
    pub owned: bool,
    /// Whether group or others can write to it.
    pub open_to_write: bool,
    /// Whether a folder above it, once resolved, lets group or others write
    /// to it without the sticky bit, so another user could rename or
    /// replace the chosen folder.
    pub ancestor_open_to_write: bool,
    /// The path with every symlink resolved, when it exists.
    pub canonical: Option<PathBuf>,
    /// The volume that holds it.
    pub volume: Result<VolumeFacts, String>,
    /// What `<folder>/Stackly` is.
    pub folder: FolderFact,
    /// The roots and location folders of the context that `<folder>/Stackly`
    /// overlaps by file identity along the ancestors (another letter case on
    /// APFS, another Unicode normalization, or a firmlink), which a
    /// comparison of the text misses.
    pub identity_overlaps: Vec<PathBuf>,
}

/// What the rules compare a folder with.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct LocationContext {
    /// The home folder of the user, for the protected folders.
    pub home: Option<PathBuf>,
    /// The canonical form of every root and bundle a location must not
    /// overlap: the four owned roots and the app bundle.
    pub roots: Vec<PathBuf>,
    /// Every location the registry holds that has a folder, with its kind.
    pub existing: Vec<(StorageLocationId, LocationKind, PathBuf)>,
}

impl LocationContext {
    /// A context from its three parts.
    #[must_use]
    pub const fn new(
        home: Option<PathBuf>,
        roots: Vec<PathBuf>,
        existing: Vec<(StorageLocationId, LocationKind, PathBuf)>,
    ) -> Self {
        Self {
            home,
            roots,
            existing,
        }
    }
}

/// The verdict of a folder that passed every rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationVerdict {
    /// A new location.
    New,
    /// The folder already is this location, of this kind, so it is used
    /// again.
    Reuse(StorageLocationId),
}

/// Reads the facts of `path` for a location, compared with the roots and
/// locations of `context`. It only reads.
///
/// # Errors
///
/// Returns the error of a read that fails for a reason other than the path
/// not existing, since no rule can say that.
pub fn gather_location_facts(
    platform: &Platform,
    path: &Path,
    context: &LocationContext,
) -> Result<LocationFacts, PlatformError> {
    let fs = platform.fs();
    let absolute = path.is_absolute();
    let info = if absolute {
        fs.symlink_metadata(path)?
    } else {
        None
    };
    let canonical = match &info {
        Some(_) => fs.canonicalize(path).ok(),
        None => None,
    };
    let (is_dir, owned, open_to_write) = match (&info, &canonical) {
        (Some(info), Some(canonical)) => {
            let target = if info.kind == FileKind::Symlink {
                fs.symlink_metadata(canonical)?
            } else {
                Some(info.clone())
            };
            match target {
                Some(target) => {
                    let user = platform.identity().current_user().ok();
                    (
                        target.kind == FileKind::Dir,
                        user.is_some() && target.owner == user,
                        target.mode.is_none_or(|mode| mode & 0o022 != 0),
                    )
                }
                None => (false, false, false),
            }
        }
        _ => (false, false, false),
    };
    let symlink_on_path = absolute && symlink_on_path(fs, path);
    let ancestor_open_to_write = canonical
        .as_deref()
        .is_some_and(|canonical| ancestor_open_to_write(fs, canonical));
    let identity_overlaps = match &canonical {
        Some(canonical) if is_dir => identity_overlaps(fs, &canonical.join(LOCATION_DIR), context)?,
        _ => Vec::new(),
    };
    let volume = match &canonical {
        Some(canonical) => fs
            .volume_facts(canonical)
            .map_err(|error| error.to_string()),
        None => Err("the folder does not exist".to_owned()),
    };
    let folder = if absolute && is_dir {
        folder_fact(fs, &path.join(LOCATION_DIR))?
    } else {
        FolderFact::Absent
    };
    Ok(LocationFacts {
        path: path.to_path_buf(),
        exists: info.is_some(),
        is_dir,
        symlink_on_path,
        owned,
        open_to_write,
        ancestor_open_to_write,
        canonical,
        volume,
        folder,
        identity_overlaps,
    })
}

/// Whether a folder above `canonical` lets group or others write to it and
/// has no sticky bit. A folder above that cannot be inspected is skipped:
/// the folder itself was read, so the way to it is open.
fn ancestor_open_to_write(fs: &dyn Filesystem, canonical: &Path) -> bool {
    canonical.ancestors().skip(1).any(|ancestor| {
        let Ok(Some(info)) = fs.symlink_metadata(ancestor) else {
            return false;
        };
        info.mode
            .is_none_or(|mode| mode & 0o022 != 0 && mode & 0o1000 == 0)
    })
}

/// The roots and location folders of `context` that `target` is, is inside,
/// or holds, compared by path and by file identity like the data root
/// overlap check of `crate::ownership`.
fn identity_overlaps(
    fs: &dyn Filesystem,
    target: &Path,
    context: &LocationContext,
) -> Result<Vec<PathBuf>, PlatformError> {
    let mut overlaps = Vec::new();
    let others = context
        .roots
        .iter()
        .chain(context.existing.iter().map(|(_, _, path)| path));
    for other in others {
        if is_same_or_inside(fs, target, other)? || is_same_or_inside(fs, other, target)? {
            overlaps.push(other.clone());
        }
    }
    Ok(overlaps)
}

/// Whether `path` or one of its ancestors is a symlink, other than a
/// system link directly under `/` that points into `/private`.
fn symlink_on_path(fs: &dyn Filesystem, path: &Path) -> bool {
    for ancestor in path.ancestors() {
        // A folder above that cannot be inspected is not a link we can name;
        // the folder itself is read next, and it fails the same way when the
        // way to it is closed.
        let Ok(Some(info)) = fs.symlink_metadata(ancestor) else {
            continue;
        };
        if info.kind != FileKind::Symlink {
            continue;
        }
        let system_link = ancestor.parent() == Some(Path::new("/"))
            && fs
                .canonicalize(ancestor)
                .is_ok_and(|target| target.starts_with("/private"));
        if !system_link {
            return true;
        }
    }
    false
}

fn folder_fact(fs: &dyn Filesystem, folder: &Path) -> Result<FolderFact, PlatformError> {
    if fs.symlink_metadata(folder)?.is_none() {
        return Ok(FolderFact::Absent);
    }
    let marker = fs
        .read_file_limited(&folder.join(MARKER_FILE), MARKER_MAX)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok());
    Ok(marker.map_or(FolderFact::Unmarked, FolderFact::Marked))
}

/// Whether the folder of a default location is fit to create in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationState {
    /// The folder and its `instances/` folder exist and are the user's.
    Ready,
    /// The folder, or its `instances/` folder, is gone.
    Missing,
    /// The folder exists but the daemon cannot write to it.
    NotWritable,
}

/// Whether the `Stackly` folder of a custom location and its `instances/`
/// folder are there, are private folders of the user, and the user can
/// write to them. A folder that is there but is not private is
/// [`LocationState::NotWritable`].
pub fn probe_location(platform: &Platform, base: &Path) -> LocationState {
    let fs = platform.fs();
    let user = platform.identity().current_user().ok();
    // The same privacy rule as the built in roots (a folder of the user, no
    // group or other access, not a symlink), plus write access for the user.
    let usable = |path: &Path| match fs.symlink_metadata(path) {
        Ok(Some(info)) => Some(user.as_ref().is_some_and(|user| {
            check_private_dir(&info, user).is_ok()
                && info.mode.is_some_and(|mode| mode & 0o200 != 0)
        })),
        Err(_) => Some(false),
        Ok(None) => None,
    };
    match (usable(base), usable(&base.join("instances"))) {
        (Some(true), Some(true)) => LocationState::Ready,
        (Some(_), Some(false)) | (Some(false), _) => LocationState::NotWritable,
        (None, _) | (_, None) => LocationState::Missing,
    }
}

/// Judges a folder for a location of `kind`: the first rule it breaks, in
/// the order of the table in the spec, or the verdict.
///
/// # Errors
///
/// Returns the [`LocationRule`] the folder broke first. `not_writable`
/// never comes from here.
pub fn check_location(
    kind: LocationKind,
    facts: &LocationFacts,
    context: &LocationContext,
) -> Result<LocationVerdict, LocationRule> {
    if !facts.path.is_absolute() {
        return Err(LocationRule::NotAbsolute);
    }
    if !facts.exists {
        return Err(LocationRule::Missing);
    }
    if !facts.is_dir {
        return Err(LocationRule::NotDirectory);
    }
    if facts.symlink_on_path {
        return Err(LocationRule::Symlink);
    }
    if !facts.owned {
        return Err(LocationRule::NotOwned);
    }
    if facts.open_to_write || facts.ancestor_open_to_write {
        return Err(LocationRule::TooOpen);
    }
    let resolved = facts.canonical.as_deref().unwrap_or(&facts.path);
    if in_protected_folder(resolved, context.home.as_deref())
        || in_protected_folder(&facts.path, context.home.as_deref())
    {
        return Err(LocationRule::ProtectedFolder);
    }
    let Ok(volume) = &facts.volume else {
        return Err(LocationRule::NotLocalVolume);
    };
    if !volume.local {
        return Err(LocationRule::NotLocalVolume);
    }
    if !volume.internal {
        return Err(LocationRule::NotInternalVolume);
    }
    let target = resolved.join(LOCATION_DIR);
    let reuse = match &facts.folder {
        FolderFact::Marked(marker) => context
            .existing
            .iter()
            .find(|(id, existing_kind, path)| {
                *id == marker.id
                    && *existing_kind == kind
                    && marker.kind == kind.as_code()
                    && *path == target
            })
            .map(|(id, _, _)| *id),
        FolderFact::Absent | FolderFact::Unmarked => None,
    };
    let overlaps = |other: &Path| {
        target.starts_with(other)
            || other.starts_with(&target)
            || facts.identity_overlaps.iter().any(|path| path == other)
    };
    if context.roots.iter().any(|root| overlaps(root))
        || context
            .existing
            .iter()
            .filter(|(id, _, _)| Some(*id) != reuse)
            .any(|(_, _, path)| overlaps(path))
    {
        return Err(LocationRule::OverlapsRoot);
    }
    match (&facts.folder, reuse) {
        (FolderFact::Absent, _) => Ok(LocationVerdict::New),
        (_, Some(id)) => Ok(LocationVerdict::Reuse(id)),
        _ => Err(LocationRule::ForeignMarker),
    }
}

fn in_protected_folder(path: &Path, home: Option<&Path>) -> bool {
    let Some(home) = home else {
        return false;
    };
    PROTECTED.iter().any(|parts| {
        let folder = parts
            .iter()
            .fold(home.to_path_buf(), |dir, part| dir.join(part));
        path.starts_with(folder)
    })
}

/// Creates the folders of a location for `kind` inside `folder`: `Stackly`
/// with mode 0700, the subfolders of its layout, and the marker file. It
/// does nothing for what exists already, so a location that is used again
/// keeps its content, but a folder that exists must pass the privacy rule
/// of the built in roots (`check_private_dir`): a folder of the user with
/// no group or other access. One that fails is rejected, never repaired.
///
/// # Errors
///
/// Returns [`LocationRule::NotWritable`] when a folder or the marker cannot
/// be created, or a folder that exists is not a private folder of the user.
pub fn prepare_location(
    platform: &Platform,
    folder: &Path,
    kind: LocationKind,
    id: StorageLocationId,
) -> Result<PathBuf, LocationRule> {
    let fs = platform.fs();
    let user = platform
        .identity()
        .current_user()
        .map_err(|_| LocationRule::NotWritable)?;
    let location = folder.join(LOCATION_DIR);
    ensure_private_dir(fs, &location, &user).map_err(|_| LocationRule::NotWritable)?;
    let subfolders: &[&str] = match kind {
        LocationKind::Data | LocationKind::Logs => &["instances"],
        LocationKind::Cache => &["downloads"],
    };
    for subfolder in subfolders {
        ensure_private_dir(fs, &location.join(subfolder), &user)
            .map_err(|_| LocationRule::NotWritable)?;
    }
    let marker_path = location.join(MARKER_FILE);
    if fs
        .symlink_metadata(&marker_path)
        .map_err(|_| LocationRule::NotWritable)?
        .is_none()
    {
        let marker = Marker {
            id,
            kind: kind.as_code().to_owned(),
        };
        let bytes = serde_json::to_vec(&marker).map_err(|_| LocationRule::NotWritable)?;
        fs.write_file_atomic(&marker_path, &bytes)
            .map_err(|_| LocationRule::NotWritable)?;
    }
    Ok(location)
}
