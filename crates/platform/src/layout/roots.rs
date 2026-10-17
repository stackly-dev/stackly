use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::error::PlatformError;

/// Longest Unix socket path, in bytes, that fits macOS `sun_path` (104
/// bytes including the trailing NUL). Linux allows 108, so macOS binds.
pub const SOCKET_PATH_MAX: usize = 103;
/// Longest socket file name a provider may use, in bytes. The longest today
/// is PostgreSQL's `.s.PGSQL.65535` (14 bytes).
pub const SOCKET_NAME_MAX: usize = 15;
/// Longest runtime root, in bytes, that still leaves room for
/// `/i/<short key>/<socket name>` under [`SOCKET_PATH_MAX`].
pub const RUNTIME_ROOT_MAX: usize = SOCKET_PATH_MAX
    - "/".len()
    - super::INSTANCE_RUNS_DIR.len()
    - "/".len()
    - SHORT_KEY_LEN
    - "/".len()
    - SOCKET_NAME_MAX;

/// Length of a short key, in bytes.
const SHORT_KEY_LEN: usize = 8;
/// FNV-1a 64 bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Resolves the per user roots Stackly owns.
pub trait DirectoryResolver: Send + Sync {
    /// Returns the four owned roots.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::HomeDirUnavailable`] or
    /// [`PlatformError::TempDirUnavailable`] when the OS cannot say where
    /// they live, [`PlatformError::RuntimeRootTooLong`] when socket paths
    /// would not fit, and [`PlatformError::NotSupported`] on an OS without
    /// an adapter.
    fn roots(&self) -> Result<RootSet, PlatformError>;
}

/// Names one of the four owned roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RootKind {
    /// Long lived data: registry, runtimes, instances, backups.
    Data,
    /// Daemon and instance logs.
    Logs,
    /// Downloads and other files that can be fetched again.
    Cache,
    /// Sockets and PID files; only files that can be made again.
    Runtime,
}

impl RootKind {
    /// Every root, in the order the layout prepares them.
    pub const ALL: [Self; 4] = [Self::Data, Self::Logs, Self::Cache, Self::Runtime];

    /// Returns the stable code, for example `data`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Logs => "logs",
            Self::Cache => "cache",
            Self::Runtime => "runtime",
        }
    }
}

impl fmt::Display for RootKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// The four roots Stackly owns for one user: `data`, `logs`, `cache`, and a
/// short `runtime` root for sockets.
///
/// A root set that exists is valid: every root is absolute with no `..`
/// component, no root equals or nests in another, and the runtime root is
/// at most [`RUNTIME_ROOT_MAX`] bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSet {
    data: PathBuf,
    logs: PathBuf,
    cache: PathBuf,
    runtime: PathBuf,
}

