use std::path::{Path, PathBuf};

use stackly_domain::ShortKey;
use stackly_platform::{
    DirectoryLayout, FileInfo, FileKind, Filesystem, Platform, PlatformError, RootKind, UserId,
};

use crate::error::CoreError;

/// The directory layout after [`prepare_layout`] checked it, with the
/// canonical form of each owned root for containment checks.
///
/// Socket paths keep using the roots as resolved (shorter); containment
/// checks use the canonical forms kept here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedLayout {
    layout: DirectoryLayout,
    canonical: [PathBuf; 4],
}

impl PreparedLayout {
    /// Returns the layout that derives every Stackly path.
    #[must_use]
    pub const fn layout(&self) -> &DirectoryLayout {
        &self.layout
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
    let layout = DirectoryLayout::new(roots);
    for dir in layout.standard_dirs() {
        ensure_private_dir(fs, &dir, &user)?;
    }
    Ok(PreparedLayout { layout, canonical })
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
    let run_dir = layout.instance_run_dir(key);
    for dir in [
        layout.roots().runtime(),
        &layout.instance_runs_dir(),
        &run_dir,
    ] {
        ensure_private_dir(fs, dir, &user)?;
    }
    Ok(run_dir)
}

/// Creates the folder when nothing is there, then checks that it is a
/// private folder of `user`.
fn ensure_private_dir(fs: &dyn Filesystem, path: &Path, user: &UserId) -> Result<(), CoreError> {
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
    check_private(path, &info, user)
}

fn check_private(path: &Path, info: &FileInfo, user: &UserId) -> Result<(), CoreError> {
    let reason = match info.kind {
        FileKind::Symlink => "a symlink",
        FileKind::File | FileKind::Other => "not a directory",
        FileKind::Dir if info.owner.as_ref() != Some(user) => "owned by another user",
        FileKind::Dir if info.mode.is_none_or(|mode| mode & 0o077 != 0) => {
            "group or others have access"
        }
        FileKind::Dir => return Ok(()),
    };
    Err(CoreError::RootNotPrivate {
        path: path.to_path_buf(),
        reason,
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
}
