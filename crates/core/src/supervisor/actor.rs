//! The memory of one instance's supervisor (spec 0010, *Status dan event*):
//! the process it runs, the readiness and health probes observed, the
//! restart history, and the numbering of every visible change.
//!
//! Nothing here is stored in the registry. A daemon restart starts every
//! actor again, with readiness and health `unknown` (reason
//! `daemon_restarted`) and `status_seq` at zero.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::Timestamp;
use stackly_domain::{
    Health, HealthState, InstanceBlocker, InstanceId, Pid, ProcessIdentity, Readiness,
    ReadinessState, ReasonCode, ResourceRevision,
};
use stackly_platform::{ExitListener, ExitNotice, FileIdentity};
use tokio::sync::{Notify, watch};

use super::drain::DrainSlot;
use super::restart::RestartHistory;
use super::status::{InstanceStatus, ProcessStatus, RestartStatus};
use super::{SupervisorConfig, SupervisorDeps};

/// The process an actor supervises.
pub(crate) struct ProcessSlot {
    /// Tells this process from an earlier one of the same instance.
    pub(crate) generation: u64,
    /// Who it is, as recorded at spawn.
    pub(crate) identity: ProcessIdentity,
    /// The process group, for the members left after an exit.
    pub(crate) group: Pid,
    /// Found running after a daemon restart, not spawned by this daemon.
    pub(crate) adopted: bool,
    /// Whether it holds the instance lock; `None` until the first
    /// readiness.
    pub(crate) adoptable: Option<bool>,
    /// The instance lock file it inherited.
    pub(crate) lock: FileIdentity,
    /// Flips to `true` after the exit handler finished: lifecycle
    /// committed, secret material removed, leftovers counted.
    pub(crate) done: watch::Sender<bool>,
    /// Flips to `true` to stop the probe loops of this process.
    pub(crate) probes_stop: watch::Sender<bool>,
    /// Reads the process's spools.
    pub(crate) drain: Arc<DrainSlot>,
}

/// Hands the exit of one process to the tasks waiting for it.
pub(crate) struct ExitSlot(watch::Sender<Option<ExitNotice>>);

impl ExitSlot {
    /// Makes a slot and the receiver the actor keeps.
    pub(crate) fn new() -> (Self, watch::Receiver<Option<ExitNotice>>) {
        let (sender, receiver) = watch::channel(None);
        (Self(sender), receiver)
    }
}

impl ExitListener for ExitSlot {
    fn exited(&self, notice: ExitNotice) {
        self.0.send_replace(Some(notice));
    }
}

/// What the actor holds; see [`InstanceActor::update`].
pub(crate) struct ActorState {
    /// The revision of the instance after the last commit that touched it.
    pub(crate) revision: ResourceRevision,
    /// Readiness as observed, before `effective`.
    pub(crate) readiness: Readiness,
    /// Health as observed, before `effective`.
    pub(crate) health: Health,
    /// The process that runs, if one does.
    pub(crate) process: Option<ProcessSlot>,
    /// Crash times in the window.
    pub(crate) restart: RestartHistory,
    /// What the `restart` status shows.
    pub(crate) restart_status: RestartStatus,
    /// Why the instance is blocked.
    pub(crate) blocker: Option<InstanceBlocker>,
    /// Group members left after the last stop.
    pub(crate) leftover: Vec<Pid>,
    /// True once readiness succeeded for the current process.
    pub(crate) ports_final: bool,
    /// The spool could not be read.
    pub(crate) logs_unavailable: bool,
    /// An actor commit waits for the registry.
    pub(crate) pending_commit: bool,
    /// A stop of the current process is under way, so its exit is the
    /// expected one.
    pub(crate) stopping: bool,
    /// A start operation watches the exit of the current process itself,
    /// so the exit task waits.
    pub(crate) op_owns_exit: bool,
    /// The next [`ProcessSlot::generation`].
    pub(crate) next_generation: u64,
    /// Flips to `true` to cancel the pause before a respawn the actor's
    /// timer waits out; `None` when no pause is pending.
    pub(crate) respawn: Option<watch::Sender<bool>>,
    seq: u64,
    published: InstanceStatus,
}

