//! Storage locations resolved to folders (spec 0032): the three built in
//! locations that resolve through the root set, and the folders a person
//! chose.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use stackly_domain::{InstancePathEntry, LocationKind, StorageLocationId};

use super::roots::RootSet;

/// A storage location the set does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownLocation(pub StorageLocationId);

impl fmt::Display for UnknownLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "storage location {} is not known", self.0)
    }
}

impl std::error::Error for UnknownLocation {}

/// The folders of every storage location: the built in ones resolve through
/// the [`RootSet`], so `STACKLY_ROOT` keeps working, and the others hold the
/// canonical `<folder>/Stackly` path the registry stores.
///
/// ```
/// use stackly_domain::LocationKind;
/// use stackly_platform::{LocationSet, RootSet};
///
/// let roots = RootSet::new("/d".into(), "/l".into(), "/c".into(), "/r".into())?;
/// let set = LocationSet::new(roots);
/// assert_eq!(
///     set.base(LocationKind::Logs.built_in_id()).as_deref(),
///     Some(std::path::Path::new("/l"))
/// );
/// # Ok::<(), stackly_platform::PlatformError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationSet {
    roots: RootSet,
    custom: BTreeMap<StorageLocationId, (LocationKind, PathBuf)>,
}

impl LocationSet {
    /// A set with only the built in locations.
    #[must_use]
    pub const fn new(roots: RootSet) -> Self {
        Self {
            roots,
            custom: BTreeMap::new(),
        }
    }

    /// This set with one more location. A built in ID is ignored: it always
    /// resolves through the root set.
    #[must_use]
    pub fn with_location(
        mut self,
        id: StorageLocationId,
        kind: LocationKind,
        path: PathBuf,
    ) -> Self {
        self.insert(id, kind, path);
        self
    }

    /// Adds a location.
    pub fn insert(&mut self, id: StorageLocationId, kind: LocationKind, path: PathBuf) {
        if built_in_kind(id).is_none() {
            self.custom.insert(id, (kind, path));
        }
    }

    /// Returns the root set the built in locations resolve through.
    #[must_use]
    pub const fn roots(&self) -> &RootSet {
        &self.roots
    }

    /// Returns the folder that holds a location: the root for a built in
    /// location, the `Stackly` folder for another one, or `None` when the
    /// set does not know the ID.
    #[must_use]
    pub fn base(&self, id: StorageLocationId) -> Option<PathBuf> {
        match built_in_kind(id) {
            Some(LocationKind::Data) => Some(self.roots.data().to_path_buf()),
            Some(LocationKind::Logs) => Some(self.roots.logs().to_path_buf()),
            Some(LocationKind::Cache) => Some(self.roots.cache().to_path_buf()),
            None => self.custom.get(&id).map(|(_, path)| path.clone()),
        }
    }

    /// Returns what a location holds, or `None` for an unknown ID.
    #[must_use]
    pub fn kind(&self, id: StorageLocationId) -> Option<LocationKind> {
        built_in_kind(id).or_else(|| self.custom.get(&id).map(|(kind, _)| *kind))
    }

    /// Resolves a stored path: under its location, or under the runtime
    /// root for a path that has none.
    ///
    /// # Errors
    ///
    /// Returns [`UnknownLocation`] when the entry names a location the set
    /// does not know.
    pub fn resolve(&self, entry: &InstancePathEntry) -> Result<PathBuf, UnknownLocation> {
        match entry.location() {
            Some(location) => {
                let base = self.base(location).ok_or(UnknownLocation(location))?;
                Ok(join_relative(&base, entry.path().as_str()))
            }
            None => Ok(self.roots.resolve(entry.root(), entry.path())),
        }
    }

    /// Every custom location with its kind and folder, by ID.
    pub fn custom(&self) -> impl Iterator<Item = (StorageLocationId, LocationKind, &Path)> {
        self.custom
            .iter()
            .map(|(id, (kind, path))| (*id, *kind, path.as_path()))
    }
}

fn join_relative(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

fn built_in_kind(id: StorageLocationId) -> Option<LocationKind> {
    LocationKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.built_in_id() == id)
}
