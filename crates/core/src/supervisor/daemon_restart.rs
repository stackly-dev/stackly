//! The `daemon_restart` operation (spec 0030, *Alur restart terencana*): the
//! plan of a planned daemon restart is written under the operation, the
//! instances the plan stops are stopped without touching their desired
//! state, and then the daemon is told it may exit.
//!
//! The request that started the operation waits for [`PlanPrepared`], the
//! only reply of `daemon.shutdown { mode: restart }`; everything later is
//! the plan's progress, published as `daemon.restart_plan`. A stop that
//! does not finish within its provider's budget aborts the plan: the
//! instances stopped so far start again, and the daemon keeps running.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use stackly_domain::{
    DetailKey, ErrorCodeName, GuardView, Guarded, Instance, InstanceFilter, InstanceId,
    InstanceLifecycle, NewRestartPlan, PlanDecision, PlanInstance, RegistryError, RestartPlan,
    RestartPlanId, Sha256Digest, StageName, StopIntent, WriteBatch, WriteOp,
};
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinSet;
use uuid::Uuid;

use super::Supervisor;
use super::error::SupervisorError;
use super::handover::{PlanEntry, plan_digest, plan_entries};
use super::pending::{self, PendingKind, PendingRun, wait_final};
use super::persist::{blocking, read_instance};
use super::plan::{PlanRefusal, write_plan_guard};
use super::stop::StopJournal;
use crate::operation::{
    BoxFuture, OperationContext, OperationError, OperationOutcome, OperationWork, StepSpec,
    WorkError, WorkOutcome, WorkResult,
};

/// How often a commit of the plan is tried again while the registry is
/// busy.
const PERSIST_TRIES: u32 = 5;

/// Tells the daemon its plan is handed over, so it may run its stop order
/// and exit 75. The daemon implements it; tests record the call.
pub trait RestartHandover: Send + Sync + 'static {
    /// Every instance the plan stops has stopped, and the `daemon_restart`
    /// operation is final.
    fn handed_over(&self);
}

/// How the plan fared when the operation tried to write it.
#[derive(Debug)]
#[non_exhaustive]
pub enum PlanPrepared {
    /// The plan is written, in `stopping`.
    Written(Box<RestartPlan>),
    /// Another plan is open, or the plan changed since the client
    /// confirmed it; nothing was written or stopped.
    Refused(PlanRefusal),
    /// The registry could not be read or written.
    Failed(String),
}

/// What [`Supervisor::daemon_restart_work`] needs.
#[non_exhaustive]
pub struct DaemonRestartRequest {
    /// The ID the plan gets.
    pub plan: RestartPlanId,
    /// The digest the client confirmed, or the one computed when the
    /// request arrived.
    pub digest: Sha256Digest,
    /// The `serverInstanceId` of this daemon.
    pub server_instance_id: Uuid,
    /// Where the operation reports the written plan, or why not.
    pub prepared: oneshot::Sender<PlanPrepared>,
    /// What to call once the plan is handed over.
    pub handover: Arc<dyn RestartHandover>,
}

impl DaemonRestartRequest {
    /// A request that writes plan `plan`, which must still have `digest`.
    #[must_use]
    pub fn new(
        plan: RestartPlanId,
        digest: Sha256Digest,
        server_instance_id: Uuid,
        prepared: oneshot::Sender<PlanPrepared>,
        handover: Arc<dyn RestartHandover>,
    ) -> Self {
        Self {
            plan,
            digest,
            server_instance_id,
            prepared,
            handover,
        }
    }
}

impl std::fmt::Debug for DaemonRestartRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRestartRequest")
            .field("plan", &self.plan)
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

/// Why the operation failed before it stopped anything.
#[derive(Debug, thiserror::Error)]
enum RestartRefused {
    #[error("another daemon restart plan is open")]
    Open(RestartPlanId),
    #[error("the restart plan changed before it was written")]
    Changed {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    #[error("the restart plan could not be written: {0}")]
    Unwritten(String),
}

impl OperationError for RestartRefused {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::Open(_) => ErrorCodeName::from_static("daemon.restart_pending"),
            Self::Changed { .. } => ErrorCodeName::from_static("daemon.restart_plan_changed"),
            Self::Unwritten(_) => ErrorCodeName::from_static("internal.unexpected"),
        }
    }

    fn retryable(&self) -> bool {
        matches!(self, Self::Open(_))
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        let key = |text: &str| -> DetailKey {
            text.parse()
                .expect("the detail keys of spec 0030 are camelCase")
        };
        match self {
            Self::Open(plan) => vec![(key("planId"), plan.to_string())],
            Self::Changed { expected, actual } => vec![
                (key("expected"), expected.to_string()),
                (key("actual"), actual.to_string()),
            ],
            Self::Unwritten(_) => Vec::new(),
        }
    }
}

