use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use stackly_platform::{
    FixedRoots, Platform, PlatformError, RootSet, SystemClock, default_runtime_root,
    validate_root_override,
};

use crate::dirs::{CACHE_DIR, DATA_DIR, LOGS_DIR, darwin_user_temp_dir, home_dir};
use crate::fetch::MacosFetcher;
use crate::fs::MacosFilesystem;
use crate::identity::MacosIdentity;
use crate::info::MacosPlatformInfo;
use crate::process::MacosProcessSupervision;

/// Assembles the macOS [`Platform`].
///
/// | Root | Default | With `STACKLY_ROOT=R` |
/// | --- | --- | --- |
/// | data | `~/Library/Application Support/Stackly` | `R/data` |
/// | logs | `~/Library/Logs/Stackly` | `R/logs` |
/// | cache | `~/Library/Caches/Stackly` | `R/cache` |
/// | runtime | `<user temp>/stackly` | `<user temp>/stackly-<fnv8(R)>` |
///
/// `~` is the home directory from the user database and `<user temp>` the
/// private per user temporary directory, so `HOME` and `TMPDIR` play no
/// part.
#[derive(Debug, Clone, Copy)]
pub struct MacosPlatform;

impl MacosPlatform {
    /// Resolves the roots once and assembles the platform. `root_override`
    /// is the value of `STACKLY_ROOT` as the binary read it: `None` when the
    /// variable is unset, which selects the default roots.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::HomeDirUnavailable`] or
    /// [`PlatformError::TempDirUnavailable`] when the OS cannot say where
    /// the roots live, [`PlatformError::InvalidRootOverride`] when the
    /// override breaks a rule (an empty value included),
    /// [`PlatformError::RuntimeRootTooLong`] when socket paths would not
    /// fit, and the error of a filesystem call that failed while checking
    /// the override.
    #[expect(
        clippy::new_ret_no_self,
        reason = "spec 0004: an adapter's constructor hands back the assembled Platform"
    )]
    pub fn new(root_override: Option<OsString>) -> Result<Platform, PlatformError> {
        let home = home_dir()?;
        let temp = darwin_user_temp_dir()?;
        let default_data = home.join(DATA_DIR);
        let roots = match root_override {
            None => RootSet::new(
                default_data,
                home.join(LOGS_DIR),
                home.join(CACHE_DIR),
                default_runtime_root(&temp),
            )?,
            Some(root) => {
                let root = validate_root_override(
                    Path::new(&root),
                    &MacosFilesystem,
                    &MacosIdentity,
                    &default_data,
                )?;
                RootSet::under_override(&root, &temp)?
            }
        };
        Ok(Self::with_roots(roots))
    }

    /// Assembles the platform with roots the caller chose, without the
    /// default resolver, with [`MacosProcessSupervision`] as its process
    /// port, and with [`MacosFetcher`] as its download port. Integration tests use it with a test root, so no
    /// folder is left in the real user temporary directory.
    #[must_use]
    pub fn with_roots(roots: RootSet) -> Platform {
        Platform::new(
            Arc::new(FixedRoots::new(roots)),
            Arc::new(MacosFilesystem),
            Arc::new(SystemClock),
            Arc::new(MacosIdentity),
            Arc::new(MacosPlatformInfo),
        )
        .with_processes(Arc::new(MacosProcessSupervision::new()))
        .with_fetcher(Arc::new(MacosFetcher::new()))
    }
}
