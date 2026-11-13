//! The `run` step of Up (spec 0046 AC-6 to AC-11): install, then create,
//! then start in graph order. It starts only child operations through
//! [`UpChildren`] and waits for them with `OperationEngine::wait_final`; it
//! never calls `ProcessSupervision`, a downloader, or writes a file.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use stackly_domain::{
    EnvironmentId, ErrorCodeName, FailureError, FailureItem, InstanceId, InstanceLifecycle,
    MemberAlias, OperationId, OperationState, OperationTrigger, Progress, ProviderName,
    ResourceRef, RuntimeId, SecretRef, ServiceReport, Sha256Digest, SkipReason, StageName, UpPhase,
    UpStage,
};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::board::{LiveService, LiveUp, UpBoard};
use super::children::{UpChildren, UpReads};
use super::graph::{Decision, Graph, NodeState, next, next_deadline};
use super::limits::{
    CHILD_QUEUE_TIMEOUT_SECONDS, SERVICE_SETTLE_TIMEOUT_SECONDS, UP_INSTALL_CONCURRENCY,
    UP_POLL_INTERVAL, UP_START_CONCURRENCY,
};
use super::report::{
    DEPENDENCY_NOT_READY, INSTANCE_BLOCKED, NEEDS_SECRET, RESOURCE_BUSY, SERVICE_NOT_READY,
};
use crate::environment::children::{ChildOrigin, ChildStart};
use crate::instance::create::{CreateDraft, EnvironmentBinding};
use crate::operation::{EngineError, OperationContext, OperationEngine, WorkError, codes};

const INSTALL: StageName = StageName::from_static("install");
const CREATE: StageName = StageName::from_static("create");
const START: StageName = StageName::from_static("start");

/// The code of a child refused by the quiesce of a planned restart.
const RESTART_PENDING: &str = "daemon.restart_pending";

/// What the driver runs on.
#[derive(Clone)]
pub struct DriverDeps {
    /// The engine the children run in.
    pub engine: OperationEngine,
    /// Starts the children.
    pub children: Arc<dyn UpChildren>,
    /// Reads instances.
    pub reads: Arc<dyn UpReads>,
    /// The progress table.
    pub board: Arc<UpBoard>,
}

/// What a `create` service needs.
#[derive(Debug, Clone)]
pub struct CreateJob {
    /// The draft, on the planned runtime until install gives the real one.
    pub draft: CreateDraft,
    /// The environment binding.
    pub binding: EnvironmentBinding,
    /// The artifact to install first, if any.
    pub install: Option<(ProviderName, Sha256Digest)>,
}

/// One service the driver works on.
#[derive(Debug, Clone)]
pub struct Service {
    /// What the Up knows of it.
    pub report: ServiceReport,
    /// The dependencies it waits for.
    pub waiting_on: Vec<MemberAlias>,
    /// Its instance, once it has one.
    pub instance: Option<InstanceId>,
    /// Its create, for a `create` item.
    pub create: Option<CreateJob>,
    /// When it entered `starting`, since the start stage began.
    starting_since: Option<Duration>,
}

impl Service {
    /// A service of the plan, `pending`.
    #[must_use]
    pub fn new(
        alias: MemberAlias,
        instance: Option<InstanceId>,
        create: Option<CreateJob>,
    ) -> Self {
        Self {
            report: ServiceReport::new(alias, UpPhase::Pending),
            waiting_on: Vec::new(),
            instance,
            create,
            starting_since: None,
        }
    }

    fn alias(&self) -> &MemberAlias {
        self.report.alias()
    }

    fn phase(&self) -> UpPhase {
        self.report.phase()
    }

    fn set(&mut self, phase: UpPhase) {
        self.report = rebuild(&self.report, phase);
    }

    fn end(&mut self, end: End) {
        let alias = self.alias().clone();
        let mut report = ServiceReport::new(alias, end.phase);
        if let Some(stage) = end.stage {
            report = report.with_stage(stage);
        }
        if let Some(code) = end.code {
            report = report.with_code(code);
        }
        if let Some((dependency, reason)) = end.dependency {
            report = report.with_dependency(dependency, reason);
        }
        if let Some(operation) = end.operation.or(self.report.operation()) {
            report = report.with_operation(operation);
        }
        if !end.references.is_empty() {
            report = report.with_references(end.references);
        }
        self.report = report;
        self.waiting_on.clear();
    }
}

