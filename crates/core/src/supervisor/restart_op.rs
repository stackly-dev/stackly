//! Restarting an instance as one operation (spec 0026, *Supervisor:
//! restart*, AC-6 to AC-9). This is the `restart_instance` operation; the
//! restart *policy* after a crash lives in [`restart`](super::restart).
//!
//! The stop half runs the provider's stop strategy exactly like a stop (no
//! `SIGKILL`, no force), after a journal that writes desired `stopped`, so a
//! restart that fails halfway leaves the instance stopped and never starts
//! it on its own. The start half begins only once the process exited and
//! the instance lock is free, so two main processes never run on the same
//! data; it is the startup transaction of spec 0010 under the same
//! operation, with its journal writing desired `running`.

use stackly_domain::{
    IdentityEvidence, InstanceBlocker, InstanceId, InstanceLifecycle, LockState as DomainLock,
    ResourceRef, StageName, StopIntent,
};
use stackly_platform::LockState;
use tokio::time::Instant;

use super::Supervisor;
use super::actor::restart_status;
use super::error::SupervisorError;
use super::persist::{active_revision_note, blocking, read_instance};
use super::startup::{StartRequest, start_stopped};
use super::stop::{StopEnd, StopJournal};
use crate::operation::{
    BoxFuture, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError, WorkOutcome,
    WorkResult,
};
use crate::secret::SecretCaller;

/// What [`Supervisor::restart_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RestartRequest {
    /// The instance to restart.
    pub instance: InstanceId,
    /// Who asks, which decides whether the secret store may show a dialog
    /// when the start half resolves secrets, as for a user start.
    pub caller: SecretCaller,
}

impl RestartRequest {
    /// Asks to restart `instance` for `caller`.
    #[must_use]
    pub const fn new(instance: InstanceId, caller: SecretCaller) -> Self {
        Self { instance, caller }
    }
}

impl Supervisor {
    /// Builds the operation work of `restart_instance` for the engine.
    #[must_use]
    pub fn restart_work(&self, request: RestartRequest) -> Box<dyn OperationWork> {
        Box::new(RestartWork {
            sup: self.clone(),
            request,
        })
    }
}

/// The work of `restart_instance`.
struct RestartWork {
    sup: Supervisor,
    request: RestartRequest,
}

impl OperationWork for RestartWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_restart(&self.sup, &self.request, ctx).await })
    }
}

