use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use stackly_domain::{
    InstanceId, InstancePathKind, LocationKind, Registry, RegistryError, ShortKey,
    StorageLocationId,
};
use stackly_platform::{
    DirectoryLayout, Filesystem, InstancePaths, LocationSet, Platform, PlatformError, RootKind,
    RootSet, UserId, check_private_dir,
};

use crate::error::CoreError;

/// The folders of the storage locations and the locations each instance was
/// created in (spec 0032), shared by every holder of one
/// [`PreparedLayout`].
///
/// Every path of an instance goes through it, so an instance created in a
/// folder the person chose keeps resolving there while new ones follow the
/// default. An instance the book does not know resolves in the built in
/// locations, which is what a registry without custom locations holds.
#[derive(Debug)]
pub struct LocationBook {
    state: RwLock<BookState>,
}

#[derive(Debug, PartialEq, Eq)]
struct BookState {
    set: LocationSet,
    instances: HashMap<InstanceId, (StorageLocationId, StorageLocationId)>,
    cache: StorageLocationId,
}

impl LocationBook {
    fn new(roots: RootSet) -> Self {
        Self {
            state: RwLock::new(BookState {
                set: LocationSet::new(roots),
                instances: HashMap::new(),
                cache: LocationKind::Cache.built_in_id(),
            }),
        }
    }

