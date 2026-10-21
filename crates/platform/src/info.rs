use stackly_domain::{Arch, Os, OsVersion, Target};

use crate::capability::PlatformCapabilities;

/// What the host says about itself, read once when the platform is made
/// (spec 0004, 0018): the target, the OS version a binary may be asked to
/// run on, and the OS build.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HostFacts {
    /// The OS and architecture.
    pub target: Target,
    /// The OS version, such as `27.0`.
    pub os_version: OsVersion,
    /// The OS build, such as `26A428`, when the OS reports one.
    pub os_build: Option<String>,
}

impl HostFacts {
    /// Bundles the facts of one host.
    #[must_use]
    pub const fn new(target: Target, os_version: OsVersion, os_build: Option<String>) -> Self {
        Self {
            target,
            os_version,
            os_build,
        }
    }
}

/// Facts about the host platform.
pub trait PlatformInfo: Send + Sync {
    /// Returns the OS and architecture this process runs on.
    fn target(&self) -> Target;

    /// Returns which platform capabilities have a working adapter.
    fn capabilities(&self) -> PlatformCapabilities;

    /// Returns the facts read when the platform was made. It never reads the
    /// OS again and cannot fail: the adapter fails to build a platform when
    /// it cannot read them.
    fn host_facts(&self) -> HostFacts;
}

/// Returns the target this binary was built for, or `None` on an OS or
/// architecture Stackly does not model.
#[must_use]
pub fn host_target() -> Option<Target> {
    let os = std::env::consts::OS.parse::<Os>().ok()?;
    let arch = std::env::consts::ARCH.parse::<Arch>().ok()?;
    Some(Target::new(os, arch))
}
