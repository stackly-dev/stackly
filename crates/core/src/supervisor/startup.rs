//! The startup transaction (spec 0010, *Startup transaction*, AC-2, AC-3,
//! AC-6, AC-8, AC-17, AC-18): one `start_instance` operation that checks
//! everything before its first side effect, spawns the main process
//! suspended, records its identity, resumes it, and waits for readiness,
//! with the restart policy applied across attempts inside the operation.
//!
//! The stages, in order, are each a stage of the operation:
//! `validate`, `lock`, `verify`, `endpoints`, `journal` (the first side
//! effect), `render`, `spawn`, `identity`, `readiness`, `publish`, and
//! `complete`. A respawn after a crash repeats `lock` to `readiness` under
//! the same operation, with no dialog.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{
    DesiredState, Endpoint, EndpointName, FailureCode, Instance, InstanceFilter, InstanceId,
    InstanceLifecycle, IpEndpoint, LifecycleEvent, Port, PortRequest, ProcessIdentity,
    ProcessObservation, ResourceRef, StageName, WriteOp, endpoints_conflict,
};
use stackly_platform::{
    ExitNotice, ExitStatus, InstanceLock, OutputTarget, SpawnRequest, StdinSource,
};
use tokio::sync::watch;
use tokio::time::Instant;

use super::actor::{ExitSlot, InstanceActor, ProcessSlot, restart_status};
use super::drain::{DrainSlot, Opening};
use super::error::{EndpointFault, SupervisorError, UnavailableReason};
use super::exit::{crash_reason, finish_process, remove_material, spawn_exit_task};
use super::init::config_invalid;
use super::lock::{guard_last_observation, take_lock};
use super::persist::{
    CommitMode, ExitRecord, apply_event, apply_exit, blocking, commit_instance, read_instance,
    read_instance_actor,
};
use super::prepare::{Checks, Prepared, prepare};
use super::readiness::spawn_probes;
use super::respawn::show_pause;
use super::restart::{JitterFactor, RestartDecision};
use super::steps::{command_parts, run_render};
use super::{
    BindOptions, BindOutcome, DrainContext, DrainOutcome, SpoolDrain, Supervisor, SupervisorDeps,
    secrets,
};
use crate::operation::{
    BoxFuture, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError, WorkOutcome,
    WorkResult,
};
use crate::provider::{MarkerRead, PreflightOutcome};
use crate::runtime::launch::{LaunchRefusal, check_launchable, classify_early_exit};
use crate::secret::{ResolvedSecrets, SecretCaller};
use crate::{ensure_instance_run_dir, remove_owned_tree};

/// Who starts the instance, which decides whether the secret store may show
/// a dialog on the first attempt.
#[derive(Debug, Clone)]
pub enum StartTrigger {
    /// A client request; the caller's interaction applies to the first
    /// attempt only.
    User(SecretCaller),
    /// The daemon itself (the restart policy or reconciliation): never a
    /// dialog.
    Background,
}

/// What [`Supervisor::start_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StartRequest {
    /// The instance to start.
    pub instance: InstanceId,
    /// Who asks.
    pub trigger: StartTrigger,
    /// A respawn the actor's timer began after a crash: the instance is
    /// `recovering`, so there is no `journal` and no `start_begin`.
    pub(crate) respawn: bool,
}

impl StartRequest {
    /// A start a client asked for.
    #[must_use]
    pub const fn user(instance: InstanceId, caller: SecretCaller) -> Self {
        Self {
            instance,
            trigger: StartTrigger::User(caller),
            respawn: false,
        }
    }

    /// A start the daemon makes on its own.
    #[must_use]
    pub const fn background(instance: InstanceId) -> Self {
        Self {
            instance,
            trigger: StartTrigger::Background,
            respawn: false,
        }
    }

    /// The respawn of an instance that crashed and waits `recovering`,
    /// begun by the actor when its pause ends (trigger `restart_policy`).
    /// It repeats `lock` to `readiness` with no dialog and no
    /// `start_begin`, and does nothing when the instance is no longer
    /// `recovering` with desired `running`.
    #[must_use]
    pub(crate) const fn respawn(instance: InstanceId) -> Self {
        Self {
            instance,
            trigger: StartTrigger::Background,
            respawn: true,
        }
    }
}

impl Supervisor {
    /// Builds the operation work of `start_instance` for the engine. The
    /// engine's kind policy for it is `CloseInterrupted`, so a cancel
    /// reaches the work at any point.
    #[must_use]
    pub fn start_work(&self, request: StartRequest) -> Box<dyn OperationWork> {
        Box::new(StartWork {
            sup: self.clone(),
            request,
        })
    }
}

struct StartWork {
    sup: Supervisor,
    request: StartRequest,
}

impl OperationWork for StartWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_start(&self.sup, &self.request, ctx).await })
    }
}

fn failed(error: SupervisorError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

fn success(id: InstanceId) -> WorkOutcome {
    WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Instance(id)],
            partial: Vec::new(),
        },
        finalize: None,
    }
}

/// Enters a stage: the journal step on the first attempt (from `spawn` on
/// it cannot be undone), then the visible stage.
async fn enter(
    ctx: &mut OperationContext,
    stage: StageName,
    first_attempt: bool,
    irreversible: bool,
) -> Result<(), WorkError> {
    if first_attempt {
        let spec = StepSpec::new(stage.clone());
        ctx.begin_step(if irreversible {
            spec.irreversible()
        } else {
            spec
        })
        .await?;
    } else {
        ctx.check_cancel()?;
    }
    ctx.set_stage(stage).await
}

/// What the checks before the first side effect hold on to: the lock, the
/// plans' inputs, the secrets, and the ports chosen.
struct Armed {
    prepared: Prepared,
    secrets: ResolvedSecrets,
    lock: Arc<InstanceLock>,
    ports: BTreeMap<EndpointName, Port>,
}

