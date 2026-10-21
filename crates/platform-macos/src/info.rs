use stackly_domain::{Arch, Os, Target};
use stackly_platform::{CapabilityStatus, PlatformCapabilities, PlatformCapability, PlatformInfo};

/// Facts about a macOS host and which capabilities this adapter provides.
///
/// Each feature that builds a capability's trait switches it to supported.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacosPlatformInfo;

/// The capability statuses of this adapter today.
const CAPABILITIES: PlatformCapabilities = PlatformCapabilities::none("")
    .with(PlatformCapability::Directories, CapabilityStatus::Supported)
    .with(PlatformCapability::Filesystem, CapabilityStatus::Supported)
    .with(PlatformCapability::Clock, CapabilityStatus::Supported)
    .with(
        PlatformCapability::UserIdentity,
        CapabilityStatus::Supported,
    )
    .with(
        PlatformCapability::ProcessSupervision,
        CapabilityStatus::Supported,
    )
    .with(PlatformCapability::SecretStore, CapabilityStatus::Supported)
    // Feature 6 provides it without a platform trait: the transport lives
    // in `stackly-ipc`, peer credentials come through tokio, and the daemon
    // lock through std (spec 0006).
    .with(PlatformCapability::IpcEndpoint, CapabilityStatus::Supported)
    .with(PlatformCapability::BackgroundAgent, CapabilityStatus::Supported)
    .with(
        PlatformCapability::ProcessMetrics,
        not_built("not built yet (feature 28)"),
    );

const fn not_built(reason: &'static str) -> CapabilityStatus {
    CapabilityStatus::NotSupported { reason }
}

impl PlatformInfo for MacosPlatformInfo {
    fn target(&self) -> Target {
        let arch = if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else {
            Arch::Aarch64
        };
        Target::new(Os::Macos, arch)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        CAPABILITIES
    }
}
