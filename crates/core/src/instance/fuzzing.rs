//! Fuzz harness of the paths create writes (spec 0020, *Fuzz target*). The
//! cargo-fuzz target `instance_path` in `fuzz/` and the regression test
//! `instance_path_seeds_hold_the_invariants` call [`owned_path`]. It panics
//! when an invariant breaks, which is what the fuzzer reports as a crash.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use stackly_domain::InvariantDetail;
use stackly_platform::{FileIdentity, Platform};

use crate::{CoreError, PreparedLayout, check_data_root_overlap, check_owned_path};

/// How deep the `deep` fixture folder goes.
const DEEP_LEVELS: usize = 64;

/// The tree [`owned_path`] runs on: the data folders that
/// `check_data_root_overlap` compares a candidate with.
#[derive(Debug, Clone)]
pub struct OwnedPathFixtures {
    /// The folder the fixture tree is in, inside the data root.
    pub root: PathBuf,
    /// The data folders of the instances that are known.
    pub data_roots: Vec<PathBuf>,
}

/// Builds the fixture tree once, under `base`, which must be a folder inside
/// an owned root: a symlink to `outside`, a symlink to `other_root` (inside
/// another owned root), a loop of symlinks, a symlink to a folder inside the
/// tree, names with Unicode and spaces, a folder 64 levels deep, and two
/// data folders of instances.
///
/// # Errors
///
/// Returns the I/O error of the first folder or link that cannot be made.
pub fn fixture_tree(
    base: &Path,
    outside: &Path,
    other_root: &Path,
) -> std::io::Result<OwnedPathFixtures> {
    let root = base.join("fixture");
    std::fs::create_dir_all(root.join("inside"))?;
    std::fs::create_dir_all(root.join("ünï cödé/with space"))?;
    let mut deep = root.join("deep");
    for level in 0..DEEP_LEVELS {
        deep.push(format!("d{level}"));
    }
    std::fs::create_dir_all(&deep)?;
    symlink(outside, root.join("link-out"))?;
    symlink(other_root, root.join("link-other-root"))?;
    symlink(root.join("inside"), root.join("link-inside"))?;
    symlink("loop-b", root.join("loop-a"))?;
    symlink("loop-a", root.join("loop-b"))?;
    symlink(&root, root.join("link-self"))?;
    let data_roots = vec![
        root.join("instances/one/data"),
        root.join("instances/two/data"),
    ];
    for data in &data_roots {
        std::fs::create_dir_all(data)?;
    }
    Ok(OwnedPathFixtures { root, data_roots })
}

/// Runs [`check_owned_path`] and [`check_data_root_overlap`] on `input`
/// read as the bytes of a path, once as given and once joined to the
/// fixture root, and checks what they promise:
///
/// - nothing panics;
/// - a relative path, or one with a `.` or `..` segment, is
///   `NotNormalized`;
/// - an accepted path resolves strictly inside one owned root;
/// - `check_data_root_overlap` accepts a candidate only if neither it nor
///   the fixture data folder lies inside the other, and rejects it with
///   `DataRootOverlap` only if one does (by file identity along the
///   ancestors).
///
/// # Panics
///
/// Panics when an invariant breaks.
pub fn owned_path(
    platform: &Platform,
    prepared: &PreparedLayout,
    fixtures: &OwnedPathFixtures,
    input: &[u8],
) {
    let raw = Path::new(OsStr::from_bytes(input));
    check_one(platform, prepared, fixtures, raw);
    check_one(platform, prepared, fixtures, &fixtures.root.join(raw));
}

fn check_one(
    platform: &Platform,
    prepared: &PreparedLayout,
    fixtures: &OwnedPathFixtures,
    path: &Path,
) {
    let dotted = path
        .as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|segment| segment == b"." || segment == b"..");
    let must_not_be_normal = !path.is_absolute() || dotted;
    match check_owned_path(platform, prepared, path) {
        Err(CoreError::NotNormalized { .. }) => {}
        Err(_) => assert!(
            !must_not_be_normal,
            "{} is not normalized, yet another error came first",
            path.display()
        ),
        Ok(owned) => {
            assert!(
                !must_not_be_normal,
                "{} is not normalized, yet it was accepted",
                path.display()
            );
            let inside = prepared
                .canonical_roots()
                .into_iter()
                .any(|(_, root)| owned.canonical() != root && owned.canonical().starts_with(root));
            assert!(
                inside,
                "{} was accepted as {}, which is not strictly inside an owned root",
                path.display(),
                owned.canonical().display()
            );
        }
    }
    check_overlap(platform, fixtures, path);
}

fn check_overlap(platform: &Platform, fixtures: &OwnedPathFixtures, path: &Path) {
    let result = check_data_root_overlap(platform, path, &fixtures.data_roots);
    let fs = platform.fs();
    match result {
        Ok(()) => {
            let Some(candidate) = resolve(platform, path) else {
                return;
            };
            for root in &fixtures.data_roots {
                let Ok(root) = fs.canonicalize(root) else {
                    continue;
                };
                assert!(
                    !candidate.starts_with(&root) && !root.starts_with(&candidate),
                    "{} overlaps {} yet it was accepted",
                    path.display(),
                    root.display()
                );
            }
        }
        Err(CoreError::Invariant(error)) => {
            assert!(
                matches!(error.detail(), InvariantDetail::DataRootOverlap { .. }),
                "an overlap check fails with DataRootOverlap, not {:?}",
                error.detail()
            );
            assert!(
                related(platform, path, &fixtures.data_roots),
                "{} was reported as overlapping, yet no ancestor shares an identity with a data folder",
                path.display()
            );
        }
        Err(_) => {}
    }
}

/// `path` with its existing part resolved through the links on the way and
/// the missing rest appended, as the checks under test resolve it.
fn resolve(platform: &Platform, path: &Path) -> Option<PathBuf> {
    let existing = nearest_existing(path);
    let resolved = platform.fs().canonicalize(existing).ok()?;
    let rest = path.strip_prefix(existing).ok()?;
    Some(resolved.join(rest))
}

/// The nearest ancestor of `path` that exists, `path` itself first.
fn nearest_existing(path: &Path) -> &Path {
    path.ancestors()
        .find(|ancestor| std::fs::symlink_metadata(ancestor).is_ok())
        .unwrap_or_else(|| Path::new("/"))
}

/// Whether the file identity of a data folder is the identity of `path` or
/// of one of its ancestors, or the other way round, following links.
fn related(platform: &Platform, path: &Path, roots: &[PathBuf]) -> bool {
    let fs = platform.fs();
    let identity = |p: &Path| -> Option<FileIdentity> {
        fs.symlink_metadata(p)
            .ok()
            .flatten()
            .map(|info| info.identity)
    };
    let candidate = resolve(platform, path);
    roots.iter().any(|root| {
        let Ok(root) = fs.canonicalize(root) else {
            return false;
        };
        let Some(candidate) = &candidate else {
            return false;
        };
        let root_identity = identity(&root);
        let candidate_identity = identity(candidate);
        candidate
            .ancestors()
            .any(|ancestor| identity(ancestor).is_some() && identity(ancestor) == root_identity)
            || root.ancestors().any(|ancestor| {
                candidate_identity.is_some() && identity(ancestor) == candidate_identity
            })
    })
}