async fn run_start(
    sup: &Supervisor,
    request: &StartRequest,
    mut ctx: OperationContext,
) -> WorkResult {
    let deps = sup.deps();
    let id = request.instance;
    let actor = sup.actor(id);
    let first_caller = match &request.trigger {
        StartTrigger::User(caller) => caller.clone(),
        StartTrigger::Background => SecretCaller::background(),
    }
    .in_operation(ctx.id());

    enter(&mut ctx, StageName::VALIDATE, true, false).await?;
    let instance = read_instance(deps, id).await.map_err(failed)?;
    if request.respawn {
        return run_respawn(sup, request, ctx, &instance).await;
    }
    match instance.lifecycle() {
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering => {
            // Nothing to do; only a stored desired `stopped` changes.
            if instance.desired() != DesiredState::Running {
                commit_instance(sup, id, None, CommitMode::Operation, |changes| {
                    changes.apply(|instance| {
                        instance.set_desired(DesiredState::Running);
                        Ok::<_, SupervisorError>(())
                    })?;
                    Ok(Vec::new())
                })
                .await
                .map_err(failed)?;
            }
            return Ok(success(id));
        }
        InstanceLifecycle::Stopping => return Err(failed(SupervisorError::InstanceStopping)),
        InstanceLifecycle::Unknown => {
            return Err(failed(SupervisorError::IdentityUnproven { pid: None }));
        }
        InstanceLifecycle::Uninitialized | InstanceLifecycle::Initializing => {
            return Err(failed(SupervisorError::NotInitialized {
                lifecycle: instance.lifecycle(),
            }));
        }
        InstanceLifecycle::Retained => return Err(failed(SupervisorError::InstanceRetained)),
        InstanceLifecycle::Stopped | InstanceLifecycle::Failed => {}
    }
    if instance.lifecycle() == InstanceLifecycle::Failed {
        // A user start from `failed` forgets the crash history.
        actor.update(deps, |state| {
            state.restart.clear();
            state.restart_status = restart_status(&deps.config, 0, None, false);
        });
    }

    // Everything up to the journal has no side effect: a cancel here leaves
    // nothing behind, and dropping `armed` releases the lock.
    let mut pass = 0;
    let armed = loop {
        pass += 1;
        let armed = gate(sup, &mut ctx, &first_caller, true).await?;
        ctx.check_cancel()?;
        enter(&mut ctx, StageName::JOURNAL, true, false).await?;
        let journaled = commit_instance(
            sup,
            id,
            Some(&armed.prepared.instance),
            CommitMode::Operation,
            |changes| {
                if changes.instance().desired() != DesiredState::Running {
                    changes.apply(|instance| {
                        instance.set_desired(DesiredState::Running);
                        Ok::<_, SupervisorError>(())
                    })?;
                }
                let now = deps.platform.clock().now();
                apply_event(changes, LifecycleEvent::StartBegin, now)?;
                Ok(Vec::new())
            },
        )
        .await;
        match journaled {
            Ok(_) => break armed,
            // The instance changed since verify read it: validate and
            // verify once more, then give up.
            Err(SupervisorError::RevisionMismatch { .. }) if pass == 1 => drop(armed),
            Err(error) => return Err(failed(error)),
        }
    };

    let result = after_journal(sup, request, &mut ctx, Some(armed), false).await;
    match result {
        Err(WorkError::Cancelled) => cancel_cleanup(sup, &ctx, id).await,
        other => other,
    }
}

/// The respawn of a crashed instance (spec 0010, *Restart policy*): the
/// instance is `recovering`, so the attempt starts at `lock` with no
/// `journal`, and a deadline of `recovery_deadline`. When the instance is
/// no longer `recovering` with desired `running` (a stop came first), there
/// is nothing to respawn and the operation ends without effect.
async fn run_respawn(
    sup: &Supervisor,
    request: &StartRequest,
    mut ctx: OperationContext,
    instance: &Instance,
) -> WorkResult {
    let id = request.instance;
    if instance.lifecycle() != InstanceLifecycle::Recovering
        || instance.desired() != DesiredState::Running
    {
        return Ok(success(id));
    }
    let result = after_journal(sup, request, &mut ctx, None, true).await;
    match result {
        Err(WorkError::Cancelled) => cancel_cleanup(sup, &ctx, id).await,
        other => other,
    }
}

/// Why the checks before a spawn stopped.
enum Gate {
    /// The operation is cancelled, abandoned, or its journal failed.
    Work(WorkError),
    /// A check failed.
    Error(SupervisorError),
}

impl From<WorkError> for Gate {
    fn from(error: WorkError) -> Self {
        Self::Work(error)
    }
}

impl From<SupervisorError> for Gate {
    fn from(error: SupervisorError) -> Self {
        Self::Error(error)
    }
}

impl From<Gate> for WorkError {
    fn from(gate: Gate) -> Self {
        match gate {
            Gate::Work(error) => error,
            Gate::Error(error) => failed(error),
        }
    }
}

/// The checks before the first side effect, and the second guard against a
/// second server: `lock`, `verify`, and `endpoints`.
async fn gate(
    sup: &Supervisor,
    ctx: &mut OperationContext,
    caller: &SecretCaller,
    first_attempt: bool,
) -> Result<Armed, Gate> {
    let deps = sup.deps();
    let id = ctx_instance(ctx);
    enter(ctx, StageName::LOCK, first_attempt, false).await?;
    // The resource lock is the engine's; the instance lock is ours.
    let prepared = prepare(sup, id, Checks::Full).await?;
    let lock = take_lock(deps, &prepared).await?;
    guard_last_observation(deps, &prepared, id).await?;

    enter(ctx, StageName::VERIFY, first_attempt, false).await?;
    verify(sup, &prepared).await?;
    let secrets = secrets::resolve(sup, id, caller).await?;
    dry_run(sup, &prepared, &secrets)?;

    enter(ctx, StageName::ENDPOINTS, first_attempt, false).await?;
    let ports = select_ports(sup, &prepared).await?;
    Ok(Armed {
        prepared,
        secrets,
        lock,
        ports,
    })
}

fn ctx_instance(ctx: &OperationContext) -> InstanceId {
    match ctx.resource() {
        stackly_domain::OperationResource::Instance(id) => id,
        _ => unreachable!("a start operation works on an instance"),
    }
}