/// The report with another phase, keeping its operation.
fn rebuild(report: &ServiceReport, phase: UpPhase) -> ServiceReport {
    let fresh = ServiceReport::new(report.alias().clone(), phase);
    match report.operation() {
        Some(operation) => fresh.with_operation(operation),
        None => fresh,
    }
}

/// How a service ended, or the phase it moves to.
#[derive(Debug, Clone)]
struct End {
    phase: UpPhase,
    stage: Option<UpStage>,
    code: Option<ErrorCodeName>,
    dependency: Option<(MemberAlias, SkipReason)>,
    operation: Option<OperationId>,
    references: Vec<SecretRef>,
}

impl End {
    const fn phase(phase: UpPhase) -> Self {
        Self {
            phase,
            stage: None,
            code: None,
            dependency: None,
            operation: None,
            references: Vec::new(),
        }
    }

    fn ready(operation: Option<OperationId>) -> Self {
        Self {
            operation,
            ..Self::phase(UpPhase::Ready)
        }
    }

    fn failed(stage: UpStage, code: ErrorCodeName, operation: Option<OperationId>) -> Self {
        Self {
            stage: Some(stage),
            code: Some(code),
            operation,
            ..Self::phase(UpPhase::Failed)
        }
    }

    /// The end of a child error: `needs_secret` with its references for
    /// `secret.needs_secret`, `failed` otherwise.
    fn of_error(
        stage: UpStage,
        code: &ErrorCodeName,
        items: &[FailureItem],
        operation: Option<OperationId>,
    ) -> Self {
        if code == &NEEDS_SECRET {
            return Self {
                stage: Some(stage),
                code: Some(code.clone()),
                operation,
                references: references_of(items),
                ..Self::phase(UpPhase::NeedsSecret)
            };
        }
        Self::failed(stage, code.clone(), operation)
    }

    fn skipped(dependency: MemberAlias, reason: SkipReason) -> Self {
        Self {
            stage: Some(UpStage::Wait),
            code: Some(DEPENDENCY_NOT_READY),
            dependency: Some((dependency, reason)),
            ..Self::phase(UpPhase::Skipped)
        }
    }
}

/// The references of a `secret.needs_secret` error's items.
fn references_of(items: &[FailureItem]) -> Vec<SecretRef> {
    items
        .iter()
        .filter(|item| item.kind().as_str() == "reference")
        .filter_map(|item| {
            item.fields()
                .iter()
                .find(|(key, _)| key.as_str() == "reference")
                .and_then(|(_, value)| value.parse().ok())
        })
        .collect()
}

/// How a child operation ended, as the driver sees it.
enum ChildEnd {
    Succeeded(Vec<ResourceRef>),
    Failed(ErrorCodeName, Vec<FailureItem>),
    Cancelled,
    QueueTimeout,
    ShuttingDown,
}