fn failed(error: SupervisorError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

/// Checks the lifecycle a restart may act on (AC-6, AC-7): `running` and
/// `recovering` go on, everything else is refused with the code of AC-7.
pub(crate) fn restart_allowed(lifecycle: InstanceLifecycle) -> Result<(), SupervisorError> {
    match lifecycle {
        InstanceLifecycle::Running | InstanceLifecycle::Recovering => Ok(()),
        InstanceLifecycle::Stopping => Err(SupervisorError::InstanceStopping),
        InstanceLifecycle::Unknown => Err(SupervisorError::IdentityUnproven { pid: None }),
        InstanceLifecycle::Initializing => Err(SupervisorError::InstanceInitializing),
        InstanceLifecycle::Retained => Err(SupervisorError::InstanceRetained),
        InstanceLifecycle::Starting
        | InstanceLifecycle::Stopped
        | InstanceLifecycle::Failed
        | InstanceLifecycle::Uninitialized => {
            Err(SupervisorError::InstanceNotRunning { lifecycle })
        }
    }
}

/// A stop error inside a restart: the same code and details, with the note
/// that the instance stays stopped (AC-8).
fn halfway(error: SupervisorError) -> WorkError {
    match error {
        SupervisorError::StopTimeout { .. } | SupervisorError::StopCommandFailed { .. } => {
            failed(SupervisorError::RestartStoppedHalfway(Box::new(error)))
        }
        other => failed(other),
    }
}

async fn run_restart(
    sup: &Supervisor,
    request: &RestartRequest,
    mut ctx: OperationContext,
) -> WorkResult {
    let deps = sup.deps();
    let id = request.instance;

    // The engine queued the restart behind any operation that held the
    // instance, a respawn included, so this read is the fresh state.
    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let instance = read_instance(deps, id).await.map_err(failed)?;
    ctx.set_audit_note(active_revision_note(deps, id).await);
    restart_allowed(instance.lifecycle()).map_err(failed)?;

    ctx.begin_step(StepSpec::new(StageName::LOCK)).await?;
    ctx.set_stage(StageName::LOCK).await?;
    restart_steps(sup, request, ctx, instance.lifecycle()).await
}

/// The restart after its checks: from the stop journal to the start, under
/// the operation `ctx`. `apply_config` and `change_endpoints` run it as
/// their restart sub step (spec 0029), after their own validate and lock.
pub(crate) async fn restart_steps(
    sup: &Supervisor,
    request: &RestartRequest,
    mut ctx: OperationContext,
    lifecycle: InstanceLifecycle,
) -> WorkResult {
    let deps = sup.deps();
    let id = request.instance;
    let actor = sup.actor(id);
    let caller = request.caller.clone().in_operation(ctx.id());
    let has_process = actor.read(|state| state.process.is_some());
    let mut lifecycle = lifecycle;
    if !has_process && lifecycle == InstanceLifecycle::Running {
        // The exit handler records the lifecycle before it clears the
        // slot, so a fresh read tells what became of the process.
        lifecycle = read_instance(deps, id).await.map_err(failed)?.lifecycle();
        restart_allowed(lifecycle).map_err(failed)?;
    }
    // A pause before a respawn holds no operation: the restart ends it and
    // starts at once.
    actor.cancel_respawn(deps);

    ctx.begin_step(StepSpec::new(StageName::JOURNAL).irreversible())
        .await?;
    ctx.set_stage(StageName::JOURNAL).await?;
    let no_process = !has_process && lifecycle == InstanceLifecycle::Recovering;
    let journaled = sup
        .journal_stop(id, no_process, StopIntent::User, Vec::new())
        .await
        .map_err(failed)?;
    // The restart window starts from zero, whatever came before (AC-6).
    actor.update(deps, |state| {
        state.restart.clear();
        state.restart_status = restart_status(&deps.config, 0, None, false);
    });

    if has_process && journaled == StopJournal::Begun {
        ctx.begin_step(StepSpec::new(StageName::STOP).irreversible())
            .await?;
        ctx.set_stage(StageName::STOP).await?;
        let abandoned = {
            let ctx = &ctx;
            async move {
                ctx.stopped().await;
                if !matches!(ctx.check_cancel(), Err(WorkError::Abandoned)) {
                    // A cancel does not interrupt a stop: it finishes, and
                    // the restart ends cancelled with the instance stopped.
                    std::future::pending::<()>().await;
                }
            }
        };
        match sup.stop_process(id, abandoned).await.map_err(halfway)? {
            StopEnd::Stopped => {}
            StopEnd::Abandoned => return Err(WorkError::Abandoned),
        }

        ctx.begin_step(StepSpec::new(StageName::AWAIT_EXIT).irreversible())
            .await?;
        ctx.set_stage(StageName::AWAIT_EXIT).await?;
        sup.settle_stopping(id).await.map_err(failed)?;
    }
    // Desired is `stopped` from here on: a cancel leaves it stopped.
    ctx.check_cancel()?;

    ctx.begin_step(StepSpec::new(StageName::LOCK_WAIT).irreversible())
        .await?;
    ctx.set_stage(StageName::LOCK_WAIT).await?;
    wait_for_free_lock(sup, &ctx, id).await?;

    ctx.begin_step(StepSpec::new(StageName::START).irreversible())
        .await?;
    ctx.set_stage(StageName::START).await?;
    let lifecycle = read_instance(deps, id).await.map_err(failed)?.lifecycle();
    if !matches!(
        lifecycle,
        InstanceLifecycle::Stopped | InstanceLifecycle::Failed
    ) {
        return Err(failed(SupervisorError::InstanceNotRunning { lifecycle }));
    }
    let start = StartRequest::user(id, caller.clone());
    let started = start_stopped(sup, &start, &mut ctx, &caller).await?;
    Ok(WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Instance(id)],
            partial: started.result.partial,
        },
        finalize: started.finalize,
    })
}

