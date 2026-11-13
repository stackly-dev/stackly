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
    DesiredState, EnvironmentId, ErrorCodeName, IdentityEvidence, InstanceBlocker, InstanceId,
    InstanceLifecycle, InstanceName, LeaseFilter, LifecycleEvent, Pid, SessionId, StageName,
    StopDecision, StopIntent, StopRequest as LeaseStopRequest, evaluate_stop,
};
use stackly_platform::{ChildRole, ExitStatus, ProcessError, ProviderSignal, SignalTarget};
use tokio::time::Instant;

use super::Supervisor;
use super::commit::InstanceChanges;
use super::error::SupervisorError;
use super::error::{EnvironmentHolder, environment_holders};
use super::exit::finish_process;
use super::helper::HelperEnd;
use super::init::config_invalid;
use super::persist::{
    CommitMode, StopCommit, StopGate, apply_event, apply_stopping_exit, blocking, commit_exit,
    commit_instance, commit_stop_decision, read_instance,
};
use super::prepare::{Checks, prepare};
use super::steps::run_command;
use crate::operation::{
    AuditNote, BoxFuture, OperationContext, OperationOutcome, OperationWork, PartialItem,
    PartialOutcome, StepSpec, WorkError, WorkOutcome, WorkResult,
};
use crate::provider::{StopPlan, StopSignal};
use crate::secret::ResolvedSecrets;
use stackly_domain::ResourceRef;

/// The journal input of the child `stop_instance` an environment that goes
/// down begins (spec 0047): its trigger is `environment_down`, recorded
/// where the stop that ends a session records `session_end`.
pub const ENVIRONMENT_DOWN_INPUT: &str = r#"{"trigger":"environment_down"}"#;

/// The `code` of the partial item a stop for an environment that goes down
/// returns when another lease still holds the instance, so it kept running
/// (spec 0047, *Operasi `environment_down`*).
pub const INSTANCE_IN_USE: ErrorCodeName = ErrorCodeName::from_static("env.instance_in_use");

/// Which leases a stop reads before it writes desired `stopped` (spec
/// 0047, *Stop dengan guard lease*). It follows the request, not the
/// [`StopIntent`]: a restart's stop and a cancelled start share
/// `StopIntent::User` with a person's stop but read no lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LeaseCheck {
    /// No lease is read: restart, a cancelled start, reconcile, planned
    /// restart, session end.
    None,
    /// A person stops the instance (BR-015).
    Manual {
        /// The login session of this daemon, whose startup lease the stop
        /// suppresses.
        session: Option<SessionId>,
        /// Whether the person chose to stop it although environments hold
        /// it.
        override_leases: bool,
    },
    /// An environment goes down (BR-014).
    EnvironmentDown(EnvironmentId),
}

/// What [`Supervisor::stop_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StopRequest {
    /// The instance to stop.
    pub instance: InstanceId,
    /// Whether the user chose to stop it although environments hold it
    /// (BR-015).
    pub override_leases: bool,
    /// Who asks (spec 0030): a person checks the leases of environments
    /// and writes desired `stopped`, the daemon after reconciliation
    /// checks no lease because the user already chose `stopped`, and a
    /// planned restart writes no desired state at all.
    pub(crate) intent: StopIntent,
    /// The environment that goes down, for the stop it begins (spec 0047).
    pub(crate) environment: Option<EnvironmentId>,
}

impl StopRequest {
    /// Asks to stop `instance`, without overriding environment leases.
    #[must_use]
    pub const fn new(instance: InstanceId) -> Self {
        Self {
            instance,
            override_leases: false,
            intent: StopIntent::User,
            environment: None,
        }
    }

    /// Asks to stop `instance` because `environment` goes down (spec 0047
    /// AC-5): the stop writes desired `stopped` only while no other lease
    /// is active, and otherwise keeps the instance running and says so.
    #[must_use]
    pub const fn environment_down(instance: InstanceId, environment: EnvironmentId) -> Self {
        Self {
            instance,
            override_leases: false,
            intent: StopIntent::User,
            environment: Some(environment),
        }
    }

