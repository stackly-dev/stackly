//! The daemon restart plan (spec 0030): which instances a planned restart
//! keeps running and which it stops and starts again, and the state
//! machine the old and the new daemon move it through.
//!
//! ```text
//! stopping ──every stop_restore stopped──▶ handed_over ──same boot──▶ restoring ──every outcome──▶ finished
//!    │                                          │
//!    ├──stop timeout──▶ aborted                 └──new boot──▶ expired
//!    └──daemon crash, same boot──▶ restoring
//! ```
//!
//! A plan never changes the desired state of an instance, and it stops each
//! instance at most once.

use std::fmt;

use jiff::Timestamp;
use uuid::Uuid;

use crate::code::code_enum;
use crate::error::{Machine, TransitionError};
use crate::id::{InstanceId, OperationId, RestartPlanId};
use crate::instance::InstanceLifecycle;
use crate::process::BootId;
use crate::value::{FailureCode, InstanceName, Sha256Digest};

code_enum! {
    /// Where a restart plan is.
    pub enum PlanState: PlanState {
        /// The old daemon stops the `stop_restore` instances.
        Stopping => "stopping",
        /// Every `stop_restore` instance stopped; the old daemon exits.
        HandedOver => "handed_over",
        /// The new daemon adopts and starts the instances again.
        Restoring => "restoring",
        /// Every instance has an outcome; final.
        Finished => "finished",
        /// A stop timed out, so the old daemon kept running; final.
        Aborted => "aborted",
        /// The Mac booted again before the plan finished; final.
        Expired => "expired",
    }
}

impl PlanState {
    /// Returns whether the plan is still open: `stopping`, `handed_over`,
    /// or `restoring`. The registry holds at most one open plan.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Stopping | Self::HandedOver | Self::Restoring)
    }
}

code_enum! {
    /// What a planned restart does with one instance.
    pub enum PlanDecision: PlanDecision {
        /// Left alone; the new daemon adopts it.
        Keep => "keep",
        /// Stopped before the old daemon exits, started again by the new
        /// one.
        StopRestore => "stop_restore",
    }
}

code_enum! {
    /// Why an instance got its [`PlanDecision`].
    pub enum PlanReason: PlanReason {
        /// Its provider line is in the handover list.
        HandoverProven => "handover_proven",
        /// Its provider line is not.
        HandoverUnproven => "handover_unproven",
        /// It waits for a respawn and has no process.
        RespawnPending => "respawn_pending",
        /// It is `unknown`, so it is never signalled.
        IdentityUnproven => "identity_unproven",
        /// The registry is in recovery mode, so nothing is stopped.
        SupervisorUnavailable => "supervisor_unavailable",
    }
}

code_enum! {
    /// What became of one instance of a plan.
    pub enum PlanOutcome: PlanOutcome {
        /// The new daemon adopted its process.
        Adopted => "adopted",
        /// It stopped and reached `ready` again.
        Restored => "restored",
        /// It did not end up running.
        Failed => "failed",
        /// It was removed, lost its data, or the plan closed first.
        Skipped => "skipped",
    }
}

code_enum! {
    /// Why a plan closed without finishing.
    pub enum PlanCloseReason: PlanCloseReason {
        /// A `stop_restore` instance did not stop within its provider's
        /// stop timeout.
        StopTimeout => "stop_timeout",
        /// The new daemon runs in another boot session.
        BootChanged => "boot_changed",
    }
}

/// Who asks the supervisor to stop an instance, which decides what the
/// stop writes besides the lifecycle (spec 0030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StopIntent {
    /// A person: the stop writes desired `stopped` and checks the leases
    /// of environments (BR-015).
    User,
    /// The daemon after reconciliation, for an instance whose desired
    /// state is already `stopped`: no lease check, and the desired state
    /// it writes is the one it already has.
    Reconcile,
    /// A planned restart: the stop writes no desired state and checks no
    /// lease, so the next daemon starts the instance again.
    PlannedRestart,
}