impl ActorState {
    fn snapshot(&self, now: Timestamp, max_age: std::time::Duration) -> InstanceStatus {
        InstanceStatus {
            status_seq: self.seq,
            readiness: self.readiness.effective(now, max_age),
            health: self.health.effective(now, max_age),
            process: self.process.as_ref().map(|slot| ProcessStatus {
                pid: slot.identity.pid,
                started_at: slot.identity.start,
                adopted: slot.adopted,
                adoptable: slot.adoptable,
            }),
            restart: self.restart_status.clone(),
            blocker: self.blocker.clone(),
            leftover: self.leftover.clone(),
            ports_final: self.ports_final,
            logs_unavailable: self.logs_unavailable,
            pending_commit: self.pending_commit,
        }
    }

    /// Returns the status to publish when it differs visibly from the last
    /// published one, and counts it.
    fn next_event(
        &mut self,
        now: Timestamp,
        max_age: std::time::Duration,
    ) -> Option<(InstanceStatus, ResourceRevision)> {
        let current = self.snapshot(now, max_age);
        if !current.differs_visibly(&self.published) {
            return None;
        }
        self.seq += 1;
        let published = InstanceStatus {
            status_seq: self.seq,
            ..current
        };
        self.published = published.clone();
        Some((published, self.revision))
    }

    /// Offers a blocker. It replaces the current one only when its priority
    /// is not lower: `identityUnproven`, then `dataInUse`, then
    /// `needsSecret` (spec 0010, *Status dan event*).
    pub(crate) fn offer_blocker(&mut self, blocker: InstanceBlocker) {
        let priority = |blocker: &InstanceBlocker| match blocker {
            InstanceBlocker::IdentityUnproven { .. } => 3,
            InstanceBlocker::DataInUse { .. } => 2,
            _ => 1,
        };
        if self
            .blocker
            .as_ref()
            .is_none_or(|current| priority(&blocker) >= priority(current))
        {
            self.blocker = Some(blocker);
        }
    }

    /// Replaces the blocker the supervisor itself sets (`identityUnproven`
    /// or `dataInUse`) with `blocker`, or removes it when `blocker` is
    /// `None`. A `needsSecret` blocker is not the supervisor's to remove.
    pub(crate) fn set_supervisor_blocker(&mut self, blocker: Option<InstanceBlocker>) {
        if matches!(
            self.blocker,
            Some(InstanceBlocker::IdentityUnproven { .. } | InstanceBlocker::DataInUse { .. })
        ) {
            self.blocker = None;
        }
        if let Some(blocker) = blocker {
            self.offer_blocker(blocker);
        }
    }

    /// Sets readiness to `not_ready` with the reason `awaiting_probe`, as a
    /// process that just spawned reads (the lifecycle event `spawned` does
    /// the same to the aggregate).
    pub(crate) fn awaiting_probe(&mut self, at: Timestamp) {
        self.readiness = Readiness::new(
            ReadinessState::NotReady,
            at,
            ReasonCode::AWAITING_PROBE,
            None,
        );
        self.health = Health::new(HealthState::Unknown, at, ReasonCode::AWAITING_PROBE, None);
        self.ports_final = false;
    }

    /// Sets readiness and health to `unknown` with the reason
    /// `not_running`, as an instance without a process reads.
    pub(crate) fn not_running(&mut self, at: Timestamp) {
        self.readiness = Readiness::new(ReadinessState::Unknown, at, ReasonCode::NOT_RUNNING, None);
        self.health = Health::new(HealthState::Unknown, at, ReasonCode::NOT_RUNNING, None);
        self.ports_final = false;
    }
}

/// The supervisor's memory of one instance.
pub(crate) struct InstanceActor {
    pub(crate) id: InstanceId,
    state: Mutex<ActorState>,
    /// Wakes the exit task when a start operation hands the exit back.
    pub(crate) released: Notify,
}

