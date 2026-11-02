//! The sampler task (spec 0028, *Sampler*).

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use stackly_platform::{Platform, UserId};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use super::read::{FactsCache, read_tick};
use super::{
    METRICS_HISTORY_CAPACITY, METRICS_PROCESS_ROWS, METRICS_SAMPLE_INTERVAL, MainProcesses,
    MetricsStore,
};

/// The constants of a [`Sampler`], shortened by tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SamplerConfig {
    /// The time between two ticks.
    pub interval: Duration,
    /// The samples a history keeps.
    pub history_capacity: NonZeroUsize,
    /// The process rows the newest sample keeps.
    pub process_rows: usize,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        Self {
            interval: METRICS_SAMPLE_INTERVAL,
            history_capacity: NonZeroUsize::new(METRICS_HISTORY_CAPACITY)
                .expect("the history capacity constant is not zero"),
            process_rows: METRICS_PROCESS_ROWS,
        }
    }
}

impl SamplerConfig {
    /// Returns a copy with another time between ticks.
    #[must_use]
    pub const fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Returns a copy with another history capacity.
    #[must_use]
    pub const fn with_history_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.history_capacity = capacity;
        self
    }

    /// Returns a copy with another row limit.
    #[must_use]
    pub const fn with_process_rows(mut self, rows: usize) -> Self {
        self.process_rows = rows;
        self
    }
}

/// Samples the resource use of every running instance.
///
/// One sampler serves the whole daemon. [`start`](Self::start) runs it on
/// its own task; tests call [`sample_once`](Self::sample_once) themselves.
pub struct Sampler {
    platform: Arc<Platform>,
    source: Arc<dyn MainProcesses>,
    store: MetricsStore,
    config: SamplerConfig,
    /// The daemon's user, read once; `None` when the platform cannot say,
    /// and then no tree has a descendant.
    uid: Option<UserId>,
    cache: FactsCache,
    /// The moment reading offsets count from.
    origin: Instant,
}

impl Sampler {
    /// Makes a sampler that writes the histories of the main processes
    /// `source` names into `store`. It reads the daemon's user from the
    /// platform's `UserIdentity` once, here.
    #[must_use]
    pub fn new(
        platform: Arc<Platform>,
        source: Arc<dyn MainProcesses>,
        store: MetricsStore,
        config: SamplerConfig,
    ) -> Self {
        let uid = match platform.identity().current_user() {
            Ok(uid) => Some(uid),
            Err(error) => {
                tracing::warn!(%error, "the metrics sampler cannot tell the daemon user, so process trees hold the main process only");
                None
            }
        };
        Self {
            platform,
            source,
            store,
            config,
            uid,
            cache: FactsCache::default(),
            origin: Instant::now(),
        }
    }

    /// Takes one sample of every running instance and records it.
    ///
    /// The OS is read in one `spawn_blocking`, with a monotonic instant
    /// taken right around each counters read. A history that has no running
    /// main process behind it goes, and a main process that changed starts a
    /// new history. A tick whose process list cannot be read records
    /// nothing.
    pub async fn sample_once(&mut self) {
        let mains = self.source.running();
        if mains.is_empty() {
            self.cache = FactsCache::default();
            self.store.clear();
            return;
        }
        let platform = Arc::clone(&self.platform);
        let uid = self.uid.clone();
        let origin = self.origin;
        let blocking_mains = mains.clone();
        let mut cache = std::mem::take(&mut self.cache);
        let joined = tokio::task::spawn_blocking(move || {
            let reads = read_tick(
                platform.processes(),
                uid.as_ref(),
                &blocking_mains,
                &mut cache,
                origin,
            );
            (cache, reads)
        })
        .await;
        let (cache, reads) = match joined {
            Ok(done) => done,
            Err(error) => {
                tracing::warn!(%error, "a metrics sampling pass did not finish");
                return;
            }
        };
        self.cache = cache;
        let reads = match reads {
            Ok(reads) => reads,
            Err(error) => {
                tracing::debug!(%error, "the process list could not be read, so this tick records nothing");
                return;
            }
        };
        self.store.record_tick(
            &mains,
            self.platform.clock().now(),
            &reads.instances,
            self.config.history_capacity,
            self.config.process_rows,
        );
    }

    /// Runs the sampler on its own task, one tick per
    /// [`interval`](SamplerConfig::interval). A tick that comes late is
    /// skipped, not caught up. Dropping the [`SamplerTask`] stops it.
    #[must_use]
    pub fn start(mut self) -> SamplerTask {
        let handle = tokio::spawn(async move {
            let mut ticks = tokio::time::interval(self.config.interval);
            ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                self.sample_once().await;
            }
        });
        SamplerTask { handle }
    }
}

/// A running [`Sampler`]. Dropping it, or calling [`stop`](Self::stop),
/// ends the task; the histories stay in the store until it is cleared.
#[derive(Debug)]
pub struct SamplerTask {
    handle: JoinHandle<()>,
}

impl SamplerTask {
    /// Ends the sampler task and clears nothing.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for SamplerTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
