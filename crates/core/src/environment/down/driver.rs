//! The `stop` step of Down (spec 0047 AC-5, AC-6, AC-8, AC-9): it stops
//! the confirmed instances in reverse dependency order through child
//! `stop_instance` operations ([`DownChildren`]), waits for them with
//! `OperationEngine::wait_final`, and ends with one final sweep. It never
//! calls `ProcessSupervision`, a downloader, or writes a file, and it
//! never cancels a stop that runs.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{
    EnvironmentId, EnvironmentMember, ErrorCodeName, InstanceId, InstanceName, OperationId,
    OperationState, OperationTrigger, Progress, Registry,
};
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::children::{DownChild, DownChildren};
use super::limits::DOWN_STOP_CONCURRENCY;
use super::order::order;
use super::select::{DownView, read_view, select};
use crate::environment::children::ChildOrigin;
use crate::environment::up::CHILD_QUEUE_TIMEOUT_SECONDS;
use crate::operation::{
    EngineError, OperationContext, OperationEngine, OperationOutcome, WorkError, codes,
};
use crate::supervisor::INSTANCE_IN_USE;
use crate::supervisor::codes::INSTANCE_RETAINED;

/// The code of a child refused by the quiesce of a planned restart.
const RESTART_PENDING: &str = "daemon.restart_pending";

/// `env.down_changed`: the code of an instance left running by the final
/// sweep, and of a refused `env.down`.
pub const DOWN_CHANGED: ErrorCodeName = ErrorCodeName::from_static("env.down_changed");

/// `resource.busy`: a child stop stayed queued too long and the sweep could
/// not settle it.
const RESOURCE_BUSY: ErrorCodeName = ErrorCodeName::from_static("resource.busy");

/// How one instance of a Down ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DownOutcome {
    /// Its stop ran and wrote desired `stopped`.
    Stopped,
    /// Nothing ran and its desired state was `stopped` already.
    AlreadyStopped,
    /// Another lease holds it, so it keeps running.
    Kept,
    /// Its stop failed, or it could not be stopped.
    Failed,
    /// The Down was cancelled or quiesced before its stop began.
    Cancelled,
    /// The final sweep found it running with no demand, outside what Down
    /// was asked to stop (AC-8).
    LeftRunning,
}

impl DownOutcome {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::AlreadyStopped => "already_stopped",
            Self::Kept => "kept",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::LeftRunning => "left_running",
        }
    }

    /// Whether the outcome makes the Down incomplete.
    #[must_use]
    pub const fn is_incomplete(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::LeftRunning)
    }
}

/// What a Down did to one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownResult {
    /// The instance.
    pub instance: InstanceId,
    /// Its name.
    pub name: InstanceName,
    /// How it ended.
    pub outcome: DownOutcome,
    /// The code: the child's error, `env.instance_in_use` for a kept one,
    /// or `env.down_changed` for one left running.
    pub code: Option<ErrorCodeName>,
    /// Its child operation, if one began.
    pub operation: Option<OperationId>,
}

/// How the `stop` step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverEnd {
    /// Every instance ended and the sweep ran.
    Done,
    /// The Down was cancelled: the stops that ran ended, the rest are
    /// `cancelled`, and no sweep ran.
    Cancelled,
    /// A planned restart refused a child: the rest are `cancelled`, and no
    /// sweep ran.
    Quiesced,
    /// The daemon shuts down: stop now.
    Abandoned,
}

/// What the driver runs on.
#[derive(Clone)]
pub struct DownDeps {
    /// The engine the children run in.
    pub engine: OperationEngine,
    /// Starts the children.
    pub children: Arc<dyn DownChildren>,
}

/// Where one instance of the stop set stands.
#[derive(Debug, Clone)]
enum Slot {
    Pending,
    Running,
    /// Its child stayed queued past the limit and was cancelled; the sweep
    /// decides it.
    Busy(Option<OperationId>),
    Ended(DownOutcome, Option<ErrorCodeName>, Option<OperationId>),
    /// Gone or retained: not in the results.
    Dropped,
}

impl Slot {
    const fn is_final(&self) -> bool {
        matches!(self, Self::Busy(_) | Self::Ended(..) | Self::Dropped)
    }
}

/// What one child task gives back.
enum TaskEnd {
    Ended(DownOutcome, Option<ErrorCodeName>, Option<OperationId>),
    Busy(Option<OperationId>),
    Dropped,
    Quiesced,
}

/// A stop of the operation that ends the driver.
struct Abandon;

/// The `stop` step's driver.
pub struct Driver<'a> {
    ctx: &'a mut OperationContext,
    deps: DownDeps,
    origin: ChildOrigin,
    environment: EnvironmentId,
    confirmed: Vec<InstanceId>,
    names: BTreeMap<InstanceId, InstanceName>,
    slots: BTreeMap<InstanceId, Slot>,
    cancelling: bool,
    quiesced: bool,
}