impl RootSet {
    /// Checks and bundles four roots.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::InvalidRoots`] when a root is relative,
    /// holds a `..` component, or equals or nests in another root, and
    /// [`PlatformError::RuntimeRootTooLong`] when the runtime root is longer
    /// than [`RUNTIME_ROOT_MAX`] bytes.
    pub fn new(
        data: PathBuf,
        logs: PathBuf,
        cache: PathBuf,
        runtime: PathBuf,
    ) -> Result<Self, PlatformError> {
        let roots = Self {
            data,
            logs,
            cache,
            runtime,
        };
        let entries = roots.entries();
        for (kind, path) in entries {
            if !path.is_absolute() {
                return Err(invalid(format!(
                    "{kind} root {} is not absolute",
                    path.display()
                )));
            }
            if path.components().any(|part| part == Component::ParentDir) {
                return Err(invalid(format!(
                    "{kind} root {} has a `..` component",
                    path.display()
                )));
            }
        }
        for (index, (a_kind, a)) in entries.iter().enumerate() {
            for (b_kind, b) in &entries[index + 1..] {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err(invalid(format!(
                        "{a_kind} root {} and {b_kind} root {} overlap",
                        a.display(),
                        b.display()
                    )));
                }
            }
        }
        let len = roots.runtime.as_os_str().as_encoded_bytes().len();
        if len > RUNTIME_ROOT_MAX {
            return Err(PlatformError::RuntimeRootTooLong {
                path: roots.runtime,
                len,
                max: RUNTIME_ROOT_MAX,
            });
        }
        Ok(roots)
    }

    /// Builds the root set for an alternative root `root` (canonical): the
    /// owned roots are `root/data`, `root/logs`, and `root/cache`, and the
    /// runtime root is `runtime_base/stackly-<fnv8>`, where `<fnv8>` is the
    /// lower 32 bits of the FNV-1a 64 bit hash of `root`'s bytes as 8
    /// lowercase hex characters. Two roots therefore never share sockets.
    ///
    /// `STACKLY_ROOT` and the test roots both go through this function.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`RootSet::new`].
    pub fn under_override(root: &Path, runtime_base: &Path) -> Result<Self, PlatformError> {
        let runtime = runtime_base.join(format!("stackly-{}", fnv8(root)));
        Self::new(
            root.join("data"),
            root.join("logs"),
            root.join("cache"),
            runtime,
        )
    }

    /// Returns the data root.
    #[must_use]
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// Returns the logs root.
    #[must_use]
    pub fn logs(&self) -> &Path {
        &self.logs
    }

    /// Returns the cache root.
    #[must_use]
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// Returns the runtime root, as resolved (not canonical, so socket
    /// paths stay short).
    #[must_use]
    pub fn runtime(&self) -> &Path {
        &self.runtime
    }

    /// Returns one root by kind.
    #[must_use]
    pub fn get(&self, kind: RootKind) -> &Path {
        match kind {
            RootKind::Data => &self.data,
            RootKind::Logs => &self.logs,
            RootKind::Cache => &self.cache,
            RootKind::Runtime => &self.runtime,
        }
    }

    /// Returns every root with its kind, in [`RootKind::ALL`] order.
    #[must_use]
    pub fn entries(&self) -> [(RootKind, &Path); 4] {
        RootKind::ALL.map(|kind| (kind, self.get(kind)))
    }
}

/// A [`DirectoryResolver`] that always returns the same root set, for an
/// adapter assembled with roots the caller chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedRoots(RootSet);

impl FixedRoots {
    /// Wraps a root set.
    #[must_use]
    pub const fn new(roots: RootSet) -> Self {
        Self(roots)
    }
}

impl DirectoryResolver for FixedRoots {
    fn roots(&self) -> Result<RootSet, PlatformError> {
        Ok(self.0.clone())
    }
}

fn invalid(reason: String) -> PlatformError {
    PlatformError::InvalidRoots { reason }
}

/// FNV-1a 64 bit hash of `bytes`.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

/// The lower 32 bits of the FNV-1a 64 bit hash of the path's bytes, as 8
/// lowercase hex characters.
fn fnv8(path: &Path) -> String {
    format!(
        "{:08x}",
        fnv1a_64(path.as_os_str().as_encoded_bytes()) & 0xffff_ffff
    )
}

#[cfg(test)]
mod tests {
    use stackly_domain::InstanceId;

    use super::*;
    use crate::layout::DirectoryLayout;

