//! `execute_reload` (spec 0010, *Executor plan*, Reload, AC-14): renders the
//! config again, tells the running process to read it, and checks readiness
//! within one shared budget. The process is never run again by a reload.

use std::collections::BTreeMap;
use std::time::Duration;

use stackly_domain::{InstanceId, InstanceLifecycle, ResourceRef, StageName};
use stackly_platform::{ChildRole, ExitStatus, ProcessError, ProviderSignal, SignalTarget};
use tokio::time::Instant;

use super::error::SupervisorError;
use super::helper::HelperEnd;
use super::init::config_invalid;
use super::persist::{blocking, read_instance};
use super::prepare::{Checks, prepare};
use super::probe::{self, ProbeContext};
use super::steps::{run_command, run_render};
use super::{Supervisor, secrets};
use crate::operation::{
    BoxFuture, OperationContext, OperationOutcome, OperationWork, WorkError, WorkOutcome,
    WorkResult,
};
use crate::provider::{PlanError, ReloadAction};
use crate::secret::SecretCaller;
use stackly_domain::Capability;

/// What [`Supervisor::execute_reload`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReloadRequest {
    /// The running instance to reload.
    pub instance: InstanceId,
    /// Who asks, which decides whether the secret store may show a dialog.
    pub caller: SecretCaller,
}

impl ReloadRequest {
    /// Asks to reload `instance` for `caller`.
    #[must_use]
    pub const fn new(instance: InstanceId, caller: SecretCaller) -> Self {
        Self { instance, caller }
    }
}

impl Supervisor {
    /// Builds the operation work of `reload_instance` for the engine.
    #[must_use]
    pub fn reload_work(&self, request: ReloadRequest) -> Box<dyn OperationWork> {
        Box::new(ReloadWork {
            sup: self.clone(),
            request,
        })
    }

    /// Reloads the configuration of a running instance: renders the config
    /// files again, sends `SIGHUP` to the main process after its identity
    /// is checked (or runs the provider's reload command), and verifies
    /// readiness, all within the plan's `reloadSeconds`. The PID does not
    /// change.
    ///
    /// # Errors
    ///
    /// Returns `supervisor.capability_unsupported` when the provider cannot
    /// reload, before any signal is sent; `process.signal_refused` when the
    /// identity check fails; and `supervisor.reload_failed` when the action
    /// or the verification fails.
    pub async fn execute_reload(&self, request: &ReloadRequest) -> Result<(), SupervisorError> {
        let deps = self.deps();
        let id = request.instance;
        let instance = read_instance(deps, id).await?;
        if instance.lifecycle() != InstanceLifecycle::Running {
            return Err(SupervisorError::ReloadFailed {
                reason: format!(
                    "the instance is {}, not running",
                    instance.lifecycle().as_code()
                ),
            });
        }
        let actor = self.actor(id);
        let target = actor.read(|state| {
            state
                .process
                .as_ref()
                .map(|slot| (slot.generation, slot.identity.clone()))
        });
        let Some((_generation, identity)) = target else {
            return Err(SupervisorError::IdentityUnproven { pid: None });
        };

        let started = Instant::now();
        let prepared = prepare(self, id, Checks::Plan).await?;
        let context = prepared.context(&BTreeMap::new());
        let plan = match prepared.package.reload_plan(&context) {
            Ok(plan) => plan,
            Err(PlanError::Unsupported {
                capability: Capability::Reload,
            }) => {
                return Err(SupervisorError::CapabilityUnsupported {
                    capability: Capability::Reload.to_string(),
                });
            }
            Err(error) => return Err(config_invalid(&error)),
        };
        let budget = plan.deadline;
        let remaining = || budget.saturating_sub(started.elapsed());
        let reload_failed = |reason: String| SupervisorError::ReloadFailed { reason };

        let secrets = secrets::resolve(self, id, &request.caller).await?;
        run_render(deps, &plan.steps, &secrets)
            .await
            .map_err(|error| reload_failed(error.to_string()))?;

        match &plan.action {
            ReloadAction::None => {}
            ReloadAction::Hangup => {
                let platform = deps.platform.clone();
                let target = SignalTarget::new(identity);
                let sent =
                    blocking(move || platform.processes().signal(&target, ProviderSignal::Hup))
                        .await?;
                match sent {
                    Ok(()) => {}
                    Err(ProcessError::IdentityMismatch { pid }) => {
                        return Err(SupervisorError::SignalRefused { pid });
                    }
                    Err(error) => return Err(reload_failed(error.to_string())),
                }
            }
            ReloadAction::Run(spec) => {
                let ended = run_command(
                    deps,
                    &prepared,
                    spec,
                    &secrets,
                    ChildRole::ReloadCommand,
                    None,
                    remaining(),
                )
                .await
                .map_err(reload_failed)?;
                match ended {
                    HelperEnd::TimedOut => {
                        return Err(reload_failed("the reload command timed out".to_owned()));
                    }
                    HelperEnd::Exited(notice) => match notice.status {
                        Some(ExitStatus::Exited(code))
                            if u8::try_from(code)
                                .is_ok_and(|code| spec.success_exit_codes.contains(&code)) => {}
                        status => {
                            return Err(reload_failed(format!(
                                "the reload command failed: {status:?}"
                            )));
                        }
                    },
                }
            }
        }

        if let Some(verify) = &plan.verify {
            let context = ProbeContext {
                platform: &deps.platform,
                redactor: &deps.redactor,
                config: &deps.config,
            };
            loop {
                let left = remaining();
                if left.is_zero() {
                    return Err(reload_failed(format!(
                        "the instance was not ready again within {} s",
                        budget.as_secs()
                    )));
                }
                let outcome =
                    probe::run(&verify.probe, verify.attempt_timeout.min(left), context).await;
                if outcome.is_ok() {
                    break;
                }
                tokio::time::sleep(verify.interval.min(remaining().max(Duration::ZERO))).await;
            }
        }
        Ok(())
    }
}

struct ReloadWork {
    sup: Supervisor,
    request: ReloadRequest,
}

impl OperationWork for ReloadWork {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.set_stage(StageName::VERIFY).await?;
            self.sup
                .execute_reload(&self.request)
                .await
                .map_err(|error| WorkError::Failed(Box::new(error)))?;
            Ok(WorkOutcome {
                result: OperationOutcome {
                    resources: vec![ResourceRef::Instance(self.request.instance)],
                    partial: Vec::new(),
                },
                finalize: None,
            })
        })
    }
}