impl InstanceActor {
    /// Makes the actor of an instance the daemon has not supervised yet:
    /// readiness and health `unknown` with reason `daemon_restarted`.
    pub(crate) fn new(id: InstanceId, now: Timestamp) -> Self {
        let readiness = Readiness::new(
            ReadinessState::Unknown,
            now,
            ReasonCode::DAEMON_RESTARTED,
            None,
        );
        let health = Health::new(
            HealthState::Unknown,
            now,
            ReasonCode::DAEMON_RESTARTED,
            None,
        );
        let mut state = ActorState {
            revision: ResourceRevision::INITIAL,
            readiness,
            health,
            process: None,
            restart: RestartHistory::new(),
            restart_status: RestartStatus::default(),
            blocker: None,
            leftover: Vec::new(),
            ports_final: false,
            logs_unavailable: false,
            pending_commit: false,
            stopping: false,
            op_owns_exit: false,
            next_generation: 1,
            respawn: None,
            seq: 0,
            published: placeholder(now),
        };
        state.published = state.snapshot(now, std::time::Duration::MAX);
        Self {
            id,
            state: Mutex::new(state),
            released: Notify::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ActorState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns the status as read at `now`.
    pub(crate) fn snapshot(&self, now: Timestamp, max_age: std::time::Duration) -> InstanceStatus {
        self.lock().snapshot(now, max_age)
    }

    /// Reads the state without changing it.
    pub(crate) fn read<R>(&self, read: impl FnOnce(&ActorState) -> R) -> R {
        read(&self.lock())
    }

    /// Changes the state under the lock, then reports the status if a
    /// client can see a difference. The report goes out after the lock is
    /// released, so a slow sink never blocks the actor.
    pub(crate) fn update<R>(
        &self,
        deps: &SupervisorDeps,
        change: impl FnOnce(&mut ActorState) -> R,
    ) -> R {
        let (result, event) = {
            let mut state = self.lock();
            let result = change(&mut state);
            let event =
                state.next_event(deps.platform.clock().now(), deps.config.readiness_max_age);
            (result, event)
        };
        if let Some((status, revision)) = event {
            deps.events.status_changed(self.id, &status, revision);
        }
        result
    }

    /// Reports the status again if time alone changed what a client reads:
    /// an observation that went stale since the last report.
    pub(crate) fn refresh(&self, deps: &SupervisorDeps) {
        self.update(deps, |_| ());
    }

    /// Hands the exit of the current process back to its exit task: a start
    /// operation calls it when it stops watching the process itself.
    pub(crate) fn release_exit(&self, deps: &SupervisorDeps) {
        self.update(deps, |state| state.op_owns_exit = false);
        self.released.notify_one();
    }

    /// Cancels the pause before a respawn, if one is pending, and clears
    /// the time it showed. A respawn that already began is not touched: it
    /// is an operation then.
    pub(crate) fn cancel_respawn(&self, deps: &SupervisorDeps) {
        self.update(deps, |state| {
            if let Some(cancel) = state.respawn.take() {
                cancel.send_replace(true);
                let attempts = state
                    .restart
                    .crashes_in_window(tokio::time::Instant::now(), &deps.config);
                state.restart_status = restart_status(&deps.config, attempts, None, false);
            }
        });
    }

    /// Stops everything this actor runs in the background, for a daemon
    /// shutdown: the probes, the spool reading, and the pause before a
    /// respawn. The process itself is left alone, and no lifecycle is
    /// committed.
    pub(crate) fn stop_background(&self, deps: &SupervisorDeps) {
        self.update(deps, |state| {
            if let Some(slot) = &state.process {
                slot.probes_stop.send_replace(true);
                slot.drain.stop();
            }
            if let Some(cancel) = state.respawn.take() {
                cancel.send_replace(true);
            }
        });
    }

    /// Notes the revision a commit left the instance at.
    pub(crate) fn note_revision(&self, revision: ResourceRevision) {
        let mut state = self.lock();
        if revision > state.revision {
            state.revision = revision;
        }
    }
}

/// A status that no real one equals, for the first comparison.
fn placeholder(now: Timestamp) -> InstanceStatus {
    InstanceStatus {
        status_seq: 0,
        readiness: Readiness::new(
            ReadinessState::Unknown,
            now,
            ReasonCode::NEVER_OBSERVED,
            None,
        ),
        health: Health::new(HealthState::Unknown, now, ReasonCode::NEVER_OBSERVED, None),
        process: None,
        restart: RestartStatus::default(),
        blocker: None,
        leftover: Vec::new(),
        ports_final: false,
        logs_unavailable: false,
        pending_commit: false,
    }
}

/// Builds the restart status for the actor from the policy constants.
pub(crate) fn restart_status(
    config: &SupervisorConfig,
    attempts: u32,
    next_attempt_at: Option<Timestamp>,
    exhausted: bool,
) -> RestartStatus {
    RestartStatus {
        attempts,
        budget: config.restart_budget,
        window_seconds: config.restart_window.as_secs(),
        next_attempt_at,
        exhausted,
    }
}