    fn abs(path: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"C:{path}"))
        } else {
            PathBuf::from(path)
        }
    }

    fn roots_with_runtime(runtime: PathBuf) -> Result<RootSet, PlatformError> {
        RootSet::new(abs("/r/data"), abs("/r/logs"), abs("/r/cache"), runtime)
    }

    /// A runtime root of exactly `len` bytes.
    fn runtime_of_len(len: usize) -> PathBuf {
        let base = abs("/");
        let base_len = base.as_os_str().len();
        base.join("x".repeat(len - base_len))
    }

    #[test]
    fn budget_constants_match_the_spec() {
        assert_eq!(SOCKET_PATH_MAX, 103);
        assert_eq!(SOCKET_NAME_MAX, 15);
        assert_eq!(RUNTIME_ROOT_MAX, 76);
    }

    #[test]
    fn runtime_root_of_76_bytes_is_accepted_and_77_is_rejected() {
        roots_with_runtime(runtime_of_len(76)).unwrap();

        let err = roots_with_runtime(runtime_of_len(77)).unwrap_err();
        assert!(
            matches!(
                err,
                PlatformError::RuntimeRootTooLong {
                    len: 77,
                    max: 76,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn socket_paths_fit_for_the_longest_runtime_root() {
        let layout = DirectoryLayout::new(roots_with_runtime(runtime_of_len(76)).unwrap());
        let longest_name = "x".repeat(SOCKET_NAME_MAX);

        assert!(layout.daemon_socket().as_os_str().len() <= SOCKET_PATH_MAX);
        for _ in 0..1000 {
            let run_dir = layout.instance_run_dir(InstanceId::generate().short_key());
            let socket = run_dir.join(&longest_name);
            assert!(
                socket.as_os_str().len() <= SOCKET_PATH_MAX,
                "{}",
                socket.display()
            );
        }
    }

    #[test]
    fn relative_roots_and_dot_dot_components_are_rejected() {
        let relative = RootSet::new(
            PathBuf::from("data"),
            abs("/r/logs"),
            abs("/r/cache"),
            abs("/run"),
        );
        let dot_dot = roots_with_runtime(abs("/run/../elsewhere"));

        assert!(
            matches!(relative, Err(PlatformError::InvalidRoots { .. })),
            "{relative:?}"
        );
        assert!(
            matches!(dot_dot, Err(PlatformError::InvalidRoots { .. })),
            "{dot_dot:?}"
        );
    }

    #[test]
    fn equal_or_nested_roots_are_rejected() {
        let equal = RootSet::new(abs("/r/data"), abs("/r/data"), abs("/r/cache"), abs("/run"));
        let nested = RootSet::new(
            abs("/r/data"),
            abs("/r/logs"),
            abs("/r/data/cache"),
            abs("/run"),
        );
        let parent = roots_with_runtime(abs("/r"));

        for result in [equal, nested, parent] {
            assert!(
                matches!(result, Err(PlatformError::InvalidRoots { .. })),
                "{result:?}"
            );
        }
    }

    #[test]
    fn similar_names_are_not_nested() {
        RootSet::new(
            abs("/r/data"),
            abs("/r/data-logs"),
            abs("/r/cache"),
            abs("/run"),
        )
        .unwrap();
    }

    #[test]
    fn fnv1a_of_a_matches_the_reference_value() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv8(Path::new("a")), "8601ec8c");
    }

    #[test]
    fn under_override_places_roots_under_the_override_and_runtime_under_the_base() {
        let roots = RootSet::under_override(&abs("/x/root"), &abs("/run")).unwrap();

        assert_eq!(roots.data(), abs("/x/root/data"));
        assert_eq!(roots.logs(), abs("/x/root/logs"));
        assert_eq!(roots.cache(), abs("/x/root/cache"));
        let expected = format!("stackly-{}", fnv8(&abs("/x/root")));
        assert_eq!(roots.runtime(), abs("/run").join(expected));
    }

    #[test]
    fn two_override_roots_get_different_runtime_roots() {
        let first = RootSet::under_override(&abs("/x/one"), &abs("/run")).unwrap();
        let second = RootSet::under_override(&abs("/x/two"), &abs("/run")).unwrap();

        assert_ne!(first.runtime(), second.runtime());
    }

    #[test]
    fn fixed_roots_always_return_the_same_set() {
        let roots = roots_with_runtime(abs("/run")).unwrap();
        let resolver = FixedRoots::new(roots.clone());

        assert_eq!(resolver.roots().unwrap(), roots);
        assert_eq!(resolver.roots().unwrap(), roots);
    }
}