/// The `verify` checks that read the disk: the data fits the runtime and
/// the run folder exists.
async fn verify(sup: &Supervisor, prepared: &Prepared) -> Result<(), SupervisorError> {
    let deps = sup.deps().clone();
    let key = prepared.key;
    let marker_path = prepared.package.data_marker_path(&prepared.paths);
    let recorded = prepared.instance.data_line().clone();
    let runtime_line = prepared.runtime.line().clone();
    let package = prepared.package.clone();
    blocking(move || -> Result<(), SupervisorError> {
        ensure_instance_run_dir(&deps.platform, &deps.layout, key).map_err(|error| {
            SupervisorError::DataUnavailable {
                path: deps
                    .layout
                    .layout()
                    .instance_run_dir(key)
                    .display()
                    .to_string(),
                reason: error.to_string(),
            }
        })?;
        let marker = marker_path.map_or(MarkerRead::NotDeclared, |path| {
            MarkerRead::from_read(
                deps.platform
                    .fs()
                    .read_file_limited(&path, crate::provider::DATA_MARKER_MAX_LEN),
            )
        });
        match package.preflight(Some(recorded), marker, &runtime_line) {
            PreflightOutcome::Compatible => Ok(()),
            PreflightOutcome::Incompatible { data, runtime } => {
                Err(SupervisorError::DataIncompatible {
                    outcome: "incompatible".to_owned(),
                    data: Some(data.to_string()),
                    runtime: runtime.to_string(),
                })
            }
            other => Err(SupervisorError::DataIncompatible {
                outcome: match other {
                    PreflightOutcome::MarkerMismatch { .. } => "marker_mismatch",
                    PreflightOutcome::MarkerMissing => "marker_missing",
                    PreflightOutcome::MarkerUnreadable => "marker_unreadable",
                    _ => "unknown",
                }
                .to_owned(),
                data: None,
                runtime: runtime_line.to_string(),
            }),
        }
    })
    .await?
}

/// The dry run: builds the run plan with candidate ports and checks its
/// argv for a secret, so any failure here changes nothing.
fn dry_run(
    sup: &Supervisor,
    prepared: &Prepared,
    secrets: &ResolvedSecrets,
) -> Result<(), SupervisorError> {
    let deps = sup.deps();
    let mut candidates = BTreeMap::new();
    for endpoint in prepared.instance.endpoints() {
        if let Endpoint::Ip(ip) = endpoint
            && ip.effective_port().is_none()
        {
            let candidate = deps
                .ports
                .ephemeral_port(ip.transport(), ip.address())
                .or_else(|| Port::new(49152).ok());
            if let Some(port) = candidate {
                candidates.insert(ip.name().clone(), port);
            }
        }
    }
    let plan = prepared
        .package
        .run_plan(&prepared.context(&candidates))
        .map_err(|error| config_invalid(&error))?;
    crate::secret::delivery::ensure_not_in_argv(&plan.spawn.args, secrets)?;
    Ok(())
}

/// The `endpoints` stage: every `Fixed` port is free, every `Auto` endpoint
/// gets its stored port back when it is still free or a new free one, and no
/// effective port conflicts with another Stackly instance (BR-004, BR-005).
async fn select_ports(
    sup: &Supervisor,
    prepared: &Prepared,
) -> Result<BTreeMap<EndpointName, Port>, SupervisorError> {
    let deps = sup.deps().clone();
    let registry = deps.registry.clone();
    let own = prepared.instance.id();
    let others = blocking(move || {
        registry.instances(InstanceFilter {
            include_retained: false,
        })
    })
    .await?
    .map_err(SupervisorError::Registry)?
    .value;
    let mut taken: Vec<(Option<InstanceId>, Endpoint)> = others
        .iter()
        .filter(|other| other.id() != own)
        .flat_map(|other| {
            other
                .endpoints()
                .iter()
                .map(|endpoint| (Some(other.id()), endpoint.clone()))
        })
        .collect();
    let mut chosen = BTreeMap::new();
    for endpoint in prepared.instance.endpoints() {
        let Endpoint::Ip(ip) = endpoint else { continue };
        let fault = |port: Port| EndpointFault {
            endpoint: ip.name().to_string(),
            address: ip.address().to_string(),
            port: port.get(),
        };
        let conflict = |port: Port, taken: &[(Option<InstanceId>, Endpoint)]| {
            let probe = Endpoint::Ip(IpEndpoint::new(
                ip.name().clone(),
                ip.transport(),
                ip.address(),
                ip.request(),
                Some(port),
            ));
            taken
                .iter()
                .find(|(_, other)| endpoints_conflict(&probe, other))
                .map(|(owner, _)| *owner)
        };
        let try_bind = |port: Port| {
            let addr = SocketAddr::new(ip.address(), port.get());
            deps.ports
                .try_bind(ip.transport(), addr, BindOptions::like_server(addr))
        };
        let port = match ip.request() {
            PortRequest::Fixed(port) => {
                match try_bind(port) {
                    BindOutcome::Free => {}
                    BindOutcome::InUse => {
                        return Err(SupervisorError::EndpointInUse(fault(port)));
                    }
                    outcome @ (BindOutcome::Denied | BindOutcome::Unavailable) => {
                        return Err(SupervisorError::EndpointUnavailable {
                            fault: fault(port),
                            reason: if outcome == BindOutcome::Denied {
                                UnavailableReason::Denied
                            } else {
                                UnavailableReason::Unavailable
                            },
                        });
                    }
                }
                if let Some(owner) = conflict(port, &taken) {
                    return Err(endpoint_conflict(ip, port, owner));
                }
                port
            }
            PortRequest::Auto => {
                let reusable = ip.bound().filter(|bound| {
                    try_bind(*bound) == BindOutcome::Free && conflict(*bound, &taken).is_none()
                });
                match reusable {
                    Some(bound) => bound,
                    None => pick_candidate(&deps, ip, &taken, &try_bind, &conflict)?,
                }
            }
        };
        taken.push((
            None,
            Endpoint::Ip(IpEndpoint::new(
                ip.name().clone(),
                ip.transport(),
                ip.address(),
                ip.request(),
                Some(port),
            )),
        ));
        chosen.insert(ip.name().clone(), port);
    }
    Ok(chosen)
}

