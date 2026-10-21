use std::fmt;

/// A platform capability: something the OS adapter has to provide.
///
/// The names differ from the provider capabilities in the domain on
/// purpose. Each capability lists the feature that builds its trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum PlatformCapability {
    /// Per user directory roots ([`DirectoryResolver`](crate::DirectoryResolver)).
    Directories,
    /// Filesystem access ([`Filesystem`](crate::Filesystem)).
    Filesystem,
    /// The current time ([`Clock`](crate::Clock)).
    Clock,
    /// The current OS user ([`UserIdentity`](crate::UserIdentity)).
    UserIdentity,
    /// Starting and watching processes (feature 10).
    ProcessSupervision,
    /// Storing secrets (feature 11).
    SecretStore,
    /// The daemon's local IPC endpoint (feature 6).
    IpcEndpoint,
    /// Running the daemon as a login agent (feature 15).
    BackgroundAgent,
    /// CPU and memory figures per process (feature 28).
    ProcessMetrics,
    /// Downloading over HTTPS ([`Fetcher`](crate::Fetcher), feature 18).
    NetworkFetch,
    /// Reading and writing code signatures ([`CodeSigning`](crate::CodeSigning),
    /// feature 18).
    CodeSigning,
}

impl PlatformCapability {
    /// Every capability, in code order.
    pub const ALL: [Self; 11] = [
        Self::Directories,
        Self::Filesystem,
        Self::Clock,
        Self::UserIdentity,
        Self::ProcessSupervision,
        Self::SecretStore,
        Self::IpcEndpoint,
        Self::BackgroundAgent,
        Self::ProcessMetrics,
        Self::NetworkFetch,
        Self::CodeSigning,
    ];

    /// Returns the stable snake_case code, for example `user_identity`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Directories => "directories",
            Self::Filesystem => "filesystem",
            Self::Clock => "clock",
            Self::UserIdentity => "user_identity",
            Self::ProcessSupervision => "process_supervision",
            Self::SecretStore => "secret_store",
            Self::IpcEndpoint => "ipc_endpoint",
            Self::BackgroundAgent => "background_agent",
            Self::ProcessMetrics => "process_metrics",
            Self::NetworkFetch => "network_fetch",
            Self::CodeSigning => "code_signing",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Directories => 0,
            Self::Filesystem => 1,
            Self::Clock => 2,
            Self::UserIdentity => 3,
            Self::ProcessSupervision => 4,
            Self::SecretStore => 5,
            Self::IpcEndpoint => 6,
            Self::BackgroundAgent => 7,
            Self::ProcessMetrics => 8,
            Self::NetworkFetch => 9,
            Self::CodeSigning => 10,
        }
    }
}

impl fmt::Display for PlatformCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Whether an adapter provides a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityStatus {
    /// The adapter provides it.
    Supported,
    /// The adapter does not provide it, and says why.
    NotSupported {
        /// Why, for example `not built yet (feature 10)`.
        reason: &'static str,
    },
}

impl CapabilityStatus {
    /// Returns true for [`CapabilityStatus::Supported`].
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// The status of every [`PlatformCapability`] on one platform.
///
/// ```
/// use stackly_platform::{CapabilityStatus, PlatformCapabilities, PlatformCapability};
///
/// let caps = PlatformCapabilities::none("adapter not built")
///     .with(PlatformCapability::Clock, CapabilityStatus::Supported);
/// assert!(caps.status(PlatformCapability::Clock).is_supported());
/// assert!(!caps.status(PlatformCapability::Filesystem).is_supported());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlatformCapabilities {
    statuses: [CapabilityStatus; PlatformCapability::ALL.len()],
}

impl PlatformCapabilities {
    /// Marks every capability as not supported for the same reason.
    #[must_use]
    pub const fn none(reason: &'static str) -> Self {
        Self {
            statuses: [CapabilityStatus::NotSupported { reason }; PlatformCapability::ALL.len()],
        }
    }

    /// Returns a copy with one capability set to `status`.
    #[must_use]
    pub const fn with(mut self, capability: PlatformCapability, status: CapabilityStatus) -> Self {
        self.statuses[capability.index()] = status;
        self
    }

    /// Returns the status of one capability.
    #[must_use]
    pub const fn status(&self, capability: PlatformCapability) -> CapabilityStatus {
        self.statuses[capability.index()]
    }
}
