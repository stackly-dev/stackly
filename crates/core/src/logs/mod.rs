//! The durable log store of every instance (spec 0027): a sink that writes
//! the redacted lines of the spool readers as JSON Lines per stream, with
//! one `seq` per instance, rotation, markers for lost output and process
//! runs, and exact dedupe after a daemon crash; and the readers that page
//! and follow it from disk.
//!
//! - [`record`]: the record format of one line of a segment.
//! - [`level`]: the keyword rule that gives every line its [`LogLevel`].
//! - [`filter`]: which records a read keeps.
//! - [`store`]: one instance's folder, its header and segments, recovery,
//!   rotation, and dedupe.
//! - [`writer`]: the bounded write queue and the one writer thread.
//! - [`read`]: pages before, after, or at the newest record.
//! - [`follow`]: the reading side of `instance.logs_follow`.
//! - [`sink`]: [`DurableLogSink`], the [`LogSink`](crate::supervisor::LogSink)
//!   the daemon puts in its sink set.
//!
//! Only text the redactor already saw reaches the store; nothing here
//! redacts again or reads a spool.

use std::time::Duration;

pub mod filter;
pub mod follow;
pub mod level;
pub mod read;
pub mod record;
mod scan;
pub mod sink;
pub mod store;
pub mod writer;

pub use filter::{LogFilter, StreamSet};
pub use follow::{FollowSource, FollowStep};
pub use level::{LogLevel, level_of};
pub use read::{LogEntry, LogGap, LogPage, LogPosition, LogReadError, ReadRequest, read_page};
pub use record::{LineRecord, Record, RecordKind, RecordSource};
pub use sink::DurableLogSink;
pub use store::{LogCursor, LogStoreId, StoreFailure};
pub use writer::{DurableLog, LogHead, LogsHealth};

/// The size at which the active segment of a stream is closed (20 MiB).
pub const LOG_SEGMENT_MAX_BYTES: u64 = 20 * 1024 * 1024;

/// How many segments one stream keeps; the oldest is deleted on rotation.
pub const LOG_SEGMENTS_PER_STREAM: usize = 5;

/// The bytes of text one instance's write queue holds before the sink
/// answers `Busy` (1 MiB). An empty queue always takes one batch.
pub const LOG_WRITE_QUEUE_MAX: usize = 1024 * 1024;

/// The bytes of text all write queues hold together before the sink answers
/// `Busy` (16 MiB).
pub const LOG_WRITE_QUEUE_TOTAL: usize = 16 * 1024 * 1024;

/// How long an instance whose write failed waits before the writer tries
/// again.
pub const LOG_WRITE_RETRY: Duration = Duration::from_millis(500);

/// The bytes one read, or one wake of a follower, scans at most across both
/// streams (8 MiB).
pub const LOG_SCAN_BUDGET: u64 = 8 * 1024 * 1024;

/// The characters at the start of a line the level rule looks at.
pub const LOG_LEVEL_SCAN_CHARS: usize = 160;

/// The run events one instance's queue holds at most.
pub const LOG_RUN_EVENTS_MAX: usize = 64;

/// How long the `instance_logs` health stays `degraded` after the last
/// failed write.
pub const LOG_HEALTH_RECOVERY: Duration = Duration::from_secs(60);

/// The limits of the store and its writer. [`Default`] holds the constants
/// above; tests shrink them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DurableLogConfig {
    /// [`LOG_SEGMENT_MAX_BYTES`].
    pub segment_max_bytes: u64,
    /// [`LOG_SEGMENTS_PER_STREAM`].
    pub segments_per_stream: usize,
    /// [`LOG_WRITE_QUEUE_MAX`].
    pub queue_max: usize,
    /// [`LOG_WRITE_QUEUE_TOTAL`].
    pub queue_total: usize,
    /// [`LOG_WRITE_RETRY`].
    pub write_retry: Duration,
    /// [`LOG_RUN_EVENTS_MAX`].
    pub run_events_max: usize,
    /// [`LOG_HEALTH_RECOVERY`].
    pub health_recovery: Duration,
}

impl Default for DurableLogConfig {
    fn default() -> Self {
        Self {
            segment_max_bytes: LOG_SEGMENT_MAX_BYTES,
            segments_per_stream: LOG_SEGMENTS_PER_STREAM,
            queue_max: LOG_WRITE_QUEUE_MAX,
            queue_total: LOG_WRITE_QUEUE_TOTAL,
            write_retry: LOG_WRITE_RETRY,
            run_events_max: LOG_RUN_EVENTS_MAX,
            health_recovery: LOG_HEALTH_RECOVERY,
        }
    }
}