fn endpoint_conflict(ip: &IpEndpoint, port: Port, owner: Option<InstanceId>) -> SupervisorError {
    match owner {
        Some(other) => SupervisorError::EndpointConflict {
            endpoint: ip.name().to_string(),
            port: port.get(),
            other,
        },
        None => SupervisorError::AutoPortUnavailable {
            endpoint: ip.name().to_string(),
        },
    }
}

/// Finds the owner of an endpoint that a port would conflict with: `Some`
/// of the other instance, or of `None` for an endpoint of this instance.
type ConflictCheck<'a> =
    dyn Fn(Port, &[(Option<InstanceId>, Endpoint)]) -> Option<Option<InstanceId>> + 'a;

/// Picks a free candidate for an `Auto` endpoint: the OS's ephemeral port,
/// bound the way the server binds and not conflicting with any other
/// endpoint, at most [`auto_port_attempts`](super::SupervisorConfig) times.
fn pick_candidate(
    deps: &super::SupervisorDeps,
    ip: &IpEndpoint,
    taken: &[(Option<InstanceId>, Endpoint)],
    try_bind: &dyn Fn(Port) -> BindOutcome,
    conflict: &ConflictCheck<'_>,
) -> Result<Port, SupervisorError> {
    for _ in 0..deps.config.auto_port_attempts {
        let Some(candidate) = deps.ports.ephemeral_port(ip.transport(), ip.address()) else {
            continue;
        };
        if try_bind(candidate) == BindOutcome::Free && conflict(candidate, taken).is_none() {
            return Ok(candidate);
        }
    }
    Err(SupervisorError::AutoPortUnavailable {
        endpoint: ip.name().to_string(),
    })
}

/// How one attempt ended.
enum AttemptEnd {
    /// The instance is ready.
    Ready,
    /// The readiness deadline passed with the process alive.
    Timeout(Duration),
    /// The process exited before it was ready.
    Exited(ExitNotice),
    /// The attempt failed before or while the process started; the
    /// lifecycle already says so.
    Failed(SupervisorError),
}

/// What happens after a process that exited without being asked to.
pub(crate) enum Next {
    /// Respawn after this pause; the lifecycle is `recovering`.
    Retry(Duration),
    /// No respawn; the lifecycle is `failed` (or the commit failed, which
    /// the error says). The operation, if one runs, fails with this error.
    Final(SupervisorError),
    /// A stop got to the lifecycle first and ended it `stopped`, or there
    /// was nothing to record: no respawn, and no failure.
    Settled,
}

/// Runs the attempts of a start after its `journal`: `first` holds what the
/// checks before the journal prepared, or is `None` for a `recovery`
/// respawn, whose first attempt makes its own checks. `recovery` also sets
/// the readiness deadline of every attempt to `recovery_deadline`.
async fn after_journal(
    sup: &Supervisor,
    request: &StartRequest,
    ctx: &mut OperationContext,
    first: Option<Armed>,
    recovery: bool,
) -> WorkResult {
    let id = request.instance;
    let mut attempt: u32 = 1;
    let mut armed = first;
    loop {
        let end = match armed.take() {
            Some(ready) => run_attempt(sup, ctx, ready, attempt, recovery).await?,
            None => {
                // A respawn repeats the checks, and never shows a dialog.
                let background = SecretCaller::background().in_operation(ctx.id());
                match gate(sup, ctx, &background, attempt == 1).await {
                    Ok(ready) => run_attempt(sup, ctx, ready, attempt, recovery).await?,
                    Err(Gate::Work(error)) => return Err(error),
                    // A lock still held counts as a failed attempt.
                    Err(Gate::Error(SupervisorError::DataInUse { .. })) => {
                        match on_lock_held(sup, id, CommitMode::Operation).await {
                            Next::Retry(delay) => {
                                wait_to_respawn(sup, ctx, delay).await?;
                                attempt += 1;
                                continue;
                            }
                            Next::Final(error) => return Err(failed(error)),
                            Next::Settled => return Err(failed(stopped_meanwhile())),
                        }
                    }
                    Err(Gate::Error(error)) => {
                        fail_lifecycle(sup, id, &error_failure_code(&error), CommitMode::Operation)
                            .await;
                        AttemptEnd::Failed(error)
                    }
                }
            }
        };
        match end {
            AttemptEnd::Ready => {
                enter(ctx, StageName::PUBLISH, attempt == 1, true).await?;
                enter(ctx, StageName::COMPLETE, attempt == 1, true).await?;
                ctx.finish_step(None).await?;
                return Ok(success(id));
            }
            AttemptEnd::Timeout(deadline) => {
                return Err(failed(SupervisorError::ReadinessTimeout {
                    deadline_seconds: deadline.as_secs(),
                }));
            }
            AttemptEnd::Failed(error) => return Err(failed(error)),
            AttemptEnd::Exited(notice) => {
                let last_exit = notice.status;
                match on_exit(sup, id, &notice).await {
                    Next::Retry(delay) => {
                        wait_to_respawn(sup, ctx, delay).await?;
                        attempt += 1;
                    }
                    Next::Final(error) => {
                        return Err(failed(with_last_exit(error, last_exit)));
                    }
                    Next::Settled => return Err(failed(stopped_meanwhile())),
                }
            }
        }
    }
}

/// The error of an operation whose instance was stopped under it.
fn stopped_meanwhile() -> SupervisorError {
    SupervisorError::InstanceStopping
}

/// Puts the last exit of the crash loop into a spent budget's details.
fn with_last_exit(error: SupervisorError, status: Option<ExitStatus>) -> SupervisorError {
    match error {
        SupervisorError::RetryBudgetExhausted { attempts, .. } => {
            SupervisorError::RetryBudgetExhausted {
                attempts,
                last_exit_code: match status {
                    Some(ExitStatus::Exited(code)) => Some(code),
                    _ => None,
                },
                last_signal: match status {
                    Some(ExitStatus::Signaled(signal)) => Some(signal),
                    _ => None,
                },
            }
        }
        other => other,
    }
}

