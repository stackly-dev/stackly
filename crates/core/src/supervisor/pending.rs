//! What reconciliation runs once the operation engine accepts operations
//! (spec 0010, *Rekonsiliasi*, step 7): the instances it decided to run
//! again, at most [`reconcile_start_concurrency`] at a time, by name.
//!
//! [`reconcile_start_concurrency`]: super::SupervisorConfig::reconcile_start_concurrency

use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{
    DesiredState, InstanceId, InstanceLifecycle, InstanceName, JournalPayload, OperationId,
    OperationKind, OperationResource,
};
use tokio::sync::Semaphore;

use super::Supervisor;
use super::error::SupervisorError;
use super::persist::read_instance;
use super::respawn::{arm_pause, pause_then_respawn};
use super::startup::StartRequest;
use super::stop::StopRequest;
use crate::operation::{BeginError, BeginRequest, BusyPolicy, Origin};

/// The journal input of an operation reconciliation begins.
const TRIGGER_INPUT: &str = r#"{"trigger":"reconcile"}"#;
/// How often the runner looks at an operation it waits for.
const POLL: Duration = Duration::from_millis(50);

/// What reconciliation runs for one instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingKind {
    /// The instance is `recovering`: the respawn of the restart policy,
    /// after `RESTART_BASE_DELAY`.
    Respawn,
    /// The instance is `stopped` or `failed` with desired `running`: a
    /// system `start_instance` (trigger `reconcile`, no dialog).
    Start,
    /// A proven process runs with desired `stopped`: a system
    /// `stop_instance` (trigger `reconcile`, no lease check).
    Stop,
}

/// One instance reconciliation decided to run again.
#[derive(Debug, Clone)]
pub(crate) struct PendingRun {
    pub(crate) id: InstanceId,
    pub(crate) name: InstanceName,
    pub(crate) kind: PendingKind,
}

impl Supervisor {
    /// Runs what reconciliation decided to run again: the instances that
    /// were `recovering`, `stopped`, or `failed` with desired `running`
    /// start, and the running ones with desired `stopped` stop, at most
    /// [`reconcile_start_concurrency`](super::SupervisorConfig::reconcile_start_concurrency)
    /// at a time, ordered by name. Call it right after
    /// [`OperationEngine::start`](crate::operation::OperationEngine::start):
    /// before that the engine answers `begin` with not ready. The work runs
    /// in the background and this returns at once; calling it again finds
    /// nothing left to run.
    pub fn start_reconciled(&self) {
        let mut pending = std::mem::take(
            &mut *self
                .shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if pending.is_empty() {
            return;
        }
        pending.sort_by(|a, b| a.name.cmp(&b.name));
        let sup = self.clone();
        tokio::spawn(async move {
            let permits = Arc::new(Semaphore::new(
                sup.deps().config.reconcile_start_concurrency.max(1),
            ));
            for run in pending {
                let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                    return;
                };
                if sup.is_shutdown() {
                    return;
                }
                let sup = sup.clone();
                tokio::spawn(async move {
                    run_one(&sup, &run).await;
                    drop(permit);
                });
            }
        });
    }
}

/// Runs one pending item and waits until its operation is final.
async fn run_one(sup: &Supervisor, run: &PendingRun) {
    let deps = sup.deps();
    let id = run.id;
    let mut degraded = false;
    let instance = loop {
        match read_instance(deps, id).await {
            Ok(instance) => break Some(instance),
            // A failed read decides nothing (AC-22): the run waits and
            // reads again, and the subsystem reads `degraded` meanwhile.
            Err(SupervisorError::Registry(error)) if !sup.is_shutdown() => {
                tracing::warn!(instance = %id, %error, "the registry could not be read for a pending run; trying again");
                degraded = true;
                deps.events
                    .registry_degraded(true, "the registry cannot be read");
                tokio::time::sleep(deps.config.restart_base_delay).await;
            }
            Err(_) => break None,
        }
    };
    if degraded {
        deps.events.registry_degraded(false, "");
    }
    let Some(instance) = instance else {
        return;
    };
    // Something may have changed since reconciliation decided: a pending
    // run only goes on when the instance still looks as it did.
    let wanted = match run.kind {
        PendingKind::Respawn => {
            instance.lifecycle() == InstanceLifecycle::Recovering
                && instance.desired() == DesiredState::Running
        }
        PendingKind::Start => {
            matches!(
                instance.lifecycle(),
                InstanceLifecycle::Stopped | InstanceLifecycle::Failed
            ) && instance.desired() == DesiredState::Running
        }
        PendingKind::Stop => {
            matches!(
                instance.lifecycle(),
                InstanceLifecycle::Running | InstanceLifecycle::Stopping
            ) && instance.desired() == DesiredState::Stopped
        }
    };
    if !wanted {
        return;
    }
    let operation = match run.kind {
        PendingKind::Respawn => {
            let delay = deps.config.restart_base_delay;
            let (actor, cancel) = arm_pause(sup, id, delay);
            pause_then_respawn(sup, &actor, delay, cancel).await
        }
        PendingKind::Start => {
            begin(
                sup,
                id,
                OperationKind::StartInstance,
                sup.start_work(StartRequest::background(id)),
            )
            .await
        }
        PendingKind::Stop => {
            begin(
                sup,
                id,
                OperationKind::StopInstance,
                sup.stop_work(StopRequest::reconcile(id)),
            )
            .await
        }
    };
    if let Some(operation) = operation {
        wait_final(sup, operation).await;
    }
}

/// Begins a system operation of reconciliation. It queues behind a user
/// operation that already holds the instance, and tries again while the
/// engine is not ready.
async fn begin(
    sup: &Supervisor,
    id: InstanceId,
    kind: OperationKind,
    work: Box<dyn crate::operation::OperationWork>,
) -> Option<OperationId> {
    let engine = sup.operations()?;
    let input = JournalPayload::new(TRIGGER_INPUT, crate::operation::limits::INPUT_MAX).ok()?;
    let mut work = Some(work);
    loop {
        if sup.is_shutdown() {
            return None;
        }
        let request = BeginRequest {
            kind,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: input.clone(),
            work: work.take()?,
            precheck: None,
        };
        match engine.begin(request).await {
            Ok(outcome) => return outcome.view().map(|view| view.operation.id()),
            Err(BeginError::NotReady) => {
                // The work went with the request; make it again.
                work = Some(match kind {
                    OperationKind::StopInstance => sup.stop_work(StopRequest::reconcile(id)),
                    _ => sup.start_work(StartRequest::background(id)),
                });
                tokio::time::sleep(POLL).await;
            }
            Err(error) => {
                tracing::warn!(instance = %id, %error, "an operation of reconciliation could not begin");
                return None;
            }
        }
    }
}

/// Waits until `operation` reaches a final state, or the daemon shuts down.
async fn wait_final(sup: &Supervisor, operation: OperationId) {
    let Some(engine) = sup.operations() else {
        return;
    };
    loop {
        if sup.is_shutdown() {
            return;
        }
        match engine.get(operation).await {
            Ok(Some(detail)) if detail.view.operation.state().is_final() => return,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return,
        }
        tokio::time::sleep(POLL).await;
    }
}
