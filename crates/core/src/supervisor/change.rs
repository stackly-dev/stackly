//! Changing an instance that exists, as operations (spec 0029, *Operasi*):
//! `apply_config` and `change_endpoints` write a new config revision and
//! make it take effect with the reload or restart steps of spec 0026, and
//! `rename_instance` changes the name. All three are queued behind any
//! operation that holds the instance, so each reads fresh facts when it
//! runs.

use std::collections::BTreeSet;
use std::path::Path;

use stackly_domain::{
    CommitGuard, ConfigRevisionId, ConfigRevisionNumber, ConfigRevisionRecord, GuardDecision,
    Guarded, Instance, InstanceFilter, InstanceId, InstanceName, RegistryError, ResourceRef,
    StageName, WriteOp, rename_allowed,
};
use stackly_platform::Platform;

use super::Supervisor;
use super::change_guard::{GuardFailure, activate_ops};
use super::drift;
use super::error::SupervisorError;
use super::persist::{blocking, read_instance};
use super::reload::ReloadRequest;
use super::restart_op::{RestartRequest, restart_steps};
use crate::instance::config::{
    ChangePlan, ConfigChange, ConfigError, ConfigImpact, Planned, Rejected, gather_config,
    plan_change, refusal_error,
};
use crate::instance::create::CreateError;
use crate::operation::{
    BoxFuture, EngineError, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError,
    WorkOutcome, WorkResult,
};
use crate::secret::SecretCaller;

/// What [`Supervisor::change_work`] needs: one config change, as the
/// operation `apply_config` (settings, roll back, restore) or
/// `change_endpoints` (ports).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChangeRequest {
    /// The instance to change.
    pub instance: InstanceId,
    /// The revision the caller read.
    pub base_revision: ConfigRevisionNumber,
    /// What to change.
    pub change: ConfigChange,
    /// Who asks, which decides whether the secret store may show a dialog
    /// when the restart resolves secrets, as for a user start.
    pub caller: SecretCaller,
}

impl ChangeRequest {
    /// Asks for `change` on `instance`, read at `base_revision`, for
    /// `caller`.
    #[must_use]
    pub const fn new(
        instance: InstanceId,
        base_revision: ConfigRevisionNumber,
        change: ConfigChange,
        caller: SecretCaller,
    ) -> Self {
        Self {
            instance,
            base_revision,
            change,
            caller,
        }
    }
}

/// What [`Supervisor::rename_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RenameRequest {
    /// The instance to rename.
    pub instance: InstanceId,
    /// The new name, already checked against VAL-001.
    pub name: InstanceName,
}

impl RenameRequest {
    /// Asks to rename `instance` to `name`.
    #[must_use]
    pub const fn new(instance: InstanceId, name: InstanceName) -> Self {
        Self { instance, name }
    }
}

fn failed(error: impl Into<ConfigError>) -> WorkError {
    WorkError::Failed(Box::new(error.into()))
}

fn supervisor_failed(error: SupervisorError) -> WorkError {
    failed(error)
}

impl Supervisor {
    /// Plans `change` for `instance` from fresh facts, without writing
    /// anything (spec 0029, AC-2, AC-3, AC-6, AC-9, AC-11). `check_drift`
    /// runs the drift check first and refuses with `config.drift_detected`:
    /// apply, rollback, and change endpoints set it, `validate` does not.
    ///
    /// # Errors
    ///
    /// Returns [`Rejected`] for a change that is not valid or not allowed.
    pub async fn plan_change(
        &self,
        instance: InstanceId,
        base_revision: ConfigRevisionNumber,
        change: &ConfigChange,
        check_drift: bool,
    ) -> Result<Planned, Rejected> {
        let deps = self.deps();
        let platform = deps.platform.clone();
        let layout = deps.layout.clone();
        let registry = deps.registry.clone();
        let (facts, others) = blocking(move || -> Result<_, SupervisorError> {
            let facts = gather_config(&platform, &layout, &*registry, instance)?;
            let others = others_of(&*registry, instance)?;
            Ok((facts, others))
        })
        .await??;
        if check_drift {
            let report = drift::observe_active(self, instance).await?;
            if !report.is_clean() {
                return Err(SupervisorError::ConfigDrift {
                    files: report.paths(),
                }
                .into());
            }
        }
        let probe = deps.ports.clone();
        let change = change.clone();
        blocking(move || plan_change(&facts, base_revision, &change, &others, &*probe))
            .await
            .map_err(Rejected::from)?
    }

    /// Builds the operation work of `apply_config` or `change_endpoints`.
    #[must_use]
    pub fn change_work(&self, request: ChangeRequest) -> Box<dyn OperationWork> {
        Box::new(ChangeWork {
            sup: self.clone(),
            request,
        })
    }

    /// Builds the operation work of `rename_instance`.
    #[must_use]
    pub fn rename_work(&self, request: RenameRequest) -> Box<dyn OperationWork> {
        Box::new(RenameWork {
            sup: self.clone(),
            request,
        })
    }
}