/// Waits for the child `operation` to end. Once `cancel` turns true, it
/// cancels the child on behalf of `trigger` and keeps waiting. A child
/// still `queued` after `queue_timeout` is cancelled too.
async fn wait_child(
    engine: OperationEngine,
    operation: OperationId,
    mut cancel: watch::Receiver<bool>,
    trigger: OperationTrigger,
    queue_timeout: Option<Duration>,
) -> ChildEnd {
    let mut cancel_sent = false;
    let mut cancel_open = true;
    let mut timed_out = false;
    let deadline = queue_timeout.map(|timeout| Instant::now() + timeout);
    loop {
        if !cancel_sent && *cancel.borrow() {
            let _ = engine.cancel(operation, trigger.clone()).await;
            cancel_sent = true;
        }
        tokio::select! {
            detail = engine.wait_final(operation) => {
                return match detail {
                    Err(EngineError::ShuttingDown) => ChildEnd::ShuttingDown,
                    Err(_) | Ok(None) => ChildEnd::Failed(codes::INTERNAL_UNEXPECTED, Vec::new()),
                    Ok(Some(detail)) => match detail.view.operation.state() {
                        OperationState::Succeeded => {
                            let resources = detail
                                .view
                                .journal
                                .result()
                                .and_then(|payload| {
                                    crate::operation::OperationOutcome::from_payload(payload).ok()
                                })
                                .map(|outcome| outcome.resources)
                                .unwrap_or_default();
                            ChildEnd::Succeeded(resources)
                        }
                        OperationState::Cancelled if timed_out => ChildEnd::QueueTimeout,
                        OperationState::Cancelled => ChildEnd::Cancelled,
                        OperationState::NeedsRecovery => {
                            ChildEnd::Failed(codes::RECOVERY_REQUIRED, Vec::new())
                        }
                        _ => {
                            let error = detail.view.journal.failure_error();
                            ChildEnd::Failed(
                                error.map_or(codes::INTERNAL_UNEXPECTED, |error| error.code().clone()),
                                error.map(|error| error.items().to_vec()).unwrap_or_default(),
                            )
                        }
                    },
                };
            }
            changed = cancel.changed(), if cancel_open && !cancel_sent => {
                if changed.is_err() {
                    cancel_open = false;
                }
            }
            () = sleep_until(deadline), if deadline.is_some() && !timed_out && !cancel_sent => {
                let queued = matches!(
                    engine.get(operation).await,
                    Ok(Some(detail)) if detail.view.operation.state() == OperationState::Queued
                );
                if queued {
                    timed_out = true;
                    let _ = engine.cancel(operation, trigger.clone()).await;
                    cancel_sent = true;
                } else {
                    // It left the queue: no more timeout.
                    timed_out = true;
                }
            }
        }
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// The outcome of the whole `run` step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// Every service ended.
    Done,
    /// The Up was cancelled; every service ended, the rest `cancelled`.
    Cancelled,
    /// The daemon shuts down: stop now, with no closing commit.
    Abandoned,
}

/// The `run` step's driver.
pub struct Driver<'a> {
    ctx: &'a mut OperationContext,
    deps: DriverDeps,
    origin: ChildOrigin,
    environment: EnvironmentId,
    started_at: Timestamp,
    graph: Graph,
    services: Vec<Service>,
    cancel: watch::Sender<bool>,
    cancelling: bool,
    quiesced: bool,
}

/// What a stop of the operation asks the driver to do.
enum Stop {
    Abandon,
}

impl<'a> Driver<'a> {
    /// A driver over `services` in manifest order, with `graph` for the
    /// start stage.
    #[must_use]
    pub fn new(
        ctx: &'a mut OperationContext,
        deps: DriverDeps,
        origin: ChildOrigin,
        environment: EnvironmentId,
        started_at: Timestamp,
        graph: Graph,
        services: Vec<Service>,
    ) -> Self {
        let (cancel, _) = watch::channel(false);
        Self {
            ctx,
            deps,
            origin,
            environment,
            started_at,
            graph,
            services,
            cancel,
            cancelling: false,
            quiesced: false,
        }
    }

    /// Returns the reports of every service, in manifest order.
    #[must_use]
    pub fn reports(&self) -> Vec<ServiceReport> {
        self.services
            .iter()
            .map(|service| service.report.clone())
            .collect()
    }

    /// Runs install, create, and start, and returns how it ended.
    ///
    /// # Errors
    ///
    /// Returns [`WorkError::Engine`] when a stage cannot be recorded.
    pub async fn run(&mut self) -> Result<Ending, WorkError> {
        self.publish();
        if self.install().await.is_err() {
            return Ok(Ending::Abandoned);
        }
        if self.create().await.is_err() {
            return Ok(Ending::Abandoned);
        }
        if self.start().await.is_err() {
            return Ok(Ending::Abandoned);
        }
        Ok(if self.cancelling {
            Ending::Cancelled
        } else {
            Ending::Done
        })
    }

    /// Writes the board and the progress.
    fn publish(&mut self) {
        let services = self
            .services
            .iter()
            .map(|service| LiveService {
                report: service.report.clone(),
                waiting_on: service.waiting_on.clone(),
            })
            .collect();
        self.deps.board.put(
            self.environment,
            LiveUp {
                operation: self.origin.operation.parent,
                started_at: self.started_at,
                services,
            },
        );
        let done = self
            .services
            .iter()
            .filter(|service| service.phase().is_final())
            .count() as u64;
        self.ctx.set_progress(Progress::Counted {
            done,
            total: self.services.len() as u64,
        });
    }

