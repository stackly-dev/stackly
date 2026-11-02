//! The new daemon's half of a planned restart (spec 0030): it takes over the
//! open plan, lets reconciliation treat the instances the plan stopped
//! (`PendingKind::Restore`, `Then::PlannedStop`), and records what became of
//! every instance until the plan is `finished`.
//!
//! The plan in the registry is the memory of the restart: a daemon that
//! comes after a crash in the middle of a restart finds the same plan in
//! the same boot session and carries it on, and a daemon in a later boot
//! session closes it `expired` and starts nothing (starting instances in a
//! new session belongs to feature 31).

use stackly_domain::{
    DesiredState, FailureCode, Instance, InstanceId, InstanceLifecycle, OperationState,
    PlanDecision, PlanInstance, PlanOutcome, PlanState, RegistryError, RestartPlan,
};

use super::Supervisor;
use super::persist::{CommitMode, blocking, commit_instance, read_instance};

/// What a restore or a respawn of a plan ended with, for [`Supervisor::settle_restored`].
pub(crate) type RunError = Option<String>;

/// The failure code a failed operation leaves on an instance that its
/// lifecycle cannot carry: the code of the error without its area, and
/// `data_missing` for the data check of the start.
pub(crate) fn failure_code_of_error(error: &str) -> Option<FailureCode> {
    if error == "supervisor.data_unavailable" {
        return Some(FailureCode::DATA_MISSING);
    }
    error.rsplit('.').next()?.parse().ok()
}

