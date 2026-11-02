//! The metrics sampler (spec 0028): one task for the whole daemon that
//! reads the CPU, memory, and Disk I/O of every running instance's main
//! process and process tree every [`METRICS_SAMPLE_INTERVAL`], and keeps the
//! last [`METRICS_HISTORY_CAPACITY`] samples of each in a [`MetricsStore`].
//!
//! - [`Sampler`] owns the tick. It asks a [`MainProcesses`] source which
//!   main processes run, reads the OS in one `spawn_blocking` per tick, and
//!   records the result with the pure rules of `stackly_domain::metrics`.
//! - [`MetricsStore`] is the shared view of the histories. The handler of
//!   `instance.get_metrics` reads it without waiting for a tick.
//!
//! Sampling never probes a provider, never signals a process, and never
//! writes the registry. A value that could not be read is unavailable with a
//! reason, never 0.

mod read;
mod sampler;
mod store;

use std::time::Duration;

use stackly_domain::{InstanceId, Pid, ProcessStart};

pub use sampler::{Sampler, SamplerConfig, SamplerTask};
pub use store::MetricsStore;

/// How often the sampler takes one sample of every running instance.
pub const METRICS_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// How many samples a history keeps; the 61st drops the oldest.
pub const METRICS_HISTORY_CAPACITY: usize = 60;

/// How many process rows the newest sample keeps.
pub const METRICS_PROCESS_ROWS: usize = 200;

/// The main process of one running instance, as the supervisor knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RunningMain {
    /// The instance.
    pub instance: InstanceId,
    /// The main process.
    pub pid: Pid,
    /// When it started, which with the PID is its identity.
    pub start: ProcessStart,
}

impl RunningMain {
    /// The main process of `instance`.
    #[must_use]
    pub const fn new(instance: InstanceId, pid: Pid, start: ProcessStart) -> Self {
        Self {
            instance,
            pid,
            start,
        }
    }
}

/// Tells the sampler which instances run, and as which process.
///
/// The daemon implements it from the registry (lifecycle `running`) and the
/// supervisor status (the verified main process). A closure that returns
/// the list does too.
pub trait MainProcesses: Send + Sync {
    /// Returns the main process of every instance that is `running` with a
    /// verified process, in no particular order.
    fn running(&self) -> Vec<RunningMain>;
}

impl<F> MainProcesses for F
where
    F: Fn() -> Vec<RunningMain> + Send + Sync,
{
    fn running(&self) -> Vec<RunningMain> {
        self()
    }
}
