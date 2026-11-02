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
//! - [`Supervisor::reconcile`] finds what became of every instance's process
//!   while no daemon watched: it gathers the evidence in a fixed order,
//!   applies the table of the spec, adopts the processes whose identity is
//!   proven, and leaves what it cannot prove `unknown`, never signalled.
//!   [`Supervisor::start_reconciled`] then runs what must run again, and
//!   [`Supervisor::shutdown`] stops the actors without a signal or a commit.
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
//! - [`HANDOVER_PROVEN`] lists the provider lines whose processes survive a
//!   daemon restart, and [`plan_entries`] sorts the instances into the
//!   `keep` and `stop_restore` of a planned restart (spec 0030).
//! - [`write_plan_guard`] writes a daemon restart plan in one guarded
//!   commit, refused while another plan is open or when its digest changed
//!   (spec 0030).
//! - [`wake::Wake`] says the Mac slept: the probe loops then read readiness
//!   and health as `waking`, probe at once, and give failures a grace
//!   (spec 0028 AC-9).
//! - [`SupervisorConfig`] holds every constant, shrinkable in tests.

mod actor;
mod adopt;
mod change;
mod change_guard;
mod classify;
pub mod codes;
mod commit;
mod config;
mod daemon_restart;
mod deps;
mod drain;
pub mod drift;
mod error;
mod evidence;
mod exit;
#[cfg(feature = "fixture")]
pub mod fixture;
mod force;
mod handover;
mod helper;
mod http;
mod init;
mod integrity;
mod lock;
mod login;
mod pending;
mod persist;
mod plan;
mod policy;
mod ports;
pub(crate) mod prepare;
pub mod probe;
mod readiness;
mod recheck;
mod reconcile;
mod reload;
mod respawn;
mod restart;
mod restart_op;
mod restore;
mod secrets;
mod session_end;
mod sink;
mod spool;
mod startup;
mod status;
mod steps;
mod stop;
mod stop_all;
pub mod wake;

use stackly_domain::ObservationDetail;

pub use change::{ChangeRequest, RenameRequest};
pub use classify::{
    Classification, GoneReason, UnprovenBlocker, classify, identity_checks, six_checks_passed,
};
pub use commit::{ActorCommitRetry, CommitRetry, InstanceChanges};
pub use config::{
    AUTO_PORT_ATTEMPTS, COMMIT_BUSY_FIRST_DELAY, COMMIT_BUSY_MAX_DELAY, COMMIT_RETRY_LIMIT,
    FAILURE_EXCERPT_LINE_CHARS, FAILURE_EXCERPT_LINES, FORCE_EXIT_WAIT, HELPER_TERM_GRACE,
    HTTP_HEADER_MAX, LOG_LINE_HARD_MAX, LOG_LINE_MAX, READINESS_FAILURE_THRESHOLD,
    READINESS_REFRESH_INTERVAL, RECONCILE_START_CONCURRENCY, REGISTRY_RETRY_INTERVAL,
    RESTART_BASE_DELAY, RESTART_BUDGET, RESTART_HEALTHY_RESET, RESTART_LOCK_POLL,
    RESTART_LOCK_WAIT, RESTART_MAX_DELAY, RESTART_QUIESCE_WAIT, RESTART_WINDOW,
    SESSION_END_STOP_BUDGET, SPOOL_BACKLOG_MAX, SPOOL_DRAIN_INTERVAL, SPOOL_READ_CHUNK,
    SupervisorConfig, TAIL_LINES, UNKNOWN_RECHECK_INTERVAL,
};
pub use daemon_restart::{DaemonRestartRequest, PlanPrepared, RestartHandover};
pub use deps::{NoSupervisorEvents, Supervisor, SupervisorDeps, SupervisorEvents, WeakSupervisor};
pub use error::{EndpointFault, SupervisorError, UnavailableReason};
pub use force::ForceStopRequest;
pub use handover::{
    HANDOVER_PROVEN, HandoverProof, PlanEntry, is_handover_proven, plan_digest, plan_entries,
    revision_line,
};
pub use init::InitRequest;
pub use integrity::{FindingKind, ReconcileFinding};
pub use login::{LOGIN_ELIGIBLE_FAILURES, StartQueue};
pub use plan::{PlanRefusal, write_plan_guard};
pub use policy::SetStartupRequest;
pub use ports::{BindOptions, BindOutcome, PortProbe, SocketPortProbe};
pub use reconcile::{ReconcilePhase, ReconcileReport, RegistryMode};
pub use reload::ReloadRequest;
pub use restart::{JitterFactor, RestartDecision, RestartHistory, backoff_delay};
pub use restart_op::RestartRequest;
pub use session_end::SessionEndReport;
pub use sink::{LogBatch, LogLine, LogSink, RunEvent, SinkOutcome, SinkSet, SpoolSource, TailSink};
pub use spool::{DrainContext, DrainOutcome, SpoolDrain};
pub use startup::{StartRequest, StartTrigger};
pub use status::{InstanceStatus, ProcessStatus, RestartStatus};
pub use stop::StopRequest;
pub use stop_all::StopAllRequest;

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