/// Waits `delay` before a respawn, showing when it happens. A cancel or a
/// shutdown ends the pause at once and reaches the caller as an error.
async fn wait_to_respawn(
    sup: &Supervisor,
    ctx: &OperationContext,
    delay: Duration,
) -> Result<(), WorkError> {
    let deps = sup.deps();
    let actor = sup.actor(ctx_instance(ctx));
    actor.update(deps, |state| show_pause(deps, state, delay));
    tokio::select! {
        () = tokio::time::sleep(delay) => {}
        () = ctx.stopped() => {}
    }
    actor.update(deps, |state| {
        let attempts = state
            .restart
            .crashes_in_window(Instant::now(), &deps.config);
        state.restart_status = restart_status(&deps.config, attempts, None, false);
    });
    ctx.check_cancel()
}

/// The failure code a start that failed before its process ran records.
fn error_failure_code(error: &SupervisorError) -> FailureCode {
    match error {
        SupervisorError::ExecutableInvalid { .. } => FailureCode::EXECUTABLE_INVALID,
        SupervisorError::SpawnFailed { .. } => FailureCode::SPAWN_FAILED,
        SupervisorError::DataInUse { .. } => FailureCode::DATA_IN_USE,
        SupervisorError::EndpointInUse(_) => FailureCode::ENDPOINT_IN_USE,
        SupervisorError::IdentityCaptureFailed => FailureCode::IDENTITY_CAPTURE_FAILED,
        _ => FailureCode::CONFIG_RENDER_FAILED,
    }
}

/// Records that the start failed: `start_failed` while `starting`, and the
/// final `crashed` while the instance already runs or recovers. Best
/// effort: the operation fails with the original error either way.
async fn fail_lifecycle(sup: &Supervisor, id: InstanceId, reason: &FailureCode, mode: CommitMode) {
    let now = sup.deps().platform.clock().now();
    let committed = commit_instance(sup, id, None, mode, |changes| {
        let event = match changes.instance().lifecycle() {
            InstanceLifecycle::Starting => LifecycleEvent::StartFailed {
                reason: reason.clone(),
            },
            InstanceLifecycle::Running | InstanceLifecycle::Recovering => LifecycleEvent::Crashed {
                will_retry: false,
                reason: reason.clone(),
            },
            // A stop got here first; the exit ends it.
            InstanceLifecycle::Stopping => LifecycleEvent::Exited,
            _ => return Ok(Vec::new()),
        };
        apply_event(changes, event, now)?;
        Ok(Vec::new())
    })
    .await;
    if let Err(error) = committed {
        tracing::warn!(instance = %id, %error, "a failed start could not be recorded");
    }
}

async fn fail(
    sup: &Supervisor,
    id: InstanceId,
    reason: FailureCode,
    error: SupervisorError,
) -> AttemptEnd {
    // The process never ran, so no exit will remove what the render wrote
    // (spec 0010, *Stop*: `start_failed` removes the secret material).
    remove_material(sup, &sup.actor(id)).await;
    fail_lifecycle(sup, id, &reason, CommitMode::Operation).await;
    AttemptEnd::Failed(error)
}

/// Removes the stale socket files of the instance's Unix endpoints; the
/// instance lock is ours, so nobody listens on them.
async fn remove_stale_sockets(sup: &Supervisor, prepared: &Prepared) {
    let deps = sup.deps().clone();
    let sockets: Vec<_> = prepared
        .instance
        .endpoints()
        .iter()
        .filter_map(|endpoint| match endpoint {
            Endpoint::Unix(unix) => Some(prepared.paths.run.join(unix.socket().as_str())),
            Endpoint::Ip(_) => None,
        })
        .collect();
    if sockets.is_empty() {
        return;
    }
    let removed = blocking(move || {
        for socket in &sockets {
            if let Err(error) = remove_owned_tree(&deps.platform, &deps.layout, socket) {
                tracing::warn!(path = %socket.display(), %error, "a stale socket could not be removed");
            }
        }
    })
    .await;
    if let Err(error) = removed {
        tracing::warn!(%error, "removing stale sockets did not run");
    }
}

/// Makes the two spool files of a new main process. An old spool is read
/// to the end into the sinks first (the lines a process wrote while no
/// daemon was reading), then removed, and the tail of the earlier run is
/// forgotten.
async fn fresh_spools(
    sup: &Supervisor,
    prepared: &Prepared,
) -> Result<stackly_platform::OutputTarget, SupervisorError> {
    let deps = sup.deps().clone();
    let key = prepared.key;
    let id = prepared.instance.id();
    blocking(move || -> Result<OutputTarget, SupervisorError> {
        let [stdout, stderr] = stackly_platform::OutputStream::ALL
            .map(|stream| deps.layout.layout().instance_spool(key, stream));
        drain_old_spools(&deps, id, &[&stdout, &stderr]);
        for path in [&stdout, &stderr] {
            remove_owned_tree(&deps.platform, &deps.layout, path)
                .map_err(|error| SupervisorError::internal(error.to_string()))?;
        }
        let processes = deps.platform.processes();
        let stdout = processes
            .create_spool(&stdout)
            .map_err(|error| SupervisorError::from_process(&error))?;
        let stderr = processes
            .create_spool(&stderr)
            .map_err(|error| SupervisorError::from_process(&error))?;
        Ok(OutputTarget::Spool { stdout, stderr })
    })
    .await?
}

/// Reads the spools a finished run left to the end into the sinks, then
/// forgets the tail of that run. A spool that is missing or cannot be read
/// only costs a warning.
fn drain_old_spools(deps: &SupervisorDeps, id: InstanceId, paths: &[&std::path::PathBuf; 2]) {
    let streams = stackly_platform::OutputStream::ALL;
    let ctx = DrainContext::new(id, &deps.redactor, &*deps.sinks, deps.platform.clock());
    for (stream, path) in streams.into_iter().zip(paths) {
        let reader = match deps.platform.processes().open_spool(path) {
            Ok(Some(reader)) => reader,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(instance = %id, stream = stream.as_code(), %error, "an old spool could not be opened");
                continue;
            }
        };
        let finished = SpoolDrain::resume(stream, reader, &deps.config)
            .and_then(|mut drain| drain.finish(&ctx));
        match finished {
            Ok(DrainOutcome::Busy) => {
                tracing::warn!(instance = %id, stream = stream.as_code(), "the log sink was busy; an old spool is dropped unread");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(instance = %id, stream = stream.as_code(), %error, "an old spool could not be read");
            }
        }
    }
    deps.tail.forget(id);
}

