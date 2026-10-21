use std::fmt;
use std::sync::Arc;

use crate::agent::{BackgroundAgent, UnsupportedBackgroundAgent};
use crate::clock::Clock;
use crate::fetch::{Fetcher, UnsupportedFetcher};
use crate::fs::Filesystem;
use crate::identity::UserIdentity;
use crate::info::PlatformInfo;
use crate::layout::DirectoryResolver;
use crate::process::{ProcessSupervision, UnsupportedProcessSupervision};
use crate::secret::{SecretStore, UnsupportedSecretStore};

/// Every platform port, bundled so a use case takes one argument.
///
/// The daemon assembles it once at start from an adapter, and tests from the
/// fakes in `stackly-testkit`. Cloning is cheap: each port is shared.
///
/// [`Platform::new`] takes the five ports of feature 4. A port added later
/// comes in through its own `with_<port>` builder and defaults to an
/// implementation that answers not supported, so the constructor never
/// changes (spec 0004, decision 18).
#[derive(Clone)]
#[non_exhaustive]
pub struct Platform {
    directories: Arc<dyn DirectoryResolver>,
    fs: Arc<dyn Filesystem>,
    clock: Arc<dyn Clock>,
    identity: Arc<dyn UserIdentity>,
    info: Arc<dyn PlatformInfo>,
    secrets: Arc<dyn SecretStore>,
    processes: Arc<dyn ProcessSupervision>,
    background_agent: Arc<dyn BackgroundAgent>,
    fetcher: Arc<dyn Fetcher>,
}

impl Platform {
    /// Bundles the five ports. The secret store starts as an
    /// [`UnsupportedSecretStore`] and process supervision as an
    /// [`UnsupportedProcessSupervision`], and the background agent an
    /// [`UnsupportedBackgroundAgent`], for the OS `info` reports; install
    /// real ones with [`with_secrets`](Self::with_secrets),
    /// [`with_processes`](Self::with_processes), and
    /// [`with_background_agent`](Self::with_background_agent).
    #[must_use]
    pub fn new(
        directories: Arc<dyn DirectoryResolver>,
        fs: Arc<dyn Filesystem>,
        clock: Arc<dyn Clock>,
        identity: Arc<dyn UserIdentity>,
        info: Arc<dyn PlatformInfo>,
    ) -> Self {
        let os = info.target().os();
        let secrets = Arc::new(UnsupportedSecretStore::new(os));
        let processes = Arc::new(UnsupportedProcessSupervision::new(os));
        let background_agent = Arc::new(UnsupportedBackgroundAgent::new(os));
        let fetcher = Arc::new(UnsupportedFetcher::new(os));
        Self {
            directories,
            fs,
            clock,
            identity,
            info,
            secrets,
            processes,
            background_agent,
            fetcher,
        }
    }

    /// Returns a copy with `secrets` as the secret store (spec 0011).
    #[must_use]
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.secrets = secrets;
        self
    }

    /// Returns a copy with `processes` as the process supervision port, the
    /// seventh port (spec 0010).
    #[must_use]
    pub fn with_processes(mut self, processes: Arc<dyn ProcessSupervision>) -> Self {
        self.processes = processes;
        self
    }

    /// Returns a copy with `agent` as the background agent port (spec 0015).
    #[must_use]
    pub fn with_background_agent(mut self, agent: Arc<dyn BackgroundAgent>) -> Self {
        self.background_agent = agent;
        self
    }

    /// Returns a copy with `fetcher` as the download port (spec 0018). A
    /// platform without one answers every download with not supported.
    #[must_use]
    pub fn with_fetcher(mut self, fetcher: Arc<dyn Fetcher>) -> Self {
        self.fetcher = fetcher;
        self
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

    /// Returns a shared handle to the clock, for a component that outlives
    /// the call that received the platform, such as the registry thread.
    #[must_use]
    pub fn shared_clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.clock)
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

    /// Returns the secret store.
    #[must_use]
    pub fn secrets(&self) -> &dyn SecretStore {
        &*self.secrets
    }

    /// Returns a shared handle to the secret store, for a component that
    /// wraps it, such as the worker that serializes every store call.
    #[must_use]
    pub fn shared_secrets(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&self.secrets)
    }

    /// Returns the process supervision port.
    #[must_use]
    pub fn processes(&self) -> &dyn ProcessSupervision {
        &*self.processes
    }

    /// Returns the background agent port.
    #[must_use]
    pub fn background_agent(&self) -> &dyn BackgroundAgent {
        &*self.background_agent
    }

    /// Returns the download port.
    #[must_use]
    pub fn fetcher(&self) -> &dyn Fetcher {
        &*self.fetcher
    }

    /// Returns a shared handle to the download port, for a download that
    /// runs on a blocking thread and outlives the call that started it.
    #[must_use]
    pub fn shared_fetcher(&self) -> Arc<dyn Fetcher> {
        Arc::clone(&self.fetcher)
    }
}

impl fmt::Debug for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Platform")
            .field("target", &self.info.target())
            .field("secrets", &self.secrets.backend())
            .finish_non_exhaustive()
    }
}
