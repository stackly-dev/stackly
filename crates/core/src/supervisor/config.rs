//! The constants of the process supervisor (spec 0010, *Konstanta* and
//! *Restart policy*), gathered in one [`SupervisorConfig`] that tests can
//! shrink.

use std::num::NonZeroU32;
use std::time::Duration;

use stackly_platform::HELPER_CAPTURE_MAX;

use crate::READINESS_MAX_AGE;

/// Pause between two routine readiness probes once the instance is ready.
// provisional: F28
pub const READINESS_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
/// Consecutive failed routine probes that turn `ready` into `not_ready`.
pub const READINESS_FAILURE_THRESHOLD: NonZeroU32 = NonZeroU32::MIN.saturating_add(1);
/// Time a helper has between `SIGTERM` and `SIGKILL` to its group.
pub const HELPER_TERM_GRACE: Duration = Duration::from_secs(5);
/// Pause between two checks of an `unknown` instance.
pub const UNKNOWN_RECHECK_INTERVAL: Duration = Duration::from_secs(30);
/// Times an actor commit reads the instance again after a revision
/// conflict before it gives up.
pub const COMMIT_RETRY_LIMIT: u32 = 5;
/// First pause after the registry answers `Busy` to an actor commit; it
/// doubles up to [`COMMIT_BUSY_MAX_DELAY`].
pub const COMMIT_BUSY_FIRST_DELAY: Duration = Duration::from_millis(100);
/// Longest pause between two actor commits the registry answered `Busy`.
pub const COMMIT_BUSY_MAX_DELAY: Duration = Duration::from_secs(5);
/// Pause between two actor commits while the registry is full or not
/// available.
pub const REGISTRY_RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// Instances reconciliation starts at the same time.
pub const RECONCILE_START_CONCURRENCY: usize = 4;
/// Pause between two reads of a running process's spools.
pub const SPOOL_DRAIN_INTERVAL: Duration = Duration::from_millis(500);
/// Most bytes one spool read takes.
pub const SPOOL_READ_CHUNK: usize = 1024 * 1024;
/// Longest log line a sink receives; a longer line is split into
/// `continued` pieces.
pub const LOG_LINE_MAX: usize = 16 * 1024;
/// Longest line the spool reader holds while it waits for its newline.
pub const LOG_LINE_HARD_MAX: usize = 64 * 1024;
/// Most unread bytes one stream of one instance may hold before the oldest
/// part is dropped with a truncation marker.
pub const SPOOL_BACKLOG_MAX: u64 = 256 * 1024 * 1024;
/// Log lines the tail sink keeps per stream per instance.
pub const TAIL_LINES: usize = 64;
/// Lines of a failure excerpt.
pub const FAILURE_EXCERPT_LINES: usize = 10;
/// Longest line of a failure excerpt, in characters.
pub const FAILURE_EXCERPT_LINE_CHARS: usize = 256;
/// Longest response head an HTTP probe reads before its status line ends.
pub const HTTP_HEADER_MAX: usize = 8 * 1024;
/// Candidate ports tried for an `Auto` endpoint before giving up.
pub const AUTO_PORT_ATTEMPTS: u32 = 16;
/// First pause before a respawn after a crash.
pub const RESTART_BASE_DELAY: Duration = Duration::from_secs(1);
/// Longest pause before a respawn. With a budget of 5 the longest pause is
/// 16 seconds, so this is a guard.
pub const RESTART_MAX_DELAY: Duration = Duration::from_secs(30);
/// Respawns allowed in one [`RESTART_WINDOW`].
pub const RESTART_BUDGET: u32 = 5;
/// The sliding window crashes are counted in, on the monotonic clock.
pub const RESTART_WINDOW: Duration = Duration::from_secs(5 * 60);
/// Continuous `ready` time after which the crash history is cleared.
pub const RESTART_HEALTHY_RESET: Duration = Duration::from_secs(60);

