use stackly_domain::{Arch, Os, Target};

use crate::capability::PlatformCapabilities;

/// Facts about the host platform.
pub trait PlatformInfo: Send + Sync {
    /// Returns the OS and architecture this process runs on.
    fn target(&self) -> Target;

    /// Returns which platform capabilities have a working adapter.
    fn capabilities(&self) -> PlatformCapabilities;
}

/// Returns the target this binary was built for, or `None` on an OS or
/// architecture Stackly does not model.
#[must_use]
pub fn host_target() -> Option<Target> {
    let os = std::env::consts::OS.parse::<Os>().ok()?;
    let arch = std::env::consts::ARCH.parse::<Arch>().ok()?;
    Some(Target::new(os, arch))
}
