//! The work of the operation `stop_all_instances` (spec 0031, *Operasi
//! `stop_all_instances`*): one step `stop` that stops each instance of its
//! list with the stop of a person, at most `RECONCILE_START_CONCURRENCY` at a
//! time, under the lock keys the engine took for the operation.
//!
//! Just before an instance stops, the selection rule runs again on a fresh
//! read of it, so an instance that changed since the list was made is left
//! alone, or dropped when it is gone. A cancel ends the work early: no new
//! stop begins, the ones running finish, and the rest are skipped with
//! `operation.cancelled`.

use std::collections::VecDeque;

use stackly_domain::{
    ErrorCodeName, InstanceId, OperationKind, OperationResource, OperationTrigger, Progress,
    ResourceRef, StageName, StopIntent,
};
use tokio::task::JoinSet;

use super::Supervisor;
use super::error::SupervisorError;
use super::persist::blocking;
use super::stop::LeaseCheck;
use crate::instance::stop_all::{StopAllOptions, read_view, select};
use crate::operation::codes::CANCELLED;
use crate::operation::{
    BoxFuture, OperationContext, OperationError, OperationOutcome, OperationWork, PartialItem,
    PartialOutcome, StepSpec, WorkError, WorkOutcome, WorkResult,
};

/// What [`Supervisor::stop_all_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StopAllRequest {
    /// The instances that get a step, in the order they start.
    pub steps: Vec<InstanceId>,
    /// Whether each step stops an instance an environment lease holds:
    /// the check before the step and the guard of its stop both take it
    /// (spec 0047 AC-16, AC-17). The leases stay active either way.
    pub override_leases: bool,
}

impl StopAllRequest {
    /// Asks to stop `steps` without overriding environment leases.
    #[must_use]
    pub const fn new(steps: Vec<InstanceId>) -> Self {
        Self {
            steps,
            override_leases: false,
        }
    }

    /// Sets whether the steps override environment leases.
    #[must_use]
    pub const fn overriding_leases(mut self, override_leases: bool) -> Self {
        self.override_leases = override_leases;
        self
    }
}

/// How one instance ended.
enum StepEnd {
    Stopped,
    Failed(ErrorCodeName),
    Skipped(ErrorCodeName),
    /// The instance is gone or retained: it is not in the result.
    Dropped,
}

impl Supervisor {
    /// Builds the operation work of `stop_all_instances`.
    #[must_use]
    pub fn stop_all_work(&self, request: StopAllRequest) -> Box<dyn OperationWork> {
        Box::new(StopAllWork {
            sup: self.clone(),
            request,
        })
    }

    /// Asks the engine to cancel every `start_instance` and
    /// `restart_instance` of `instances` that is not final, so a start that
    /// is long does not hold the lock a stop waits for. The stop of a start
    /// that was cancelled after its spawn is the ordinary one.
    pub async fn cancel_open_starts(&self, instances: &[InstanceId]) {
        let Some(engine) = self.operations() else {
            return;
        };
        let registry = self.deps().registry.clone();
        let Ok(Ok(open)) = blocking(move || registry.unfinished_operations()).await else {
            return;
        };
        for (operation, _) in open.value {
            let OperationResource::Instance(id) = operation.resource() else {
                continue;
            };
            if instances.contains(&id)
                && matches!(
                    operation.kind(),
                    OperationKind::StartInstance | OperationKind::RestartInstance
                )
                && let Err(error) = engine
                    .cancel(operation.id(), OperationTrigger::background())
                    .await
            {
                tracing::warn!(instance = %id, ?error, "an open start could not be cancelled");
            }
        }
    }

