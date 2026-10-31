//! Force stopping an instance that is still `stopping` (spec 0026, *Supervisor:
//! force stop*, AC-2 to AC-4).
//!
//! A force stop is the deliberate way out of a stop that did not finish: it
//! runs only while the lifecycle is `stopping`, and sends `SIGKILL` to the
//! main process alone, through
//! [`ProcessSupervision::force_kill`](stackly_platform::ProcessSupervision::force_kill),
//! right after its identity is checked again. A process whose identity no
//! longer matches is never signalled: the instance becomes `unknown`. The
//! `kill` step is in the operation journal before the signal leaves, so the
//! exit handler records the exit as `forced`, also after a daemon restart.
//! This is the one module `cargo xtask check-deps` lets call `force_kill`.

use stackly_domain::{DesiredState, InstanceId, InstanceLifecycle, ResourceRef, StageName};
use stackly_platform::{ProcessError, SignalTarget};

use super::Supervisor;
use super::error::SupervisorError;
use super::persist::{CommitMode, blocking, commit_instance, read_instance};
use crate::operation::{
    BoxFuture, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError, WorkOutcome,
    WorkResult,
};

/// What [`Supervisor::force_stop_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ForceStopRequest {
    /// The instance to force stop.
    pub instance: InstanceId,
}

impl ForceStopRequest {
    /// Asks to force stop `instance`.
    #[must_use]
    pub const fn new(instance: InstanceId) -> Self {
        Self { instance }
    }
}

impl Supervisor {
    /// Builds the operation work of `force_stop_instance` for the engine.
    #[must_use]
    pub fn force_stop_work(&self, request: ForceStopRequest) -> Box<dyn OperationWork> {
        Box::new(ForceStopWork {
            sup: self.clone(),
            request,
        })
    }
}

/// The work of `force_stop_instance`.
struct ForceStopWork {
    sup: Supervisor,
    request: ForceStopRequest,
}

impl OperationWork for ForceStopWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_force(&self.sup, self.request.instance, ctx).await })
    }
}