fn failed(error: impl OperationError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

impl Supervisor {
    /// Reads the instances that are not retained and sorts them into the
    /// entries of a restart plan, with this supervisor's handover list
    /// (spec 0030 AC-7).
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Registry`] when the instances cannot be
    /// read.
    pub async fn restart_plan_entries(&self) -> Result<Vec<PlanEntry>, SupervisorError> {
        let registry = self.deps().registry.clone();
        let instances = blocking(move || {
            registry.instances(InstanceFilter {
                include_retained: false,
            })
        })
        .await?
        .map_err(SupervisorError::Registry)?
        .value;
        Ok(plan_entries(
            &instances,
            true,
            &self.deps().config.handover_proven,
        ))
    }

    /// How long a planned restart waits for the operations in flight.
    #[must_use]
    pub fn restart_quiesce_wait(&self) -> std::time::Duration {
        self.deps().config.restart_quiesce_wait
    }

    /// Builds the operation work of `daemon_restart`.
    #[must_use]
    pub fn daemon_restart_work(&self, request: DaemonRestartRequest) -> Box<dyn OperationWork> {
        Box::new(DaemonRestartWork {
            sup: self.clone(),
            request,
        })
    }

    /// Stops one instance for a planned restart: the provider's stop
    /// strategy, with no change to the desired state and no lease check.
    /// An instance that no longer runs is done at once.
    pub(crate) async fn stop_for_plan(&self, id: InstanceId) -> Result<(), SupervisorError> {
        let deps = self.deps();
        let instance = read_instance(deps, id).await?;
        match instance.lifecycle() {
            InstanceLifecycle::Stopped
            | InstanceLifecycle::Failed
            | InstanceLifecycle::Uninitialized => return Ok(()),
            InstanceLifecycle::Unknown => {
                return Err(SupervisorError::IdentityUnproven { pid: None });
            }
            InstanceLifecycle::Retained => return Err(SupervisorError::InstanceRetained),
            InstanceLifecycle::Initializing => return Err(SupervisorError::InstanceInitializing),
            InstanceLifecycle::Starting
            | InstanceLifecycle::Running
            | InstanceLifecycle::Recovering
            | InstanceLifecycle::Stopping => {}
        }
        let actor = self.actor(id);
        let has_process = actor.read(|state| state.process.is_some());
        let mut lifecycle = instance.lifecycle();
        if !has_process
            && matches!(
                lifecycle,
                InstanceLifecycle::Starting
                    | InstanceLifecycle::Running
                    | InstanceLifecycle::Stopping
            )
        {
            // The exit handler records the lifecycle before it clears the
            // slot, so a fresh read tells what became of the process.
            lifecycle = read_instance(deps, id).await?.lifecycle();
            match lifecycle {
                InstanceLifecycle::Stopping
                | InstanceLifecycle::Stopped
                | InstanceLifecycle::Failed => {
                    return self
                        .settle_gone_process(id, StopIntent::PlannedRestart)
                        .await;
                }
                InstanceLifecycle::Recovering => {}
                _ => return Err(SupervisorError::IdentityUnproven { pid: None }),
            }
        }
        actor.cancel_respawn(deps);
        let no_process = !has_process && lifecycle == InstanceLifecycle::Recovering;
        let journaled = self
            .journal_stop(id, no_process, StopIntent::PlannedRestart)
            .await?;
        if has_process && journaled == StopJournal::Begun {
            self.stop_process(id, std::future::pending()).await?;
        }
        Ok(())
    }

    /// Writes `plan` over its stored row, trying again while the registry
    /// is busy.
    pub(crate) async fn persist_plan(&self, plan: &RestartPlan) -> Result<(), SupervisorError> {
        let mut last = None;
        for _ in 0..PERSIST_TRIES {
            let registry = self.deps().registry.clone();
            let row = plan.clone();
            let committed = blocking(move || {
                registry.commit(WriteBatch::new(vec![WriteOp::UpdateRestartPlan(row)]))
            })
            .await?;
            match committed {
                Ok(_) => return Ok(()),
                Err(error) if matches!(error.error, RegistryError::Busy) => {
                    last = Some(error.error);
                    tokio::time::sleep(self.deps().config.commit_busy_first_delay).await;
                }
                Err(error) => return Err(SupervisorError::Registry(error.error)),
            }
        }
        Err(SupervisorError::Registry(
            last.unwrap_or(RegistryError::Busy),
        ))
    }
}

struct DaemonRestartWork {
    sup: Supervisor,
    request: DaemonRestartRequest,
}

impl OperationWork for DaemonRestartWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run(self.sup, self.request, ctx).await })
    }
}

/// The `PlanEntry` of each instance the plan stops, from the plan's own
/// rows.
fn stoppers(plan: &RestartPlan) -> Vec<InstanceId> {
    plan.instances()
        .iter()
        .filter(|entry| entry.decision == PlanDecision::StopRestore)
        .map(|entry| entry.instance_id)
        .collect()
}

/// What the plan is written from.
#[derive(Clone, Copy)]
struct PlanSpec {
    id: RestartPlanId,
    digest: Sha256Digest,
    server_instance_id: Uuid,
}

fn new_plan(
    sup: &Supervisor,
    spec: PlanSpec,
    operation: stackly_domain::OperationId,
    entries: &[PlanEntry],
) -> Result<RestartPlan, String> {
    let boot_id = sup
        .deps()
        .platform
        .processes()
        .boot_id()
        .map_err(|error| error.to_string())?;
    Ok(RestartPlan::new(NewRestartPlan {
        id: spec.id,
        operation,
        digest: spec.digest,
        boot_id,
        server_instance_id_before: spec.server_instance_id,
        created_at: sup.deps().platform.clock().now(),
        instances: entries
            .iter()
            .map(|entry| {
                PlanInstance::new(
                    entry.instance_id,
                    entry.name.clone(),
                    entry.decision,
                    entry.reason,
                )
            })
            .collect(),
    }))
}

/// Recomputes the plan from the instances the guard's transaction sees.
fn recompute(
    proven: Vec<super::HandoverProof>,
) -> impl FnOnce(&dyn GuardView) -> Result<Sha256Digest, RegistryError> + Send + 'static {
    move |view| {
        let instances: Vec<Instance> = view.instances(InstanceFilter {
            include_retained: false,
        })?;
        Ok(plan_digest(&plan_entries(&instances, true, &proven)))
    }
}

/// The step that writes the plan: the plan from the instances now, then the
/// guarded commit. Tells the request how it went.
async fn write_plan(
    sup: &Supervisor,
    spec: PlanSpec,
    prepared: oneshot::Sender<PlanPrepared>,
    ctx: &mut OperationContext,
) -> Result<RestartPlan, WorkError> {
    let unwritten = |prepared: oneshot::Sender<PlanPrepared>, text: String| {
        let _ = prepared.send(PlanPrepared::Failed(text.clone()));
        failed(RestartRefused::Unwritten(text))
    };
    let entries = match sup.restart_plan_entries().await {
        Ok(entries) => entries,
        Err(error) => return Err(unwritten(prepared, error.to_string())),
    };
    let actual = plan_digest(&entries);
    if actual != spec.digest {
        let expected = spec.digest;
        let _ = prepared.send(PlanPrepared::Refused(PlanRefusal::DigestChanged {
            expected,
            actual,
        }));
        return Err(failed(RestartRefused::Changed { expected, actual }));
    }
    let plan = match new_plan(sup, spec, ctx.id(), &entries) {
        Ok(plan) => plan,
        Err(text) => return Err(unwritten(prepared, text)),
    };
    let guard = write_plan_guard(
        plan.clone(),
        Vec::new(),
        recompute(sup.deps().config.handover_proven.clone()),
    );
    match ctx.commit_guarded(guard).await {
        Ok(Guarded::Committed(_)) => {
            let _ = prepared.send(PlanPrepared::Written(Box::new(plan.clone())));
            Ok(plan)
        }
        Ok(Guarded::Refused(reason)) => {
            match reason.downcast::<PlanRefusal>().map(|boxed| *boxed) {
                Ok(PlanRefusal::OpenPlan(open)) => {
                    let _ = prepared.send(PlanPrepared::Refused(PlanRefusal::OpenPlan(open)));
                    Err(failed(RestartRefused::Open(open)))
                }
                Ok(PlanRefusal::DigestChanged { expected, actual }) => {
                    let _ = prepared.send(PlanPrepared::Refused(PlanRefusal::DigestChanged {
                        expected,
                        actual,
                    }));
                    Err(failed(RestartRefused::Changed { expected, actual }))
                }
                Err(_) => Err(unwritten(
                    prepared,
                    "the plan guard refused for a reason this build cannot name".to_owned(),
                )),
            }
        }
        Err(error) => {
            let _ = prepared.send(PlanPrepared::Failed(format!("{error:?}")));
            Err(error)
        }
    }
}

async fn run(sup: Supervisor, request: DaemonRestartRequest, ctx: OperationContext) -> WorkResult {
    let result = restart_steps(&sup, request, ctx).await;
    if result.is_err()
        && let Some(engine) = sup.operations()
    {
        // The daemon keeps running, so it takes requests again.
        engine.set_quiesce(false);
    }
    result
}

async fn restart_steps(
    sup: &Supervisor,
    request: DaemonRestartRequest,
    mut ctx: OperationContext,
) -> WorkResult {
    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let DaemonRestartRequest {
        plan: id,
        digest,
        server_instance_id,
        prepared,
        handover,
    } = request;
    let spec = PlanSpec {
        id,
        digest,
        server_instance_id,
    };
    let mut plan = write_plan(sup, spec, prepared, &mut ctx).await?;
    sup.deps().events.restart_plan(&plan);

    ctx.begin_step(StepSpec::new(StageName::STOP).irreversible())
        .await?;
    ctx.set_stage(StageName::STOP).await?;
    let failure = stop_instances(sup, &mut plan).await;

    ctx.begin_step(StepSpec::new(StageName::COMPLETE).irreversible())
        .await?;
    ctx.set_stage(StageName::COMPLETE).await?;
    let now = sup.deps().platform.clock().now();
    let operation = ctx.id();
    if let Some(error) = failure {
        plan.abort(now).map_err(|error| {
            WorkError::Failed(Box::new(SupervisorError::internal(error.to_string())))
        })?;
        // A plan that cannot be written as aborted stays `stopping` in the
        // registry, which the next daemon of this boot takes over; the
        // instances it stopped start again here all the same.
        match sup.persist_plan(&plan).await {
            Ok(()) => sup.deps().events.restart_plan(&plan),
            Err(persist) => {
                tracing::warn!(%persist, "the aborted restart plan could not be written");
            }
        }
        queue_restore(sup, &plan);
        let sup = sup.clone();
        tokio::spawn(async move {
            wait_final(&sup, operation).await;
            sup.start_reconciled();
        });
        return Err(WorkError::Failed(Box::new(error)));
    }
    plan.hand_over(now).map_err(|error| {
        WorkError::Failed(Box::new(SupervisorError::internal(error.to_string())))
    })?;
    sup.persist_plan(&plan)
        .await
        .map_err(|error| WorkError::Failed(Box::new(error)))?;
    sup.deps().events.restart_plan(&plan);
    ctx.finish_step(None).await?;
    let waiting = sup.clone();
    tokio::spawn(async move {
        wait_final(&waiting, operation).await;
        if !waiting.is_shutdown() {
            handover.handed_over();
        }
    });
    Ok(WorkOutcome {
        result: OperationOutcome {
            resources: Vec::new(),
            partial: Vec::new(),
        },
        finalize: None,
    })
}

/// Stops every instance the plan stops, at most
/// `reconcile_start_concurrency` at a time, and records each stop in the
/// plan. The first failure keeps the instances that have not begun from
/// starting; the ones in flight finish. Returns that first failure.
async fn stop_instances(sup: &Supervisor, plan: &mut RestartPlan) -> Option<SupervisorError> {
    let permits = Arc::new(Semaphore::new(
        sup.deps().config.reconcile_start_concurrency.max(1),
    ));
    let aborted = Arc::new(AtomicBool::new(false));
    let mut set: JoinSet<(InstanceId, Option<Result<(), SupervisorError>>)> = JoinSet::new();
    for id in stoppers(plan) {
        let (sup, permits, aborted) = (sup.clone(), Arc::clone(&permits), Arc::clone(&aborted));
        set.spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return (id, None);
            };
            if aborted.load(Ordering::SeqCst) {
                return (id, None);
            }
            let result = sup.stop_for_plan(id).await;
            if result.is_err() {
                aborted.store(true, Ordering::SeqCst);
            }
            (id, Some(result))
        });
    }
    let mut first = None;
    while let Some(joined) = set.join_next().await {
        let (id, result) = match joined {
            Ok(done) => done,
            Err(error) => {
                aborted.store(true, Ordering::SeqCst);
                first.get_or_insert(SupervisorError::internal(format!(
                    "a stop task failed: {error}"
                )));
                continue;
            }
        };
        match result {
            Some(Ok(())) => {
                let now = sup.deps().platform.clock().now();
                if plan.record_stopped(id, now).is_ok() {
                    match sup.persist_plan(plan).await {
                        Ok(()) => sup.deps().events.restart_plan(plan),
                        Err(error) => {
                            aborted.store(true, Ordering::SeqCst);
                            first.get_or_insert(error);
                        }
                    }
                }
            }
            Some(Err(error)) => {
                tracing::warn!(instance = %id, %error, "an instance did not stop for the daemon restart");
                first.get_or_insert(error);
            }
            None => {}
        }
    }
    first
}

/// Queues the start again of every instance a plan stopped, for
/// [`Supervisor::start_reconciled`].
fn queue_restore(sup: &Supervisor, plan: &RestartPlan) {
    let mut pending = sup
        .shared
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for entry in plan.instances() {
        if entry.stopped_at.is_some() {
            pending.push(PendingRun {
                id: entry.instance_id,
                name: entry.name.clone(),
                kind: PendingKind::Restore,
                trigger: pending::PLANNED_RESTART_INPUT,
            });
        }
    }
}