/// One attempt: `render`, `spawn`, `identity`, and `readiness`.
#[allow(clippy::too_many_lines)]
async fn run_attempt(
    sup: &Supervisor,
    ctx: &mut OperationContext,
    armed: Armed,
    attempt: u32,
    recovery: bool,
) -> Result<AttemptEnd, WorkError> {
    let deps = sup.deps();
    let id = ctx_instance(ctx);
    let actor = sup.actor(id);
    let first = attempt == 1;
    let Armed {
        prepared,
        secrets,
        lock,
        ports,
    } = armed;
    let context = prepared.context(&ports);

    enter(ctx, StageName::RENDER, first, false).await?;
    let plan = match prepared.package.run_plan(&context) {
        Ok(plan) => plan,
        Err(error) => {
            let error = config_invalid(&error);
            return Ok(fail(sup, id, FailureCode::CONFIG_RENDER_FAILED, error).await);
        }
    };
    if let Err(error) = run_render(deps, &plan.render, &secrets).await {
        return Ok(fail(sup, id, FailureCode::CONFIG_RENDER_FAILED, error).await);
    }
    remove_stale_sockets(sup, &prepared).await;

    enter(ctx, StageName::SPAWN, first, true).await?;
    let parts = match command_parts(deps, &prepared, &plan.spawn, &secrets) {
        Ok(parts) => parts,
        Err(error) => {
            let reason = error_failure_code(&error);
            return Ok(fail(sup, id, reason, error).await);
        }
    };
    // A quarantined runtime is refused before the spool and the spawn.
    let launchable = {
        let platform = deps.platform.clone();
        let program = parts.program.clone();
        blocking(move || check_launchable(&platform, &program)).await
    };
    match launchable {
        Ok(Ok(())) => {}
        Ok(Err(LaunchRefusal::Quarantined { path, .. })) => {
            let error = SupervisorError::Quarantined {
                path: path.display().to_string(),
            };
            return Ok(fail(sup, id, FailureCode::QUARANTINED, error).await);
        }
        Ok(Err(LaunchRefusal::BinaryRejectedByOs { .. })) | Err(_) => {}
    }
    let output = match fresh_spools(sup, &prepared).await {
        Ok(output) => output,
        Err(error) => return Ok(fail(sup, id, FailureCode::SPAWN_FAILED, error).await),
    };
    let child = {
        let platform = deps.platform.clone();
        let lock = Arc::clone(&lock);
        blocking(move || {
            platform.processes().spawn_main(SpawnRequest {
                program: &parts.program,
                args: &parts.args,
                env: &parts.env,
                cwd: &parts.cwd,
                stdin: StdinSource::Null,
                output,
                lock: Some(&lock),
                role: stackly_platform::ChildRole::Main,
            })
        })
        .await
    };
    let child = match child {
        Ok(Ok(child)) => child,
        Ok(Err(error)) => {
            let error = SupervisorError::from_process(&error);
            return Ok(fail(sup, id, FailureCode::SPAWN_FAILED, error).await);
        }
        Err(error) => return Ok(fail(sup, id, FailureCode::SPAWN_FAILED, error).await),
    };

    // From here until `resume`, the child is suspended and unrecorded: a
    // failure or a cancel discards it, so no process ever runs unrecorded.
    if let Err(stop) = enter(ctx, StageName::IDENTITY, first, true).await {
        let _ = discard(sup, child).await;
        return Err(stop);
    }
    let facts = child.facts().clone();
    let pid = child.pid();
    let boot = {
        let platform = deps.platform.clone();
        blocking(move || platform.processes().boot_id()).await
    };
    let boot = match boot {
        Ok(Ok(boot)) => boot,
        _ => {
            let status = discard(sup, child).await;
            return Ok(identity_failed(sup, id, &facts.executable, status).await);
        }
    };
    let identity = ProcessIdentity {
        pid,
        start: facts.start,
        executable: facts.executable.clone(),
        session: facts.session,
        boot,
    };
    let now = deps.platform.clock().now();
    let auto: Vec<(EndpointName, Port)> = prepared
        .instance
        .endpoints()
        .iter()
        .filter_map(|endpoint| match endpoint {
            Endpoint::Ip(ip) if ip.request() == PortRequest::Auto => {
                ports.get(ip.name()).map(|port| (ip.name().clone(), *port))
            }
            _ => None,
        })
        .collect();
    let recorded = commit_instance(sup, id, None, CommitMode::Operation, |changes| {
        apply_event(changes, LifecycleEvent::Spawned, now)?;
        for (name, port) in &auto {
            changes.apply(|instance| {
                instance
                    .set_bound_port(name, *port)
                    .map_err(|error| SupervisorError::internal(error.to_string()))
            })?;
        }
        Ok(vec![WriteOp::RecordProcessObservation(
            ProcessObservation {
                instance: id,
                identity: identity.clone(),
                observed_at: now,
            },
        )])
    })
    .await;
    if let Err(error) = recorded {
        tracing::warn!(instance = %id, %error, "the identity of a new process could not be recorded");
        let status = discard(sup, child).await;
        return Ok(identity_failed(sup, id, &facts.executable, status).await);
    }

    let (drain, logs_unavailable) = DrainSlot::start(sup, id, Opening::Fresh).await;
    let (exit_slot, exit_rx) = ExitSlot::new();
    let (done, _) = watch::channel(false);
    let (probes_stop, probes_stop_rx) = watch::channel(false);
    let generation = actor.update(deps, |state| {
        let generation = state.next_generation;
        state.next_generation += 1;
        state.process = Some(ProcessSlot {
            generation,
            identity: identity.clone(),
            group: facts.group,
            adopted: false,
            adoptable: None,
            lock: lock.identity(),
            done,
            probes_stop,
            drain,
        });
        state.op_owns_exit = true;
        state.stopping = false;
        state.leftover.clear();
        state.logs_unavailable = logs_unavailable;
        state.awaiting_probe(now);
        generation
    });
    let watched = {
        let platform = deps.platform.clone();
        blocking(move || platform.processes().watch_exit(pid, Arc::new(exit_slot))).await
    };
    if !matches!(watched, Ok(Ok(()))) {
        let status = discard(sup, child).await;
        finish_process(sup, &actor, generation, Vec::new());
        return Ok(identity_failed(sup, id, &facts.executable, status).await);
    }
    spawn_exit_task(sup, Arc::clone(&actor), generation, exit_rx.clone());
    let resumed = {
        let platform = deps.platform.clone();
        blocking(move || platform.processes().resume(child)).await
    };
    if !matches!(resumed, Ok(Ok(_))) {
        tracing::warn!(instance = %id, "a recorded process could not be resumed");
        finish_process(sup, &actor, generation, Vec::new());
        let error = SupervisorError::IdentityCaptureFailed;
        return Ok(fail(sup, id, FailureCode::IDENTITY_CAPTURE_FAILED, error).await);
    }
    // The child holds the lock now; our copy of the descriptor goes.
    drop(lock);
    drop(secrets);
    let resumed_at = Instant::now();
    let deadline = if first && !recovery {
        plan.readiness.deadline
    } else {
        plan.recovery_deadline
    };
    let mut ready = spawn_probes(
        sup.clone(),
        Arc::clone(&actor),
        generation,
        plan.readiness.clone(),
        plan.health.clone(),
        probes_stop_rx,
    );

    if let Err(stop) = enter(ctx, StageName::READINESS, first, true).await {
        actor.release_exit(deps);
        return Err(stop);
    }
    let mut exit = exit_rx;
    let end = tokio::select! {
        biased;
        seen = exit.wait_for(Option::is_some) => {
            match seen.ok().and_then(|notice| notice.clone()) {
                Some(notice) => AttemptEnd::Exited(notice),
                None => AttemptEnd::Failed(SupervisorError::IdentityCaptureFailed),
            }
        }
        _ = ready.wait_for(|ready| *ready) => AttemptEnd::Ready,
        () = ctx.stopped() => {
            actor.release_exit(deps);
            return Err(ctx.check_cancel().err().unwrap_or(WorkError::Cancelled));
        }
        () = tokio::time::sleep_until(resumed_at + deadline) => AttemptEnd::Timeout(deadline),
    };
    if !matches!(end, AttemptEnd::Exited(_)) {
        actor.release_exit(deps);
    }
    Ok(end)
}

