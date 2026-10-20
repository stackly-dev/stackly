use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{Arch, Os, Target};
use stackly_platform::{
    CapabilityStatus, Clock, DirectoryResolver, FixedRoots, Platform, PlatformCapabilities,
    PlatformCapability, PlatformError, PlatformInfo, RootSet, UserId, UserIdentity,
};

use crate::{
    FakeBackgroundAgent, FakeFilesystem, FakeProcessSupervision, FakeSecretStore, TestRoot,
};

/// Where [`FakeClock`] starts: 2026-01-01T00:00:00Z.
const FAKE_CLOCK_START: Timestamp = Timestamp::constant(1_767_225_600, 0);

/// A clock that only moves when the test moves it. Clones share the time.
#[derive(Debug, Clone)]
pub struct FakeClock {
    now: Arc<Mutex<Timestamp>>,
}

impl FakeClock {
    /// Creates a clock at 2026-01-01T00:00:00Z.
    #[must_use]
    pub fn new() -> Self {
        Self {
            now: Arc::new(Mutex::new(FAKE_CLOCK_START)),
        }
    }

    /// Sets the current time.
    pub fn set(&self, now: Timestamp) {
        *self.lock() = now;
    }

    /// Moves the clock forward (or back, with a negative duration).
    pub fn advance(&self, by: SignedDuration) {
        let mut now = self.lock();
        *now = now
            .checked_add(by)
            .expect("a fake clock moved by test sized durations stays in jiff's range");
    }

    fn lock(&self) -> MutexGuard<'_, Timestamp> {
        self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *self.lock()
    }
}

/// A user identity the test can switch, to act as another user.
#[derive(Debug)]
pub struct FakeIdentity {
    current: Mutex<UserId>,
}

impl FakeIdentity {
    /// Starts as the user that owns `root`'s folder, read from its
    /// metadata, so files the test creates belong to the current user.
    #[must_use]
    pub fn new(root: &TestRoot) -> Self {
        use std::os::unix::fs::MetadataExt;

        let uid = std::fs::metadata(root.path())
            .expect("a test root exists while the test holds it")
            .uid();
        Self {
            current: Mutex::new(UserId::Unix(uid)),
        }
    }

    /// Makes the platform report `user` as the current user, for example
    /// to see folders the test created as owned by someone else.
    pub fn set_current(&self, user: UserId) {
        *self.lock() = user;
    }

    fn lock(&self) -> MutexGuard<'_, UserId> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl UserIdentity for FakeIdentity {
    fn current_user(&self) -> Result<UserId, PlatformError> {
        Ok(self.lock().clone())
    }
}

/// Platform facts the test controls: `macos-aarch64` with the capability
/// statuses of `MacosPlatform` by default, except that process supervision
/// and the background agent are supported, because [`FakePlatform`] installs
/// a working fake of each (`MacosPlatform` turns them on with spec 0010
/// build step 3 and spec 0015 build step 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakePlatformInfo {
    target: Target,
    capabilities: PlatformCapabilities,
}

impl FakePlatformInfo {
    /// Creates the default facts.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            target: Target::new(Os::Macos, Arch::Aarch64),
            capabilities: PlatformCapabilities::none("")
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
                .with(
                    PlatformCapability::IpcEndpoint,
                    not_built("not built yet (feature 6)"),
                )
                .with(
                    PlatformCapability::BackgroundAgent,
                    CapabilityStatus::Supported,
                )
                .with(
                    PlatformCapability::ProcessMetrics,
                    not_built("not built yet (feature 28)"),
                ),
        }
    }

    /// Returns a copy that reports `target`.
    #[must_use]
    pub const fn with_target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    /// Returns a copy with one capability overridden.
    #[must_use]
    pub const fn with_capability(
        mut self,
        capability: PlatformCapability,
        status: CapabilityStatus,
    ) -> Self {
        self.capabilities = self.capabilities.with(capability, status);
        self
    }
}

/// A capability `MacosPlatform` does not provide yet.
const fn not_built(reason: &'static str) -> CapabilityStatus {
    CapabilityStatus::NotSupported { reason }
}