impl StopIntent {
    /// Returns whether the stop writes desired `stopped`.
    #[must_use]
    pub const fn writes_desired(self) -> bool {
        !matches!(self, Self::PlannedRestart)
    }

    /// Returns whether the stop asks whether environments hold the
    /// instance.
    #[must_use]
    pub const fn checks_leases(self) -> bool {
        matches!(self, Self::User)
    }
}

/// Returns the decision a restart makes for an instance in `lifecycle`,
/// or `None` when the instance is not part of a plan (spec 0030 AC-7).
///
/// `supervisor_available` is false while the registry is in recovery mode;
/// `handover_proven` says whether the instance's provider line is in the
/// handover list.
#[must_use]
pub const fn plan_decision(
    lifecycle: InstanceLifecycle,
    supervisor_available: bool,
    handover_proven: bool,
) -> Option<(PlanDecision, PlanReason)> {
    use InstanceLifecycle as L;
    let in_plan = matches!(
        lifecycle,
        L::Running | L::Starting | L::Stopping | L::Recovering | L::Unknown
    );
    if !in_plan {
        return None;
    }
    if !supervisor_available {
        return Some((PlanDecision::Keep, PlanReason::SupervisorUnavailable));
    }
    Some(match lifecycle {
        L::Recovering => (PlanDecision::Keep, PlanReason::RespawnPending),
        L::Unknown => (PlanDecision::Keep, PlanReason::IdentityUnproven),
        _ if handover_proven => (PlanDecision::Keep, PlanReason::HandoverProven),
        _ => (PlanDecision::StopRestore, PlanReason::HandoverUnproven),
    })
}

/// Returns the text a plan digest hashes: one `instanceId:decision:reason`
/// line per entry, sorted by instance ID, joined by newlines.
#[must_use]
pub fn plan_digest_text(entries: &[(InstanceId, PlanDecision, PlanReason)]) -> String {
    let mut sorted: Vec<_> = entries.to_vec();
    sorted.sort_by_key(|(id, _, _)| *id);
    sorted
        .iter()
        .map(|(id, decision, reason)| format!("{id}:{decision}:{reason}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One instance of a [`RestartPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanInstance {
    /// The instance; it may be removed while the plan is open.
    pub instance_id: InstanceId,
    /// The instance name when the plan was written.
    pub name: InstanceName,
    /// What the restart does with it.
    pub decision: PlanDecision,
    /// Why.
    pub reason: PlanReason,
    /// When the old daemon stopped it, for `stop_restore`.
    pub stopped_at: Option<Timestamp>,
    /// When it was `ready` again, or adopted.
    pub ready_at: Option<Timestamp>,
    /// What became of it, or `None` while the plan is open.
    pub outcome: Option<PlanOutcome>,
    /// The failure or blocker code of a `failed` outcome.
    pub failure_code: Option<FailureCode>,
}

impl PlanInstance {
    /// A new entry with nothing recorded yet.
    #[must_use]
    pub const fn new(
        instance_id: InstanceId,
        name: InstanceName,
        decision: PlanDecision,
        reason: PlanReason,
    ) -> Self {
        Self {
            instance_id,
            name,
            decision,
            reason,
            stopped_at: None,
            ready_at: None,
            outcome: None,
            failure_code: None,
        }
    }
}

/// The values a new [`RestartPlan`] starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRestartPlan {
    /// The plan's ID, created by the daemon.
    pub id: RestartPlanId,
    /// The `daemon_restart` operation that writes it.
    pub operation: OperationId,
    /// The digest of its decisions.
    pub digest: Sha256Digest,
    /// The boot session the old daemon runs in.
    pub boot_id: BootId,
    /// The `serverInstanceId` of the old daemon.
    pub server_instance_id_before: Uuid,
    /// When it was written.
    pub created_at: Timestamp,
    /// Every instance of the plan.
    pub instances: Vec<PlanInstance>,
}

/// A plan as the registry stores it, with every field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRestartPlan {
    /// The plan's ID.
    pub id: RestartPlanId,
    /// The `daemon_restart` operation that wrote it.
    pub operation: OperationId,
    /// Where it is.
    pub state: PlanState,
    /// The digest of its decisions.
    pub digest: Sha256Digest,
    /// The boot session the old daemon ran in.
    pub boot_id: BootId,
    /// The `serverInstanceId` of the old daemon.
    pub server_instance_id_before: Uuid,
    /// The `serverInstanceId` of the daemon that restores it.
    pub server_instance_id_after: Option<Uuid>,
    /// When it was written.
    pub created_at: Timestamp,
    /// When every `stop_restore` instance had stopped.
    pub handed_over_at: Option<Timestamp>,
    /// When the new daemon finished reconciling.
    pub reconciled_at: Option<Timestamp>,
    /// When it became final.
    pub finished_at: Option<Timestamp>,
    /// Why an `aborted` or `expired` plan closed.
    pub close_reason: Option<PlanCloseReason>,
    /// Every instance of the plan.
    pub instances: Vec<PlanInstance>,
}

