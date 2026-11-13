//! The constants of Up (spec 0046, *Aturan graph* and *Operasi anak*).

use std::time::Duration;

/// The most `install_runtime` children one Up runs at once (AC-6).
pub const UP_INSTALL_CONCURRENCY: usize = 2;

/// The most starts one Up runs at once (AC-8).
pub const UP_START_CONCURRENCY: usize = 4;

/// The wait limit of an edge whose manifest wrote none, in seconds (AC-8).
pub const DEFAULT_DEPENDENCY_TIMEOUT_SECONDS: u32 = 600;

/// How long a service whose instance already runs may take to become
/// ready, in seconds (*Aturan graph* rule 3).
pub const SERVICE_SETTLE_TIMEOUT_SECONDS: u64 = 120;

/// How long a child `start_instance` may stay `queued` before its parent
/// cancels it, in seconds (*Operasi anak*).
pub const CHILD_QUEUE_TIMEOUT_SECONDS: u64 = 120;

/// How often a waiting service reads its instance again.
pub const UP_POLL_INTERVAL: Duration = Duration::from_millis(250);