    /// Handles a stop of the operation: a cancel cancels the children and
    /// lets the stages close; a shutdown abandons.
    fn on_stop(&mut self) -> Result<(), Stop> {
        match self.ctx.check_cancel() {
            Ok(()) => Ok(()),
            Err(WorkError::Cancelled) => {
                self.cancelling = true;
                let _ = self.cancel.send(true);
                Ok(())
            }
            Err(_) => Err(Stop::Abandon),
        }
    }

    /// Awaits `future` while honouring a stop of the operation.
    async fn guarded<T>(&mut self, future: impl Future<Output = T>) -> Result<T, Stop> {
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

    fn stopped_starting(&self) -> bool {
        self.cancelling || self.quiesced
    }

    /// Ends every service that has not ended `cancelled`.
    fn cancel_rest(&mut self, only: impl Fn(&Service) -> bool) {
        for service in &mut self.services {
            if !service.phase().is_final() && only(service) {
                service.end(End::phase(UpPhase::Cancelled));
            }
        }
    }

    async fn install(&mut self) -> Result<(), Stop> {
        let mut artifacts: BTreeMap<Sha256Digest, ProviderName> = BTreeMap::new();
        for service in &mut self.services {
            if let Some((provider, artifact)) =
                service.create.as_ref().and_then(|job| job.install.clone())
            {
                artifacts.entry(artifact).or_insert(provider);
                service.set(UpPhase::Installing);
            }
        }
        if artifacts.is_empty() {
            return Ok(());
        }
        if self.ctx.set_stage(INSTALL).await.is_err() {
            return Err(Stop::Abandon);
        }
        self.publish();
        let mut queue: Vec<(Sha256Digest, ProviderName)> = artifacts.into_iter().collect();
        queue.reverse();
        let mut tasks: JoinSet<(Sha256Digest, Result<RuntimeId, End>)> = JoinSet::new();
        loop {
            while tasks.len() < UP_INSTALL_CONCURRENCY && !self.stopped_starting() {
                let Some((artifact, provider)) = queue.pop() else {
                    break;
                };
                let deps = self.deps.clone();
                let origin = self.origin.clone();
                let cancel = self.cancel.subscribe();
                tasks.spawn(async move {
                    let result = install_one(deps, origin, cancel, provider, artifact).await;
                    (artifact, result)
                });
            }
            if tasks.is_empty() {
                break;
            }
            let joined = self.guarded(tasks.join_next()).await?;
            let Some(Ok((artifact, result))) = joined else {
                continue;
            };
            if matches!(&result, Err(end) if end.phase == UpPhase::Cancelled && self.quiesced_end(end))
            {
                self.quiesced = true;
            }
            for service in &mut self.services {
                let Some(job) = service.create.as_mut() else {
                    continue;
                };
                if job.install.as_ref().map(|(_, sha)| *sha) != Some(artifact) {
                    continue;
                }
                match &result {
                    Ok(runtime) => {
                        job.draft.runtime_id = *runtime;
                        job.install = None;
                        service.set(UpPhase::Pending);
                    }
                    Err(end) => service.end(end.clone()),
                }
            }
            self.publish();
        }
        // Whatever was never installed ends here.
        self.cancel_rest(|service| service.phase() == UpPhase::Installing);
        self.publish();
        Ok(())
    }

    /// Whether `end` is the mark of a child refused by quiesce.
    fn quiesced_end(&self, end: &End) -> bool {
        end.code
            .as_ref()
            .is_some_and(|code| code.as_str() == RESTART_PENDING)
    }

    async fn create(&mut self) -> Result<(), Stop> {
        let creating: Vec<usize> = self
            .services
            .iter()
            .enumerate()
            .filter(|(_, service)| service.create.is_some() && service.phase() == UpPhase::Pending)
            .map(|(index, _)| index)
            .collect();
        if creating.is_empty() {
            return Ok(());
        }
        if self.ctx.set_stage(CREATE).await.is_err() {
            return Err(Stop::Abandon);
        }
        for index in creating {
            if self.stopped_starting() {
                break;
            }
            let Some(job) = self.services[index].create.clone() else {
                continue;
            };
            self.services[index].set(UpPhase::Creating);
            self.publish();
            let deps = self.deps.clone();
            let origin = self.origin.clone();
            let cancel = self.cancel.subscribe();
            let end = self.guarded(create_one(deps, origin, cancel, job)).await?;
            match end {
                Ok((instance, operation)) => {
                    let service = &mut self.services[index];
                    service.instance = Some(instance);
                    service.report = match operation {
                        Some(operation) => {
                            ServiceReport::new(service.alias().clone(), UpPhase::Pending)
                                .with_operation(operation)
                        }
                        None => ServiceReport::new(service.alias().clone(), UpPhase::Pending),
                    };
                }
                Err(end) => {
                    if self.quiesced_end(&end) {
                        self.quiesced = true;
                    }
                    self.services[index].end(end);
                }
            }
            self.publish();
        }
        self.cancel_rest(|service| service.create.is_some() && service.instance.is_none());
        self.publish();
        Ok(())
    }

    async fn start(&mut self) -> Result<(), Stop> {
        if self
            .services
            .iter()
            .all(|service| service.phase().is_final())
        {
            return Ok(());
        }
        if self.ctx.set_stage(START).await.is_err() {
            return Err(Stop::Abandon);
        }
        let began = Instant::now();
        let index: HashMap<MemberAlias, usize> = self
            .services
            .iter()
            .enumerate()
            .map(|(index, service)| (service.alias().clone(), index))
            .collect();
        let mut tasks: JoinSet<(usize, End)> = JoinSet::new();
        loop {
            if self.stopped_starting() {
                self.cancel_rest(|service| {
                    matches!(service.phase(), UpPhase::Pending | UpPhase::Waiting)
                });
            } else {
                self.decide(&index, &mut tasks, began.elapsed()).await;
            }
            self.publish();
            if tasks.is_empty()
                && self
                    .services
                    .iter()
                    .all(|service| service.phase().is_final())
            {
                break;
            }
            if tasks.is_empty() && self.stopped_starting() {
                self.cancel_rest(|_| true);
                self.publish();
                break;
            }
            let states = self.states();
            let wake = next_deadline(&self.graph, &states)
                .map(|at| at.saturating_sub(began.elapsed()))
                .unwrap_or(Duration::from_secs(3600));
            let joined = self
                .guarded(async {
                    tokio::select! {
                        joined = tasks.join_next(), if !tasks.is_empty() => joined,
                        () = tokio::time::sleep(wake.max(Duration::from_millis(1))) => None,
                    }
                })
                .await?;
            if let Some(Ok((at, end))) = joined {
                if self.quiesced_end(&end) {
                    self.quiesced = true;
                    self.services[at].end(End::phase(UpPhase::Cancelled));
                } else {
                    self.services[at].end(end);
                }
                self.services[at].starting_since = None;
            }
        }
        Ok(())
    }

    fn states(&self) -> BTreeMap<MemberAlias, NodeState> {
        self.services
            .iter()
            .map(|service| {
                (
                    service.alias().clone(),
                    NodeState {
                        phase: service.phase(),
                        starting_since: service.starting_since,
                    },
                )
            })
            .collect()
    }

    /// Applies the graph's decisions: skips, waits, and starts.
    async fn decide(
        &mut self,
        index: &HashMap<MemberAlias, usize>,
        tasks: &mut JoinSet<(usize, End)>,
        now: Duration,
    ) {
        // A skip changes what later services see, so decide until nothing
        // changes.
        loop {
            let decisions = next(&self.graph, &self.states(), now, UP_START_CONCURRENCY);
            let mut changed = false;
            for decision in decisions {
                match decision {
                    Decision::Skip {
                        service,
                        dependency,
                        reason,
                    } => {
                        if let Some(&at) = index.get(&service) {
                            self.services[at].end(End::skipped(dependency, reason));
                            changed = true;
                        }
                    }
                    Decision::Wait {
                        service,
                        waiting_on,
                    } => {
                        if let Some(&at) = index.get(&service) {
                            self.services[at].set(UpPhase::Waiting);
                            self.services[at].waiting_on = waiting_on;
                        }
                    }
                    Decision::Start(service) => {
                        let Some(&at) = index.get(&service) else {
                            continue;
                        };
                        // Right before it starts, every dependency must
                        // still run and be ready (rule 2).
                        let mut not_ready = None;
                        for edge in self.graph.edges(&service) {
                            let instance = index
                                .get(&edge.dependency)
                                .and_then(|&dep| self.services[dep].instance);
                            let ready = match instance {
                                Some(id) => self
                                    .deps
                                    .reads
                                    .instance(id)
                                    .await
                                    .is_some_and(|now| now.running_and_ready()),
                                None => false,
                            };
                            if !ready {
                                not_ready = Some(edge.dependency.clone());
                                break;
                            }
                        }
                        if let Some(dependency) = not_ready {
                            self.services[at].end(End::skipped(dependency, SkipReason::NotReady));
                            changed = true;
                            continue;
                        }
                        let Some(instance) = self.services[at].instance else {
                            self.services[at].end(End::failed(
                                UpStage::Start,
                                INSTANCE_BLOCKED,
                                None,
                            ));
                            changed = true;
                            continue;
                        };
                        self.services[at].set(UpPhase::Starting);
                        self.services[at].waiting_on.clear();
                        self.services[at].starting_since = Some(now);
                        let deps = self.deps.clone();
                        let origin = self.origin.clone();
                        let cancel = self.cancel.subscribe();
                        tasks.spawn(async move {
                            (at, start_one(deps, origin, cancel, instance).await)
                        });
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }
}

/// The end of a child that the port refused to start.
fn refused(stage: UpStage, error: &FailureError) -> End {
    End::of_error(stage, error.code(), error.items(), None)
}

async fn install_one(
    deps: DriverDeps,
    origin: ChildOrigin,
    cancel: watch::Receiver<bool>,
    provider: ProviderName,
    artifact: Sha256Digest,
) -> Result<RuntimeId, End> {
    if *cancel.borrow() {
        return Err(End::phase(UpPhase::Cancelled));
    }
    let started = deps.children.install(&origin, &provider, artifact).await;
    let operation = match started {
        Err(error) => return Err(refused_or_quiesced(UpStage::Install, &error)),
        Ok(ChildStart::AlreadyDone(runtime)) => return Ok(runtime),
        Ok(ChildStart::Started { operation }) => operation,
    };
    let trigger = origin.operation.trigger.clone();
    match wait_child(deps.engine, operation, cancel, trigger, None).await {
        ChildEnd::Succeeded(resources) => resources
            .iter()
            .find_map(|resource| match resource {
                ResourceRef::Runtime(id) => Some(*id),
                _ => None,
            })
            .ok_or_else(|| {
                End::failed(
                    UpStage::Install,
                    codes::INTERNAL_UNEXPECTED,
                    Some(operation),
                )
            }),
        ChildEnd::Failed(code, items) => Err(End::of_error(
            UpStage::Install,
            &code,
            &items,
            Some(operation),
        )),
        ChildEnd::Cancelled | ChildEnd::QueueTimeout | ChildEnd::ShuttingDown => Err(End {
            operation: Some(operation),
            ..End::phase(UpPhase::Cancelled)
        }),
    }
}

/// The end of a refused child; a quiesce refusal keeps its code so the
/// driver stops starting children, and the service ends `cancelled`.
fn refused_or_quiesced(stage: UpStage, error: &FailureError) -> End {
    if error.code().as_str() == RESTART_PENDING {
        return End {
            code: Some(error.code().clone()),
            ..End::phase(UpPhase::Cancelled)
        };
    }
    refused(stage, error)
}

async fn create_one(
    deps: DriverDeps,
    origin: ChildOrigin,
    cancel: watch::Receiver<bool>,
    job: CreateJob,
) -> Result<(InstanceId, Option<OperationId>), End> {
    if *cancel.borrow() {
        return Err(End::phase(UpPhase::Cancelled));
    }
    let started = deps.children.create(&origin, job.draft, job.binding).await;
    let operation = match started {
        Err(error) => return Err(refused_or_quiesced(UpStage::Create, &error)),
        Ok(ChildStart::AlreadyDone(instance)) => return Ok((instance, None)),
        Ok(ChildStart::Started { operation }) => operation,
    };
    let trigger = origin.operation.trigger.clone();
    match wait_child(deps.engine, operation, cancel, trigger, None).await {
        ChildEnd::Succeeded(resources) => resources
            .iter()
            .find_map(|resource| match resource {
                ResourceRef::Instance(id) => Some((*id, Some(operation))),
                _ => None,
            })
            .ok_or_else(|| {
                End::failed(UpStage::Create, codes::INTERNAL_UNEXPECTED, Some(operation))
            }),
        ChildEnd::Failed(code, items) => Err(End::of_error(
            UpStage::Create,
            &code,
            &items,
            Some(operation),
        )),
        ChildEnd::Cancelled | ChildEnd::QueueTimeout | ChildEnd::ShuttingDown => Err(End {
            operation: Some(operation),
            ..End::phase(UpPhase::Cancelled)
        }),
    }
}

/// Brings one instance to `running` and `ready` (rule 3): waits for one
/// that is on its way, starts one that is stopped or failed, and refuses
/// one that is blocked.
async fn start_one(
    deps: DriverDeps,
    origin: ChildOrigin,
    mut cancel: watch::Receiver<bool>,
    instance: InstanceId,
) -> End {
    let began = Instant::now();
    let settle = Duration::from_secs(SERVICE_SETTLE_TIMEOUT_SECONDS);
    let mut started: Option<OperationId> = None;
    loop {
        if *cancel.borrow() {
            return End {
                operation: started,
                ..End::phase(UpPhase::Cancelled)
            };
        }
        let Some(now) = deps.reads.instance(instance).await else {
            return End::failed(UpStage::Start, INSTANCE_BLOCKED, started);
        };
        match now.lifecycle {
            InstanceLifecycle::Running if now.ready => return End::ready(started),
            InstanceLifecycle::Running
            | InstanceLifecycle::Starting
            | InstanceLifecycle::Initializing
            | InstanceLifecycle::Recovering
            | InstanceLifecycle::Stopping => {
                if began.elapsed() >= settle {
                    return End::failed(UpStage::Start, SERVICE_NOT_READY, started);
                }
                tokio::select! {
                    () = tokio::time::sleep(UP_POLL_INTERVAL) => {}
                    _ = cancel.changed() => {}
                }
            }
            InstanceLifecycle::Stopped
            | InstanceLifecycle::Failed
            | InstanceLifecycle::Uninitialized
                if !now.refuses_start =>
            {
                if started.is_some() {
                    // It stopped again after its start succeeded.
                    return End::failed(UpStage::Start, SERVICE_NOT_READY, started);
                }
                let operation = match deps.children.start(&origin, instance).await {
                    Err(error) => return refused_or_quiesced(UpStage::Start, &error),
                    // It is on its way already: wait for it.
                    Ok(ChildStart::AlreadyDone(_)) => {
                        tokio::time::sleep(UP_POLL_INTERVAL).await;
                        continue;
                    }
                    Ok(ChildStart::Started { operation }) => operation,
                };
                started = Some(operation);
                let trigger = origin.operation.trigger.clone();
                let queue = Some(Duration::from_secs(CHILD_QUEUE_TIMEOUT_SECONDS));
                match wait_child(
                    deps.engine.clone(),
                    operation,
                    cancel.clone(),
                    trigger,
                    queue,
                )
                .await
                {
                    ChildEnd::Succeeded(_) => {}
                    ChildEnd::Failed(code, items) => {
                        return End::of_error(UpStage::Start, &code, &items, started);
                    }
                    ChildEnd::QueueTimeout => {
                        return End::failed(UpStage::Start, RESOURCE_BUSY, started);
                    }
                    ChildEnd::Cancelled | ChildEnd::ShuttingDown => {
                        return End {
                            operation: started,
                            ..End::phase(UpPhase::Cancelled)
                        };
                    }
                }
            }
            _ => return End::failed(UpStage::Start, INSTANCE_BLOCKED, started),
        }
    }
}