/// Every constant of the supervisor, with the production values of spec
/// 0010 as its [`Default`]. Tests change single fields:
///
/// ```
/// use std::time::Duration;
///
/// use stackly_core::supervisor::SupervisorConfig;
///
/// let mut config = SupervisorConfig::default();
/// config.helper_term_grace = Duration::from_millis(50);
/// assert_eq!(config.restart_budget, 5);
/// ```
///
/// The fields are the constants of the same name in this module. No
/// provider and no instance overrides them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SupervisorConfig {
    /// [`READINESS_REFRESH_INTERVAL`].
    pub readiness_refresh_interval: Duration,
    /// [`READINESS_FAILURE_THRESHOLD`].
    pub readiness_failure_threshold: NonZeroU32,
    /// [`READINESS_MAX_AGE`](crate::READINESS_MAX_AGE).
    pub readiness_max_age: Duration,
    /// [`HELPER_TERM_GRACE`].
    pub helper_term_grace: Duration,
    /// [`UNKNOWN_RECHECK_INTERVAL`].
    pub unknown_recheck_interval: Duration,
    /// [`COMMIT_RETRY_LIMIT`].
    pub commit_retry_limit: u32,
    /// [`COMMIT_BUSY_FIRST_DELAY`].
    pub commit_busy_first_delay: Duration,
    /// [`COMMIT_BUSY_MAX_DELAY`].
    pub commit_busy_max_delay: Duration,
    /// [`REGISTRY_RETRY_INTERVAL`].
    pub registry_retry_interval: Duration,
    /// [`RECONCILE_START_CONCURRENCY`].
    pub reconcile_start_concurrency: usize,
    /// [`SPOOL_DRAIN_INTERVAL`].
    pub spool_drain_interval: Duration,
    /// [`SPOOL_READ_CHUNK`].
    pub spool_read_chunk: usize,
    /// [`LOG_LINE_MAX`].
    pub log_line_max: usize,
    /// [`LOG_LINE_HARD_MAX`].
    pub log_line_hard_max: usize,
    /// [`SPOOL_BACKLOG_MAX`].
    pub spool_backlog_max: u64,
    /// [`TAIL_LINES`].
    pub tail_lines: usize,
    /// [`FAILURE_EXCERPT_LINES`].
    pub failure_excerpt_lines: usize,
    /// [`FAILURE_EXCERPT_LINE_CHARS`].
    pub failure_excerpt_line_chars: usize,
    /// [`HELPER_CAPTURE_MAX`](stackly_platform::HELPER_CAPTURE_MAX), the
    /// tail of each helper stream the platform keeps. The adapter owns the
    /// buffer, so this mirrors it for the readers in core.
    pub helper_capture_max: usize,
    /// [`HTTP_HEADER_MAX`].
    pub http_header_max: usize,
    /// [`AUTO_PORT_ATTEMPTS`].
    pub auto_port_attempts: u32,
    /// [`RESTART_BASE_DELAY`].
    pub restart_base_delay: Duration,
    /// [`RESTART_MAX_DELAY`].
    pub restart_max_delay: Duration,
    /// [`RESTART_BUDGET`].
    pub restart_budget: u32,
    /// [`RESTART_WINDOW`].
    pub restart_window: Duration,
    /// [`RESTART_HEALTHY_RESET`].
    pub restart_healthy_reset: Duration,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            readiness_refresh_interval: READINESS_REFRESH_INTERVAL,
            readiness_failure_threshold: READINESS_FAILURE_THRESHOLD,
            readiness_max_age: READINESS_MAX_AGE,
            helper_term_grace: HELPER_TERM_GRACE,
            unknown_recheck_interval: UNKNOWN_RECHECK_INTERVAL,
            commit_retry_limit: COMMIT_RETRY_LIMIT,
            commit_busy_first_delay: COMMIT_BUSY_FIRST_DELAY,
            commit_busy_max_delay: COMMIT_BUSY_MAX_DELAY,
            registry_retry_interval: REGISTRY_RETRY_INTERVAL,
            reconcile_start_concurrency: RECONCILE_START_CONCURRENCY,
            spool_drain_interval: SPOOL_DRAIN_INTERVAL,
            spool_read_chunk: SPOOL_READ_CHUNK,
            log_line_max: LOG_LINE_MAX,
            log_line_hard_max: LOG_LINE_HARD_MAX,
            spool_backlog_max: SPOOL_BACKLOG_MAX,
            tail_lines: TAIL_LINES,
            failure_excerpt_lines: FAILURE_EXCERPT_LINES,
            failure_excerpt_line_chars: FAILURE_EXCERPT_LINE_CHARS,
            helper_capture_max: HELPER_CAPTURE_MAX,
            http_header_max: HTTP_HEADER_MAX,
            auto_port_attempts: AUTO_PORT_ATTEMPTS,
            restart_base_delay: RESTART_BASE_DELAY,
            restart_max_delay: RESTART_MAX_DELAY,
            restart_budget: RESTART_BUDGET,
            restart_window: RESTART_WINDOW,
            restart_healthy_reset: RESTART_HEALTHY_RESET,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_holds_the_production_values_of_spec_0010() {
        let config = SupervisorConfig::default();

        assert_eq!(config.readiness_refresh_interval, Duration::from_secs(10));
        assert_eq!(config.readiness_failure_threshold.get(), 2);
        assert_eq!(config.readiness_max_age, Duration::from_secs(30));
        assert_eq!(config.helper_term_grace, Duration::from_secs(5));
        assert_eq!(config.unknown_recheck_interval, Duration::from_secs(30));
        assert_eq!(config.commit_retry_limit, 5);
        assert_eq!(config.commit_busy_first_delay, Duration::from_millis(100));
        assert_eq!(config.commit_busy_max_delay, Duration::from_secs(5));
        assert_eq!(config.registry_retry_interval, Duration::from_secs(5));
        assert_eq!(config.reconcile_start_concurrency, 4);
        assert_eq!(config.spool_drain_interval, Duration::from_millis(500));
        assert_eq!(config.spool_read_chunk, 1 << 20);
        assert_eq!(config.log_line_max, 16 << 10);
        assert_eq!(config.log_line_hard_max, 64 << 10);
        assert_eq!(config.spool_backlog_max, 256 << 20);
        assert_eq!(config.tail_lines, 64);
        assert_eq!(config.failure_excerpt_lines, 10);
        assert_eq!(config.failure_excerpt_line_chars, 256);
        assert_eq!(config.helper_capture_max, 64 << 10);
        assert_eq!(config.http_header_max, 8 << 10);
        assert_eq!(config.auto_port_attempts, 16);
        assert_eq!(config.restart_base_delay, Duration::from_secs(1));
        assert_eq!(config.restart_max_delay, Duration::from_secs(30));
        assert_eq!(config.restart_budget, 5);
        assert_eq!(config.restart_window, Duration::from_secs(300));
        assert_eq!(config.restart_healthy_reset, Duration::from_secs(60));
    }
}