    /// Stops `id` as a person's stop does, after the selection rule ran
    /// again on a fresh read with the same `options` as the operation.
    async fn stop_all_step(&self, id: InstanceId, options: StopAllOptions) -> StepEnd {
        let registry = self.deps().registry.clone();
        let session = self.session();
        let view = match blocking({
            let session = session.clone();
            move || read_view(&*registry, session.as_ref())
        })
        .await
        {
            Ok(Ok(view)) => view,
            Ok(Err(error)) => return StepEnd::Failed(SupervisorError::Registry(error).code()),
            Err(error) => return StepEnd::Failed(error.code()),
        };
        let Some(one) = view.of_instance(id) else {
            return StepEnd::Dropped;
        };
        let selection = select(&one, options);
        if let Some(skipped) = selection.skipped.into_iter().next() {
            return StepEnd::Skipped(skipped.code);
        }
        let leases = one.instances.into_iter().next().map(|item| item.leases);
        if leases.is_none() || self.is_retained(id).await {
            return StepEnd::Dropped;
        }
        // The guard reads the leases again in the commit that writes
        // desired `stopped` (spec 0047): without the override, a lease an
        // Up recorded after the selection makes the step skipped, never a
        // stop.
        let check = LeaseCheck::Manual {
            session,
            override_leases: options.override_leases,
        };
        match self.stop_direct(id, StopIntent::User, check).await {
            Ok(()) => StepEnd::Stopped,
            Err(SupervisorError::InstanceRetained | SupervisorError::NotFound(_)) => {
                StepEnd::Dropped
            }
            Err(error @ SupervisorError::StopRequiresOverride { .. }) => {
                StepEnd::Skipped(error.code())
            }
            Err(error) => StepEnd::Failed(error.code()),
        }
    }

    async fn is_retained(&self, id: InstanceId) -> bool {
        matches!(
            super::persist::read_instance(self.deps(), id).await,
            Ok(instance) if instance.lifecycle() == stackly_domain::InstanceLifecycle::Retained
        )
    }
}

struct StopAllWork {
    sup: Supervisor,
    request: StopAllRequest,
}

impl OperationWork for StopAllWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run(&self.sup, &self.request, ctx).await })
    }
}

async fn run(sup: &Supervisor, request: &StopAllRequest, mut ctx: OperationContext) -> WorkResult {
    let total = request.steps.len() as u64;
    ctx.begin_step(StepSpec::new(StageName::STOP).irreversible())
        .await?;
    ctx.set_stage(StageName::STOP).await?;
    ctx.set_progress(Progress::Counted { done: 0, total });

    let limit = sup.deps().config.reconcile_start_concurrency.max(1);
    let mut queue: VecDeque<InstanceId> = request.steps.iter().copied().collect();
    let mut running: JoinSet<(InstanceId, StepEnd)> = JoinSet::new();
    let mut stopped = Vec::new();
    let mut partial = Vec::new();
    let mut done = 0_u64;
    let mut cancelled = false;
    loop {
        while !cancelled && running.len() < limit && !queue.is_empty() {
            match ctx.check_cancel() {
                Ok(()) => {}
                Err(WorkError::Cancelled) => {
                    cancelled = true;
                    break;
                }
                Err(other) => return Err(other),
            }
            let Some(id) = queue.pop_front() else { break };
            let sup = sup.clone();
            let options = StopAllOptions::new(request.override_leases);
            running.spawn(async move { (id, sup.stop_all_step(id, options).await) });
        }
        let Some(finished) = running.join_next().await else {
            break;
        };
        let (id, end) = finished.map_err(|error| {
            WorkError::Failed(Box::new(SupervisorError::internal(format!(
                "a stop task failed: {error}"
            ))))
        })?;
        done += 1;
        match end {
            StepEnd::Stopped => stopped.push(ResourceRef::Instance(id)),
            StepEnd::Failed(code) => partial.push(partial_item(id, PartialOutcome::Failed, &code)),
            StepEnd::Skipped(code) => {
                partial.push(partial_item(id, PartialOutcome::Skipped, &code));
            }
            StepEnd::Dropped => {}
        }
        ctx.set_progress(Progress::Counted {
            done: done.min(total),
            total,
        });
    }
    for id in queue {
        partial.push(partial_item(id, PartialOutcome::Skipped, &CANCELLED));
    }
    ctx.finish_step(None).await?;
    Ok(WorkOutcome {
        result: OperationOutcome {
            resources: stopped,
            partial,
        },
        finalize: None,
    })
}

fn partial_item(id: InstanceId, outcome: PartialOutcome, code: &ErrorCodeName) -> PartialItem {
    PartialItem {
        item: format!("instance:{id}"),
        resource: Some(ResourceRef::Instance(id)),
        outcome,
        code: Some(code.to_string()),
    }
}