impl<'a> Driver<'a> {
    /// A driver over the confirmed stop set of `environment`.
    #[must_use]
    pub fn new(
        ctx: &'a mut OperationContext,
        deps: DownDeps,
        origin: ChildOrigin,
        environment: EnvironmentId,
        confirmed: Vec<InstanceId>,
    ) -> Self {
        let slots = confirmed.iter().map(|id| (*id, Slot::Pending)).collect();
        Self {
            ctx,
            deps,
            origin,
            environment,
            confirmed,
            names: BTreeMap::new(),
            slots,
            cancelling: false,
            quiesced: false,
        }
    }

    /// Returns what happened to every instance, by name and then ID.
    #[must_use]
    pub fn results(&self) -> Vec<DownResult> {
        let mut results: Vec<DownResult> = self
            .slots
            .iter()
            .filter_map(|(id, slot)| {
                let (outcome, code, operation) = match slot {
                    Slot::Ended(outcome, code, operation) => (*outcome, code.clone(), *operation),
                    Slot::Busy(operation) => (DownOutcome::Failed, Some(RESOURCE_BUSY), *operation),
                    Slot::Pending | Slot::Running => (DownOutcome::Cancelled, None, None),
                    Slot::Dropped => return None,
                };
                Some(DownResult {
                    instance: *id,
                    name: self.names.get(id)?.clone(),
                    outcome,
                    code,
                    operation,
                })
            })
            .collect();
        results.sort_by(|a, b| (&a.name, a.instance).cmp(&(&b.name, b.instance)));
        results
    }

