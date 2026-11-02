//! The `instance` area of the daemon (spec 0010 build step 13, spec 0020
//! build step 9, spec 0026 build step 9): `instance.create`,
//! `instance.start`, `instance.stop`, `instance.restart`,
//! `instance.reload`, `instance.force_stop`, `instance.get`,
//! `instance.list`, `instance.get_create_form`, `instance.preflight`, and
//! `instance.prepare_terminal` (spec 0070).
//!
//! Every method that starts an operation goes through
//! [`start_operation`](crate::ipc::operation::start_operation): the key
//! check and the lookup of the key first (a repeated request gets its
//! reply back), then the feature's precheck, which answers the refusals a
//! state decides and the `alreadyInState` replies without an operation
//! (AC-23). `ipc::instance` never calls `expose()` on a secret (K15); the
//! conversion of a wire secret is `ipc::secret`'s.

mod config;
mod create;
mod detail;
mod draft;
pub mod dto;
mod duplicate;
mod form;
pub mod metrics;
mod open_with;

use stackly_core::operation::{
    BeginRequest, BusyPolicy, FinalBatch, NO_OP_STAGE, NoOpReply, Origin, Precheck, PrecheckOutcome,
};
use stackly_core::supervisor::SupervisorError;
use stackly_core::supervisor::{
    ForceStopRequest, InstanceChanges, ReloadRequest, RestartRequest, StartRequest, StopRequest,
    Supervisor,
};
use stackly_domain::{
    Capability, DesiredState, Instance, InstanceFilter, InstanceId, InstanceLifecycle,
    JournalPayload, LeaseFilter, OperationKind, OperationResource, Registry, ResourceRevision,
    StopDecision, StopRequest as LeaseStopRequest, evaluate_stop,
};
use stackly_protocol::codes::resource;
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::{
    ForceStopInstanceParams, GetInstanceParams, INSTANCE_PAGE_BYTES_MAX, InstanceActionAccepted,
    InstancePage, InstanceSummary, ListInstancesParams, ReloadInstanceParams,
    RestartInstanceParams, StartInstanceParams, StopInstanceParams,
};
use stackly_protocol::message::Call;
use stackly_protocol::{ErrorBody, ResourceKind, ResourceRef};
use stackly_storage::SqliteRegistry;
use uuid::Uuid;

pub use config::{
    apply_config, change_endpoints, get_config, keep_revision, rename, validate_config,
};
pub use create::{create, work_deps};
pub use detail::{get_data_usage, get_instance_detail};
pub use duplicate::get_duplicate_form;
pub use form::{get_create_form, preflight};
pub use metrics::get_metrics;
pub use open_with::prepare_terminal;

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::{StartOutcome, StartRequest as OperationStart, start_operation};
use crate::ipc::secret::caller;
use crate::map;
use crate::state::DaemonCore;

/// The real `instance.get`.
pub fn get(core: &DaemonCore, params: GetInstanceParams) -> HandlerFuture<'_, InstanceSummary> {
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let instance = read_instance(core, instance_id(params.instance_id)?).await?;
        Ok(summary_of(core, &supervisor, &instance))
    })
}

/// The real `instance.list`.
pub fn list(core: &DaemonCore, params: ListInstancesParams) -> HandlerFuture<'_, InstancePage> {
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let limit = usize::try_from(params.limit_or_default()).unwrap_or(usize::MAX);
        let after = params.before.as_deref().and_then(parse_cursor);
        let instances = tokio::task::spawn_blocking(move || {
            registry.instances(InstanceFilter {
                include_retained: false,
            })
        })
        .await
        .map_err(|_| unexpected("the instance list task failed"))?
        .map_err(|error| Box::new(map::registry_error(&error)))?
        .value;
        let remaining: Vec<&Instance> = instances
            .iter()
            .filter(|instance| match &after {
                Some((name, id)) => {
                    (instance.name().as_str(), instance.id().as_uuid()) > (name.as_str(), *id)
                }
                None => true,
            })
            .collect();
        let mut items = Vec::new();
        let mut bytes = 0;
        for instance in &remaining {
            let summary = summary_of(core, &supervisor, instance);
            let size = serde_json::to_vec(&summary).map_or(0, |encoded| encoded.len());
            if !items.is_empty() && (items.len() >= limit || bytes + size > INSTANCE_PAGE_BYTES_MAX)
            {
                break;
            }
            bytes += size;
            items.push(summary);
        }
        // `next` names the last instance of the page, and only when more
        // follow it.
        let next = remaining
            .get(items.len())
            .and(
                items
                    .len()
                    .checked_sub(1)
                    .and_then(|last| remaining.get(last)),
            )
            .map(|last| format!("{}_{}", last.name(), last.id().as_uuid()));
        Ok(InstancePage { items, next })
    })
}

