//! The `instance` area of the daemon (spec 0010 build step 13, spec 0020
//! build step 9): the seven methods `instance.create`, `instance.start`,
//! `instance.stop`, `instance.get`, `instance.list`,
//! `instance.get_create_form`, and `instance.preflight`.
//!
//! Every method that starts an operation goes through
//! [`start_operation`](crate::ipc::operation::start_operation): the key
//! check and the lookup of the key first (a repeated request gets its
//! reply back), then the feature's precheck, which answers the refusals a
//! state decides and the `alreadyInState` replies without an operation
//! (AC-23). `ipc::instance` never calls `expose()` on a secret (K15); the
//! conversion of a wire secret is `ipc::secret`'s.

mod create;
mod draft;
pub mod dto;
mod duplicate;
mod form;

use stackly_core::operation::{
    BeginRequest, BusyPolicy, FinalBatch, NO_OP_STAGE, NoOpReply, Origin, Precheck, PrecheckOutcome,
};
use stackly_core::supervisor::SupervisorError;
use stackly_core::supervisor::{InstanceChanges, StartRequest, StopRequest, Supervisor};
use stackly_domain::{
    DesiredState, Instance, InstanceFilter, InstanceId, InstanceLifecycle, JournalPayload,
    LeaseFilter, OperationKind, OperationResource, Registry, ResourceRevision, StopDecision,
    StopRequest as LeaseStopRequest, evaluate_stop,
};
use stackly_protocol::codes::resource;
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::{
    GetInstanceParams, INSTANCE_PAGE_BYTES_MAX, InstanceActionAccepted, InstancePage,
    InstanceSummary, ListInstancesParams, StartInstanceParams, StopInstanceParams,
};
use stackly_protocol::message::Call;
use stackly_protocol::{ErrorBody, ResourceKind, ResourceRef};
use stackly_storage::SqliteRegistry;
use uuid::Uuid;

pub use create::{create, work_deps};
pub use duplicate::get_duplicate_form;
pub use form::{get_create_form, preflight};

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
