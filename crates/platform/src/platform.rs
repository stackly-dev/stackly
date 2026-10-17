use std::fmt;
use std::sync::Arc;

use crate::clock::Clock;
use crate::fs::Filesystem;
use crate::identity::UserIdentity;
use crate::info::PlatformInfo;
use crate::layout::DirectoryResolver;

/// Every platform port, bundled so a use case takes one argument.
///
/// The daemon assembles it once at start from an adapter, and tests from the
/// fakes in `stackly-testkit`. Cloning is cheap: each port is shared.
#[derive(Clone)]
#[non_exhaustive]
pub struct Platform {
    directories: Arc<dyn DirectoryResolver>,
    fs: Arc<dyn Filesystem>,
    clock: Arc<dyn Clock>,
    identity: Arc<dyn UserIdentity>,
    info: Arc<dyn PlatformInfo>,
}

impl Platform {
    /// Bundles the five ports.
    #[must_use]
    pub fn new(
        directories: Arc<dyn DirectoryResolver>,
        fs: Arc<dyn Filesystem>,
        clock: Arc<dyn Clock>,
        identity: Arc<dyn UserIdentity>,
        info: Arc<dyn PlatformInfo>,
    ) -> Self {
        Self {
            directories,
            fs,
            clock,
            identity,
            info,
        }
    }

    /// Returns the directory resolver.
    #[must_use]
    pub fn directories(&self) -> &dyn DirectoryResolver {
        &*self.directories
    }

    /// Returns the filesystem.
    #[must_use]
    pub fn fs(&self) -> &dyn Filesystem {
        &*self.fs
    }

    /// Returns the clock.
    #[must_use]
    pub fn clock(&self) -> &dyn Clock {
        &*self.clock
    }

    /// Returns the user identity.
    #[must_use]
    pub fn identity(&self) -> &dyn UserIdentity {
        &*self.identity
    }

    /// Returns the platform facts.
    #[must_use]
    pub fn info(&self) -> &dyn PlatformInfo {
        &*self.info
    }
}

impl fmt::Debug for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Platform")
            .field("target", &self.info.target())
            .finish_non_exhaustive()
    }
}