/// Why a [`RestartPlan`] refused a change. The plan does not change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RestartPlanError {
    /// The plan's state has no such transition.
    #[error(transparent)]
    Transition(#[from] TransitionError),
    /// The instance is not part of the plan.
    #[error("instance {0} is not part of the restart plan")]
    UnknownInstance(InstanceId),
    /// Only a `stop_restore` instance is stopped by the plan.
    #[error("instance {0} is kept by the restart plan, so the plan never stops it")]
    NotStopRestore(InstanceId),
    /// The plan already stopped the instance once.
    #[error("instance {0} was already stopped by the restart plan")]
    AlreadyStopped(InstanceId),
    /// The instance already has an outcome.
    #[error("instance {0} already has an outcome in the restart plan")]
    OutcomeRecorded(InstanceId),
    /// A `stop_restore` instance has not stopped yet.
    #[error("instance {0} has not stopped yet, so the plan cannot hand over")]
    NotStopped(InstanceId),
    /// A stored plan breaks a rule a new plan keeps.
    #[error("the stored restart plan is inconsistent: {0}")]
    Inconsistent(&'static str),
}

/// A daemon restart plan and its state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPlan {
    stored: StoredRestartPlan,
}

impl RestartPlan {
    /// A new plan in `stopping`, its instances sorted by ID.
    #[must_use]
    pub fn new(new: NewRestartPlan) -> Self {
        let mut instances = new.instances;
        instances.sort_by_key(|entry| entry.instance_id);
        Self {
            stored: StoredRestartPlan {
                id: new.id,
                operation: new.operation,
                state: PlanState::Stopping,
                digest: new.digest,
                boot_id: new.boot_id,
                server_instance_id_before: new.server_instance_id_before,
                server_instance_id_after: None,
                created_at: new.created_at,
                handed_over_at: None,
                reconciled_at: None,
                finished_at: None,
                close_reason: None,
                instances,
            },
        }
    }

    /// Rebuilds a plan the registry stored.
    ///
    /// # Errors
    ///
    /// Returns [`RestartPlanError::Inconsistent`] when a final plan has no
    /// `finished_at` or an instance without an outcome, a closed plan has
    /// no close reason or an open one has one, or `handed_over_at` is
    /// missing past `stopping`.
    pub fn restore(mut stored: StoredRestartPlan) -> Result<Self, RestartPlanError> {
        let state = stored.state;
        if state.is_open() == stored.finished_at.is_some() {
            return Err(RestartPlanError::Inconsistent(
                "finished_at must be set exactly when the plan is final",
            ));
        }
        let closed = matches!(state, PlanState::Aborted | PlanState::Expired);
        if closed != stored.close_reason.is_some() {
            return Err(RestartPlanError::Inconsistent(
                "close_reason must be set exactly when the plan is aborted or expired",
            ));
        }
        if !state.is_open() && stored.instances.iter().any(|i| i.outcome.is_none()) {
            return Err(RestartPlanError::Inconsistent(
                "every instance of a final plan has an outcome",
            ));
        }
        if matches!(state, PlanState::HandedOver | PlanState::Finished)
            && stored.handed_over_at.is_none()
        {
            return Err(RestartPlanError::Inconsistent(
                "a plan past stopping that handed over has handed_over_at",
            ));
        }
        stored.instances.sort_by_key(|entry| entry.instance_id);
        Ok(Self { stored })
    }