impl Supervisor {
    /// Takes over the open restart plan, if there is one: the same boot
    /// session moves it to `restoring`, another one closes it `expired`
    /// (AC-13, AC-14). Call it after the operations recovered and before
    /// reconciliation settles the instances.
    pub(crate) async fn take_over_plan(&self) {
        let deps = self.deps();
        let registry = deps.registry.clone();
        let open = match blocking(move || registry.open_restart_plan()).await {
            Ok(Ok(versioned)) => versioned.value,
            Ok(Err(error)) => {
                tracing::warn!(%error, "the open restart plan could not be read");
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "reading the open restart plan did not run");
                return;
            }
        };
        let Some(mut plan) = open else { return };
        let boot = match deps.platform.processes().boot_id() {
            Ok(boot) => boot,
            Err(error) => {
                tracing::warn!(%error, "the boot session is unknown, so the restart plan stays open");
                return;
            }
        };
        let now = deps.platform.clock().now();
        if plan.state() == PlanState::Stopping
            && boot == plan.stored().boot_id
            && self.plan_operation_ended(&plan).await
        {
            // The old daemon ended the operation without handing over and
            // could not store the abort: it already started again what it
            // stopped, so this daemon only closes the plan.
            if plan.abort(now).is_ok() && self.persist_plan(&plan).await.is_ok() {
                deps.events.restart_plan(&plan);
                tracing::info!(plan = %plan.id(), "an unfinished restart plan was closed as aborted");
            }
            return;
        }
        let state = match plan.take_over(boot, deps.server_instance_id, now) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(%error, "the restart plan could not be taken over");
                return;
            }
        };
        if let Err(error) = self.persist_plan(&plan).await {
            tracing::warn!(%error, "the restart plan could not be stored");
            return;
        }
        deps.events.restart_plan(&plan);
        tracing::info!(plan = %plan.id(), state = %state, "a restart plan was taken over");
        if state == PlanState::Restoring {
            *self.shared.plan.lock().await = Some(plan);
        }
    }

    /// Whether the operation of a `stopping` plan is final for a reason
    /// other than the daemon ending: a cancel, or a failure that is not
    /// `operation.interrupted` (spec 0030, *Edge cases*, an abort that
    /// could not be stored). An unreadable operation counts as not ended.
    async fn plan_operation_ended(&self, plan: &RestartPlan) -> bool {
        let registry = self.deps().registry.clone();
        let operation = plan.stored().operation;
        let Ok(Ok(versioned)) = blocking(move || {
            let state = registry.operation(operation)?.value.map(|op| op.state());
            let failure = registry
                .operation_journal(operation)?
                .value
                .and_then(|view| view.journal.failure_error().map(|e| e.code().to_string()));
            Ok::<_, RegistryError>((state, failure))
        })
        .await
        else {
            return false;
        };
        match versioned {
            (Some(OperationState::Cancelled), _) => true,
            (Some(OperationState::Failed), Some(code)) => code != "operation.interrupted",
            _ => false,
        }
    }

    /// The entry of `id` in the plan this daemon restores, while it has no
    /// outcome.
    pub(crate) async fn planned_entry(&self, id: InstanceId) -> Option<PlanInstance> {
        let guard = self.shared.plan.lock().await;
        guard
            .as_ref()?
            .instance(id)
            .filter(|entry| entry.outcome.is_none())
            .cloned()
    }

    /// Records `outcome` for `id` and moves the plan on; the plan is
    /// final once every instance has one.
    async fn record_plan_outcome(
        &self,
        id: InstanceId,
        outcome: PlanOutcome,
        ready: bool,
        failure: Option<FailureCode>,
    ) {
        let mut guard = self.shared.plan.lock().await;
        let Some(plan) = guard.as_mut() else { return };
        let now = self.deps().platform.clock().now();
        match plan.record_outcome(id, outcome, ready.then_some(now), failure, now) {
            Ok(state) => {
                if let Err(error) = self.persist_plan(plan).await {
                    tracing::warn!(instance = %id, %error, "a restart outcome could not be stored");
                }
                self.deps().events.restart_plan(plan);
                if state != PlanState::Restoring {
                    *guard = None;
                }
            }
            Err(error) => {
                tracing::debug!(instance = %id, %error, "a restart outcome was not recorded");
            }
        }
    }

    /// The end of reconciliation for the plan: records that the new daemon
    /// reconciled, then the outcome of every instance that needs no
    /// further run. `deferred` are the instances reconciliation queued a
    /// run for; their outcome follows the run. `lifecycles` is what each
    /// instance reads after reconciliation.
    pub(crate) async fn settle_plan_after_reconcile(
        &self,
        lifecycles: &[(InstanceId, InstanceLifecycle)],
        deferred: &[InstanceId],
    ) {
        let waiting: Vec<PlanInstance> = {
            let mut guard = self.shared.plan.lock().await;
            let Some(plan) = guard.as_mut() else { return };
            let now = self.deps().platform.clock().now();
            let state = match plan.record_reconciled(now) {
                Ok(state) => state,
                Err(error) => {
                    tracing::warn!(%error, "reconciliation of the restart plan could not be recorded");
                    return;
                }
            };
            if let Err(error) = self.persist_plan(plan).await {
                tracing::warn!(%error, "the restart plan could not be stored");
            }
            self.deps().events.restart_plan(plan);
            if state != PlanState::Restoring {
                // An empty plan has no outcome to wait for.
                *guard = None;
                return;
            }
            plan.instances()
                .iter()
                .filter(|entry| entry.outcome.is_none())
                .cloned()
                .collect()
        };
        for entry in waiting {
            let id = entry.instance_id;
            if deferred.contains(&id) {
                continue;
            }
            let Some((_, lifecycle)) = lifecycles.iter().find(|(other, _)| *other == id) else {
                self.record_plan_outcome(id, PlanOutcome::Skipped, false, None)
                    .await;
                continue;
            };
            let instance = read_instance(self.deps(), id).await.ok();
            self.record_after(&entry, *lifecycle, instance.as_ref(), None)
                .await;
        }
    }

    /// Records the outcome of one plan entry from the lifecycle the
    /// instance ended with. `failed` is the code a run (a restore or a
    /// respawn) failed with.
    async fn record_after(
        &self,
        entry: &PlanInstance,
        lifecycle: InstanceLifecycle,
        instance: Option<&Instance>,
        failed: Option<FailureCode>,
    ) {
        let id = entry.instance_id;
        let flagged = instance.and_then(Instance::last_failure).cloned();
        if flagged == Some(FailureCode::DATA_MISSING) && lifecycle == InstanceLifecycle::Failed {
            // The registry keeps a failure code only on a `failed` outcome;
            // the instance itself carries the `data_missing` reason.
            self.record_plan_outcome(id, PlanOutcome::Skipped, false, None)
                .await;
            return;
        }
        // Spec 0030 (spec 0031 amendment): an instance that nobody wants
        // any more (a user stop, `agent_off`, a session rotation) is not
        // started by the plan, and the plan can still finish.
        if failed.is_none()
            && matches!(
                lifecycle,
                InstanceLifecycle::Stopped | InstanceLifecycle::Failed
            )
            && instance.is_some_and(|instance| instance.desired() == DesiredState::Stopped)
        {
            self.record_plan_outcome(id, PlanOutcome::Skipped, false, None)
                .await;
            return;
        }
        match lifecycle {
            InstanceLifecycle::Running if failed.is_none() => {
                let outcome = match entry.decision {
                    PlanDecision::StopRestore if entry.stopped_at.is_some() => {
                        PlanOutcome::Restored
                    }
                    _ => PlanOutcome::Adopted,
                };
                self.record_plan_outcome(id, outcome, true, None).await;
            }
            InstanceLifecycle::Unknown => {
                let code = "identity_unproven".parse().ok();
                self.record_plan_outcome(id, PlanOutcome::Failed, false, code)
                    .await;
            }
            _ => {
                let code = flagged.or(failed).or_else(|| "not_running".parse().ok());
                self.record_plan_outcome(id, PlanOutcome::Failed, false, code)
                    .await;
            }
        }
    }

    /// Settles a plan entry after the run reconciliation queued for it
    /// ended: a restore, a stop and restore, or a respawn. `error` is the
    /// code the run's operation failed with. A restore that failed before
    /// the instance had a process leaves the failure on the stopped
    /// instance, because the start's checks come before its journal
    /// (spec 0030 AC-5, AC-6).
    pub(crate) async fn settle_restored(&self, id: InstanceId, error: RunError) {
        let Some(entry) = self.planned_entry(id).await else {
            return;
        };
        let instance = match read_instance(self.deps(), id).await {
            Ok(instance) => instance,
            Err(_) => {
                self.record_plan_outcome(id, PlanOutcome::Skipped, false, None)
                    .await;
                return;
            }
        };
        let mut lifecycle = instance.lifecycle();
        let mut seen = instance;
        if let Some(code) = error.as_deref().and_then(failure_code_of_error)
            && lifecycle == InstanceLifecycle::Stopped
            && seen.last_failure().is_none()
            && seen.desired() == DesiredState::Running
        {
            let recorded = commit_instance(self, id, None, CommitMode::Actor, |changes| {
                changes.apply(|instance| {
                    instance.record_start_refusal(code.clone());
                    Ok::<_, super::SupervisorError>(())
                })?;
                Ok(Vec::new())
            })
            .await;
            if let Err(failure) = recorded {
                tracing::warn!(instance = %id, error = %failure, "the failure of a restore could not be recorded");
            }
            if let Ok(fresh) = read_instance(self.deps(), id).await {
                lifecycle = fresh.lifecycle();
                seen = fresh;
            }
        }
        // A run that failed without a code the registry knows still failed.
        let failed = error.map(|text| {
            failure_code_of_error(&text)
                .or_else(|| "start_failed".parse().ok())
                .unwrap_or(FailureCode::INTERRUPTED)
        });
        self.record_after(&entry, lifecycle, Some(&seen), failed)
            .await;
    }
}
