//! The process supervisor (spec 0010): the pure rules and the executor the
//! daemon's per-instance actors build on.
//!
//! - [`classify`] and [`identity_checks`] decide whether a recorded process
//!   still belongs to its instance; uncertainty is always
//!   [`Classification::Unproven`], never a signal.
//! - [`RestartHistory`] and [`backoff_delay`] hold the restart policy: 1
//!   second doubling with jitter, 5 respawns in a sliding 5 minute window,
//!   and a reset after 60 seconds of continuous `ready`. The actor applies
//!   it to a crash outside any operation: a timer holds the pause, then a
//!   system `start_instance` operation respawns the process
//!   ([`Supervisor::attach_operations`] gives the actors their engine).
//! - [`InstanceChanges`] and [`ActorCommitRetry`] are the commit rules: one
//!   `UpdateInstance` per mutation, and how an actor commit retries.
//! - [`PortProbe`] tells whether an endpoint's port is free, with the
//!   socket options the server uses.
//! - [`probe`] runs one attempt of a `tcp`, `http`, or `exec` probe and
//!   reports a [`ProbeOutcome`](probe::ProbeOutcome) with a redacted
//!   detail.
//! - [`SpoolDrain`] reads one spool stream into redacted [`LogLine`]s for a
//!   [`LogSink`]; [`SinkSet`] fans them out and [`TailSink`] keeps the tail
//!   for failure excerpts.
//! - [`Supervisor`] is the executor: [`Supervisor::execute_init`] runs an
//!   init plan, [`Supervisor::start_work`] the startup transaction (checks
//!   before the first side effect, a suspended spawn whose identity is
//!   recorded before it runs, readiness, and the restart policy across
//!   attempts), [`Supervisor::stop_work`] the safe stop, and
//!   [`Supervisor::execute_reload`] the reload. Each instance has an actor
//!   that holds its process, readiness, health, and
//!   [`InstanceStatus::status_seq`]; [`SupervisorEvents`] reports what a
//!   client can see change.
//! - [`SupervisorConfig`] holds every constant, shrinkable in tests.

mod actor;
mod classify;
pub mod codes;
mod commit;
mod config;
mod deps;
mod drain;
mod error;
mod exit;
mod helper;
mod http;
mod init;
mod lock;
mod persist;
mod ports;
mod prepare;
pub mod probe;
mod readiness;
mod reload;
mod respawn;
mod restart;
mod secrets;
mod sink;
mod spool;
mod startup;
mod status;
mod steps;
mod stop;

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
pub use deps::{NoSupervisorEvents, Supervisor, SupervisorDeps, SupervisorEvents};
pub use error::{EndpointFault, SupervisorError, UnavailableReason};
pub use init::InitRequest;
pub use ports::{BindOptions, BindOutcome, PortProbe, SocketPortProbe};
pub use reload::ReloadRequest;
pub use restart::{JitterFactor, RestartDecision, RestartHistory, backoff_delay};
pub use sink::{LogBatch, LogLine, LogSink, SinkOutcome, SinkSet, TailSink};
pub use spool::{DrainContext, DrainOutcome, SpoolDrain};
pub use startup::{StartRequest, StartTrigger};
pub use status::{InstanceStatus, ProcessStatus, RestartStatus};
pub use stop::StopRequest;

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
