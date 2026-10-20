//! Stopping an instance safely (spec 0010, *Stop*, AC-5).
//!
//! A normal stop runs only the provider's strategy: a signal to the main
//! process whose identity is checked again right before `kill`, or the
//! provider's stop command. It never sends `SIGKILL`, never kills by name,
//! and never signals a process whose check failed; that instance becomes
//! `unknown`. When the budget runs out the operation fails with
//! `supervisor.stop_timeout`, the lifecycle stays `stopping`, and the
//! process stays watched: the exit handler that ends every stop finishes it
//! later.

use std::collections::BTreeMap;
use std::future::Future;

use stackly_domain::{
    DesiredState, IdentityEvidence, InstanceBlocker, InstanceId, InstanceLifecycle, LeaseFilter,
    LifecycleEvent, Pid, StageName, StopDecision, StopRequest as LeaseStopRequest, evaluate_stop,
};
use stackly_platform::{ChildRole, ExitStatus, ProcessError, ProviderSignal, SignalTarget};
use tokio::time::Instant;

use super::Supervisor;
use super::error::SupervisorError;
use super::exit::finish_process;
use super::helper::HelperEnd;
use super::init::config_invalid;
use super::persist::{CommitMode, apply_event, blocking, commit_instance, read_instance};
use super::prepare::{Checks, prepare};
use super::steps::run_command;
use crate::operation::{
    BoxFuture, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError, WorkOutcome,
    WorkResult,
};
use crate::provider::{StopPlan, StopSignal};
use crate::secret::ResolvedSecrets;
use stackly_domain::ResourceRef;

/// What [`Supervisor::stop_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StopRequest {
    /// The instance to stop.
    pub instance: InstanceId,
    /// Whether the user chose to stop it although environments hold it
    /// (BR-015).
    pub override_leases: bool,
    /// A stop the daemon makes itself after reconciliation (trigger
    /// `reconcile`): it does not ask whether environments hold the
    /// instance, because the user already chose `stopped`.
    pub(crate) system: bool,
}

impl StopRequest {
    /// Asks to stop `instance`, without overriding environment leases.
    #[must_use]
    pub const fn new(instance: InstanceId) -> Self {
        Self {
            instance,
            override_leases: false,
            system: false,
        }
    }

    /// A stop the daemon makes after reconciliation, with no lease check.
    #[must_use]
    pub(crate) const fn reconcile(instance: InstanceId) -> Self {
        Self {
            instance,
            override_leases: false,
            system: true,
        }
    }

    /// Returns the request with the override of environment leases set.
    #[must_use]
    pub const fn overriding_leases(mut self) -> Self {
        self.override_leases = true;
        self
    }
}

/// How a stop of a live process ended, short of an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopEnd {
    /// The process exited and the exit handler recorded it.
    Stopped,
    /// A shutdown abandoned the operation; the process is left alone.
    Abandoned,
}

impl Supervisor {
    /// Builds the operation work of `stop_instance` for the engine.
    #[must_use]
    pub fn stop_work(&self, request: StopRequest) -> Box<dyn OperationWork> {
        Box::new(StopWork {
            sup: self.clone(),
            request,
        })
    }