impl Default for FakePlatformInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformInfo for FakePlatformInfo {
    fn target(&self) -> Target {
        self.target
    }

    fn capabilities(&self) -> PlatformCapabilities {
        self.capabilities
    }
}

/// Every fake port wired together for one [`TestRoot`].
///
/// The roots are the test root's [`roots`](TestRoot::roots), the filesystem
/// is a [`FakeFilesystem`] confined to the test folders, the secret store
/// is a [`FakeSecretStore`] in memory, process supervision is a
/// [`FakeProcessSupervision`] whose locks and spools stay in the test
/// folders, and the clock, identity, and platform facts are fakes the test
/// can drive through the handles.
///
/// ```
/// use stackly_testkit::{FakePlatform, TestRoot};
///
/// let root = TestRoot::new()?;
/// let fake = FakePlatform::new(&root);
/// let roots = fake.platform().directories().roots()?;
/// assert_eq!(roots.data(), root.path().join("data"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct FakePlatform {
    directories: Arc<FixedRoots>,
    fs: Arc<FakeFilesystem>,
    clock: Arc<FakeClock>,
    identity: Arc<FakeIdentity>,
    info: Arc<FakePlatformInfo>,
    secrets: FakeSecretStore,
    processes: FakeProcessSupervision,
    background_agent: FakeBackgroundAgent,
}

impl FakePlatform {
    /// Wires the fakes for `root`.
    #[must_use]
    pub fn new(root: &TestRoot) -> Self {
        Self {
            directories: Arc::new(root.resolver()),
            fs: Arc::new(FakeFilesystem::new(root)),
            clock: Arc::new(FakeClock::new()),
            identity: Arc::new(FakeIdentity::new(root)),
            info: Arc::new(FakePlatformInfo::new()),
            secrets: FakeSecretStore::new(),
            processes: FakeProcessSupervision::new(root),
            background_agent: FakeBackgroundAgent::new(),
        }
    }

    /// Returns a copy that resolves `roots` instead of the test root's.
    /// The filesystem stays confined to the test folders.
    #[must_use]
    pub fn with_roots(mut self, roots: RootSet) -> Self {
        self.directories = Arc::new(FixedRoots::new(roots));
        self
    }

    /// Returns a copy that reports `info`.
    #[must_use]
    pub fn with_info(mut self, info: FakePlatformInfo) -> Self {
        self.info = Arc::new(info);
        self
    }

    /// Returns the platform bundle to hand to a use case.
    #[must_use]
    pub fn platform(&self) -> Platform {
        let directories: Arc<dyn DirectoryResolver> = self.directories.clone();
        Platform::new(
            directories,
            self.fs.clone(),
            self.clock.clone(),
            self.identity.clone(),
            self.info.clone(),
        )
        .with_secrets(Arc::new(self.secrets.clone()))
        .with_processes(Arc::new(self.processes.clone()))
        .with_background_agent(Arc::new(self.background_agent.clone()))
    }

    /// Returns the fake filesystem, to inject faults or read its logs.
    #[must_use]
    pub fn fs(&self) -> &FakeFilesystem {
        &self.fs
    }

    /// Returns the fake clock.
    #[must_use]
    pub fn clock(&self) -> &FakeClock {
        &self.clock
    }

    /// Returns the fake identity.
    #[must_use]
    pub fn identity(&self) -> &FakeIdentity {
        &self.identity
    }

    /// Returns the fake secret store, to seed, lock, deny, hold, or fail
    /// it and to read its call log. Every platform this fake hands out
    /// shares it.
    #[must_use]
    pub fn secrets(&self) -> &FakeSecretStore {
        &self.secrets
    }

    /// Returns the fake process supervision, to start, change, and end its
    /// processes, inject failures, and read its call log. Every platform
    /// this fake hands out shares it.
    #[must_use]
    pub fn processes(&self) -> &FakeProcessSupervision {
        &self.processes
    }

    /// Returns the fake background agent, to script its registration and
    /// read its call log (spec 0015). Every platform this fake hands out
    /// shares it.
    #[must_use]
    pub fn background_agent(&self) -> &FakeBackgroundAgent {
        &self.background_agent
    }
}