    /// Returns every field.
    #[must_use]
    pub const fn stored(&self) -> &StoredRestartPlan {
        &self.stored
    }

    /// Returns the plan's ID.
    #[must_use]
    pub const fn id(&self) -> RestartPlanId {
        self.stored.id
    }

    /// Returns where the plan is.
    #[must_use]
    pub const fn state(&self) -> PlanState {
        self.stored.state
    }

    /// Returns the instances, sorted by ID.
    #[must_use]
    pub fn instances(&self) -> &[PlanInstance] {
        &self.stored.instances
    }

    /// Returns the entry of `instance`.
    #[must_use]
    pub fn instance(&self, instance: InstanceId) -> Option<&PlanInstance> {
        self.stored
            .instances
            .iter()
            .find(|entry| entry.instance_id == instance)
    }

    /// Records that the old daemon stopped `instance` at `at`.
    ///
    /// # Errors
    ///
    /// Returns a transition error outside `stopping` and `restoring` (a
    /// daemon that crashed while stopping finishes the stop while
    /// restoring, AC-14), or [`RestartPlanError::UnknownInstance`],
    /// [`RestartPlanError::NotStopRestore`], or
    /// [`RestartPlanError::AlreadyStopped`].
    pub fn record_stopped(
        &mut self,
        instance: InstanceId,
        at: Timestamp,
    ) -> Result<(), RestartPlanError> {
        self.require(
            matches!(
                self.stored.state,
                PlanState::Stopping | PlanState::Restoring
            ),
            "record_stopped",
        )?;
        let entry = self.entry_mut(instance)?;
        if entry.decision != PlanDecision::StopRestore {
            return Err(RestartPlanError::NotStopRestore(instance));
        }
        if entry.stopped_at.is_some() {
            return Err(RestartPlanError::AlreadyStopped(instance));
        }
        entry.stopped_at = Some(at);
        Ok(())
    }

    /// Moves `stopping` to `handed_over` once every `stop_restore`
    /// instance stopped.
    ///
    /// # Errors
    ///
    /// Returns a transition error outside `stopping`, or
    /// [`RestartPlanError::NotStopped`] for the first instance that did
    /// not stop.
    pub fn hand_over(&mut self, at: Timestamp) -> Result<(), RestartPlanError> {
        self.require(self.stored.state == PlanState::Stopping, "hand_over")?;
        if let Some(entry) = self
            .stored
            .instances
            .iter()
            .find(|e| e.decision == PlanDecision::StopRestore && e.stopped_at.is_none())
        {
            return Err(RestartPlanError::NotStopped(entry.instance_id));
        }
        self.stored.state = PlanState::HandedOver;
        self.stored.handed_over_at = Some(at);
        Ok(())
    }

    /// Moves `stopping` to `aborted` with `stop_timeout`; every instance
    /// without an outcome becomes `skipped`.
    ///
    /// # Errors
    ///
    /// Returns a transition error outside `stopping`.
    pub fn abort(&mut self, at: Timestamp) -> Result<(), RestartPlanError> {
        self.require(self.stored.state == PlanState::Stopping, "abort")?;
        self.close(PlanState::Aborted, PlanCloseReason::StopTimeout, at);
        Ok(())
    }

    /// The new daemon takes an open plan over in the boot session `boot`:
    /// the same boot moves `stopping` or `handed_over` to `restoring`, a
    /// new boot moves any open plan to `expired` with `boot_changed`
    /// (AC-13, AC-14). A plan already `restoring` in the same boot (a
    /// daemon that crashed while restoring) stays `restoring` under the
    /// new daemon.
    ///
    /// # Errors
    ///
    /// Returns a transition error for a final plan.
    pub fn take_over(
        &mut self,
        boot: BootId,
        server_instance_id: Uuid,
        at: Timestamp,
    ) -> Result<PlanState, RestartPlanError> {
        self.require(self.stored.state.is_open(), "take_over")?;
        if boot == self.stored.boot_id {
            self.stored.state = PlanState::Restoring;
            self.stored.server_instance_id_after = Some(server_instance_id);
        } else {
            self.close(PlanState::Expired, PlanCloseReason::BootChanged, at);
        }
        Ok(self.stored.state)
    }