    /// Reads the view of the environment now.
    fn view(&self) -> impl Future<Output = Option<DownView>> + Send + 'static {
        read(Arc::clone(self.ctx.registry()), self.environment)
    }

    fn publish(&mut self) {
        let done = self.slots.values().filter(|slot| slot.is_final()).count() as u64;
        self.ctx.set_progress(Progress::Counted {
            done,
            total: self.confirmed.len() as u64,
        });
    }

    /// Handles a stop of the operation: a cancel stops beginning children;
    /// a shutdown abandons.
    fn on_stop(&mut self) -> Result<(), Abandon> {
        match self.ctx.check_cancel() {
            Ok(()) => Ok(()),
            Err(WorkError::Cancelled) => {
                self.cancelling = true;
                Ok(())
            }
            Err(_) => Err(Abandon),
        }
    }

    /// Awaits `future` while honouring a stop of the operation.
    async fn guarded<T>(&mut self, future: impl Future<Output = T>) -> Result<T, Abandon> {
        tokio::pin!(future);
        loop {
            let stopping = self.cancelling;
            {
                let ctx: &OperationContext = self.ctx;
                tokio::select! {
                    out = &mut future => return Ok(out),
                    () = ctx.stopped(), if !stopping => {}
                }
            }
            self.on_stop()?;
        }
    }

    const fn stopped_beginning(&self) -> bool {
        self.cancelling || self.quiesced
    }

    fn end(&mut self, id: InstanceId, end: TaskEnd) {
        let slot = match end {
            TaskEnd::Ended(outcome, code, operation) => Slot::Ended(outcome, code, operation),
            TaskEnd::Busy(operation) => Slot::Busy(operation),
            TaskEnd::Dropped => Slot::Dropped,
            TaskEnd::Quiesced => {
                self.quiesced = true;
                Slot::Ended(DownOutcome::Cancelled, None, None)
            }
        };
        self.slots.insert(id, slot);
    }

    /// Runs the stops and the final sweep, and returns how it ended.
    ///
    /// # Errors
    ///
    /// Never today; the signature leaves room for a stage write.
    pub async fn run(&mut self) -> Result<DriverEnd, WorkError> {
        let members: Vec<EnvironmentMember> = match self.view().await {
            Some(view) => {
                for instance in &view.instances {
                    self.names.insert(instance.id, instance.name.clone());
                }
                view.members
            }
            None => Vec::new(),
        };
        // An instance of the set that is gone has no name and no result.
        for id in &self.confirmed {
            if !self.names.contains_key(id) {
                self.slots.insert(*id, Slot::Dropped);
            }
        }
        self.publish();
        let mut tasks: JoinSet<(InstanceId, TaskEnd)> = JoinSet::new();
        let mut spawned: HashMap<tokio::task::Id, InstanceId> = HashMap::new();
        loop {
            if !self.stopped_beginning()
                && self
                    .begin(&members, &mut tasks, &mut spawned)
                    .await
                    .is_err()
            {
                return Ok(DriverEnd::Abandoned);
            }
            self.publish();
            if tasks.is_empty() {
                if self.stopped_beginning() || self.slots.values().all(Slot::is_final) {
                    break;
                }
                continue;
            }
            let Ok(joined) = self.guarded(tasks.join_next()).await else {
                return Ok(DriverEnd::Abandoned);
            };
            match joined {
                Some(Ok((id, end))) => self.end(id, end),
                Some(Err(error)) => {
                    if let Some(id) = spawned.get(&error.id()).copied() {
                        self.end(
                            id,
                            TaskEnd::Ended(
                                DownOutcome::Failed,
                                Some(codes::INTERNAL_UNEXPECTED),
                                None,
                            ),
                        );
                    }
                }
                None => {}
            }
        }
        // Whatever never began ends `cancelled`.
        for slot in self.slots.values_mut() {
            if matches!(slot, Slot::Pending | Slot::Running) {
                *slot = Slot::Ended(DownOutcome::Cancelled, None, None);
            }
        }
        if self.cancelling {
            self.publish();
            return Ok(DriverEnd::Cancelled);
        }
        if self.quiesced {
            self.publish();
            return Ok(DriverEnd::Quiesced);
        }
        self.sweep().await;
        self.publish();
        Ok(DriverEnd::Done)
    }

    /// Begins every stop `order` allows, deciding each instance again just
    /// before its child.
    async fn begin(
        &mut self,
        members: &[EnvironmentMember],
        tasks: &mut JoinSet<(InstanceId, TaskEnd)>,
        spawned: &mut HashMap<tokio::task::Id, InstanceId>,
    ) -> Result<(), Abandon> {
        loop {
            let started: BTreeSet<InstanceId> = self
                .slots
                .iter()
                .filter(|(_, slot)| !matches!(slot, Slot::Pending))
                .map(|(id, _)| *id)
                .collect();
            let ended: BTreeSet<InstanceId> = self
                .slots
                .iter()
                .filter(|(_, slot)| slot.is_final())
                .map(|(id, _)| *id)
                .collect();
            let next = order(
                members,
                &self.confirmed,
                &started,
                &ended,
                DOWN_STOP_CONCURRENCY,
            );
            if next.is_empty() {
                return Ok(());
            }
            let mut began = false;
            for id in next {
                if self.stopped_beginning() {
                    return Ok(());
                }
                let reading = self.view();
                let view = self.guarded(reading).await?;
                let decision = view.as_ref().and_then(|view| view.of_instance(id));
                let Some(single) = decision else {
                    self.end(id, TaskEnd::Dropped);
                    continue;
                };
                let selection = select(&single);
                if !selection.keep.is_empty() {
                    self.end(
                        id,
                        TaskEnd::Ended(DownOutcome::Kept, Some(INSTANCE_IN_USE), None),
                    );
                } else if !selection.idle.is_empty() {
                    self.end(id, TaskEnd::Ended(DownOutcome::AlreadyStopped, None, None));
                } else if let Some(blocked) = selection.blocked.first() {
                    self.end(
                        id,
                        TaskEnd::Ended(DownOutcome::Failed, Some(blocked.code.clone()), None),
                    );
                } else if selection.stop.is_empty() {
                    // Retained since the preview.
                    self.end(id, TaskEnd::Dropped);
                } else {
                    self.slots.insert(id, Slot::Running);
                    let deps = self.deps.clone();
                    let origin = self.origin.clone();
                    let environment = self.environment;
                    let handle = tasks
                        .spawn(async move { (id, stop_one(deps, origin, id, environment).await) });
                    spawned.insert(handle.id(), id);
                    began = true;
                }
            }
            self.publish();
            if !began && tasks.is_empty() && self.slots.values().all(Slot::is_final) {
                return Ok(());
            }
            if began {
                return Ok(());
            }
        }
    }

    /// *Sapuan akhir*: decides the busy ones, and names every instance the
    /// fresh selection would stop that this Down did not, or that a kept
    /// stop left without demand.
    async fn sweep(&mut self) {
        let Some(view) = self.view().await else {
            return;
        };
        let selection = select(&view);
        let keep: BTreeSet<InstanceId> = selection.keep.iter().map(|kept| kept.entry.id).collect();
        let idle: BTreeSet<InstanceId> = selection.idle.iter().map(|entry| entry.id).collect();
        for (id, slot) in &mut self.slots {
            if let Slot::Busy(operation) = slot {
                *slot = if keep.contains(id) {
                    Slot::Ended(DownOutcome::Kept, Some(INSTANCE_IN_USE), *operation)
                } else if idle.contains(id) {
                    Slot::Ended(DownOutcome::AlreadyStopped, None, *operation)
                } else {
                    Slot::Ended(DownOutcome::Failed, Some(RESOURCE_BUSY), *operation)
                };
            }
        }
        for entry in &selection.stop {
            match self.slots.get(&entry.id) {
                None => {
                    self.names.insert(entry.id, entry.name.clone());
                    self.slots.insert(
                        entry.id,
                        Slot::Ended(DownOutcome::LeftRunning, Some(DOWN_CHANGED), None),
                    );
                }
                Some(Slot::Ended(DownOutcome::Kept, _, operation)) => {
                    let operation = *operation;
                    self.slots.insert(
                        entry.id,
                        Slot::Ended(DownOutcome::LeftRunning, Some(DOWN_CHANGED), operation),
                    );
                }
                Some(_) => {}
            }
        }
    }
}

