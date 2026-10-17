use std::ffi::OsStr;
use std::path::{Path, PathBuf, is_separator};

use stackly_platform::{Platform, RootKind};

use crate::error::CoreError;
use crate::layout::PreparedLayout;

/// A path checked to lie strictly inside one owned root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedPath {
    root: RootKind,
    canonical: PathBuf,
}

impl OwnedPath {
    /// Returns the owned root the path lies in.
    #[must_use]
    pub const fn root(&self) -> RootKind {
        self.root
    }

    /// Returns the canonical path: the existing part resolved, the missing
    /// rest appended as given.
    #[must_use]
    pub fn canonical(&self) -> &Path {
        &self.canonical
    }
}

/// Checks that a path a destructive operation is about to touch lies inside
/// an owned root, and is not the root itself.
///
/// The path must be absolute with no `.` or `..` component; that is checked
/// before any I/O. Then its nearest existing ancestor is resolved (so a
/// symlink on the way is followed) and the missing rest is appended, and the
/// result is compared with the canonical roots.
///
/// # Errors
///
/// Returns [`CoreError::NotNormalized`], [`CoreError::OutsideOwnedRoots`],
/// [`CoreError::IsOwnedRoot`], or [`CoreError::Platform`] when resolving
/// fails.
pub fn check_owned_path(
    platform: &Platform,
    prepared: &PreparedLayout,
    path: &Path,
) -> Result<OwnedPath, CoreError> {
    if !is_normalized(path) {
        return Err(CoreError::NotNormalized {
            path: path.to_path_buf(),
        });
    }
    let canonical = resolve_existing_prefix(platform, path)?;
    for (root, root_path) in prepared.canonical_roots() {
        if canonical == root_path {
            return Err(CoreError::IsOwnedRoot {
                path: path.to_path_buf(),
            });
        }
        if canonical.starts_with(root_path) {
            return Ok(OwnedPath { root, canonical });
        }
    }
    Err(CoreError::OutsideOwnedRoots {
        path: path.to_path_buf(),
    })
}

/// True for an absolute path with no `.` or `..` segment. Looks at the raw
/// bytes, because `Path::components` silently drops an inner `.`.
fn is_normalized(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .as_os_str()
            .as_encoded_bytes()
            .split(|byte| is_separator(char::from(*byte)))
            .any(|segment| segment == b"." || segment == b"..")
}

/// Canonicalizes the nearest existing ancestor of `path` and appends the
/// components that do not exist yet.
fn resolve_existing_prefix(platform: &Platform, path: &Path) -> Result<PathBuf, CoreError> {
    let fs = platform.fs();
    let mut missing: Vec<&OsStr> = Vec::new();
    let mut current = path;
    loop {
        if fs.symlink_metadata(current)?.is_some() {
            let mut canonical = fs.canonicalize(current)?;
            canonical.extend(missing.iter().rev());
            return Ok(canonical);
        }
        match (current.file_name(), current.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name);
                current = parent;
            }
            // Only the filesystem root has no parent, and it always exists.
            _ => {
                return Err(CoreError::OutsideOwnedRoots {
                    path: path.to_path_buf(),
                });
            }
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::symlink;

    use stackly_testkit::{FakePlatform, TestRoot};

    use super::*;
    use crate::prepare_layout;

    fn prepared(root: &TestRoot) -> (FakePlatform, PreparedLayout) {
        let fake = FakePlatform::new(root);
        let prepared = prepare_layout(&fake.platform()).unwrap();
        (fake, prepared)
    }

    #[test]
    fn relative_and_dotted_paths_are_rejected_before_any_io() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let data = prepared.layout().roots().data();
        let cases = [
            PathBuf::from("relative/path"),
            data.join(".").join("instances"),
            data.join("instances").join(".."),
            PathBuf::from("/outside/../anywhere"),
        ];

        for path in cases {
            let result = check_owned_path(&fake.platform(), &prepared, &path);
            assert_eq!(result, Err(CoreError::NotNormalized { path }));
        }
        assert_eq!(fake.fs().violations(), 0);
    }

    #[test]
    fn an_owned_root_itself_is_rejected() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);

        for owned in prepared.layout().owned_roots() {
            let result = check_owned_path(&fake.platform(), &prepared, owned);
            assert_eq!(
                result,
                Err(CoreError::IsOwnedRoot {
                    path: owned.to_path_buf()
                })
            );
        }
    }

    #[test]
    fn a_path_inside_a_root_is_accepted_even_before_it_exists() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let paths = prepared
            .layout()
            .instance_paths(stackly_domain::InstanceId::generate());

        let data = check_owned_path(&fake.platform(), &prepared, &paths.data).unwrap();
        let logs = check_owned_path(&fake.platform(), &prepared, &paths.logs).unwrap();
        let run = check_owned_path(&fake.platform(), &prepared, &paths.run).unwrap();

        assert_eq!(
            (data.root(), data.canonical()),
            (RootKind::Data, paths.data.as_path())
        );
        assert_eq!(logs.root(), RootKind::Logs);
        assert_eq!(run.root(), RootKind::Runtime);
    }

    #[test]
    fn a_path_outside_every_root_is_rejected() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let outside = root.path().join("elsewhere").join("file");

        let result = check_owned_path(&fake.platform(), &prepared, &outside);

        assert_eq!(result, Err(CoreError::OutsideOwnedRoots { path: outside }));
    }

    #[test]
    fn a_symlink_out_of_a_root_is_followed_and_rejected() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let escape = prepared.layout().instances_dir().join("escape");
        symlink(&elsewhere, &escape).unwrap();
        let path = escape.join("data");

        let result = check_owned_path(&fake.platform(), &prepared, &path);

        assert_eq!(result, Err(CoreError::OutsideOwnedRoots { path }));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_path_through_the_tmp_symlink_resolves_into_the_runtime_root() {
        use stackly_platform::RootSet;

        let root = TestRoot::new().unwrap();
        let short_run =
            Path::new("/tmp").join(root.run_dir().strip_prefix("/private/tmp").unwrap());
        let roots = RootSet::under_override(root.path(), &short_run).unwrap();
        let fake = FakePlatform::new(&root).with_roots(roots);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).unwrap();
        let run = prepared.layout().instance_runs_dir().join("0000beef");

        let owned = check_owned_path(&platform, &prepared, &run).unwrap();

        assert_eq!(owned.root(), RootKind::Runtime);
        assert!(owned.canonical().starts_with("/private/tmp"));
        assert!(run.starts_with("/tmp"));
    }
}