/// The instances other than `id` that are not retained.
fn others_of(
    registry: &dyn stackly_domain::Registry,
    id: InstanceId,
) -> Result<Vec<Instance>, RegistryError> {
    Ok(registry
        .instances(InstanceFilter {
            include_retained: false,
        })?
        .value
        .into_iter()
        .filter(|other| other.id() != id)
        .collect())
}

struct ChangeWork {
    sup: Supervisor,
    request: ChangeRequest,
}

impl OperationWork for ChangeWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_change(&self.sup, &self.request, ctx).await })
    }
}

fn done(id: InstanceId) -> WorkOutcome {
    WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Instance(id)],
            partial: Vec::new(),
        },
        finalize: None,
    }
}

/// The error of a plan that cannot go on, as the error of the work.
fn rejected(rejection: Rejected) -> WorkError {
    match rejection {
        Rejected::Fields(_) => failed(CreateError::SettingsInvalid),
        Rejected::Endpoints(issues) => match issues.into_iter().next() {
            Some(issue) => failed(issue.error),
            None => failed(CreateError::SettingsInvalid),
        },
        Rejected::Error(error) => failed(error),
    }
}

async fn run_change(
    sup: &Supervisor,
    request: &ChangeRequest,
    mut ctx: OperationContext,
) -> WorkResult {
    let deps = sup.deps();
    let id = request.instance;

    // The engine queued the change behind any operation that held the
    // instance, so this plan reads the state the commit will meet.
    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let plan = match sup
        .plan_change(id, request.base_revision, &request.change, true)
        .await
        .map_err(rejected)?
    {
        Planned::Unchanged => return Ok(done(id)),
        Planned::Change(plan) => plan,
    };

    ctx.begin_step(StepSpec::new(StageName::LOCK)).await?;
    ctx.set_stage(StageName::LOCK).await?;

    ctx.begin_step(StepSpec::new(StageName::COMMIT).irreversible())
        .await?;
    ctx.set_stage(StageName::COMMIT).await?;
    commit(sup, &mut ctx, id, &plan).await?;

    ctx.begin_step(StepSpec::new(StageName::APPLY)).await?;
    ctx.set_stage(StageName::APPLY).await?;
    let lifecycle = read_instance(deps, id)
        .await
        .map_err(supervisor_failed)?
        .lifecycle();
    // The instance may have stopped or crashed since the plan: the impact
    // follows the lifecycle now.
    let impact = plan
        .impact_at(lifecycle)
        .map_err(|refusal| supervisor_failed(refusal_error(refusal, lifecycle)))?;
    match impact {
        ConfigImpact::None | ConfigImpact::NextStart => Ok(done(id)),
        ConfigImpact::Reload => {
            ctx.set_stage(StageName::VERIFY).await?;
            let caller = request.caller.clone().in_operation(ctx.id());
            sup.execute_reload(&ReloadRequest::new(id, caller))
                .await
                .map_err(supervisor_failed)?;
            Ok(done(id))
        }
        ConfigImpact::Restart => {
            let restart = RestartRequest::new(id, request.caller.clone());
            restart_steps(sup, &restart, ctx, lifecycle).await
        }
    }
}

/// Writes the new revision, the instance it follows, and the prune in one
/// guarded transaction, then removes the history copies of the revisions
/// the prune deleted (spec 0029, AC-12, AC-14).
async fn commit(
    sup: &Supervisor,
    ctx: &mut OperationContext,
    id: InstanceId,
    plan: &ChangePlan,
) -> Result<(), WorkError> {
    let deps = sup.deps();
    let registry = deps.registry.clone();
    let (active, before) = blocking(move || -> Result<_, RegistryError> {
        let active = registry.active_config_revision(id)?.value;
        let before = registry.config_revisions(id, u32::MAX)?.value;
        Ok((active, before))
    })
    .await
    .map_err(supervisor_failed)?
    .map_err(|error| supervisor_failed(error.into()))?;
    let active = active.ok_or_else(|| {
        supervisor_failed(SupervisorError::PackageUnavailable {
            provider: String::new(),
            reason: "no_config_revision".to_owned(),
        })
    })?;
    if active.number != plan.active {
        return Err(failed(ConfigError::RevisionConflict {
            base: plan.active,
            active: active.number,
        }));
    }
    let record = ConfigRevisionRecord::following(
        &active,
        ConfigRevisionId::generate(),
        plan.values.clone(),
        plan.endpoints.clone(),
        plan.origin,
        plan.source,
        Some(ctx.id()),
        deps.platform.clock().now(),
    );
    let guard: CommitGuard = {
        let plan = plan.clone();
        Box::new(move |view| {
            Ok(match activate_ops(view, id, &plan, record) {
                Ok(ops) => GuardDecision::Write(ops),
                Err(GuardFailure::Refused(error)) => GuardDecision::Refuse(Box::new(error)),
                Err(GuardFailure::Registry(error)) => return Err(error),
            })
        })
    };
    let committed = match ctx.commit_guarded(guard).await {
        Ok(Guarded::Committed(committed)) => committed,
        Ok(Guarded::Refused(reason)) => {
            return Err(match reason.downcast::<ConfigError>() {
                Ok(error) => failed(*error),
                Err(_) => supervisor_failed(SupervisorError::internal(
                    "the change guard refused with an unknown reason",
                )),
            });
        }
        Err(WorkError::Engine(EngineError::Registry(error))) => {
            return Err(supervisor_failed(SupervisorError::Registry(error)));
        }
        Err(other) => return Err(other),
    };
    let actor = sup.actor(id);
    for (resource, revision) in committed.revisions {
        if resource == ResourceRef::Instance(id) {
            actor.note_revision(revision);
        }
    }
    remove_pruned_copies(sup, id, before).await;
    Ok(())
}

