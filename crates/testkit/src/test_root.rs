use std::io;
use std::path::{Path, PathBuf};

use stackly_platform::{FixedRoots, RootSet};

/// Prefix of every test root directory, so a leftover one is easy to spot.
const PREFIX: &str = "stackly-test-";
/// Prefix of every short run folder. Not `sk-`, because the spec 0003 spike
/// cleanup kills processes in and deletes `/tmp/sk-*`.
const RUN_PREFIX: &str = "skt-";

/// A fresh, empty directory that belongs to one test and is deleted when the
/// value is dropped, plus a short run folder for the test's runtime root.
///
/// Use it instead of `~/Library/Application Support/Stackly`. Cleanup removes
/// only these directories and what the test created inside them: a symlink
/// inside is removed, never followed. Both paths are canonical (on macOS the
/// temporary directory sits behind the `/var` to `/private/var` symlink), so
/// checks that a path lies under the root compare like with like.
///
/// The run folder lives in `/tmp` on Unix with mode 0700, because socket
/// paths must stay under 104 bytes and the system temporary directory is
/// already long.
///
/// ```
/// use stackly_testkit::TestRoot;
///
/// let root = TestRoot::new()?;
/// std::fs::write(root.path().join("registry.db"), b"")?;
/// let path = root.path().to_path_buf();
/// drop(root);
/// assert!(!path.exists());
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Debug)]
pub struct TestRoot {
    dir: tempfile::TempDir,
    run: tempfile::TempDir,
}

impl TestRoot {
    /// Creates a new test root under the system temporary directory and a
    /// run folder under `/tmp`.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when a temporary directory cannot be resolved
    /// or a folder cannot be created in it.
    pub fn new() -> io::Result<Self> {
        let base = std::env::temp_dir().canonicalize()?;
        let dir = tempfile::Builder::new().prefix(PREFIX).tempdir_in(base)?;
        let run = private_dir_builder()
            .prefix(RUN_PREFIX)
            .tempdir_in(run_base()?)?;
        Ok(Self { dir, run })
    }

    /// Returns the canonical path of this test root.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Returns the canonical path of the short run folder that holds this
    /// test's runtime root.
    #[must_use]
    pub fn run_dir(&self) -> &Path {
        self.run.path()
    }

    /// Returns the root set for this test: `data`, `logs`, and `cache` under
    /// [`path`](Self::path), and the runtime root under
    /// [`run_dir`](Self::run_dir). It comes from
    /// [`RootSet::under_override`], the same function `STACKLY_ROOT` uses.
    /// Nothing is created until a use case prepares the layout.
    #[must_use]
    pub fn roots(&self) -> RootSet {
        RootSet::under_override(self.path(), self.run_dir())
            .expect("a test root is absolute and its run folder is short, so its root set is valid")
    }

    /// Returns a directory resolver that hands out [`roots`](Self::roots).
    #[must_use]
    pub fn resolver(&self) -> FixedRoots {
        FixedRoots::new(self.roots())
    }
}

#[cfg(unix)]
fn private_dir_builder() -> tempfile::Builder<'static, 'static> {
    use std::os::unix::fs::PermissionsExt;

    let mut builder = tempfile::Builder::new();
    builder.permissions(std::fs::Permissions::from_mode(0o700));
    builder
}

#[cfg(not(unix))]
fn private_dir_builder() -> tempfile::Builder<'static, 'static> {
    tempfile::Builder::new()
}

#[cfg(unix)]
fn run_base() -> io::Result<PathBuf> {
    Path::new("/tmp").canonicalize()
}

#[cfg(not(unix))]
fn run_base() -> io::Result<PathBuf> {
    std::env::temp_dir().canonicalize()
}

#[cfg(test)]
mod tests {
    use stackly_platform::{DirectoryResolver, RUNTIME_ROOT_MAX};

    use super::*;

    #[test]
    fn each_test_root_is_a_new_empty_directory() {
        let first = TestRoot::new().unwrap();
        let second = TestRoot::new().unwrap();

        assert_ne!(first.path(), second.path());
        assert_ne!(first.run_dir(), second.run_dir());
        assert_eq!(std::fs::read_dir(first.path()).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(first.run_dir()).unwrap().count(), 0);
    }

    #[test]
    fn test_root_and_run_folder_are_canonical() {
        let root = TestRoot::new().unwrap();

        assert_eq!(root.path().canonicalize().unwrap(), root.path());
        assert_eq!(root.run_dir().canonicalize().unwrap(), root.run_dir());
    }

    #[cfg(unix)]
    #[test]
    fn run_folder_is_private_and_short() {
        use std::os::unix::fs::PermissionsExt;

        let root = TestRoot::new().unwrap();
        let mode = std::fs::metadata(root.run_dir())
            .unwrap()
            .permissions()
            .mode();

        assert_eq!(mode & 0o777, 0o700);
        assert!(root.run_dir().starts_with("/private/tmp") || root.run_dir().starts_with("/tmp"));
        assert!(
            root.run_dir()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("skt-")
        );
        let runtime_len = root.roots().runtime().as_os_str().len();
        assert!(runtime_len <= RUNTIME_ROOT_MAX, "{runtime_len}");
    }

    #[test]
    fn roots_and_resolver_point_inside_the_test_root() {
        let root = TestRoot::new().unwrap();
        let roots = root.resolver().roots().unwrap();

        assert_eq!(roots, root.roots());
        assert_eq!(roots.data(), root.path().join("data"));
        assert_eq!(roots.logs(), root.path().join("logs"));
        assert_eq!(roots.cache(), root.path().join("cache"));
        assert_eq!(roots.runtime().parent().unwrap(), root.run_dir());
    }

    #[cfg(unix)]
    #[test]
    fn dropping_the_root_removes_a_symlink_but_not_its_target() {
        let outside = TestRoot::new().unwrap();
        let kept = outside.path().join("kept.txt");
        std::fs::write(&kept, b"outside the test root").unwrap();

        let root = TestRoot::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.run_dir().join("link")).unwrap();
        let root_path = root.path().to_path_buf();
        let run_path = root.run_dir().to_path_buf();
        drop(root);

        assert!(!root_path.exists());
        assert!(!run_path.exists());
        assert!(kept.exists());
    }
}