/// Discards a suspended child that never ran, and returns the status of a
/// child that had already died by itself, which `discard` reaps first.
async fn discard(sup: &Supervisor, child: stackly_platform::SuspendedChild) -> Option<ExitStatus> {
    let platform = sup.deps().platform.clone();
    let discarded = blocking(move || platform.processes().discard(child)).await;
    match discarded {
        Ok(Ok(status)) => status,
        _ => {
            tracing::warn!("a suspended child could not be discarded");
            None
        }
    }
}

/// Fails a start whose identity stage broke after the child was discarded:
/// a child that had died of `SIGKILL` on its own was a binary the OS
/// rejected (spec 0018, K7); anything else is an identity that could not be
/// captured.
async fn identity_failed(
    sup: &Supervisor,
    id: InstanceId,
    executable: &std::path::Path,
    status: Option<ExitStatus>,
) -> AttemptEnd {
    match classify_early_exit(status, false) {
        Some(LaunchRefusal::BinaryRejectedByOs { signal }) => {
            let error = SupervisorError::BinaryRejectedByOs {
                path: executable.display().to_string(),
                signal,
            };
            fail(sup, id, FailureCode::BINARY_REJECTED_BY_OS, error).await
        }
        _ => {
            let error = SupervisorError::IdentityCaptureFailed;
            fail(sup, id, FailureCode::IDENTITY_CAPTURE_FAILED, error).await
        }
    }
}

/// A process that exited before it was ready: its spools are read to the
/// end, its secret material removed, then the early exit check of the
/// endpoints (spec 0010 K2) and the restart policy.
async fn on_exit(sup: &Supervisor, id: InstanceId, notice: &ExitNotice) -> Next {
    let deps = sup.deps();
    let actor = sup.actor(id);
    let target = actor.read(|state| {
        state.process.as_ref().map(|slot| {
            (
                slot.generation,
                slot.group,
                Arc::clone(&slot.drain),
                slot.identity.executable.clone(),
            )
        })
    });
    let mut executable = None;
    if let Some((generation, group, drain, path)) = target {
        executable = Some(path);
        actor.update(deps, |state| {
            if let Some(slot) = &state.process {
                slot.probes_stop.send_replace(true);
            }
        });
        drain.finish(sup, id).await;
        remove_material(sup, &actor).await;
        let platform = deps.platform.clone();
        let main = notice.pid;
        let leftover = blocking(move || platform.processes().group_members(group))
            .await
            .ok()
            .and_then(Result::ok)
            .map(|members| members.into_iter().filter(|pid| *pid != main).collect())
            .unwrap_or_default();
        finish_process(sup, &actor, generation, leftover);
    }
    // A child of this daemon that macOS killed before it was ready is a
    // binary the OS rejected (spec 0018, K7): it uses no restart budget.
    if let (Some(LaunchRefusal::BinaryRejectedByOs { signal }), Some(path)) =
        (classify_early_exit(notice.status, false), executable)
    {
        return binary_rejected(sup, id, &path, signal).await;
    }
    after_unrequested_exit(sup, id, notice, CommitMode::Operation).await
}