    /// A stop the daemon makes after reconciliation, with no lease check.
    #[must_use]
    pub(crate) const fn reconcile(instance: InstanceId) -> Self {
        Self {
            instance,
            override_leases: false,
            intent: StopIntent::Reconcile,
            environment: None,
        }
    }

    /// A stop the daemon makes for a planned restart: no lease check, and
    /// the desired state stays as it is so the next daemon starts the
    /// instance again.
    #[must_use]
    pub(crate) const fn planned_restart(instance: InstanceId) -> Self {
        Self {
            instance,
            override_leases: false,
            intent: StopIntent::PlannedRestart,
            environment: None,
        }
    }

    /// A stop that ends a login session (spec 0031, *Akhir sesi*): like a
    /// planned restart it checks no lease and writes no desired state, no
    /// suppression and no `stopped`, so the instance comes back with the
    /// next start queue.
    #[must_use]
    pub(crate) const fn session_end(instance: InstanceId) -> Self {
        Self::planned_restart(instance)
    }

    /// Returns the request with the override of environment leases set.
    #[must_use]
    pub const fn overriding_leases(mut self) -> Self {
        self.override_leases = true;
        self
    }

    /// The lease check of this request (spec 0047, the table of callers):
    /// a stop for an environment that goes down checks that environment's
    /// leases, a person's stop checks the environments' leases with its
    /// override, and the daemon's own stops check none.
    pub(crate) fn lease_check(&self, session: Option<SessionId>) -> LeaseCheck {
        match (self.environment, self.intent) {
            (Some(environment), _) => LeaseCheck::EnvironmentDown(environment),
            (None, StopIntent::User) => LeaseCheck::Manual {
                session,
                override_leases: self.override_leases,
            },
            (None, StopIntent::Reconcile | StopIntent::PlannedRestart) => LeaseCheck::None,
        }
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

/// What [`Supervisor::journal_stop`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopJournal {
    /// The lifecycle is `stopping`: a process may still have to stop.
    Begun,
    /// Nothing runs any more: the stop has no `stop` stage to run.
    AlreadyStopped,
    /// Another lease holds the instance, so an environment that goes down
    /// keeps it: nothing was written (spec 0047 AC-5).
    Kept,
}

/// What a guarded stop commit decided, as the callers of the stop paths
/// act on it: go on, or keep the instance.
fn settled(commit: StopCommit) -> Result<SettledStop, SupervisorError> {
    match commit {
        StopCommit::Committed { overridden } => Ok(SettledStop::Committed(overridden)),
        StopCommit::Kept => Ok(SettledStop::Kept),
        StopCommit::RequiresOverride(holders) => {
            Err(SupervisorError::StopRequiresOverride { holders })
        }
    }
}

/// How a stop commit with a lease check ended, short of an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettledStop {
    /// The stop committed; the environments a person's override passed.
    Committed(Vec<EnvironmentHolder>),
    /// An environment that goes down keeps the instance.
    Kept,
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
        let end = self.signal_and_wait(id, abandoned).await?;
        if end == StopEnd::Stopped {
            self.settle_stopping(id).await?;
        }
        Ok(end)
    }

    /// Makes sure a stop whose process is gone does not leave the lifecycle
    /// `stopping`. The exit handler writes `exited`, but an exit that was
    /// already being recorded as something else when the stop journaled
    /// (the process died on its own at that moment) could finish before it
    /// saw `stopping`; this closes that window.
    pub(crate) async fn settle_stopping(&self, id: InstanceId) -> Result<(), SupervisorError> {
        let now = self.deps().platform.clock().now();
        commit_exit(self, id, None, CommitMode::Actor, |changes, force| {
            if changes.instance().lifecycle() == InstanceLifecycle::Stopping {
                apply_stopping_exit(changes, force, now)?;
            }
            Ok(Vec::new())
        })
        .await
    }

    /// Ends a stop whose process left before the stop took the lock: the
    /// desired state becomes `stopped`, and a lifecycle the exit handler
    /// left `stopping` goes on to `stopped`.
    ///
    /// With a lease check the commit is guarded (spec 0047): the lease
    /// decision guards only desired `stopped` and the lease ops, and
    /// `stopping` always goes on to `stopped`, so no instance is left
    /// `stopping` without a process. Nothing runs, so a person's stop never
    /// needs an override here.
    pub(crate) async fn settle_gone_process(
        &self,
        id: InstanceId,
        intent: StopIntent,
        check: LeaseCheck,
    ) -> Result<SettledStop, SupervisorError> {
        let now = self.deps().platform.clock().now();
        if check == LeaseCheck::None {
            commit_exit(self, id, None, CommitMode::Operation, |changes, force| {
                if intent.writes_desired() && changes.instance().desired() != DesiredState::Stopped
                {
                    changes.apply(|instance| {
                        instance.set_desired(DesiredState::Stopped);
                        Ok::<_, SupervisorError>(())
                    })?;
                }
                if changes.instance().lifecycle() == InstanceLifecycle::Stopping {
                    apply_stopping_exit(changes, force, now)?;
                }
                Ok(Vec::new())
            })
            .await?;
            return Ok(SettledStop::Committed(Vec::new()));
        }
        let gate = StopGate { check, quiet: true };
        let commit = commit_stop_decision(self, id, gate, true, |changes, force, stop| {
            if stop
                && intent.writes_desired()
                && changes.instance().desired() != DesiredState::Stopped
            {
                changes.apply(|instance| {
                    instance.set_desired(DesiredState::Stopped);
                    Ok::<_, SupervisorError>(())
                })?;
            }
            if changes.instance().lifecycle() == InstanceLifecycle::Stopping {
                apply_stopping_exit(changes, force, now)?;
            }
            Ok(Vec::new())
        })
        .await?;
        settled(commit)
    }

    async fn signal_and_wait(
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
        // Both callers saw a process before they journaled the stop. An
        // empty slot now means its exit was handled in between, so there is
        // nothing left to stop (`stop_process` closes `stopping`).
        let Some((generation, identity, mut done)) = target else {
            return Ok(StopEnd::Stopped);
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
                    Ok(()) => actor.update(deps, |state| {
                        if let Some(slot) = state
                            .process
                            .as_mut()
                            .filter(|slot| slot.generation == generation)
                        {
                            slot.sent_signal = Some(signal.number());
                        }
                    }),
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

    /// Records the start of a stop from the aggregate read inside the
    /// commit: the desired state becomes `stopped`, and a lifecycle that
    /// can begin a stop becomes `stopping` (one that already is `stopping`
    /// stays). With `no_process` the lifecycle goes on to `stopped` at
    /// once, because no process remains to stop (an instance paused between
    /// two respawns, or one whose start was cancelled before it spawned).
    /// A lifecycle that is `stopped`, `failed`, or `uninitialized` has
    /// nothing left to stop, because the process left after the caller saw
    /// it: only the desired state is written, and the answer says so.
    ///
    /// With a lease check the whole commit is guarded (spec 0047): a lease
    /// decision other than a stop writes nothing, and the answer is
    /// [`StopJournal::Kept`] or [`SupervisorError::StopRequiresOverride`].
    /// The second value names the environments a person's override passed.
    pub(crate) async fn journal_stop(
        &self,
        id: InstanceId,
        no_process: bool,
        intent: StopIntent,
        check: LeaseCheck,
    ) -> Result<(StopJournal, Vec<EnvironmentHolder>), SupervisorError> {
        let now = self.deps().platform.clock().now();
        let mut journaled = StopJournal::Begun;
        let mut build = |changes: &mut InstanceChanges| {
            if intent.writes_desired() && changes.instance().desired() != DesiredState::Stopped {
                changes.apply(|instance| {
                    instance.set_desired(DesiredState::Stopped);
                    Ok::<_, SupervisorError>(())
                })?;
            }
            journaled = StopJournal::Begun;
            match changes.instance().lifecycle() {
                InstanceLifecycle::Stopped
                | InstanceLifecycle::Failed
                | InstanceLifecycle::Uninitialized => {
                    journaled = StopJournal::AlreadyStopped;
                    return Ok(Vec::new());
                }
                InstanceLifecycle::Stopping => {}
                _ => apply_event(changes, LifecycleEvent::StopBegin, now)?,
            }
            if no_process {
                // No process left, so `last_exit` stays as it is.
                apply_event(changes, LifecycleEvent::Exited { exit: None }, now)?;
            }
            Ok(Vec::new())
        };
        if check == LeaseCheck::None {
            commit_instance(self, id, None, CommitMode::Operation, build).await?;
            return Ok((journaled, Vec::new()));
        }
        let gate = StopGate {
            check,
            quiet: false,
        };
        let commit = commit_stop_decision(self, id, gate, false, |changes, _, stop| {
            if stop { build(changes) } else { Ok(Vec::new()) }
        })
        .await?;
        Ok(match settled(commit)? {
            SettledStop::Committed(overridden) => (journaled, overridden),
            SettledStop::Kept => (StopJournal::Kept, Vec::new()),
        })
    }

    /// Records that the identity of the process changed: the instance
    /// becomes `unknown`, with the `identityUnproven` blocker, and nothing
    /// is signalled again.
    pub(crate) async fn identity_lost(&self, id: InstanceId, generation: u64, pid: Pid) {
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

/// The outcome of a stop for an environment that goes down when another
/// lease keeps the instance (spec 0047, *Operasi `environment_down`*): no
/// resource stopped, and one skipped partial item with
/// [`INSTANCE_IN_USE`].
fn kept_outcome(id: InstanceId, name: &InstanceName) -> WorkOutcome {
    WorkOutcome {
        result: OperationOutcome {
            resources: Vec::new(),
            partial: vec![PartialItem {
                item: name.to_string(),
                resource: Some(ResourceRef::Instance(id)),
                outcome: PartialOutcome::Skipped,
                code: Some(INSTANCE_IN_USE.to_string()),
            }],
        },
        finalize: None,
    }
}

/// Marks in the audit note that the stop did not happen (spec 0035, the
/// rows added by spec 0047), and returns the kept outcome.
fn kept(ctx: &OperationContext, id: InstanceId, name: &InstanceName) -> WorkOutcome {
    ctx.set_audit_note(AuditNote::new(None).with("kept", true));
    kept_outcome(id, name)
}

/// Notes the environments a person's override stopped the instance
/// against (spec 0047 AC-13): their names, in the order the guard gave.
fn note_overridden(ctx: &OperationContext, overridden: &[EnvironmentHolder]) {
    if overridden.is_empty() {
        return;
    }
    let names: Vec<&str> = overridden
        .iter()
        .map(|holder| holder.name.as_str())
        .collect();
    ctx.set_audit_note(AuditNote::new(None).with("overriddenEnvironments", names.join(", ")));
}

/// Refuses a person's stop of a live instance that environments hold
/// without the override, before the stop takes its lock (BR-015). The
/// commit's guard checks the same again; this read only fails early.
async fn early_override_check(
    sup: &Supervisor,
    id: InstanceId,
    check: &LeaseCheck,
) -> Result<(), SupervisorError> {
    let LeaseCheck::Manual {
        session,
        override_leases: false,
    } = check
    else {
        return Ok(());
    };
    let registry = sup.deps().registry.clone();
    let leases = blocking(move || registry.leases(id, LeaseFilter::Unreleased))
        .await?
        .map_err(SupervisorError::Registry)?
        .value;
    let decision = evaluate_stop(
        id,
        &leases,
        &LeaseStopRequest::Manual {
            session: session.clone(),
            override_leases: false,
        },
    );
    if !matches!(decision, StopDecision::RequiresOverride { .. }) {
        return Ok(());
    }
    let registry = sup.deps().registry.clone();
    let environments = blocking(move || registry.environments())
        .await?
        .map_err(SupervisorError::Registry)?
        .value;
    Err(SupervisorError::StopRequiresOverride {
        holders: environment_holders(id, &leases, &environments),
    })
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
    let check = request.lease_check(sup.session());

    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let instance = read_instance(deps, id).await.map_err(failed)?;
    let name = instance.name().clone();
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
            // Nothing runs; only a stored desired `running` changes, and a
            // planned restart leaves even that alone. A person's stop ends
            // what keeps the instance wanted (spec 0031, AC-8) in the same
            // commit: the guard releases the manual leases and suppresses
            // the startup lease of this session.
            if request.intent.writes_desired() && instance.desired() == DesiredState::Running {
                if check == LeaseCheck::None {
                    commit_instance(sup, id, None, CommitMode::Operation, |changes| {
                        changes.apply(|instance| {
                            instance.set_desired(DesiredState::Stopped);
                            Ok::<_, SupervisorError>(())
                        })?;
                        Ok(Vec::new())
                    })
                    .await
                    .map_err(failed)?;
                } else {
                    let gate = StopGate {
                        check: check.clone(),
                        quiet: true,
                    };
                    let commit = commit_stop_decision(sup, id, gate, false, |changes, _, stop| {
                        if stop && changes.instance().desired() != DesiredState::Stopped {
                            changes.apply(|instance| {
                                instance.set_desired(DesiredState::Stopped);
                                Ok::<_, SupervisorError>(())
                            })?;
                        }
                        Ok(Vec::new())
                    })
                    .await
                    .map_err(failed)?;
                    if settled(commit).map_err(failed)? == SettledStop::Kept {
                        return Ok(kept(&ctx, id, &name));
                    }
                }
            }
            return Ok(outcome());
        }
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering
        | InstanceLifecycle::Stopping => {}
    }
    early_override_check(sup, id, &check)
        .await
        .map_err(failed)?;

    ctx.begin_step(StepSpec::new(StageName::LOCK)).await?;
    ctx.set_stage(StageName::LOCK).await?;

    let actor = sup.actor(id);
    let has_process = actor.read(|state| state.process.is_some());
    let mut lifecycle = instance.lifecycle();
    if !has_process
        && matches!(
            lifecycle,
            InstanceLifecycle::Starting | InstanceLifecycle::Running | InstanceLifecycle::Stopping
        )
    {
        // The exit handler records the lifecycle before it clears the
        // process, so an empty slot makes a fresh read current. A process
        // that left between the read in `validate` and now is a stop that
        // is done, or a crash the stop takes over from; only a process the
        // actor never knew is left to reconciliation.
        lifecycle = read_instance(deps, id).await.map_err(failed)?.lifecycle();
        match lifecycle {
            // This path ends the operation from the `lock` stage, without
            // the `journal`, `stop`, and `complete` stages.
            InstanceLifecycle::Stopping
            | InstanceLifecycle::Stopped
            | InstanceLifecycle::Failed => {
                let settled = sup
                    .settle_gone_process(id, request.intent, check)
                    .await
                    .map_err(failed)?;
                return Ok(match settled {
                    SettledStop::Committed(_) => outcome(),
                    SettledStop::Kept => kept(&ctx, id, &name),
                });
            }
            // A crash with a respawn pending: the stop goes on and ends
            // the pause.
            InstanceLifecycle::Recovering => {}
            _ => return Err(failed(SupervisorError::IdentityUnproven { pid: None })),
        }
    }

    // The pause before a respawn holds no operation, so the stop has the
    // lock; it ends the pause. A stop with a lease check ends it only once
    // its guard committed, so an instance the guard keeps keeps its
    // respawn (spec 0047, *Respawn*).
    let checked = check != LeaseCheck::None;
    if !checked {
        actor.cancel_respawn(deps);
    }

    ctx.begin_step(StepSpec::new(StageName::JOURNAL).irreversible())
        .await?;
    ctx.set_stage(StageName::JOURNAL).await?;
    let no_process = !has_process && lifecycle == InstanceLifecycle::Recovering;
    // A pause between two respawns has no process to stop.
    let (journaled, overridden) = sup
        .journal_stop(id, no_process, request.intent, check)
        .await
        .map_err(failed)?;
    if journaled == StopJournal::Kept {
        return Ok(kept(&ctx, id, &name));
    }
    if checked {
        actor.cancel_respawn(deps);
    }
    note_overridden(&ctx, &overridden);

    if has_process && journaled == StopJournal::Begun {
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