fn failed(error: SupervisorError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

/// Checks the lifecycle a force stop may act on (AC-2). `Ok(true)` goes on,
/// `Ok(false)` has nothing to do because no process remains.
fn force_allowed(lifecycle: InstanceLifecycle) -> Result<bool, SupervisorError> {
    match lifecycle {
        InstanceLifecycle::Stopping => Ok(true),
        InstanceLifecycle::Uninitialized
        | InstanceLifecycle::Stopped
        | InstanceLifecycle::Failed => Ok(false),
        InstanceLifecycle::Running
        | InstanceLifecycle::Starting
        | InstanceLifecycle::Recovering => {
            Err(SupervisorError::ForceRequiresStopping { lifecycle })
        }
        InstanceLifecycle::Unknown => Err(SupervisorError::IdentityUnproven { pid: None }),
        InstanceLifecycle::Initializing => Err(SupervisorError::InstanceInitializing),
        InstanceLifecycle::Retained => Err(SupervisorError::InstanceRetained),
    }
}

async fn run_force(sup: &Supervisor, id: InstanceId, mut ctx: OperationContext) -> WorkResult {
    let deps = sup.deps();
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
    if !force_allowed(instance.lifecycle()).map_err(failed)? {
        return Ok(outcome());
    }

    ctx.begin_step(StepSpec::new(StageName::LOCK)).await?;
    ctx.set_stage(StageName::LOCK).await?;
    let actor = sup.actor(id);
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
        // The exit handler records the lifecycle before it clears the
        // slot: `stopped` or `failed` is a stop that finished, and a
        // lifecycle still `stopping` is closed here, with no signal.
        let lifecycle = read_instance(deps, id).await.map_err(failed)?.lifecycle();
        if lifecycle == InstanceLifecycle::Stopping {
            sup.settle_stopping(id).await.map_err(failed)?;
        }
        return Ok(outcome());
    };

    ctx.begin_step(StepSpec::new(StageName::JOURNAL).irreversible())
        .await?;
    ctx.set_stage(StageName::JOURNAL).await?;
    let mut still_stopping = false;
    commit_instance(sup, id, None, CommitMode::Operation, |changes| {
        still_stopping = changes.instance().lifecycle() == InstanceLifecycle::Stopping;
        if still_stopping && changes.instance().desired() != DesiredState::Stopped {
            changes.apply(|instance| {
                instance.set_desired(DesiredState::Stopped);
                Ok::<_, SupervisorError>(())
            })?;
        }
        Ok(Vec::new())
    })
    .await
    .map_err(failed)?;
    if !still_stopping {
        // The process left since the check: its exit ended the stop.
        return Ok(outcome());
    }

    // The engine records this step as running before it returns, so the
    // exit of the process reads `forced` from here on (AC-4).
    ctx.begin_step(StepSpec::new(StageName::KILL).irreversible())
        .await?;
    ctx.set_stage(StageName::KILL).await?;
    let pid = identity.pid;
    let platform = deps.platform.clone();
    let target = SignalTarget::new(identity);
    let sent = blocking(move || platform.processes().force_kill(&target))
        .await
        .map_err(failed)?;
    match sent {
        // `NotFound` is ESRCH: the process left between the check and the
        // kill, and its exit is on the way.
        Ok(()) | Err(ProcessError::NotFound { .. }) => {}
        Err(ProcessError::IdentityMismatch { pid }) => {
            sup.identity_lost(id, generation, pid).await;
            return Err(failed(SupervisorError::SignalRefused { pid }));
        }
        // No signal left: the `kill` step closes `failed`, so it is no
        // force intent.
        Err(_) => return Err(failed(SupervisorError::SignalRefused { pid })),
    }
    actor.update(deps, |state| state.stopping = true);

    ctx.begin_step(StepSpec::new(StageName::AWAIT_EXIT).irreversible())
        .await?;
    ctx.set_stage(StageName::AWAIT_EXIT).await?;
    let wait = deps.config.force_exit_wait;
    let exited = {
        let ctx = &ctx;
        tokio::select! {
            biased;
            seen = done.wait_for(|finished| *finished) => {
                drop(seen);
                true
            }
            () = async {
                ctx.stopped().await;
                if !matches!(ctx.check_cancel(), Err(WorkError::Abandoned)) {
                    // A cancel does not interrupt the wait for the exit.
                    std::future::pending::<()>().await;
                }
            } => return Err(WorkError::Abandoned),
            () = tokio::time::sleep(wait) => false,
        }
    };
    if !exited {
        return Err(failed(SupervisorError::ForceTimeout {
            wait_seconds: wait.as_secs(),
        }));
    }
    sup.settle_stopping(id).await.map_err(failed)?;

    ctx.begin_step(StepSpec::new(StageName::COMPLETE).irreversible())
        .await?;
    ctx.set_stage(StageName::COMPLETE).await?;
    ctx.finish_step(None).await?;
    Ok(outcome())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stopping_goes_on_and_a_stopped_instance_has_nothing_to_do() {
        assert_eq!(force_allowed(InstanceLifecycle::Stopping).ok(), Some(true));
        for lifecycle in [
            InstanceLifecycle::Stopped,
            InstanceLifecycle::Failed,
            InstanceLifecycle::Uninitialized,
        ] {
            assert_eq!(force_allowed(lifecycle).ok(), Some(false), "{lifecycle}");
        }
    }

    #[test]
    fn a_running_instance_needs_a_normal_stop_first() {
        use crate::operation::OperationError as _;

        for lifecycle in [
            InstanceLifecycle::Running,
            InstanceLifecycle::Starting,
            InstanceLifecycle::Recovering,
        ] {
            let error = force_allowed(lifecycle).unwrap_err();
            assert_eq!(
                error.code().to_string(),
                "supervisor.force_requires_stopping"
            );
        }
        let unknown = force_allowed(InstanceLifecycle::Unknown).unwrap_err();
        assert_eq!(unknown.code().to_string(), "supervisor.identity_unproven");
    }
}