    /// Records that the new daemon finished reconciling at `at`.
    ///
    /// # Errors
    ///
    /// Returns a transition error outside `restoring`.
    pub fn record_reconciled(&mut self, at: Timestamp) -> Result<(), RestartPlanError> {
        self.require(self.stored.state == PlanState::Restoring, "reconciled")?;
        self.stored.reconciled_at = Some(at);
        Ok(())
    }

    /// Records what became of `instance`; the plan moves to `finished` at
    /// `at` once every instance has an outcome. `ready_at` is when it was
    /// adopted or `ready` again, and `failure_code` the code of a `failed`
    /// outcome.
    ///
    /// # Errors
    ///
    /// Returns a transition error outside `restoring`, or
    /// [`RestartPlanError::UnknownInstance`] or
    /// [`RestartPlanError::OutcomeRecorded`].
    pub fn record_outcome(
        &mut self,
        instance: InstanceId,
        outcome: PlanOutcome,
        ready_at: Option<Timestamp>,
        failure_code: Option<FailureCode>,
        at: Timestamp,
    ) -> Result<PlanState, RestartPlanError> {
        self.require(self.stored.state == PlanState::Restoring, "record_outcome")?;
        let entry = self.entry_mut(instance)?;
        if entry.outcome.is_some() {
            return Err(RestartPlanError::OutcomeRecorded(instance));
        }
        entry.outcome = Some(outcome);
        entry.ready_at = ready_at;
        entry.failure_code = failure_code;
        if self.stored.instances.iter().all(|e| e.outcome.is_some()) {
            self.stored.state = PlanState::Finished;
            self.stored.finished_at = Some(at);
        }
        Ok(self.stored.state)
    }

    /// Ends the plan in `state` with `reason`; instances without an
    /// outcome become `skipped`.
    fn close(&mut self, state: PlanState, reason: PlanCloseReason, at: Timestamp) {
        for entry in &mut self.stored.instances {
            entry.outcome.get_or_insert(PlanOutcome::Skipped);
        }
        self.stored.state = state;
        self.stored.close_reason = Some(reason);
        self.stored.finished_at = Some(at);
    }

    fn require(&self, allowed: bool, event: &'static str) -> Result<(), RestartPlanError> {
        if allowed {
            Ok(())
        } else {
            Err(
                TransitionError::new(Machine::RestartPlan, self.stored.state.as_code(), event)
                    .into(),
            )
        }
    }

    fn entry_mut(&mut self, instance: InstanceId) -> Result<&mut PlanInstance, RestartPlanError> {
        self.stored
            .instances
            .iter_mut()
            .find(|entry| entry.instance_id == instance)
            .ok_or(RestartPlanError::UnknownInstance(instance))
    }
}

