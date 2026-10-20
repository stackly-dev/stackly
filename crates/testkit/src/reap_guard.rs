//! A guard that leaves no process and no runtime root behind a test
//! (spec 0014, *Suite ujung ke ujung*).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ProcessTable;

/// How long the guard waits for SIGTERM before it uses SIGKILL.
pub const REAP_GRACE: Duration = Duration::from_secs(5);
/// The name every runtime root of a root override starts with.
const RUNTIME_PREFIX: &str = "stackly-";

/// On drop, stops every process whose arguments contain the test root and
/// removes the runtime root of that root, also when the test panicked.
///
/// Declare the guard before the [`SpawnedDaemon`](crate::SpawnedDaemon), so
/// the daemon is stopped first and the guard then reaps the instance
/// processes it left running. The runtime root lies in the user's
/// temporary directory, outside the test root; only a folder named
/// `stackly-*` is ever removed.
#[derive(Debug)]
pub struct ReapGuard {
    root: PathBuf,
    runtime_root: PathBuf,
}

impl ReapGuard {
    /// Guards `root` (the canonical path of a [`TestRoot`](crate::TestRoot))
    /// and its `runtime_root` (from `MacosPlatform::new(Some(root))`).
    #[must_use]
    pub fn new(root: &Path, runtime_root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            runtime_root: runtime_root.to_path_buf(),
        }
    }

    /// Stops the processes under the root now.
    ///
    /// # Errors
    ///
    /// The errors of [`ProcessTable::reap_under`].
    pub fn reap(&self) -> std::io::Result<usize> {
        ProcessTable::reap_under(&self.root, REAP_GRACE)
    }
}

impl Drop for ReapGuard {
    fn drop(&mut self) {
        let reaped = self.reap();
        let named_like_a_runtime_root = self
            .runtime_root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(RUNTIME_PREFIX));
        if named_like_a_runtime_root && !self.runtime_root.starts_with(&self.root) {
            let _ = std::fs::remove_dir_all(&self.runtime_root);
        }
        if let Err(error) = reaped
            && !std::thread::panicking()
        {
            panic!("a process outlived the test: {error}");
        }
    }
}