    /// Runs the provider's stop strategy on the live process of `id` and
    /// waits for its exit to be recorded, within the strategy's budget.
    ///
    /// `abandoned` completes when a shutdown abandons the operation: the
    /// process is then left alone and the exit handler finishes the stop.
    ///
    /// # Errors
    ///
    /// Returns `supervisor.identity_unproven` when no process is known,
    /// `process.signal_refused` when the identity check failed (the instance
    /// becomes `unknown`), `supervisor.stop_command_failed`, and
    /// `supervisor.stop_timeout`.
    pub(crate) async fn stop_process(
        &self,
        id: InstanceId,
        abandoned: impl Future<Output = ()>,
    ) -> Result<StopEnd, SupervisorError> {
        let deps = self.deps();
        let actor = self.actor(id);
        let target = actor.read(|state| {
            state.process.as_ref().map(|slot| {
                (
                    slot.generation,
                    slot.identity.clone(),
                    slot.done.subscribe(),
                )
            })
        });
        let Some((generation, identity, mut done)) = target else {
            return Err(SupervisorError::IdentityUnproven { pid: None });
        };
        actor.update(deps, |state| state.stopping = true);
        if *done.borrow() {
            return Ok(StopEnd::Stopped);
        }
        let prepared = prepare(self, id, Checks::Plan).await?;
        let context = prepared.context(&BTreeMap::new());
        let plan = prepared
            .package
            .run_plan(&context)
            .map_err(|error| config_invalid(&error))?;
        let started = Instant::now();
        let budget = match &plan.stop {
            StopPlan::Signal { timeout, .. } | StopPlan::Command { timeout, .. } => *timeout,
        };
        match &plan.stop {
            StopPlan::Signal { signal, .. } => {
                let signal = match signal {
                    StopSignal::Term => ProviderSignal::Term,
                    StopSignal::Int => ProviderSignal::Int,
                    StopSignal::Quit => ProviderSignal::Quit,
                };
                let platform = deps.platform.clone();
                let target = SignalTarget::new(identity.clone());
                let sent = blocking(move || platform.processes().signal(&target, signal)).await?;
                match sent {
                    Ok(()) => {}
                    // The process is gone already: its exit is on the way.
                    Err(ProcessError::NotFound { .. }) => {}
                    Err(ProcessError::IdentityMismatch { pid }) => {
                        self.identity_lost(id, generation, pid).await;
                        return Err(SupervisorError::SignalRefused { pid });
                    }
                    Err(error) => return Err(SupervisorError::from_process(&error)),
                }
            }
            StopPlan::Command { run, .. } => {
                let secrets = ResolvedSecrets::new(BTreeMap::new());
                let ended = run_command(
                    deps,
                    &prepared,
                    run,
                    &secrets,
                    ChildRole::StopCommand,
                    None,
                    budget,
                )
                .await
                .map_err(|reason| SupervisorError::ConfigInvalid {
                    field: "stop command".to_owned(),
                    reason,
                })?;
                match ended {
                    HelperEnd::TimedOut => {
                        return Err(SupervisorError::StopTimeout {
                            timeout_seconds: budget.as_secs(),
                        });
                    }
                    HelperEnd::Exited(notice) => match notice.status {
                        Some(ExitStatus::Exited(code))
                            if u8::try_from(code)
                                .is_ok_and(|code| run.success_exit_codes.contains(&code)) => {}
                        Some(ExitStatus::Exited(code)) => {
                            return Err(SupervisorError::StopCommandFailed { exit_code: code });
                        }
                        _ => return Err(SupervisorError::StopCommandFailed { exit_code: -1 }),
                    },
                }
            }
        }
        let remaining = budget.saturating_sub(started.elapsed());
        tokio::select! {
            biased;
            seen = done.wait_for(|finished| *finished) => {
                drop(seen);
                Ok(StopEnd::Stopped)
            }
            () = abandoned => Ok(StopEnd::Abandoned),
            () = tokio::time::sleep(remaining) => Err(SupervisorError::StopTimeout {
                timeout_seconds: budget.as_secs(),
            }),
        }
    }

    /// Records the start of a stop: the desired state becomes `stopped` and
    /// the lifecycle `stopping`; with `no_process` the lifecycle goes on to
    /// `stopped` at once, because no process remains to stop (an instance
    /// paused between two respawns, or one whose start was cancelled before
    /// it spawned).
    pub(crate) async fn journal_stop(
        &self,
        id: InstanceId,
        no_process: bool,
    ) -> Result<(), SupervisorError> {
        let now = self.deps().platform.clock().now();
        commit_instance(self, id, None, CommitMode::Operation, |changes| {
            if changes.instance().desired() != DesiredState::Stopped {
                changes.apply(|instance| {
                    instance.set_desired(DesiredState::Stopped);
                    Ok::<_, SupervisorError>(())
                })?;
            }
            if changes.instance().lifecycle() != InstanceLifecycle::Stopping {
                apply_event(changes, LifecycleEvent::StopBegin, now)?;
            }
            if no_process {
                apply_event(changes, LifecycleEvent::Exited, now)?;
            }
            Ok(Vec::new())
        })
        .await
    }

    /// Records that the identity of the process changed: the instance
    /// becomes `unknown`, with the `identityUnproven` blocker, and nothing
    /// is signalled again.
    async fn identity_lost(&self, id: InstanceId, generation: u64, pid: Pid) {
        let deps = self.deps();
        let now = deps.platform.clock().now();
        let committed = commit_instance(self, id, None, CommitMode::Actor, |changes| {
            apply_event(changes, LifecycleEvent::IdentityLost, now)?;
            Ok(Vec::new())
        })
        .await;
        if let Err(error) = committed {
            tracing::warn!(instance = %id, %error, "the loss of an identity could not be recorded");
        }
        let blocker = InstanceBlocker::IdentityUnproven {
            evidence: IdentityEvidence::new().with_pid(pid),
            observed_at: now,
        };
        let actor = self.actor(id);
        actor.update(deps, |state| state.offer_blocker(blocker));
        finish_process(self, &actor, generation, Vec::new());
    }
}