/// Reads the view of `environment`; a read that fails is logged and reads
/// as no environment.
async fn read(registry: Arc<dyn Registry>, environment: EnvironmentId) -> Option<DownView> {
    match tokio::task::spawn_blocking(move || read_view(registry.as_ref(), environment)).await {
        Ok(Ok(view)) => view,
        Ok(Err(error)) => {
            tracing::warn!(%error, "a down could not read its environment");
            None
        }
        Err(_) => None,
    }
}

/// Stops one instance through the port and waits for its child.
async fn stop_one(
    deps: DownDeps,
    origin: ChildOrigin,
    instance: InstanceId,
    environment: EnvironmentId,
) -> TaskEnd {
    let started = deps.children.stop(&origin, instance, environment).await;
    let operation = match started {
        Err(error) if error.code().as_str() == RESTART_PENDING => return TaskEnd::Quiesced,
        Err(error) if error.code() == &codes::NOT_FOUND || error.code() == &INSTANCE_RETAINED => {
            return TaskEnd::Dropped;
        }
        Err(error) => {
            return TaskEnd::Ended(DownOutcome::Failed, Some(error.code().clone()), None);
        }
        Ok(DownChild::Kept) => {
            return TaskEnd::Ended(DownOutcome::Kept, Some(INSTANCE_IN_USE), None);
        }
        Ok(DownChild::AlreadyStopped) => {
            return TaskEnd::Ended(DownOutcome::AlreadyStopped, None, None);
        }
        Ok(DownChild::Started { operation }) => operation,
    };
    let trigger = origin.operation.trigger.clone();
    wait_child(
        deps.engine,
        operation,
        trigger,
        Duration::from_secs(CHILD_QUEUE_TIMEOUT_SECONDS),
    )
    .await
}

/// Waits for the child stop `operation`. A child still `queued` after
/// `queue_timeout` is cancelled and ends busy; a stop that runs is never
/// cancelled.
async fn wait_child(
    engine: OperationEngine,
    operation: OperationId,
    trigger: OperationTrigger,
    queue_timeout: Duration,
) -> TaskEnd {
    let deadline = Instant::now() + queue_timeout;
    let mut checked = false;
    let mut timed_out = false;
    loop {
        tokio::select! {
            detail = engine.wait_final(operation) => {
                let ended = |outcome, code| TaskEnd::Ended(outcome, code, Some(operation));
                return match detail {
                    Err(EngineError::ShuttingDown) => ended(DownOutcome::Cancelled, None),
                    Err(_) | Ok(None) => {
                        ended(DownOutcome::Failed, Some(codes::INTERNAL_UNEXPECTED))
                    }
                    Ok(Some(detail)) => match detail.view.operation.state() {
                        OperationState::Succeeded => {
                            let kept = detail
                                .view
                                .journal
                                .result()
                                .and_then(|payload| OperationOutcome::from_payload(payload).ok())
                                .is_some_and(|outcome| {
                                    outcome.partial.iter().any(|item| {
                                        item.code.as_deref() == Some(INSTANCE_IN_USE.as_str())
                                    })
                                });
                            if kept {
                                ended(DownOutcome::Kept, Some(INSTANCE_IN_USE))
                            } else {
                                ended(DownOutcome::Stopped, None)
                            }
                        }
                        OperationState::Cancelled if timed_out => TaskEnd::Busy(Some(operation)),
                        OperationState::Cancelled => {
                            ended(DownOutcome::Failed, Some(codes::CANCELLED))
                        }
                        OperationState::NeedsRecovery => {
                            ended(DownOutcome::Failed, Some(codes::RECOVERY_REQUIRED))
                        }
                        _ => {
                            let code = detail
                                .view
                                .journal
                                .failure_error()
                                .map_or(codes::INTERNAL_UNEXPECTED, |error| error.code().clone());
                            ended(DownOutcome::Failed, Some(code))
                        }
                    },
                };
            }
            () = tokio::time::sleep_until(deadline), if !checked => {
                checked = true;
                let queued = matches!(
                    engine.get(operation).await,
                    Ok(Some(detail)) if detail.view.operation.state() == OperationState::Queued
                );
                if queued {
                    timed_out = true;
                    let _ = engine.cancel(operation, trigger.clone()).await;
                }
            }
        }
    }
}
