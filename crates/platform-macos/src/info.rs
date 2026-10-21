use std::ffi::CString;
use std::ptr;

use stackly_domain::{Arch, Os, OsVersion, Target};
use stackly_platform::{
    CapabilityStatus, HostFacts, PlatformCapabilities, PlatformCapability, PlatformError,
    PlatformInfo,
};

/// Facts about a macOS host and which capabilities this adapter provides.
///
/// Each feature that builds a capability's trait switches it to supported.
/// The OS version and build are read once, when the value is made.
#[derive(Debug, Clone)]
pub struct MacosPlatformInfo {
    facts: HostFacts,
}

impl MacosPlatformInfo {
    /// Reads `kern.osproductversion` and `kern.osversion` with
    /// `sysctlbyname`.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::HostFactsUnavailable`] when the OS version
    /// cannot be read or is not a valid [`OsVersion`]. An unreadable build
    /// only leaves `os_build` empty.
    pub fn read() -> Result<Self, PlatformError> {
        let version =
            sysctl_text("kern.osproductversion").ok_or(PlatformError::HostFactsUnavailable {
                reason: "kern.osproductversion could not be read",
            })?;
        let os_version =
            version
                .parse::<OsVersion>()
                .map_err(|_| PlatformError::HostFactsUnavailable {
                    reason: "kern.osproductversion is not a valid OS version",
                })?;
        let os_build = sysctl_text("kern.osversion")
            .filter(|build| !build.is_empty() && build.bytes().all(|b| b.is_ascii_alphanumeric()));
        Ok(Self {
            facts: HostFacts::new(host_target(), os_version, os_build),
        })
    }
}

fn host_target() -> Target {
    let arch = if cfg!(target_arch = "x86_64") {
        Arch::X86_64
    } else {
        Arch::Aarch64
    };
    Target::new(Os::Macos, arch)
}

/// Reads one string `sysctl` value, or `None` when it cannot be read.
fn sysctl_text(name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut len: libc::size_t = 0;
    // SAFETY: `name` is NUL terminated and lives through the call; with a
    // null buffer sysctlbyname only writes the needed length into `len`.
    let sized = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            ptr::null_mut(),
            &raw mut len,
            ptr::null_mut(),
            0,
        )
    };
    if sized != 0 || len == 0 || len > 256 {
        return None;
    }
    let mut buffer = vec![0_u8; len];
    // SAFETY: `buffer` is writable for `len` bytes, which `len` states, and
    // sysctlbyname writes at most that many and updates `len`.
    let read = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &raw mut len,
            ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    buffer.truncate(len);
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    String::from_utf8(buffer).ok()
}

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
    )
    .with(PlatformCapability::NetworkFetch, CapabilityStatus::Supported)
    .with(PlatformCapability::CodeSigning, CapabilityStatus::Supported);

const fn not_built(reason: &'static str) -> CapabilityStatus {
    CapabilityStatus::NotSupported { reason }
}

impl PlatformInfo for MacosPlatformInfo {
    fn target(&self) -> Target {
        self.facts.target
    }

    fn capabilities(&self) -> PlatformCapabilities {
        CAPABILITIES
    }

    fn host_facts(&self) -> HostFacts {
        self.facts.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_mac_reports_a_version_and_a_build() {
        let info = MacosPlatformInfo::read().expect("a Mac reports its version");
        let facts = info.host_facts();

        assert_eq!(facts.target.os(), Os::Macos);
        assert!(facts.os_version.major() >= 11, "{}", facts.os_version);
        assert!(facts.os_build.is_some());
        assert_eq!(info.target(), facts.target);
    }
}