    fn read<T>(&self, read: impl FnOnce(&BookState) -> T) -> T {
        read(&self.state.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn write<T>(&self, write: impl FnOnce(&mut BookState) -> T) -> T {
        write(&mut self.state.write().unwrap_or_else(PoisonError::into_inner))
    }

    /// Returns the location set as the book knows it now.
    #[must_use]
    pub fn set(&self) -> LocationSet {
        self.read(|state| state.set.clone())
    }

    /// Returns the folder that holds a location, or `None` for one the book
    /// does not know.
    #[must_use]
    pub fn base(&self, id: StorageLocationId) -> Option<PathBuf> {
        self.read(|state| state.set.base(id))
    }

    /// Learns a location, for example one `storage.set_location` added.
    pub fn add_location(&self, id: StorageLocationId, kind: LocationKind, path: PathBuf) {
        self.write(|state| state.set.insert(id, kind, path));
    }

    /// Learns every custom location in `locations` the book does not know.
    pub fn learn(&self, locations: &[stackly_domain::StorageLocation]) {
        self.write(|state| {
            for location in locations.iter().filter(|location| !location.is_built_in()) {
                if let Some(path) = location.path()
                    && state.set.base(location.id()).is_none()
                {
                    state
                        .set
                        .insert(location.id(), location.kind(), PathBuf::from(path));
                }
            }
        });
    }

    /// Records the data and log locations an instance lives in.
    pub fn note_instance(&self, id: InstanceId, data: StorageLocationId, logs: StorageLocationId) {
        self.write(|state| state.instances.insert(id, (data, logs)));
    }

    /// Returns the data and log locations of an instance, the built in ones
    /// for an instance the book does not know.
    #[must_use]
    pub fn locations_of(&self, id: InstanceId) -> (StorageLocationId, StorageLocationId) {
        self.read(|state| {
            state.instances.get(&id).copied().unwrap_or((
                LocationKind::Data.built_in_id(),
                LocationKind::Logs.built_in_id(),
            ))
        })
    }

    /// Returns the folder of every custom location with the owned root kind
    /// that stands for it: data for a data location, logs for a log
    /// location, cache for a cache location. A custom folder is an owned
    /// root like the built in ones, so destructive operations may touch
    /// what lies inside it.
    #[must_use]
    pub fn extra_roots(&self) -> Vec<(RootKind, PathBuf)> {
        self.read(|state| {
            state
                .set
                .custom()
                .map(|(_, kind, path)| {
                    let root = match kind {
                        LocationKind::Data => RootKind::Data,
                        LocationKind::Logs => RootKind::Logs,
                        LocationKind::Cache => RootKind::Cache,
                    };
                    (root, path.to_path_buf())
                })
                .collect()
        })
    }

    /// Makes `id` the location new downloads go to.
    pub fn set_cache(&self, id: StorageLocationId) {
        self.write(|state| state.cache = id);
    }

    /// Returns the folder new downloads go to: `downloads/` in the default
    /// cache location.
    #[must_use]
    pub fn downloads_dir(&self) -> PathBuf {
        self.read(|state| {
            let base = state
                .set
                .base(state.cache)
                .unwrap_or_else(|| state.set.roots().cache().to_path_buf());
            DirectoryLayout::downloads_dir_in(&base)
        })
    }

    /// Reads every location, the default cache location, and the locations
    /// of every instance from the registry. Run once when the daemon opens
    /// the registry.
    ///
    /// # Errors
    ///
    /// Returns the registry error of the first read that fails.
    pub fn load(&self, registry: &dyn Registry) -> Result<(), RegistryError> {
        let locations = registry.storage_locations()?.value;
        let settings = registry.daemon_settings()?.value;
        let ids = registry.known_instance_ids()?.value;
        let mut instances = HashMap::new();
        for id in ids {
            let Some(entries) = registry.instance_paths(id)?.value else {
                continue;
            };
            let find = |kind: InstancePathKind| {
                entries
                    .iter()
                    .find(|entry| entry.kind() == kind)
                    .and_then(|entry| entry.location())
            };
            if let (Some(data), Some(logs)) =
                (find(InstancePathKind::Data), find(InstancePathKind::Logs))
            {
                instances.insert(id, (data, logs));
            }
        }
        self.write(|state| {
            for location in locations.iter().filter(|location| !location.is_built_in()) {
                if let Some(path) = location.path() {
                    state
                        .set
                        .insert(location.id(), location.kind(), PathBuf::from(path));
                }
            }
            state.cache = settings.cache_location;
            state.instances = instances;
        });
        Ok(())
    }
}

/// A shared [`LocationBook`] that compares by what it holds, so two layouts
/// prepared from the same roots are equal.
#[derive(Debug, Clone)]
struct SharedBook(Arc<LocationBook>);

impl PartialEq for SharedBook {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || *self.0.state.read().unwrap_or_else(PoisonError::into_inner)
                == *other.0.state.read().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Eq for SharedBook {}

/// The directory layout after [`prepare_layout`] checked it, with the
/// canonical form of each owned root for containment checks.
///
/// Socket paths keep using the roots as resolved (shorter); containment
/// checks use the canonical forms kept here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedLayout {
    layout: DirectoryLayout,
    canonical: [PathBuf; 4],
    book: SharedBook,
}

impl PreparedLayout {
    /// Returns the layout that derives every Stackly path.
    #[must_use]
    pub const fn layout(&self) -> &DirectoryLayout {
        &self.layout
    }

    /// Returns the book of storage locations and the locations of each
    /// instance.
    #[must_use]
    pub fn locations(&self) -> &LocationBook {
        &self.book.0
    }

    /// Returns the paths of an active instance in the locations it was
    /// created in. Use this, not [`DirectoryLayout::instance_paths`], for
    /// an instance of the registry.
    #[must_use]
    pub fn instance_paths(&self, id: InstanceId) -> InstancePaths {
        let (data, logs) = self.bases_of(id);
        self.layout.instance_paths_in(id, &data, &logs)
    }

    /// Returns the paths of a retained instance in the locations it was
    /// created in.
    #[must_use]
    pub fn retained_instance_paths(&self, id: InstanceId) -> InstancePaths {
        let (data, logs) = self.bases_of(id);
        self.layout.retained_instance_paths_in(id, &data, &logs)
    }

    /// Returns the log folder of an instance, `instances/<id>/` in its log
    /// location.
    #[must_use]
    pub fn instance_log_dir(&self, id: InstanceId) -> PathBuf {
        self.instance_paths(id).logs
    }

    /// Returns the durable log store of an instance.
    #[must_use]
    pub fn instance_captured_logs_dir(&self, id: InstanceId) -> PathBuf {
        self.instance_log_dir(id)
            .join(stackly_platform::CAPTURED_LOGS_DIR)
    }

    /// Returns the folder new downloads go to.
    #[must_use]
    pub fn downloads_dir(&self) -> PathBuf {
        self.book.0.downloads_dir()
    }

    fn bases_of(&self, id: InstanceId) -> (PathBuf, PathBuf) {
        let (data, logs) = self.book.0.locations_of(id);
        let roots = self.layout.roots();
        (
            self.book
                .0
                .base(data)
                .unwrap_or_else(|| roots.data().to_path_buf()),
            self.book
                .0
                .base(logs)
                .unwrap_or_else(|| roots.logs().to_path_buf()),
        )
    }

    /// Returns the canonical form of one owned root.
    #[must_use]
    pub fn canonical_root(&self, kind: RootKind) -> &Path {
        match kind {
            RootKind::Data => &self.canonical[0],
            RootKind::Logs => &self.canonical[1],
            RootKind::Cache => &self.canonical[2],
            RootKind::Runtime => &self.canonical[3],
        }
    }

    /// Returns every owned root's canonical form with its kind, in
    /// [`RootKind::ALL`] order.
    #[must_use]
    pub fn canonical_roots(&self) -> [(RootKind, &Path); 4] {
        RootKind::ALL.map(|kind| (kind, self.canonical_root(kind)))
    }
}

/// Makes sure the owned roots and their standard folders exist as private
/// folders of the current user, and returns the checked layout.
///
/// Missing roots are created with mode 0700, but never their parents (so
/// `~/Library` is never created). Every root is then checked: a directory,
/// not a symlink, owned by the current user, with no group or other access.
/// The roots must not be the same folder or nest once resolved. Last, the
/// standard folders are created or checked the same way. Running it again
/// changes nothing.
///
/// # Errors
///
/// Returns [`CoreError::RootNotPrivate`] for a root or standard folder that
/// fails a check (it is never repaired), [`CoreError::RootsOverlap`] when
/// the canonical roots overlap, and [`CoreError::Platform`] when a port
/// fails, for example because a root's parent does not exist.
pub fn prepare_layout(platform: &Platform) -> Result<PreparedLayout, CoreError> {
    let roots = platform.directories().roots()?;
    let fs = platform.fs();
    let user = platform.identity().current_user()?;

    for (_, root) in roots.entries() {
        ensure_private_dir(fs, root, &user)?;
    }
    let canonical = canonical_roots(fs, &roots.entries())?;
    let book = SharedBook(Arc::new(LocationBook::new(roots.clone())));
    let layout = DirectoryLayout::new(roots);
    for dir in layout.standard_dirs() {
        ensure_private_dir(fs, &dir, &user)?;
    }
    Ok(PreparedLayout {
        layout,
        canonical,
        book,
    })
}

/// Makes sure the run folder of one instance exists, `<runtime>/i/<key>/`,
/// and returns it. The OS may empty the runtime root (at boot, for example),
/// so the runtime root and `i/` are created again with mode 0700 when they
/// are missing, and checked like [`prepare_layout`] does when they exist.
///
/// # Errors
///
/// Returns [`CoreError::RootNotPrivate`] for a folder on the way that fails
/// a check, and [`CoreError::Platform`] when a port fails.
pub fn ensure_instance_run_dir(
    platform: &Platform,
    prepared: &PreparedLayout,
    key: ShortKey,
) -> Result<PathBuf, CoreError> {
    let fs = platform.fs();
    let user = platform.identity().current_user()?;
    let layout = prepared.layout();
    ensure_runtime_dirs(fs, layout, &user)?;
    let run_dir = layout.instance_run_dir(key);
    ensure_private_dir(fs, &run_dir, &user)?;
    Ok(run_dir)
}

/// Makes sure the runtime root and its `i/` folder exist, so the daemon
/// can bind its socket again after the OS emptied the runtime root (at
/// boot, for example). Missing folders are created with mode 0700, and
/// existing ones are checked like [`prepare_layout`] does; a folder that
/// fails a check is never repaired. Running it again changes nothing.
///
/// # Errors
///
/// Returns [`CoreError::RootNotPrivate`] for a folder that fails a check,
/// and [`CoreError::Platform`] when a port fails.
pub fn ensure_runtime_root(
    platform: &Platform,
    prepared: &PreparedLayout,
) -> Result<(), CoreError> {
    let user = platform.identity().current_user()?;
    ensure_runtime_dirs(platform.fs(), prepared.layout(), &user)
}

/// The runtime part of the layout both [`ensure_runtime_root`] and
/// [`ensure_instance_run_dir`] rely on: the runtime root, then `i/`.
fn ensure_runtime_dirs(
    fs: &dyn Filesystem,
    layout: &DirectoryLayout,
    user: &UserId,
) -> Result<(), CoreError> {
    for dir in [layout.roots().runtime(), &layout.instance_runs_dir()] {
        ensure_private_dir(fs, dir, user)?;
    }
    Ok(())
}

/// Creates the folder with mode 0700 when nothing is there (but never
/// its parent), then checks that it is a private folder of `user` with
/// [`check_private_dir`]: a directory, not a symlink, owned by `user`, with
/// no group or other access. A folder that fails the check is rejected,
/// never repaired, and running it again changes nothing.
///
/// # Errors
///
/// Returns [`CoreError::RootNotPrivate`] when the folder fails the check,
/// and [`CoreError::Platform`] when a port fails, for example because the
/// parent does not exist.
pub fn ensure_private_dir(
    fs: &dyn Filesystem,
    path: &Path,
    user: &UserId,
) -> Result<(), CoreError> {
    let info = match fs.symlink_metadata(path)? {
        Some(info) => info,
        None => {
            match fs.create_dir_private(path) {
                // Created by someone else in between: check it like any other.
                Ok(()) | Err(PlatformError::AlreadyExists { .. }) => {}
                Err(err) => return Err(err.into()),
            }
            fs.symlink_metadata(path)?
                .ok_or_else(|| PlatformError::NotFound {
                    path: path.to_path_buf(),
                })?
        }
    };
    check_private_dir(&info, user).map_err(|violation| CoreError::RootNotPrivate {
        path: path.to_path_buf(),
        reason: violation.reason(),
    })
}

/// Resolves every root and rejects two roots that are the same folder (by
/// path or by file identity) or nest in each other once resolved.
fn canonical_roots(
    fs: &dyn Filesystem,
    roots: &[(RootKind, &Path); 4],
) -> Result<[PathBuf; 4], CoreError> {
    let mut canonical: [PathBuf; 4] = Default::default();
    let mut identities = Vec::with_capacity(roots.len());
    for (slot, (_, root)) in canonical.iter_mut().zip(roots) {
        *slot = fs.canonicalize(root)?;
        let info = fs
            .symlink_metadata(slot)?
            .ok_or_else(|| PlatformError::NotFound { path: slot.clone() })?;
        identities.push(info.identity);
    }
    for (index, a) in canonical.iter().enumerate() {
        for (offset, b) in canonical[index + 1..].iter().enumerate() {
            let same_folder = identities[index] == identities[index + 1 + offset];
            if same_folder || a.starts_with(b) || b.starts_with(a) {
                return Err(CoreError::RootsOverlap {
                    a: a.clone(),
                    b: b.clone(),
                });
            }
        }
    }
    Ok(canonical)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs::Permissions;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use stackly_domain::InstanceId;
    use stackly_platform::RootSet;
    use stackly_testkit::{FakePlatform, Fault, FsOp, TestRoot};

    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }

    fn rejected(result: Result<PreparedLayout, CoreError>) -> (PathBuf, &'static str) {
        match result {
            Err(CoreError::RootNotPrivate { path, reason }) => (path, reason),
            other => panic!("expected RootNotPrivate, got {other:?}"),
        }
    }

    #[test]
    fn prepare_creates_every_root_and_standard_folder_with_mode_0700() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);