/// The real `instance.start`.
pub fn start<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: StartInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let expected = expected_revision(params.expected_revision)?;
        let fingerprint = request_fingerprint(&Call::InstanceStart(params));
        let work = supervisor.start_work(StartRequest::user(id, caller(&context)));
        let precheck: Box<dyn Precheck> = Box::new(move || start_precheck(&registry, id, expected));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::StartInstance,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            // A start waits behind the operation that holds the instance
            // (spec 0007).
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input: JournalPayload::empty(),
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "instance.start",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        announce(core, id, &outcome).await;
        Ok(action_reply(outcome))
    })
}

/// The real `instance.stop`.
pub fn stop<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: StopInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let expected = expected_revision(params.expected_revision)?;
        let override_leases = params.override_leases;
        let fingerprint = request_fingerprint(&Call::InstanceStop(params));
        let mut stop_request = StopRequest::new(id);
        if override_leases {
            stop_request = stop_request.overriding_leases();
        }
        let work = supervisor.stop_work(stop_request);
        let precheck: Box<dyn Precheck> =
            Box::new(move || stop_precheck(&registry, id, expected, override_leases));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::StopInstance,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            // A stop never waits behind a start: the user cancels the
            // start first (spec 0010, AC-7).
            busy: BusyPolicy::Reject,
            expected_revision: None,
            origin,
            input: JournalPayload::empty(),
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "instance.stop",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        announce(core, id, &outcome).await;
        Ok(action_reply(outcome))
    })
}

/// The real `instance.restart` (spec 0026): one `restart_instance`
/// operation that waits behind the operation holding the instance.
pub fn restart<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: RestartInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let expected = expected_revision(params.expected_revision)?;
        let fingerprint = request_fingerprint(&Call::InstanceRestart(params));
        let work = supervisor.restart_work(RestartRequest::new(id, caller(&context)));
        let precheck: Box<dyn Precheck> =
            Box::new(move || restart_precheck(&registry, id, expected));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::RestartInstance,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            // A restart waits behind the operation that holds the instance,
            // a respawn included, then checks the state again (AC-9).
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input: restart_input(),
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "instance.restart",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        Ok(action_reply(outcome))
    })
}

/// The real `instance.reload` (spec 0026): a `reload_instance` operation
/// on a running instance whose provider can reload.
pub fn reload<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ReloadInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let expected = expected_revision(params.expected_revision)?;
        let fingerprint = request_fingerprint(&Call::InstanceReload(params));
        let work = supervisor.reload_work(ReloadRequest::new(id, caller(&context)));
        let precheck: Box<dyn Precheck> =
            Box::new(move || reload_precheck(&registry, id, expected));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::ReloadInstance,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input: JournalPayload::empty(),
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "instance.reload",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        Ok(action_reply(outcome))
    })
}

/// The real `instance.force_stop` (spec 0026): a `force_stop_instance`
/// operation, only for an instance that is still `stopping` and that no
/// other operation holds.
pub fn force_stop<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ForceStopInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let expected = expected_revision(params.expected_revision)?;
        let fingerprint = request_fingerprint(&Call::InstanceForceStop(params));
        let work = supervisor.force_stop_work(ForceStopRequest::new(id));
        let precheck: Box<dyn Precheck> =
            Box::new(move || force_stop_precheck(&registry, id, expected));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::ForceStopInstance,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            // Another operation that holds the instance answers
            // `resource.busy` (AC-2).
            busy: BusyPolicy::Reject,
            expected_revision: None,
            origin,
            input: JournalPayload::empty(),
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "instance.force_stop",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        Ok(action_reply(outcome))
    })
}

/// The journal input of a restart: the trigger it records (spec 0026,
/// *Supervisor: restart*).
fn restart_input() -> JournalPayload {
    JournalPayload::new(
        r#"{"trigger":"user_restart"}"#,
        stackly_domain::OperationJournal::INPUT_MAX,
    )
    .unwrap_or_else(|_| JournalPayload::empty())
}

