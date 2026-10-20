//! The limits of the operation engine (spec 0007, "Batas"). Benchmarks of
//! feature 64 may tune them through an update of the spec.

use std::time::Duration;

use stackly_domain::{CreatedRecord, CreatedSeq, JournalStep, OperationJournal, StepSeq};

/// The most operations that run at once.
pub const MAX_RUNNING_OPERATIONS: u32 = 8;
/// The most operations that wait in the queue.
pub const MAX_QUEUED_OPERATIONS: u32 = 64;
/// The shortest time between two commits that change only progress.
pub const PROGRESS_COMMIT_INTERVAL: Duration = Duration::from_secs(1);
/// How long shutdown waits for running operations.
pub const OPERATION_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
/// How long the engine waits before it retries a journal write that failed.
pub const JOURNAL_RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// How often a final batch is rebuilt after a generation conflict.
pub const FINALIZE_RETRIES: u32 = 5;
/// The most steps one operation records.
pub const MAX_STEPS: u16 = StepSeq::MAX;
/// The most created items one operation records.
pub const MAX_CREATED: u16 = CreatedSeq::MAX;
/// The longest input of a journal, in bytes.
pub const INPUT_MAX: usize = OperationJournal::INPUT_MAX;
/// The longest result of a journal, in bytes.
pub const RESULT_MAX: usize = OperationJournal::RESULT_MAX;
/// The longest step checkpoint, in bytes.
pub const CHECKPOINT_MAX: usize = JournalStep::CHECKPOINT_MAX;
/// The longest custom item payload, in bytes.
pub const CUSTOM_PAYLOAD_MAX: usize = CreatedRecord::PAYLOAD_MAX;
/// The most bytes the stored form of a failure's items takes (the
/// `failure_items` column), measured as compact JSON.
pub const FAILURE_ITEMS_MAX_BYTES: usize = 65_536;

/// The limits one engine runs with. [`EngineLimits::default`] holds the
/// constants of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct EngineLimits {
    /// The most operations that run at once.
    pub max_running: u32,
    /// The most operations that wait in the queue.
    pub max_queued: u32,
    /// The shortest time between two progress only commits.
    pub progress_interval: Duration,
    /// How long to wait before retrying a failed journal write.
    pub journal_retry: Duration,
    /// How often a final batch is rebuilt after a generation conflict.
    pub finalize_retries: u32,
}

impl EngineLimits {
    /// Returns the limits with `max_running` and `max_queued` replaced, for
    /// tests of the queue.
    #[must_use]
    pub const fn with_slots(mut self, max_running: u32, max_queued: u32) -> Self {
        self.max_running = max_running;
        self.max_queued = max_queued;
        self
    }
}

impl EngineLimits {
    /// Returns the limits with the progress and journal retry intervals
    /// replaced, for tests that should not wait seconds.
    #[must_use]
    pub const fn with_intervals(mut self, progress: Duration, journal_retry: Duration) -> Self {
        self.progress_interval = progress;
        self.journal_retry = journal_retry;
        self
    }
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            max_running: MAX_RUNNING_OPERATIONS,
            max_queued: MAX_QUEUED_OPERATIONS,
            progress_interval: PROGRESS_COMMIT_INTERVAL,
            journal_retry: JOURNAL_RETRY_INTERVAL,
            finalize_retries: FINALIZE_RETRIES,
        }
    }
}