/// The work of `stop_instance`.
struct StopWork {
    sup: Supervisor,
    request: StopRequest,
}

impl OperationWork for StopWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_stop(&self.sup, &self.request, ctx).await })
    }
}

fn failed(error: SupervisorError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

async fn run_stop(
    sup: &Supervisor,
    request: &StopRequest,
    mut ctx: OperationContext,
) -> WorkResult {
    let deps = sup.deps();
    let id = request.instance;
    let outcome = || WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Instance(id)],
            partial: Vec::new(),
        },
        finalize: None,
    };

    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let instance = read_instance(deps, id).await.map_err(failed)?;
    match instance.lifecycle() {
        InstanceLifecycle::Retained => return Err(failed(SupervisorError::InstanceRetained)),
        InstanceLifecycle::Unknown => {
            return Err(failed(SupervisorError::IdentityUnproven { pid: None }));
        }
        InstanceLifecycle::Initializing => {
            return Err(failed(SupervisorError::InstanceInitializing));
        }
        InstanceLifecycle::Uninitialized
        | InstanceLifecycle::Stopped
        | InstanceLifecycle::Failed => {
            // Nothing runs; only a stored desired `running` changes.
            if instance.desired() == DesiredState::Running {
                commit_instance(sup, id, None, CommitMode::Operation, |changes| {
                    changes.apply(|instance| {
                        instance.set_desired(DesiredState::Stopped);
                        Ok::<_, SupervisorError>(())
                    })?;
                    Ok(Vec::new())
                })
                .await
                .map_err(failed)?;
            }
            return Ok(outcome());
        }
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering
        | InstanceLifecycle::Stopping => {}
    }
    if !request.system {
        let registry = deps.registry.clone();
        let leases = blocking(move || registry.leases(id, LeaseFilter::Unreleased))
            .await
            .map_err(failed)?
            .map_err(|error| failed(SupervisorError::Registry(error)))?
            .value;
        let decision = evaluate_stop(
            id,
            &leases,
            &LeaseStopRequest::Manual {
                session: None,
                override_leases: request.override_leases,
            },
        );
        if matches!(decision, StopDecision::RequiresOverride { .. }) {
            return Err(failed(SupervisorError::StopRequiresOverride));
        }
    }

    ctx.begin_step(StepSpec::new(StageName::LOCK)).await?;
    ctx.set_stage(StageName::LOCK).await?;

    let actor = sup.actor(id);
    let has_process = actor.read(|state| state.process.is_some());
    if !has_process
        && matches!(
            instance.lifecycle(),
            InstanceLifecycle::Starting | InstanceLifecycle::Running | InstanceLifecycle::Stopping
        )
    {
        // A process the actor does not know: reconciliation owns it.
        return Err(failed(SupervisorError::IdentityUnproven { pid: None }));
    }

    // The pause before a respawn holds no operation, so the stop has the
    // lock; it ends the pause.
    actor.cancel_respawn(deps);

    ctx.begin_step(StepSpec::new(StageName::JOURNAL).irreversible())
        .await?;
    ctx.set_stage(StageName::JOURNAL).await?;
    let no_process = !has_process && instance.lifecycle() == InstanceLifecycle::Recovering;
    // A pause between two respawns has no process to stop.
    sup.journal_stop(id, no_process).await.map_err(failed)?;

    if has_process {
        ctx.begin_step(StepSpec::new(StageName::STOP).irreversible())
            .await?;
        ctx.set_stage(StageName::STOP).await?;
        let abandoned = {
            let ctx = &ctx;
            async move {
                ctx.stopped().await;
                if !matches!(ctx.check_cancel(), Err(WorkError::Abandoned)) {
                    // A cancel does not interrupt a stop: it finishes.
                    std::future::pending::<()>().await;
                }
            }
        };
        match sup.stop_process(id, abandoned).await.map_err(failed)? {
            StopEnd::Stopped => {}
            StopEnd::Abandoned => return Err(WorkError::Abandoned),
        }
    }

    ctx.begin_step(StepSpec::new(StageName::COMPLETE).irreversible())
        .await?;
    ctx.set_stage(StageName::COMPLETE).await?;
    ctx.finish_step(None).await?;
    Ok(outcome())
}