impl fmt::Display for RestartPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "restart plan {} ({})", self.stored.id, self.stored.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + second).unwrap()
    }

    fn boot(n: u128) -> BootId {
        Uuid::from_u128(n).hyphenated().to_string().parse().unwrap()
    }

    fn entry(decision: PlanDecision) -> PlanInstance {
        let reason = match decision {
            PlanDecision::Keep => PlanReason::HandoverProven,
            PlanDecision::StopRestore => PlanReason::HandoverUnproven,
        };
        PlanInstance::new(
            InstanceId::generate(),
            "db".parse().unwrap(),
            decision,
            reason,
        )
    }

    fn plan(instances: Vec<PlanInstance>) -> RestartPlan {
        RestartPlan::new(NewRestartPlan {
            id: RestartPlanId::generate(),
            operation: OperationId::generate(),
            digest: "a1".repeat(32).parse().unwrap(),
            boot_id: boot(1),
            server_instance_id_before: Uuid::from_u128(10),
            created_at: at(0),
            instances,
        })
    }

    #[test]
    fn a_plan_hands_over_only_after_every_stop_restore_instance_stopped() {
        let keep = entry(PlanDecision::Keep);
        let stop = entry(PlanDecision::StopRestore);
        let (keep_id, stop_id) = (keep.instance_id, stop.instance_id);
        let mut plan = plan(vec![keep, stop]);
        assert_eq!(plan.state(), PlanState::Stopping);
        assert_eq!(
            plan.hand_over(at(1)),
            Err(RestartPlanError::NotStopped(stop_id))
        );
        assert_eq!(
            plan.record_stopped(keep_id, at(1)),
            Err(RestartPlanError::NotStopRestore(keep_id))
        );
        plan.record_stopped(stop_id, at(2)).unwrap();
        assert_eq!(
            plan.record_stopped(stop_id, at(3)),
            Err(RestartPlanError::AlreadyStopped(stop_id)),
            "a plan stops each instance at most once"
        );
        plan.hand_over(at(3)).unwrap();
        assert_eq!(plan.state(), PlanState::HandedOver);
        assert_eq!(plan.stored().handed_over_at, Some(at(3)));
    }

    #[test]
    fn the_same_boot_restores_and_finishes_once_every_instance_has_an_outcome() {
        let keep = entry(PlanDecision::Keep);
        let stop = entry(PlanDecision::StopRestore);
        let (keep_id, stop_id) = (keep.instance_id, stop.instance_id);
        let mut plan = plan(vec![keep, stop]);
        plan.record_stopped(stop_id, at(1)).unwrap();
        plan.hand_over(at(2)).unwrap();
        let after = Uuid::from_u128(11);
        assert_eq!(
            plan.take_over(boot(1), after, at(3)).unwrap(),
            PlanState::Restoring
        );
        assert_eq!(plan.stored().server_instance_id_after, Some(after));
        plan.record_reconciled(at(4)).unwrap();
        let state = plan
            .record_outcome(keep_id, PlanOutcome::Adopted, Some(at(4)), None, at(4))
            .unwrap();
        assert_eq!(state, PlanState::Restoring);
        assert_eq!(
            plan.record_outcome(keep_id, PlanOutcome::Failed, None, None, at(5)),
            Err(RestartPlanError::OutcomeRecorded(keep_id))
        );
        let state = plan
            .record_outcome(stop_id, PlanOutcome::Restored, Some(at(6)), None, at(6))
            .unwrap();
        assert_eq!(state, PlanState::Finished);
        assert_eq!(plan.stored().finished_at, Some(at(6)));
        assert_eq!(plan.stored().close_reason, None);
    }

    #[test]
    fn a_new_boot_expires_any_open_plan_and_skips_every_instance_left() {
        for hand_over in [false, true] {
            let stop = entry(PlanDecision::StopRestore);
            let stop_id = stop.instance_id;
            let mut plan = plan(vec![stop, entry(PlanDecision::Keep)]);
            if hand_over {
                plan.record_stopped(stop_id, at(1)).unwrap();
                plan.hand_over(at(2)).unwrap();
            }
            let state = plan.take_over(boot(2), Uuid::from_u128(11), at(3)).unwrap();
            assert_eq!(state, PlanState::Expired);
            let stored = plan.stored();
            assert_eq!(stored.close_reason, Some(PlanCloseReason::BootChanged));
            assert_eq!(stored.server_instance_id_after, None);
            assert!(
                stored
                    .instances
                    .iter()
                    .all(|e| e.outcome == Some(PlanOutcome::Skipped))
            );
            assert!(plan.take_over(boot(2), Uuid::from_u128(12), at(4)).is_err());
        }
    }

    #[test]
    fn a_daemon_that_crashed_while_stopping_restores_and_may_finish_the_stop() {
        let stop = entry(PlanDecision::StopRestore);
        let stop_id = stop.instance_id;
        let mut plan = plan(vec![stop]);
        plan.take_over(boot(1), Uuid::from_u128(11), at(1)).unwrap();
        assert_eq!(plan.state(), PlanState::Restoring);
        plan.record_stopped(stop_id, at(2)).unwrap();
        assert_eq!(
            plan.record_stopped(stop_id, at(3)),
            Err(RestartPlanError::AlreadyStopped(stop_id))
        );
    }

    #[test]
    fn an_abort_closes_a_stopping_plan_with_stop_timeout() {
        let stop = entry(PlanDecision::StopRestore);
        let mut plan = plan(vec![stop]);
        plan.abort(at(5)).unwrap();
        assert_eq!(plan.state(), PlanState::Aborted);
        assert_eq!(
            plan.stored().close_reason,
            Some(PlanCloseReason::StopTimeout)
        );
        let err = plan.abort(at(6)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "restart plan cannot apply abort in state aborted"
        );
    }

    #[test]
    fn a_stored_plan_restores_only_when_its_fields_fit_its_state() {
        let mut plan = plan(vec![entry(PlanDecision::StopRestore)]);
        plan.abort(at(1)).unwrap();
        let stored = plan.stored().clone();
        assert_eq!(RestartPlan::restore(stored.clone()).unwrap(), plan);
        let mut no_reason = stored.clone();
        no_reason.close_reason = None;
        assert!(RestartPlan::restore(no_reason).is_err());
        let mut open_with_end = stored;
        open_with_end.state = PlanState::Stopping;
        open_with_end.close_reason = None;
        assert!(RestartPlan::restore(open_with_end).is_err());
    }

    #[test]
    fn decisions_follow_the_lifecycle_the_handover_list_and_recovery_mode() {
        use InstanceLifecycle as L;
        use PlanDecision::{Keep, StopRestore};
        for lifecycle in [L::Running, L::Starting, L::Stopping] {
            assert_eq!(
                plan_decision(lifecycle, true, true),
                Some((Keep, PlanReason::HandoverProven))
            );
            assert_eq!(
                plan_decision(lifecycle, true, false),
                Some((StopRestore, PlanReason::HandoverUnproven))
            );
        }
        for proven in [false, true] {
            assert_eq!(
                plan_decision(L::Recovering, true, proven),
                Some((Keep, PlanReason::RespawnPending))
            );
            assert_eq!(
                plan_decision(L::Unknown, true, proven),
                Some((Keep, PlanReason::IdentityUnproven))
            );
            assert_eq!(
                plan_decision(L::Running, false, proven),
                Some((Keep, PlanReason::SupervisorUnavailable))
            );
        }
        for lifecycle in [
            L::Stopped,
            L::Failed,
            L::Initializing,
            L::Uninitialized,
            L::Retained,
        ] {
            assert_eq!(plan_decision(lifecycle, true, false), None, "{lifecycle}");
            assert_eq!(plan_decision(lifecycle, false, false), None, "{lifecycle}");
        }
    }

    #[test]
    fn the_digest_text_sorts_by_instance_id() {
        let first = InstanceId::generate();
        let second = InstanceId::generate();
        let text = plan_digest_text(&[
            (second, PlanDecision::Keep, PlanReason::HandoverProven),
            (
                first,
                PlanDecision::StopRestore,
                PlanReason::HandoverUnproven,
            ),
        ]);
        assert_eq!(
            text,
            format!("{first}:stop_restore:handover_unproven\n{second}:keep:handover_proven")
        );
        assert_eq!(plan_digest_text(&[]), "");
    }

    #[test]
    fn only_a_person_checks_leases_and_a_planned_restart_never_writes_desired() {
        assert!(StopIntent::User.writes_desired() && StopIntent::User.checks_leases());
        assert!(StopIntent::Reconcile.writes_desired() && !StopIntent::Reconcile.checks_leases());
        let planned = StopIntent::PlannedRestart;
        assert!(!planned.writes_desired() && !planned.checks_leases());
    }
}