/// Refuses a restart the state decides (spec 0026, AC-7): only `running`
/// and `recovering` go on, and a refusal makes no operation.
fn restart_precheck(
    registry: &SqliteRegistry,
    id: InstanceId,
    expected: Option<ResourceRevision>,
) -> PrecheckOutcome {
    let instance = match read_for_precheck(registry, id, expected) {
        Ok(instance) => instance,
        Err(rejected) => return rejected,
    };
    match instance.lifecycle() {
        InstanceLifecycle::Running | InstanceLifecycle::Recovering => PrecheckOutcome::Proceed,
        InstanceLifecycle::Stopping => reject(SupervisorError::InstanceStopping),
        InstanceLifecycle::Unknown => reject(SupervisorError::IdentityUnproven { pid: None }),
        InstanceLifecycle::Initializing => reject(SupervisorError::InstanceInitializing),
        InstanceLifecycle::Retained => reject(SupervisorError::InstanceRetained),
        lifecycle @ (InstanceLifecycle::Starting
        | InstanceLifecycle::Stopped
        | InstanceLifecycle::Failed
        | InstanceLifecycle::Uninitialized) => {
            reject(SupervisorError::InstanceNotRunning { lifecycle })
        }
    }
}

/// Refuses a reload the provider or the state decides (spec 0026, AC-10):
/// a provider revision without the `reload` capability first, then any
/// lifecycle but `running`, both without an operation.
fn reload_precheck(
    registry: &SqliteRegistry,
    id: InstanceId,
    expected: Option<ResourceRevision>,
) -> PrecheckOutcome {
    let instance = match read_for_precheck(registry, id, expected) {
        Ok(instance) => instance,
        Err(rejected) => return rejected,
    };
    if !instance.capabilities().contains(Capability::Reload) {
        return reject(SupervisorError::CapabilityUnsupported {
            capability: Capability::Reload.to_string(),
        });
    }
    match instance.lifecycle() {
        InstanceLifecycle::Running => PrecheckOutcome::Proceed,
        InstanceLifecycle::Stopping => reject(SupervisorError::InstanceStopping),
        InstanceLifecycle::Unknown => reject(SupervisorError::IdentityUnproven { pid: None }),
        InstanceLifecycle::Retained => reject(SupervisorError::InstanceRetained),
        lifecycle => reject(SupervisorError::InstanceNotRunning { lifecycle }),
    }
}

/// Refuses a force stop the state decides (spec 0026, AC-2): `running`,
/// `starting`, and `recovering` need a normal stop first, `unknown` is
/// never signalled, and `stopped` or `failed` answer `alreadyInState`.
fn force_stop_precheck(
    registry: &SqliteRegistry,
    id: InstanceId,
    expected: Option<ResourceRevision>,
) -> PrecheckOutcome {
    let instance = match read_for_precheck(registry, id, expected) {
        Ok(instance) => instance,
        Err(rejected) => return rejected,
    };
    match instance.lifecycle() {
        InstanceLifecycle::Stopping => PrecheckOutcome::Proceed,
        InstanceLifecycle::Stopped
        | InstanceLifecycle::Failed
        | InstanceLifecycle::Uninitialized => PrecheckOutcome::NoOp(NoOpReply::nothing()),
        lifecycle @ (InstanceLifecycle::Running
        | InstanceLifecycle::Starting
        | InstanceLifecycle::Recovering) => {
            reject(SupervisorError::ForceRequiresStopping { lifecycle })
        }
        InstanceLifecycle::Unknown => reject(SupervisorError::IdentityUnproven { pid: None }),
        InstanceLifecycle::Initializing => reject(SupervisorError::InstanceInitializing),
        InstanceLifecycle::Retained => reject(SupervisorError::InstanceRetained),
    }
}

/// Refuses a start the state decides, answers one that has nothing to do,
/// and lets the rest go on (spec 0010, *Startup transaction*, AC-23).
fn start_precheck(
    registry: &SqliteRegistry,
    id: InstanceId,
    expected: Option<ResourceRevision>,
) -> PrecheckOutcome {
    let instance = match read_for_precheck(registry, id, expected) {
        Ok(instance) => instance,
        Err(rejected) => return rejected,
    };
    match instance.lifecycle() {
        InstanceLifecycle::Retained => reject(SupervisorError::InstanceRetained),
        InstanceLifecycle::Uninitialized | InstanceLifecycle::Initializing => {
            reject(SupervisorError::NotInitialized {
                lifecycle: instance.lifecycle(),
            })
        }
        InstanceLifecycle::Stopping => reject(SupervisorError::InstanceStopping),
        InstanceLifecycle::Unknown => reject(SupervisorError::IdentityUnproven { pid: None }),
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering => no_op_with_desired(instance, DesiredState::Running),
        // A `failed` instance whose init failed has no initialized data
        // yet (BR-008).
        InstanceLifecycle::Stopped | InstanceLifecycle::Failed
            if instance.data_initialized_at().is_none() =>
        {
            reject(SupervisorError::NotInitialized {
                lifecycle: instance.lifecycle(),
            })
        }
        InstanceLifecycle::Stopped | InstanceLifecycle::Failed => PrecheckOutcome::Proceed,
    }
}

