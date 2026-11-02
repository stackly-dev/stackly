//! Changing when an instance starts on its own (spec 0031, AC-16), as the
//! operation `set_instance_startup`: one guarded commit that writes the
//! policy and, when it becomes `manual`, releases the startup lease the
//! session holds. Changing to `at_login` creates no lease and starts
//! nothing; the next new session does. A running instance is never stopped.

use stackly_domain::{
    CommitGuard, GuardDecision, Guarded, InstanceFilter, InstanceId, LeaseFilter, LeaseHolder,
    ResourceRef, SessionId, StageName, StartupPolicy, WriteOp,
};

use super::Supervisor;
use super::error::SupervisorError;
use super::persist::read_instance;
use crate::operation::{
    BoxFuture, EngineError, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError,
    WorkOutcome, WorkResult,
};

/// What [`Supervisor::set_startup_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SetStartupRequest {
    /// The instance to change.
    pub instance: InstanceId,
    /// The policy to set.
    pub startup: StartupPolicy,
    /// The login session of this daemon run; `None` in a daemon that
    /// launchd did not start, which holds no startup lease.
    pub session: Option<SessionId>,
}

impl SetStartupRequest {
    /// Asks to set the policy of `instance` to `startup`, in `session`.
    #[must_use]
    pub const fn new(
        instance: InstanceId,
        startup: StartupPolicy,
        session: Option<SessionId>,
    ) -> Self {
        Self {
            instance,
            startup,
            session,
        }
    }
}

impl Supervisor {
    /// Builds the operation work of `set_instance_startup`.
    #[must_use]
    pub fn set_startup_work(&self, request: SetStartupRequest) -> Box<dyn OperationWork> {
        Box::new(SetStartupWork {
            sup: self.clone(),
            request,
        })
    }
}

struct SetStartupWork {
    sup: Supervisor,
    request: SetStartupRequest,
}

impl OperationWork for SetStartupWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_set_startup(&self.sup, &self.request, ctx).await })
    }
}

fn failed(error: SupervisorError) -> WorkError {
    WorkError::Failed(Box::new(error))
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

async fn run_set_startup(
    sup: &Supervisor,
    request: &SetStartupRequest,
    mut ctx: OperationContext,
) -> WorkResult {
    let deps = sup.deps();
    let id = request.instance;

    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let instance = read_instance(deps, id).await.map_err(failed)?;
    if instance.lifecycle() == stackly_domain::InstanceLifecycle::Retained {
        return Err(failed(SupervisorError::InstanceRetained));
    }
    if instance.startup() == request.startup {
        return Ok(done(id));
    }

    ctx.begin_step(StepSpec::new(StageName::COMMIT)).await?;
    ctx.set_stage(StageName::COMMIT).await?;
    let startup = request.startup;
    let session = request.session.clone();
    let now = deps.platform.clock().now();
    let guard: CommitGuard = Box::new(move |view| {
        let instances = view.instances(InstanceFilter {
            include_retained: true,
        })?;
        let Some(current) = instances.iter().find(|other| other.id() == id) else {
            return Ok(GuardDecision::Refuse(Box::new(SupervisorError::NotFound(
                id,
            ))));
        };
        if current.lifecycle() == stackly_domain::InstanceLifecycle::Retained {
            return Ok(GuardDecision::Refuse(Box::new(
                SupervisorError::InstanceRetained,
            )));
        }
        if current.startup() == startup {
            return Ok(GuardDecision::Write(Vec::new()));
        }
        let expected = current.revision();
        let mut changed = current.clone();
        changed.set_startup(startup);
        let mut ops = vec![WriteOp::UpdateInstance {
            instance: changed,
            expected,
        }];
        if startup == StartupPolicy::Manual
            && let Some(session) = &session
        {
            for lease in view.leases(id, LeaseFilter::Unreleased)? {
                if lease.holder() == LeaseHolder::Startup && lease.session() == Some(session) {
                    let mut lease = lease;
                    if lease.release(now).is_ok() {
                        ops.push(WriteOp::UpdateLease(lease));
                    }
                }
            }
        }
        Ok(GuardDecision::Write(ops))
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
            return Err(match reason.downcast::<SupervisorError>() {
                Ok(error) => failed(*error),
                Err(_) => failed(SupervisorError::internal(
                    "the startup guard refused with an unknown reason",
                )),
            });
        }
        Err(WorkError::Engine(EngineError::Registry(error))) => {
            return Err(failed(SupervisorError::Registry(error)));
        }
        Err(other) => return Err(other),
    }
    Ok(done(id))
}