/// Records `crashed { will_retry: false, reason: binary_rejected_by_os }`
/// and ends the start with `runtime.binary_rejected_by_os`, without using
/// the restart budget.
async fn binary_rejected(
    sup: &Supervisor,
    id: InstanceId,
    path: &std::path::Path,
    signal: i32,
) -> Next {
    let deps = sup.deps();
    let now = deps.platform.clock().now();
    let mut recorded = ExitRecord::Nothing;
    let committed = commit_instance(sup, id, None, CommitMode::Operation, |changes| {
        recorded = apply_exit(
            changes,
            LifecycleEvent::Crashed {
                will_retry: false,
                reason: FailureCode::BINARY_REJECTED_BY_OS,
            },
            now,
        )?;
        Ok(Vec::new())
    })
    .await;
    match committed {
        Ok(()) if recorded == ExitRecord::Crashed => {
            Next::Final(SupervisorError::BinaryRejectedByOs {
                path: path.display().to_string(),
                signal,
            })
        }
        Ok(()) => {
            forget_crash(deps, &sup.actor(id));
            Next::Settled
        }
        Err(error) => Next::Final(error),
    }
}

/// Decides what follows a process that exited without being asked to, and
/// records it. The port may have been taken by someone else: that is not a
/// crash of the provider and uses no budget (`endpoint_in_use`). Otherwise
/// the restart policy runs: `crashed { will_retry: true }` and a pause, or
/// `crashed { will_retry: false, reason: crash_loop }` when the budget is
/// spent. `mode` is how the commit reacts to a registry refusal: an
/// operation decides on it, an actor retries.
pub(crate) async fn after_unrequested_exit(
    sup: &Supervisor,
    id: InstanceId,
    notice: &ExitNotice,
    mode: CommitMode,
) -> Next {
    let deps = sup.deps();
    let read = match mode {
        // An actor waits for the registry; an operation decides on the error.
        CommitMode::Actor => read_instance_actor(sup, id).await,
        CommitMode::Operation => read_instance(deps, id).await,
    };
    if let Ok(instance) = read {
        for endpoint in instance.endpoints() {
            let Endpoint::Ip(ip) = endpoint else { continue };
            let Some(port) = ip.effective_port() else {
                continue;
            };
            let addr = SocketAddr::new(ip.address(), port.get());
            if deps
                .ports
                .try_bind(ip.transport(), addr, BindOptions::like_server(addr))
                == BindOutcome::InUse
            {
                let reason = FailureCode::ENDPOINT_IN_USE;
                fail_lifecycle(sup, id, &reason, mode).await;
                return Next::Final(SupervisorError::EndpointInUse(EndpointFault {
                    endpoint: ip.name().to_string(),
                    address: ip.address().to_string(),
                    port: port.get(),
                }));
            }
        }
    }
    apply_policy(sup, id, crash_reason(notice.status), mode).await
}

/// A respawn whose instance lock was still held counts as a failed attempt
/// with the reason `data_in_use`.
async fn on_lock_held(sup: &Supervisor, id: InstanceId, mode: CommitMode) -> Next {
    apply_policy(sup, id, FailureCode::DATA_IN_USE, mode).await
}

/// Records a crash in the history and decides: respawn after a pause, or
/// stop trying with `crash_loop`.
async fn apply_policy(
    sup: &Supervisor,
    id: InstanceId,
    reason: FailureCode,
    mode: CommitMode,
) -> Next {
    let deps = sup.deps();
    let actor = sup.actor(id);
    let decision = actor.update(deps, |state| {
        state
            .restart
            .record_crash(Instant::now(), JitterFactor::random(), &deps.config)
    });
    let now = deps.platform.clock().now();
    match decision {
        RestartDecision::Retry { delay, .. } => {
            let mut recorded = ExitRecord::Nothing;
            let committed = commit_instance(sup, id, None, mode, |changes| {
                recorded = apply_exit(
                    changes,
                    LifecycleEvent::Crashed {
                        will_retry: true,
                        reason: reason.clone(),
                    },
                    now,
                )?;
                Ok(Vec::new())
            })
            .await;
            match committed {
                Ok(()) if recorded == ExitRecord::Crashed => Next::Retry(delay),
                Ok(()) => {
                    forget_crash(deps, &actor);
                    Next::Settled
                }
                Err(error) => Next::Final(error),
            }
        }
        RestartDecision::Exhausted { crashes } => {
            let mut recorded = ExitRecord::Nothing;
            let committed = commit_instance(sup, id, None, mode, |changes| {
                recorded = apply_exit(
                    changes,
                    LifecycleEvent::Crashed {
                        will_retry: false,
                        reason: FailureCode::CRASH_LOOP,
                    },
                    now,
                )?;
                Ok(Vec::new())
            })
            .await;
            actor.update(deps, |state| {
                state.restart_status = restart_status(&deps.config, crashes, None, true);
            });
            match committed {
                Ok(()) if recorded == ExitRecord::Crashed => {
                    Next::Final(SupervisorError::RetryBudgetExhausted {
                        attempts: crashes,
                        last_exit_code: None,
                        last_signal: None,
                    })
                }
                Ok(()) => {
                    forget_crash(deps, &actor);
                    Next::Settled
                }
                Err(error) => Next::Final(error),
            }
        }
    }
}

/// The exit was a stop, not a crash: it uses no restart budget.
fn forget_crash(deps: &SupervisorDeps, actor: &InstanceActor) {
    actor.update(deps, |state| state.restart.forget_last_crash());
}

/// A cancel after the journal: with a process alive, the graceful stop of
/// spec 0010 and then `cancelled`; without one (before the spawn or during a
/// pause between respawns), `stop_begin`, `exited`, and desired `stopped`.
async fn cancel_cleanup(sup: &Supervisor, ctx: &OperationContext, id: InstanceId) -> WorkResult {
    let deps = sup.deps();
    let actor = sup.actor(id);
    actor.release_exit(deps);
    let has_process = actor.read(|state| state.process.is_some());
    if !has_process {
        // No process will exit to remove what a render wrote.
        remove_material(sup, &actor).await;
    }
    sup.journal_stop(id, !has_process).await.map_err(failed)?;
    if has_process {
        let abandoned = async {
            ctx.stopped().await;
            if !matches!(ctx.check_cancel(), Err(WorkError::Abandoned)) {
                std::future::pending::<()>().await;
            }
        };
        match sup.stop_process(id, abandoned).await.map_err(failed)? {
            super::stop::StopEnd::Stopped => {}
            super::stop::StopEnd::Abandoned => return Err(WorkError::Abandoned),
        }
    }
    Err(WorkError::Cancelled)
}