/// Refuses a stop the state decides, answers one that has nothing to do,
/// and lets the rest go on (spec 0010, *Stop*, AC-23).
fn stop_precheck(
    registry: &SqliteRegistry,
    id: InstanceId,
    expected: Option<ResourceRevision>,
    override_leases: bool,
) -> PrecheckOutcome {
    let instance = match read_for_precheck(registry, id, expected) {
        Ok(instance) => instance,
        Err(rejected) => return rejected,
    };
    match instance.lifecycle() {
        InstanceLifecycle::Retained => return reject(SupervisorError::InstanceRetained),
        InstanceLifecycle::Initializing => {
            return reject(SupervisorError::InstanceInitializing);
        }
        InstanceLifecycle::Unknown => {
            return reject(SupervisorError::IdentityUnproven { pid: None });
        }
        InstanceLifecycle::Uninitialized => return PrecheckOutcome::NoOp(NoOpReply::nothing()),
        InstanceLifecycle::Stopped | InstanceLifecycle::Failed => {
            return no_op_with_desired(instance, DesiredState::Stopped);
        }
        InstanceLifecycle::Starting
        | InstanceLifecycle::Running
        | InstanceLifecycle::Recovering
        | InstanceLifecycle::Stopping => {}
    }
    let leases = match registry.leases(id, LeaseFilter::Unreleased) {
        Ok(leases) => leases.value,
        Err(error) => return reject(SupervisorError::Registry(error)),
    };
    let decision = evaluate_stop(
        id,
        &leases,
        &LeaseStopRequest::Manual {
            session: None,
            override_leases,
        },
    );
    if matches!(decision, StopDecision::RequiresOverride { .. }) {
        return reject(SupervisorError::StopRequiresOverride);
    }
    PrecheckOutcome::Proceed
}

/// Reads the instance the precheck decides about and checks the revision
/// the request expects.
fn read_for_precheck(
    registry: &SqliteRegistry,
    id: InstanceId,
    expected: Option<ResourceRevision>,
) -> Result<Instance, PrecheckOutcome> {
    let instance = registry
        .instance(id)
        .map_err(|error| reject(SupervisorError::Registry(error)))?
        .value
        .ok_or_else(|| reject(SupervisorError::NotFound(id)))?;
    if let Some(expected) = expected
        && expected != instance.revision()
    {
        return Err(reject(SupervisorError::RevisionMismatch {
            instance: id,
            expected,
            actual: instance.revision(),
        }));
    }
    Ok(instance)
}

fn reject(error: SupervisorError) -> PrecheckOutcome {
    PrecheckOutcome::Reject(Box::new(error))
}

/// The `alreadyInState` answer of an instance that is in the state asked
/// for: it writes nothing, unless the stored desired state differs, which
/// one commit then sets.
fn no_op_with_desired(instance: Instance, desired: DesiredState) -> PrecheckOutcome {
    if instance.desired() == desired {
        return PrecheckOutcome::NoOp(NoOpReply::nothing());
    }
    let mut changes = InstanceChanges::new(instance);
    let applied = changes.apply(|instance| {
        instance.set_desired(desired);
        Ok::<_, SupervisorError>(())
    });
    match applied {
        Ok(()) => PrecheckOutcome::NoOp(NoOpReply::writing(FinalBatch {
            expected_generation: None,
            ops: changes.into_ops(),
        })),
        Err(error) => reject(error),
    }
}

/// Publishes `instance.state_changed` for an `alreadyInState` answer that
/// committed a desired state itself: the supervisor did not write it.
async fn announce(core: &DaemonCore, id: InstanceId, outcome: &StartOutcome) {
    let StartOutcome::Accepted(accepted) = outcome else {
        return;
    };
    if accepted.replayed || accepted.operation.stage.as_deref() != Some(NO_OP_STAGE.as_str()) {
        return;
    }
    let Ok(instance) = read_instance(core, id).await else {
        return;
    };
    if let Some(events) = core.supervision().events() {
        stackly_core::supervisor::SupervisorEvents::instance_changed(&*events, &instance);
    }
}