/// Waits until no process holds the instance lock, trying every
/// `restart_lock_poll` for at most `restart_lock_wait` (AC-8). A lock still
/// held puts the `dataInUse` blocker on the instance and fails with the
/// members the old process left in its group.
async fn wait_for_free_lock(
    sup: &Supervisor,
    ctx: &OperationContext,
    id: InstanceId,
) -> Result<(), WorkError> {
    let deps = sup.deps();
    let path = deps.layout.instance_paths(id).lock;
    let deadline = Instant::now() + deps.config.restart_lock_wait;
    loop {
        let platform = deps.platform.clone();
        let probe = path.clone();
        let state = blocking(move || platform.processes().probe_lock(&probe))
            .await
            .map_err(failed)?;
        match state {
            Ok(LockState::Free) => return Ok(()),
            Ok(LockState::Held) => {}
            Err(error) => return Err(failed(SupervisorError::from_process(&error))),
        }
        ctx.check_cancel()?;
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(deps.config.restart_lock_poll).await;
    }
    let actor = sup.actor(id);
    let now = deps.platform.clock().now();
    let leftover = actor.read(|state| state.leftover.clone());
    let blocker = InstanceBlocker::DataInUse {
        evidence: IdentityEvidence::new().with_lock(DomainLock::Held),
        observed_at: now,
    };
    actor.update(deps, |state| state.set_supervisor_blocker(Some(blocker)));
    Err(failed(SupervisorError::DataStillHeld {
        lock_path: path.display().to_string(),
        leftover,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationError as _;

    #[test]
    fn only_running_and_recovering_restart() {
        for lifecycle in [InstanceLifecycle::Running, InstanceLifecycle::Recovering] {
            assert!(restart_allowed(lifecycle).is_ok(), "{lifecycle}");
        }
    }

    #[test]
    fn every_other_lifecycle_gets_the_code_of_ac_7() {
        let cases = [
            (
                InstanceLifecycle::Starting,
                "supervisor.instance_not_running",
            ),
            (
                InstanceLifecycle::Stopped,
                "supervisor.instance_not_running",
            ),
            (InstanceLifecycle::Failed, "supervisor.instance_not_running"),
            (
                InstanceLifecycle::Uninitialized,
                "supervisor.instance_not_running",
            ),
            (InstanceLifecycle::Stopping, "supervisor.instance_stopping"),
            (InstanceLifecycle::Unknown, "supervisor.identity_unproven"),
            (
                InstanceLifecycle::Initializing,
                "supervisor.instance_initializing",
            ),
            (InstanceLifecycle::Retained, "supervisor.instance_retained"),
        ];
        for (lifecycle, code) in cases {
            let error = restart_allowed(lifecycle).unwrap_err();
            assert_eq!(error.code().to_string(), code, "{lifecycle}");
        }
    }

    #[test]
    fn a_stop_timeout_in_a_restart_keeps_its_code_and_says_it_stays_stopped() {
        let WorkError::Failed(error) = halfway(SupervisorError::StopTimeout { timeout_seconds: 3 })
        else {
            panic!("a failed restart");
        };
        assert_eq!(error.code().to_string(), "supervisor.stop_timeout");
        let details: Vec<(String, String)> = error
            .details()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        assert!(details.contains(&("timeoutSeconds".to_owned(), "3".to_owned())));
        assert!(details.contains(&("after".to_owned(), "stays_stopped".to_owned())));
    }
}