        let prepared = prepare_layout(&fake.platform()).unwrap();

        let layout = prepared.layout();
        let mut expected: Vec<PathBuf> = layout
            .owned_roots()
            .iter()
            .map(|root| root.to_path_buf())
            .collect();
        expected.extend(layout.standard_dirs());
        for dir in &expected {
            assert_eq!(mode(dir), 0o700, "{}", dir.display());
        }
        assert_eq!(fake.fs().mutations().len(), expected.len());
        assert_eq!(fake.fs().violations(), 0);
    }

    #[test]
    fn a_second_prepare_changes_nothing() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let first = prepare_layout(&fake.platform()).unwrap();
        let mutations = fake.fs().mutations().len();

        let second = prepare_layout(&fake.platform()).unwrap();

        assert_eq!(second, first);
        assert_eq!(fake.fs().mutations().len(), mutations);
    }

    #[test]
    fn prepared_layout_keeps_the_canonical_roots() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);

        let prepared = prepare_layout(&fake.platform()).unwrap();

        let roots = root.roots();
        for (kind, canonical) in prepared.canonical_roots() {
            assert_eq!(canonical, roots.get(kind).canonicalize().unwrap(), "{kind}");
        }
        assert_eq!(prepared.layout().roots(), &roots);
    }

    #[test]
    fn a_root_owned_by_another_user_is_rejected() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        prepare_layout(&fake.platform()).unwrap();
        fake.identity().set_current(UserId::Unix(u32::MAX - 1));

        let (path, reason) = rejected(prepare_layout(&fake.platform()));

        assert_eq!(path, root.roots().data());
        assert_eq!(reason, "owned by another user");
    }

    #[test]
    fn a_symlinked_root_is_rejected_and_its_target_is_not_touched() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let target = root.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, Permissions::from_mode(0o700)).unwrap();
        symlink(&target, root.roots().data()).unwrap();

        let (path, reason) = rejected(prepare_layout(&fake.platform()));

        assert_eq!(path, root.roots().data());
        assert_eq!(reason, "a symlink");
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn a_root_open_to_group_or_others_is_rejected_and_not_fixed() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let logs = root.roots().logs().to_path_buf();
        std::fs::create_dir(&logs).unwrap();
        std::fs::set_permissions(&logs, Permissions::from_mode(0o755)).unwrap();

        let (path, reason) = rejected(prepare_layout(&fake.platform()));

        assert_eq!(path, logs);
        assert_eq!(reason, "group or others have access");
        assert_eq!(mode(&logs), 0o755);
    }

    #[test]
    fn a_file_in_place_of_a_root_is_rejected() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        std::fs::write(root.roots().cache(), b"").unwrap();

        let (path, reason) = rejected(prepare_layout(&fake.platform()));

        assert_eq!(path, root.roots().cache());
        assert_eq!(reason, "not a directory");
    }

    #[test]
    fn a_symlinked_standard_folder_is_rejected() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let prepared = prepare_layout(&fake.platform()).unwrap();
        let instances = prepared.layout().instances_dir();
        let target = root.path().join("other");
        std::fs::create_dir(&target).unwrap();
        std::fs::remove_dir(&instances).unwrap();
        symlink(&target, &instances).unwrap();

        let (path, reason) = rejected(prepare_layout(&fake.platform()));

        assert_eq!(path, instances);
        assert_eq!(reason, "a symlink");
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn roots_that_overlap_once_resolved_are_rejected() {
        let root = TestRoot::new().unwrap();
        let data = root.path().join("d");
        symlink(&data, root.path().join("alias")).unwrap();
        let roots = RootSet::new(
            data.clone(),
            root.path().join("alias").join("logs"),
            root.path().join("c"),
            root.run_dir().join("r"),
        )
        .unwrap();
        let fake = FakePlatform::new(&root).with_roots(roots);

        let result = prepare_layout(&fake.platform());

        assert_eq!(
            result,
            Err(CoreError::RootsOverlap {
                a: data.clone(),
                b: data.join("logs"),
            })
        );
        assert_eq!(fake.fs().violations(), 0);
    }

    #[test]
    fn the_parent_of_a_root_is_never_created() {
        let root = TestRoot::new().unwrap();
        let missing = root.path().join("no-library");
        let roots = RootSet::new(
            missing.join("data"),
            root.path().join("logs"),
            root.path().join("cache"),
            root.run_dir().join("r"),
        )
        .unwrap();
        let fake = FakePlatform::new(&root).with_roots(roots);

        let result = prepare_layout(&fake.platform());

        assert!(
            matches!(
                result,
                Err(CoreError::Platform(PlatformError::NotFound { .. }))
            ),
            "{result:?}"
        );
        assert!(!missing.exists());
        assert!(fake.fs().mutations().is_empty());
    }

    #[test]
    fn a_failed_create_is_reported_as_a_platform_error() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        fake.fs()
            .fail_when(FsOp::CreateDirPrivate, "downloads", Fault::NoSpace);

        let result = prepare_layout(&fake.platform());

        assert!(
            matches!(
                result,
                Err(CoreError::Platform(PlatformError::NoSpace { .. }))
            ),
            "{result:?}"
        );
    }

    #[test]
    fn instance_run_dir_is_created_private_under_the_runtime_root() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let key = InstanceId::generate().short_key();

        let run_dir = ensure_instance_run_dir(&platform, &prepared, key).unwrap();

        assert_eq!(run_dir, prepared.layout().instance_run_dir(key));
        assert_eq!(mode(&run_dir), 0o700);
        let again = ensure_instance_run_dir(&platform, &prepared, key).unwrap();
        assert_eq!(again, run_dir);
    }

    #[test]
    fn instance_run_dir_recreates_a_runtime_root_the_os_emptied() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let runtime = prepared.layout().roots().runtime().to_path_buf();
        std::fs::remove_dir_all(&runtime).unwrap();

        let run_dir =
            ensure_instance_run_dir(&platform, &prepared, InstanceId::generate().short_key())
                .unwrap();

        for dir in [&runtime, &prepared.layout().instance_runs_dir(), &run_dir] {
            assert_eq!(mode(dir), 0o700, "{}", dir.display());
        }
        assert_eq!(fake.fs().violations(), 0);
    }

    #[test]
    fn runtime_root_recreates_a_runtime_root_the_os_emptied() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let runtime = prepared.layout().roots().runtime().to_path_buf();
        std::fs::remove_dir_all(&runtime).unwrap();
        let mutations = fake.fs().mutations().len();

        ensure_runtime_root(&platform, &prepared).unwrap();

        for dir in [&runtime, &prepared.layout().instance_runs_dir()] {
            assert_eq!(mode(dir), 0o700, "{}", dir.display());
        }
        assert_eq!(fake.fs().mutations().len(), mutations + 2);
        assert_eq!(fake.fs().violations(), 0);
    }

    #[test]
    fn runtime_root_that_exists_is_left_unchanged() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let mutations = fake.fs().mutations().len();

        ensure_runtime_root(&platform, &prepared).unwrap();
        ensure_runtime_root(&platform, &prepared).unwrap();

        assert_eq!(fake.fs().mutations().len(), mutations);
    }

    #[test]
    fn runtime_root_recreates_only_a_missing_runs_folder() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let runs = prepared.layout().instance_runs_dir();
        std::fs::remove_dir(&runs).unwrap();
        let mutations = fake.fs().mutations().len();

        ensure_runtime_root(&platform, &prepared).unwrap();

        assert_eq!(mode(&runs), 0o700);
        assert_eq!(fake.fs().mutations().len(), mutations + 1);
    }

    #[test]
    fn runtime_root_open_to_group_or_others_is_rejected_and_not_fixed() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let runtime = prepared.layout().roots().runtime().to_path_buf();
        std::fs::set_permissions(&runtime, Permissions::from_mode(0o755)).unwrap();

        let result = ensure_runtime_root(&platform, &prepared);

        assert_eq!(
            result,
            Err(CoreError::RootNotPrivate {
                path: runtime.clone(),
                reason: "group or others have access",
            })
        );
        assert_eq!(mode(&runtime), 0o755);
    }

    #[test]
    fn runtime_root_replaced_by_a_symlink_is_rejected_and_its_target_is_not_touched() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let runtime = prepared.layout().roots().runtime().to_path_buf();
        let target = root.run_dir().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(&runtime).unwrap();
        symlink(&target, &runtime).unwrap();

        let result = ensure_runtime_root(&platform, &prepared);

        assert_eq!(
            result,
            Err(CoreError::RootNotPrivate {
                path: runtime,
                reason: "a symlink",
            })
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn runtime_root_owned_by_another_user_is_rejected() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        fake.identity().set_current(UserId::Unix(u32::MAX - 1));

        let result = ensure_runtime_root(&platform, &prepared);

        assert_eq!(
            result,
            Err(CoreError::RootNotPrivate {
                path: prepared.layout().roots().runtime().to_path_buf(),
                reason: "owned by another user",
            })
        );
    }

    #[test]
    fn runtime_root_reports_a_failed_create_as_a_platform_error() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let runtime = prepared.layout().roots().runtime().to_path_buf();
        std::fs::remove_dir_all(&runtime).unwrap();
        let name = runtime.file_name().unwrap().to_string_lossy().into_owned();
        fake.fs()
            .fail_when(FsOp::CreateDirPrivate, &name, Fault::NoSpace);

        let result = ensure_runtime_root(&platform, &prepared);

        assert!(
            matches!(
                result,
                Err(CoreError::Platform(PlatformError::NoSpace { .. }))
            ),
            "{result:?}"
        );
        assert!(!runtime.exists());
    }

    #[test]
    fn instance_run_dir_rejects_an_open_runs_folder() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let runs = prepared.layout().instance_runs_dir();
        std::fs::set_permissions(&runs, Permissions::from_mode(0o755)).unwrap();

        let result =
            ensure_instance_run_dir(&platform, &prepared, InstanceId::generate().short_key());

        assert_eq!(
            result,
            Err(CoreError::RootNotPrivate {
                path: runs,
                reason: "group or others have access",
            })
        );
    }

    #[test]
    fn ensure_private_dir_creates_the_secret_folders_private_and_rejects_open_ones() {
        let root = TestRoot::new().unwrap();
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let user = platform.identity().current_user().unwrap();
        let layout = prepared.layout();
        let key = InstanceId::generate().short_key();
        ensure_instance_run_dir(&platform, &prepared, key).unwrap();

        for dir in [layout.keychain_dir(), layout.instance_secrets_dir(key)] {
            ensure_private_dir(platform.fs(), &dir, &user).unwrap();
            assert_eq!(mode(&dir), 0o700, "{}", dir.display());
            ensure_private_dir(platform.fs(), &dir, &user).unwrap();

            std::fs::set_permissions(&dir, Permissions::from_mode(0o750)).unwrap();
            let result = ensure_private_dir(platform.fs(), &dir, &user);
            assert_eq!(
                result,
                Err(CoreError::RootNotPrivate {
                    path: dir.clone(),
                    reason: "group or others have access",
                })
            );
            assert_eq!(mode(&dir), 0o750, "never repaired");
        }
    }
}