/// Removes the history folder of every revision the prune deleted. A folder
/// that cannot be removed is logged and left: the next prune finds it
/// again, and a copy of a deleted revision is never read.
async fn remove_pruned_copies(sup: &Supervisor, id: InstanceId, before: Vec<ConfigRevisionRecord>) {
    let deps = sup.deps();
    let registry = deps.registry.clone();
    let platform = deps.platform.clone();
    let history = deps.layout.layout().instance_paths(id).config_history;
    let removed = blocking(move || -> Result<(), String> {
        let after: BTreeSet<ConfigRevisionId> = registry
            .config_revisions(id, u32::MAX)
            .map_err(|error| error.to_string())?
            .value
            .into_iter()
            .map(|revision| revision.id)
            .collect();
        for gone in before
            .iter()
            .filter(|revision| !after.contains(&revision.id))
        {
            remove_copy(&platform, &history, gone.id)?;
        }
        Ok(())
    })
    .await;
    match removed {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(instance = %id, %error, "the history copies of pruned config revisions were not removed");
        }
        Err(error) => {
            tracing::warn!(instance = %id, %error, "the history copies of pruned config revisions were not removed");
        }
    }
}

fn remove_copy(
    platform: &Platform,
    history: &Path,
    revision: ConfigRevisionId,
) -> Result<(), String> {
    platform
        .fs()
        .remove_tree(history, Path::new(&revision.to_string()))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

struct RenameWork {
    sup: Supervisor,
    request: RenameRequest,
}

impl OperationWork for RenameWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_rename(&self.sup, &self.request, ctx).await })
    }
}

/// Renames the instance (spec 0029, AC-13): validate the lifecycle, lock,
/// and one guarded commit that checks the name against every other
/// instance. Nothing about the process changes.
async fn run_rename(
    sup: &Supervisor,
    request: &RenameRequest,
    mut ctx: OperationContext,
) -> WorkResult {
    let deps = sup.deps();
    let id = request.instance;

    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let instance = read_instance(deps, id).await.map_err(supervisor_failed)?;
    if let Err(refusal) = rename_allowed(instance.lifecycle()) {
        return Err(supervisor_failed(refusal_error(
            refusal,
            instance.lifecycle(),
        )));
    }
    if instance.name() == &request.name {
        return Ok(done(id));
    }

    ctx.begin_step(StepSpec::new(StageName::LOCK)).await?;
    ctx.set_stage(StageName::LOCK).await?;

    ctx.begin_step(StepSpec::new(StageName::COMMIT)).await?;
    ctx.set_stage(StageName::COMMIT).await?;
    let name = request.name.clone();
    let guard: CommitGuard = Box::new(move |view| {
        let instances = view.instances(InstanceFilter {
            include_retained: false,
        })?;
        let Some(current) = instances.iter().find(|other| other.id() == id) else {
            return Ok(GuardDecision::Refuse(Box::new(ConfigError::Supervisor(
                SupervisorError::NotFound(id),
            ))));
        };
        if let Err(refusal) = rename_allowed(current.lifecycle()) {
            return Ok(GuardDecision::Refuse(Box::new(ConfigError::Supervisor(
                refusal_error(refusal, current.lifecycle()),
            ))));
        }
        if let Some(taken) = instances
            .iter()
            .find(|other| other.id() != id && other.name() == &name)
        {
            return Ok(GuardDecision::Refuse(Box::new(ConfigError::Create(
                CreateError::NameTaken {
                    existing: taken.id(),
                },
            ))));
        }
        let expected = current.revision();
        let mut renamed = current.clone();
        renamed.rename(name.clone());
        Ok(GuardDecision::Write(vec![WriteOp::UpdateInstance {
            instance: renamed,
            expected,
        }]))
    });
    match ctx.commit_guarded(guard).await {
        Ok(Guarded::Committed(committed)) => {
            let actor = sup.actor(id);
            for (resource, revision) in committed.revisions {
                if resource == ResourceRef::Instance(id) {
                    actor.note_revision(revision);
                }
            }
        }
        Ok(Guarded::Refused(reason)) => {
            return Err(match reason.downcast::<ConfigError>() {
                Ok(error) => failed(*error),
                Err(_) => supervisor_failed(SupervisorError::internal(
                    "the rename guard refused with an unknown reason",
                )),
            });
        }
        Err(WorkError::Engine(EngineError::Registry(error))) => {
            return Err(supervisor_failed(SupervisorError::Registry(error)));
        }
        Err(other) => return Err(other),
    }
    Ok(done(id))
}