/// The reply of a method that starts an operation.
fn action_reply(outcome: StartOutcome) -> InstanceActionAccepted {
    match outcome {
        StartOutcome::Accepted(accepted) => {
            let already_in_state =
                accepted.operation.stage.as_deref() == Some(NO_OP_STAGE.as_str());
            InstanceActionAccepted {
                accepted: Some(*accepted),
                already_in_state,
            }
        }
        StartOutcome::NothingToDo => InstanceActionAccepted {
            accepted: None,
            already_in_state: true,
        },
    }
}

/// The revision a request expects, if it names one.
fn expected_revision(
    revision: Option<stackly_protocol::Revision>,
) -> Result<Option<ResourceRevision>, Box<ErrorBody>> {
    revision
        .map(|revision| {
            ResourceRevision::new(revision.get()).map_err(|_| {
                Box::new(
                    ErrorBody::new(
                        &stackly_protocol::codes::request::INVALID_PARAMS,
                        "expectedRevision is not a revision",
                    )
                    .with_field(stackly_protocol::FieldError::new(
                        "expectedRevision",
                        stackly_protocol::json::FieldCode::OutOfRange,
                    )),
                )
            })
        })
        .transpose()
}

/// The snapshot of `instance` with the status the supervisor shows now.
fn summary_of(core: &DaemonCore, supervisor: &Supervisor, instance: &Instance) -> InstanceSummary {
    let status = supervisor.status(instance.id());
    let excerpt = if instance.last_failure().is_some() {
        supervisor
            .failure_excerpt(instance.id())
            .iter()
            .map(|line| dto::excerpt_line(line))
            .collect()
    } else {
        Vec::new()
    };
    let sockets = core
        .supervision()
        .sockets()
        .map(|sockets| sockets.of(instance))
        .unwrap_or_default();
    dto::summary(instance, &status, excerpt, &sockets)
}

/// Checks that the instance `id` exists, retained ones included, and
/// returns its ID; the log methods answer an unknown one like
/// `instance.get` (spec 0027).
///
/// # Errors
///
/// `resource.not_found` for an unknown instance and the registry errors of
/// `instance.get`.
pub async fn existing(core: &DaemonCore, id: Uuid) -> Result<InstanceId, Box<ErrorBody>> {
    let id = instance_id(id)?;
    read_instance(core, id).await?;
    Ok(id)
}

/// Reads one instance, retained ones included.
async fn read_instance(core: &DaemonCore, id: InstanceId) -> Result<Instance, Box<ErrorBody>> {
    let registry = registry_of(core)?;
    tokio::task::spawn_blocking(move || registry.instance(id))
        .await
        .map_err(|_| unexpected("the instance read task failed"))?
        .map_err(|error| Box::new(map::registry_error(&error)))?
        .value
        .ok_or_else(|| {
            Box::new(
                ErrorBody::new(&resource::NOT_FOUND, "the instance does not exist").with_resource(
                    ResourceRef {
                        kind: ResourceKind::Instance,
                        id: id.to_string(),
                    },
                ),
            )
        })
}

/// The instance a request names; an ID that cannot be an instance's is
/// one that does not exist.
fn instance_id(id: Uuid) -> Result<InstanceId, Box<ErrorBody>> {
    InstanceId::from_uuid(id).map_err(|_| {
        Box::new(
            ErrorBody::new(&resource::NOT_FOUND, "the instance does not exist").with_resource(
                ResourceRef {
                    kind: ResourceKind::Instance,
                    id: id.to_string(),
                },
            ),
        )
    })
}

/// The registry handle, once the registry is ready.
fn registry_of(core: &DaemonCore) -> Result<SqliteRegistry, Box<ErrorBody>> {
    core.registry().ok_or_else(|| {
        Box::new(ErrorBody::new(
            &stackly_protocol::codes::registry::UNAVAILABLE,
            "the registry is not available",
        ))
    })
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(
        &stackly_protocol::codes::internal::UNEXPECTED,
        message,
    ))
}

/// A cursor of `instance.list`: `<name>_<uuid>`.
fn parse_cursor(text: &str) -> Option<(String, Uuid)> {
    let (name, id) = text.rsplit_once('_')?;
    Some((name.to_owned(), id.parse().ok()?))
}
