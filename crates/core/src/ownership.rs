use std::ffi::OsStr;
use std::path::{Path, PathBuf, is_separator};

use stackly_domain::{InvariantDetail, InvariantError};
use stackly_platform::{Platform, RootKind, is_same_or_inside};

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
    let canonical = resolve_normalized(platform, path)?;
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

/// Checks that a candidate data root shares no folder with an existing
/// data root: it is not the same folder, not inside one, and does not hold
/// one (BR-002).
///
/// Each path must be absolute with no `.` or `..` component. It is resolved
/// like [`check_owned_path`] does (the nearest existing ancestor resolved,
/// the missing rest appended), then compared as text and by file identity
/// (device and inode) along its ancestors, so a symlink alias, a different
/// letter case on APFS, or a different Unicode normalization of the same
/// folder is still caught.
///
/// # Errors
///
/// Returns [`CoreError::Invariant`] with
/// [`InvariantDetail::DataRootOverlap`] (BR-002) naming the candidate and
/// the first existing root it overlaps, both as given;
/// [`CoreError::NotNormalized`] for a relative or dotted path; and
/// [`CoreError::Platform`] when resolving fails.
pub fn check_data_root_overlap<I>(
    platform: &Platform,
    candidate: &Path,
    existing: I,
) -> Result<(), CoreError>
where
    I: IntoIterator,
    I::Item: AsRef<Path>,
{
    let fs = platform.fs();
    let resolved = resolve_normalized(platform, candidate)?;
    for root in existing {
        let root = root.as_ref();
        let resolved_root = resolve_normalized(platform, root)?;
        if is_same_or_inside(fs, &resolved, &resolved_root)?
            || is_same_or_inside(fs, &resolved_root, &resolved)?
        {
            return Err(InvariantError::new(InvariantDetail::DataRootOverlap {
                candidate: candidate.to_path_buf(),
                existing: root.to_path_buf(),
            })
            .into());
        }
    }
    Ok(())
}

/// Rejects a path that is not normalized, then resolves its existing part.
fn resolve_normalized(platform: &Platform, path: &Path) -> Result<PathBuf, CoreError> {
    if !is_normalized(path) {
        return Err(CoreError::NotNormalized {
            path: path.to_path_buf(),
        });
    }
    resolve_existing_prefix(platform, path)
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

    use stackly_domain::{InstanceId, Rule};
    use stackly_testkit::evidence::{Kind, evidence};
    use stackly_testkit::{FakePlatform, TestRoot};

    use super::*;
    use crate::create_instance_layout;
    use crate::prepare_layout;
    use crate::test_support::manifest_for;

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

    fn overlap(candidate: &Path, existing: &Path) -> Result<(), CoreError> {
        Err(InvariantError::new(InvariantDetail::DataRootOverlap {
            candidate: candidate.to_path_buf(),
            existing: existing.to_path_buf(),
        })
        .into())
    }

    /// True when `variant` reaches the existing folder `dir`, for example on
    /// a case insensitive APFS volume with a name that differs only in
    /// letter case.
    fn same_folder_on_this_volume(dir: &Path, variant: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;

        match (std::fs::metadata(dir), std::fs::metadata(variant)) {
            (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
            _ => false,
        }
    }

    /// On macOS, `variant` and a path inside it when `variant` reaches `dir`
    /// on this volume; nothing on other systems.
    fn case_variants(dir: &Path, variant: &Path) -> Vec<PathBuf> {
        if cfg!(target_os = "macos") && same_folder_on_this_volume(dir, variant) {
            vec![variant.to_path_buf(), variant.join("base")]
        } else {
            Vec::new()
        }
    }

    #[test]
    fn data_roots_that_are_equal_nested_or_aliased_are_rejected() {
        let ev = evidence!(
            covers = ["BR-002"],
            kind = Kind::Fixture,
            expected = "a data root equal to, inside, holding, or a symlink alias of an existing data root (and on macOS one that differs only in letter case or Unicode normalization) is rejected with DataRootOverlap (BR-002); siblings are accepted",
        );
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let id = InstanceId::generate();
        let existing =
            create_instance_layout(&platform, &prepared, &manifest_for(&platform, id), &[])
                .unwrap();
        let other = prepared.layout().instance_paths(InstanceId::generate());
        let alias = root.path().join("alias");
        symlink(&existing.root, &alias).unwrap();
        let instances_alias = root.path().join("instances-alias");
        symlink(prepared.layout().instances_dir(), &instances_alias).unwrap();
        let upper = prepared
            .layout()
            .instances_dir()
            .join(id.to_string().to_uppercase())
            .join("DATA");
        let mut rejected = vec![
            existing.data.clone(),
            existing.data.join("base"),
            existing.root.clone(),
            alias.join("data"),
            alias.join("data").join("base"),
            instances_alias.join(id.to_string()).join("data"),
        ];
        rejected.extend(case_variants(&existing.data, &upper));

        for candidate in &rejected {
            let result =
                check_data_root_overlap(&platform, candidate, [&other.data, &existing.data]);

            assert_eq!(
                result,
                overlap(candidate, &existing.data),
                "{}",
                candidate.display()
            );
            assert_eq!(result.unwrap_err().rule(), Some(Rule::Br002));
        }
        for accepted in [
            other.data.clone(),
            existing.root.join("data-other"),
            existing.config.clone(),
        ] {
            assert_eq!(
                check_data_root_overlap(&platform, &accepted, [&existing.data]),
                Ok(()),
                "{}",
                accepted.display()
            );
        }
        assert_eq!(fake.fs().violations(), 0);
        ev.pass();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn data_roots_that_differ_only_in_unicode_normalization_overlap() {
        let root = TestRoot::new().unwrap();
        let (fake, _) = prepared(&root);
        let composed = root.path().join("caf\u{e9}");
        std::fs::create_dir(&composed).unwrap();
        let decomposed = root.path().join("cafe\u{301}");
        if !same_folder_on_this_volume(&composed, &decomposed) {
            return;
        }

        let result =
            check_data_root_overlap(&fake.platform(), &decomposed.join("data"), [&composed]);

        assert_eq!(result, overlap(&decomposed.join("data"), &composed));
    }

    #[test]
    fn overlap_check_needs_normalized_paths() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let data = prepared.layout().instances_dir();

        for (candidate, existing) in [
            (PathBuf::from("relative"), data.clone()),
            (data.clone(), data.join("..").join("x")),
        ] {
            let result = check_data_root_overlap(&fake.platform(), &candidate, [&existing]);

            assert!(
                matches!(result, Err(CoreError::NotNormalized { .. })),
                "{result:?}"
            );
        }
        assert_eq!(
            check_data_root_overlap(&fake.platform(), &data, Vec::<PathBuf>::new()),
            Ok(())
        );
    }
}
