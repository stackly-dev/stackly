//! What the supervisor shows about an instance besides its registry row
//! (spec 0010, *Status dan event*): readiness, health, the process, the
//! restart policy, the blocker, and the numbering that orders every
//! change.
//!
//! These values live in the memory of the instance's actor. `stacklyd`
//! maps an [`InstanceStatus`] to the DTO of `instance.get`,
//! `instance.list`, and `instance.health_changed`; a client applies a
//! status update when its `status_seq` is greater than the one it holds,
//! never by revision.

use jiff::Timestamp;
use stackly_domain::{Health, InstanceBlocker, Pid, ProcessStart, Readiness};

/// The process of an instance, as the status shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStatus {
    /// The process ID.
    pub pid: Pid,
    /// When the process started.
    pub started_at: ProcessStart,
    /// True for a process the daemon found running after its own restart.
    pub adopted: bool,
    /// Whether the process holds the instance lock, so a daemon restart
    /// can adopt it. `None` until readiness succeeded once (`holds_file`
    /// runs then); `Some(false)` means the instance runs normally but
    /// becomes `unknown` after a daemon restart.
    pub adoptable: Option<bool>,
}

/// The restart policy's state for an instance.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestartStatus {
    /// Crashes in the sliding window.
    pub attempts: u32,
    /// The respawns the window allows.
    pub budget: u32,
    /// The window, in seconds.
    pub window_seconds: u64,
    /// When the next respawn happens, while one waits.
    pub next_attempt_at: Option<Timestamp>,
    /// True after the budget ran out.
    pub exhausted: bool,
}

/// An instance's status at one moment: a snapshot of the actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceStatus {
    /// Counts the changes of this status per instance and per daemon run,
    /// from 1; zero before the first change.
    pub status_seq: u64,
    /// Readiness as read now (`effective`).
    pub readiness: Readiness,
    /// Health as read now (`effective`).
    pub health: Health,
    /// The process, if one runs.
    pub process: Option<ProcessStatus>,
    /// The restart policy.
    pub restart: RestartStatus,
    /// Why the instance is blocked, if it is.
    pub blocker: Option<InstanceBlocker>,
    /// Members of the process group left after the last stop, reported and
    /// never signalled.
    pub leftover: Vec<Pid>,
    /// True once readiness succeeded for the current process: the
    /// instance's `Auto` ports show as final (`bound: true`, REQ-010).
    pub ports_final: bool,
    /// True when the spool of the running process could not be read.
    pub logs_unavailable: bool,
    /// True while an actor commit waits for the registry; the state in
    /// memory is then what is shown.
    pub pending_commit: bool,
}

impl InstanceStatus {
    /// Returns whether `other` differs in a field a client can see, which
    /// is when [`status_seq`](Self::status_seq) must rise. A new
    /// `observed_at` with the same state, reason, and detail is not a
    /// change (spec 0010: the update is not sent).
    #[must_use]
    pub fn differs_visibly(&self, other: &Self) -> bool {
        let observed = |status: &Self| {
            (
                status.readiness.state(),
                status.readiness.reason().clone(),
                status.readiness.detail().cloned(),
                status.health.state(),
                status.health.reason().clone(),
                status.health.detail().cloned(),
            )
        };
        observed(self) != observed(other)
            || self.process != other.process
            || self.restart != other.restart
            || self.blocker != other.blocker
            || self.leftover != other.leftover
            || self.ports_final != other.ports_final
            || self.logs_unavailable != other.logs_unavailable
            || self.pending_commit != other.pending_commit
    }
}

#[cfg(test)]
mod tests {
    use stackly_domain::{HealthState, ReadinessState, ReasonCode};

    use super::*;

    fn status(at: &str) -> InstanceStatus {
        let at: Timestamp = at.parse().unwrap();
        InstanceStatus {
            status_seq: 1,
            readiness: Readiness::new(ReadinessState::Ready, at, "probe_ok".parse().unwrap(), None),
            health: Health::new(HealthState::Healthy, at, "probe_ok".parse().unwrap(), None),
            process: None,
            restart: RestartStatus::default(),
            blocker: None,
            leftover: Vec::new(),
            ports_final: true,
            logs_unavailable: false,
            pending_commit: false,
        }
    }

    #[test]
    fn a_newer_observation_time_alone_is_not_a_visible_change() {
        let before = status("2026-01-01T00:00:00Z");
        let after = status("2026-01-01T00:00:10Z");

        assert!(!before.differs_visibly(&after));
    }

    #[test]
    fn a_new_state_or_reason_is_a_visible_change() {
        let before = status("2026-01-01T00:00:00Z");
        let mut after = before.clone();
        after.readiness = Readiness::new(
            ReadinessState::NotReady,
            before.readiness.observed_at(),
            ReasonCode::AWAITING_PROBE,
            None,
        );

        assert!(before.differs_visibly(&after));

        let mut stale = before.clone();
        stale.readiness = before.readiness.effective(
            "2026-01-01T00:05:00Z".parse().unwrap(),
            std::time::Duration::from_secs(30),
        );
        assert!(before.differs_visibly(&stale));
    }
}
