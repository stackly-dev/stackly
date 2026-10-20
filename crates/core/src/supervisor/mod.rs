//! The process supervisor (spec 0010): the pure rules the daemon's
//! per-instance actors build on.
//!
//! - [`classify`] and [`identity_checks`] decide whether a recorded process
//!   still belongs to its instance; uncertainty is always
//!   [`Classification::Unproven`], never a signal.
//! - [`RestartHistory`] and [`backoff_delay`] hold the restart policy: 1
//!   second doubling with jitter, 5 respawns in a sliding 5 minute window,
//!   and a reset after 60 seconds of continuous `ready`.
//! - [`InstanceChanges`] and [`ActorCommitRetry`] are the commit rules: one
//!   `UpdateInstance` per mutation, and how an actor commit retries.
//! - [`PortProbe`] tells whether an endpoint's port is free, with the
//!   socket options the server uses.
//! - [`SupervisorConfig`] holds every constant, shrinkable in tests.

mod classify;
mod commit;
mod config;
mod ports;
mod restart;

use stackly_domain::ObservationDetail;

pub use classify::{
    Classification, GoneReason, UnprovenBlocker, classify, identity_checks, six_checks_passed,
};
pub use commit::{ActorCommitRetry, CommitRetry, InstanceChanges};
pub use config::{
    AUTO_PORT_ATTEMPTS, COMMIT_BUSY_FIRST_DELAY, COMMIT_BUSY_MAX_DELAY, COMMIT_RETRY_LIMIT,
    FAILURE_EXCERPT_LINE_CHARS, FAILURE_EXCERPT_LINES, HELPER_TERM_GRACE, HTTP_HEADER_MAX,
    LOG_LINE_HARD_MAX, LOG_LINE_MAX, READINESS_FAILURE_THRESHOLD, READINESS_REFRESH_INTERVAL,
    RECONCILE_START_CONCURRENCY, REGISTRY_RETRY_INTERVAL, RESTART_BASE_DELAY, RESTART_BUDGET,
    RESTART_HEALTHY_RESET, RESTART_MAX_DELAY, RESTART_WINDOW, SPOOL_BACKLOG_MAX,
    SPOOL_DRAIN_INTERVAL, SPOOL_READ_CHUNK, SupervisorConfig, TAIL_LINES, UNKNOWN_RECHECK_INTERVAL,
};
pub use ports::{BindOptions, BindOutcome, PortProbe, SocketPortProbe};
pub use restart::{JitterFactor, RestartDecision, RestartHistory, backoff_delay};

/// Longest [`ObservationDetail`], in characters.
const DETAIL_MAX_CHARS: usize = 256;

/// Cuts `text` to the first 256 characters and wraps it as an
/// [`ObservationDetail`], or `None` when it is empty. Text that may hold a
/// secret is redacted before this cut, never after.
fn observation_detail(text: &str) -> Option<ObservationDetail> {
    let cut: String = text.chars().take(DETAIL_MAX_CHARS).collect();
    cut.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_detail_is_cut_to_256_characters() {
        let long = "é".repeat(300);

        let detail = observation_detail(&long).unwrap();

        assert_eq!(detail.as_str().chars().count(), 256);
    }

    #[test]
    fn an_empty_text_has_no_detail() {
        assert_eq!(observation_detail(""), None);
    }
}
