//! The `operation` area of the daemon (spec 0007 build step 14): the six
//! operation methods, [`start_operation`] for every method that starts an
//! operation, the errors of the engine as error bodies, and the event sink
//! that publishes each committed change.

pub mod dto;

use std::sync::Arc;

use stackly_core::operation::{
    BeginError, BeginOutcome, BeginRequest, EngineError, LockHolder, OperationEvents,
    OperationView, Origin, RequestOrigin,
};
use stackly_domain::{
    ClientCode, IdempotencyKey, MethodName, OperationCursor, OperationId, OperationQuery,
    OperationResource, ResourceRevision, Sha256Digest,
};
use stackly_protocol::codes::{daemon, internal, operation, request, resource};
use stackly_protocol::daemon::subsystem;
use stackly_protocol::events::EventKind;
use stackly_protocol::handshake::ClientKind;
use stackly_protocol::json::FieldCode;
use stackly_protocol::operation::{
    AbandonOperationParams, AbandonReply, CancelOperationParams, CancelReply, GetOperationParams,
    ListOperationsParams, OPERATION_PAGE_BYTES_MAX, OperationAccepted, OperationCursor as Cursor,
    OperationDetail, OperationPage, OperationStats, OperationSummary, ResumeOperationParams,
};
use stackly_protocol::{ErrorBody, FieldError, ResourceKind, ResourceRef, Revision};
use uuid::Uuid;

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::events::EventBus;
use crate::map;
use crate::state::DaemonCore;

/// Publishes every committed change of an operation as
/// `operation.state_changed` or `operation.progress`, with the operation
/// as the resource and its committed revision (spec 0007, *Progress dan
/// event*).
#[derive(Debug, Clone)]
pub struct BusEvents {
    bus: Arc<EventBus>,
}

impl BusEvents {
    /// A sink that publishes on `bus`.
    #[must_use]
    pub const fn new(bus: Arc<EventBus>) -> Self {
        Self { bus }
    }

    fn publish(&self, view: &OperationView, kind: fn(Box<OperationSummary>) -> EventKind) {
        let summary = dto::summary(view);
        let resource = ResourceRef {
            kind: ResourceKind::Operation,
            id: summary.id.to_string(),
        };
        let revision = summary.revision;
        self.bus
            .publish_resource(kind(Box::new(summary)), resource, revision);
    }
}

impl OperationEvents for BusEvents {
    fn state_changed(&self, view: &OperationView) {
        self.publish(view, EventKind::OperationStateChanged);
    }

    fn progress(&self, view: &OperationView) {
        self.publish(view, EventKind::OperationProgress);
    }
}

/// The real `operation.get`.
pub fn get<'a>(
    core: &'a DaemonCore,
    params: GetOperationParams,
) -> HandlerFuture<'a, OperationDetail> {
    Box::pin(async move {
        let engine = core.operations().reader()?;
        let id = operation_id(params.operation_id)?;
        match engine.get(id).await {
            Ok(Some(detail)) => Ok(dto::detail(&detail)),
            Ok(None) => Err(not_found(id)),
            Err(error) => Err(boxed(engine_error(&error))),
        }
    })
}

/// The real `operation.list`: newest first, and a page that stops before
/// its summaries pass [`OPERATION_PAGE_BYTES_MAX`] of JSON, its `next`
/// naming the last operation that fit (spec 0007 AC-20).
pub fn list<'a>(
    core: &'a DaemonCore,
    params: ListOperationsParams,
) -> HandlerFuture<'a, OperationPage> {
    Box::pin(async move {
        let engine = core.operations().reader()?;
        let query = query(&params)?;
        let page = engine
            .list(query)
            .await
            .map_err(|error| boxed(engine_error(&error)))?;
        let mut items = Vec::with_capacity(page.items.len());
        let mut bytes = 0_usize;
        let mut next = page.next;
        for (at, view) in page.items.iter().enumerate() {
            let summary = dto::summary(view);
            let size = serde_json::to_vec(&summary)
                .map(|encoded| encoded.len())
                .unwrap_or(usize::MAX);
            if at > 0 && bytes.saturating_add(size) > OPERATION_PAGE_BYTES_MAX {
                // The cursor keeps the registry's nanoseconds, which the
                // summary's millisecond time has lost.
                next = Some(OperationCursor::of(&page.items[at - 1].operation));
                break;
            }
            bytes = bytes.saturating_add(size);
            items.push(summary);
        }
        Ok(OperationPage {
            items,
            next: next.map(|cursor| wire_cursor(cursor).to_string()),
        })
    })
}

/// The real `operation.cancel`.
pub fn cancel<'a>(
    core: &'a DaemonCore,
    params: CancelOperationParams,
) -> HandlerFuture<'a, CancelReply> {
    Box::pin(async move {
        let engine = core.operations().writer()?;
        let id = operation_id(params.operation_id)?;
        let outcome = engine
            .cancel(id)
            .await
            .map_err(|error| boxed(engine_error(&error)))?;
        Ok(CancelReply {
            operation: dto::summary(&outcome.view),
            disposition: dto::disposition(outcome.disposition),
        })
    })
}

/// The real `operation.resume`.
pub fn resume<'a>(
    core: &'a DaemonCore,
    params: ResumeOperationParams,
) -> HandlerFuture<'a, OperationSummary> {
    Box::pin(async move {
        let engine = core.operations().writer()?;
        let id = operation_id(params.operation_id)?;
        let expected = params
            .expected_revision
            .map(|revision| expected_revision(revision, "expectedRevision"))
            .transpose()?;
        let view = engine
            .resume(id, expected)
            .await
            .map_err(|error| boxed(engine_error(&error)))?;
        Ok(dto::summary(&view))
    })
}

/// The real `operation.abandon`.
pub fn abandon<'a>(
    core: &'a DaemonCore,
    params: AbandonOperationParams,
) -> HandlerFuture<'a, AbandonReply> {
    Box::pin(async move {
        let engine = core.operations().writer()?;
        let id = operation_id(params.operation_id)?;
        let expected = expected_revision(params.expected_revision, "expectedRevision")?;
        let outcome = engine
            .abandon(id, expected)
            .await
            .map_err(|error| boxed(engine_error(&error)))?;
        Ok(AbandonReply {
            operation: dto::summary(&outcome.view),
            leftovers: outcome.leftovers.iter().map(dto::created_info).collect(),
        })
    })
}

/// The real `operation.get_stats`.
pub fn get_stats(core: &DaemonCore) -> HandlerFuture<'_, OperationStats> {
    Box::pin(async move {
        let engine = core.operations().writer()?;
        Ok(dto::stats(&engine.stats()))
    })
}

/// A method outside the contract that starts an operation, registered
/// only by tests through
/// [`Handlers::test_starting_method`](crate::dispatch::Handlers::test_starting_method).
/// The connection sends its request through the same admission, recovery
/// mode check, [`start_operation`], and request log line as a contract
/// method, and writes its reply as a raw response frame whose reply `type`
/// is the method. provisional: F10
#[doc(hidden)]
pub trait TestStartingMethod: Send + Sync + 'static {
    /// The method name, never one of the contract's.
    fn method(&self) -> &'static str;

    /// Registers the method's factories and compensators on the engine,
    /// right after the start slot creates it and before `recover`.
    fn register(&self, engine: &stackly_core::operation::OperationEngine) {
        let _ = engine;
    }

    /// Reads the request's `call.content` and returns its fingerprint and
    /// how to build the begin request.
    ///
    /// # Errors
    ///
    /// Returns the error body for params the method refuses.
    fn prepare(&self, params: Option<&serde_json::Value>) -> Result<TestStart, Box<ErrorBody>>;
}

/// What a [`TestStartingMethod`] prepared. provisional: F10
#[doc(hidden)]
pub struct TestStart {
    /// The request's fingerprint.
    pub fingerprint: [u8; 32],
    /// Builds the begin request from the request's origin.
    pub build: Box<dyn FnOnce(Origin) -> BeginRequest + Send>,
}

impl std::fmt::Debug for TestStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestStart").finish_non_exhaustive()
    }
}

/// What a method that starts an operation asks [`start_operation`] for.
#[derive(Debug, Clone, Copy)]
pub struct StartRequest<'a> {
    /// The wire method.
    pub method: &'static str,
    /// The request's `idempotencyKey`, already checked for its format.
    pub idempotency_key: Option<&'a str>,
    /// `request_fingerprint` of the request's call (spec 0007,
    /// *Idempotency*).
    pub fingerprint: [u8; 32],
}

/// What [`start_operation`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    /// An operation was created, or replayed from the idempotency key; a
    /// recorded no operation answer is replayed the same way.
    Accepted(Box<OperationAccepted>),
    /// The feature's precheck answered without an operation and wrote
    /// nothing, so no key was bound; the method answers in its own words.
    NothingToDo,
}

impl StartOutcome {
    /// The operation it names, if any.
    #[must_use]
    pub fn operation_id(&self) -> Option<Uuid> {
        match self {
            Self::Accepted(accepted) => Some(accepted.operation.id),
            Self::NothingToDo => None,
        }
    }
}

/// Starts an operation for a client request (spec 0007, *Idempotency*): a
/// request without an idempotency key gets
/// `operation.idempotency_key_required` before anything is read, then
/// `build` makes the begin request with the request's origin (method, key,
/// fingerprint, the client's kind, and whether it is interactive), and the
/// engine begins it. The engine runs the critical part of `begin` on its
/// own task, so a request task cancelled after this returns, or while it
/// waits, never leaves a half written operation behind (AC-6).
///
/// # Errors
///
/// Returns the error body of the refusal: the missing key, a key the
/// domain refuses (`request.malformed`), or the [`BeginError`] in the
/// engine's check order, mapped by [`begin_error`].
pub async fn start_operation(
    core: &DaemonCore,
    context: &RequestContext,
    start: StartRequest<'_>,
    build: impl FnOnce(Origin) -> BeginRequest,
) -> Result<StartOutcome, Box<ErrorBody>> {
    let Some(key) = start.idempotency_key else {
        return Err(boxed(
            ErrorBody::new(
                &operation::IDEMPOTENCY_KEY_REQUIRED,
                "this method starts an operation, so it needs an idempotency key",
            )
            .with_detail("method", start.method)
            .with_hint("send the request again with an idempotencyKey"),
        ));
    };
    let key: IdempotencyKey = key.parse().map_err(|_| {
        boxed(
            ErrorBody::new(&request::MALFORMED, "the idempotency key has a bad format")
                .with_field(FieldError::new("idempotencyKey", FieldCode::InvalidFormat)),
        )
    })?;
    let method: MethodName = start.method.parse().map_err(|_| {
        tracing::error!(
            method = start.method,
            "a starting method has no valid method name"
        );
        unexpected("a method that starts an operation has no valid name")
    })?;
    let engine = core.operations().writer()?;
    let origin = Origin::Request(RequestOrigin {
        method,
        key,
        fingerprint: Sha256Digest::from_bytes(start.fingerprint),
        client: client_code(context.client.kind),
        interactive: context.client.is_interactive(),
    });
    let outcome = engine
        .begin(build(origin))
        .await
        .map_err(|error| boxed(begin_error(&error, start.method)))?;
    Ok(match outcome {
        BeginOutcome::Created(view) => accepted(&view, false),
        BeginOutcome::Replayed(view) => accepted(&view, true),
        BeginOutcome::NoOp(no_op) => match no_op.recorded {
            Some(view) => accepted(&view, no_op.replayed),
            None => StartOutcome::NothingToDo,
        },
    })
}

fn accepted(view: &OperationView, replayed: bool) -> StartOutcome {
    StartOutcome::Accepted(Box::new(OperationAccepted {
        operation: dto::summary(view),
        replayed,
    }))
}

/// The client code a request's operation records, from its handshake.
fn client_code(kind: ClientKind) -> ClientCode {
    let code = match kind {
        ClientKind::Cli => "cli",
        ClientKind::Gui => "gui",
        ClientKind::Unrecognized => "unrecognized",
    };
    code.parse()
        .expect("the client kinds are fixed snake_case words, valid client codes")
}

/// The error body of a refused begin (spec 0007, *Kode error*). Nothing
/// was written.
#[must_use]
pub fn begin_error(error: &BeginError, method: &str) -> ErrorBody {
    match error {
        BeginError::NotReady => starting(subsystem::OPERATIONS),
        BeginError::ShuttingDown => shutting_down(),
        BeginError::InvalidRequest { reason } => {
            tracing::error!(
                method,
                reason,
                "a handler built an invalid operation request"
            );
            ErrorBody::new(
                &internal::UNEXPECTED,
                "the daemon built an invalid operation request",
            )
        }
        BeginError::PayloadTooLarge { field, max } => payload_too_large(field, *max),
        BeginError::KeyReused { existing } => ErrorBody::new(
            &operation::IDEMPOTENCY_KEY_REUSED,
            "the idempotency key belongs to an operation another request started",
        )
        .with_detail("operationId", existing.to_string())
        .with_detail("method", method)
        .with_hint("send the request again with a new idempotency key"),
        BeginError::Rejected(rejection) => rejected(rejection.error()),
        BeginError::ResourceNotFound => {
            ErrorBody::new(&resource::NOT_FOUND, "the resource does not exist")
        }
        BeginError::RevisionMismatch { expected, actual } => revision_mismatch(*expected, *actual),
        BeginError::RecoveryRequired { operation: id, key } => ErrorBody::new(
            &operation::RECOVERY_REQUIRED,
            "an operation that needs recovery holds the resource",
        )
        .with_detail("operationId", id.to_string())
        .with_detail("lockKey", key.to_string())
        .with_operation_id(id.as_uuid())
        .with_hint("resume or abandon that operation first"),
        BeginError::Busy { holder } => {
            let body = ErrorBody::new(&resource::BUSY, "another operation holds the resource");
            match holder {
                LockHolder::Operation { id, .. } => body.with_detail("operationId", id.to_string()),
                LockHolder::Internal(_) => body,
            }
        }
        BeginError::QueueFull { max } => {
            ErrorBody::new(&operation::QUEUE_FULL, "the operation queue is full")
                .with_detail("max", max.to_string())
                .with_hint("wait for running operations to finish, then try again")
        }
        BeginError::Registry(error) => map::registry_error(error),
        _ => ErrorBody::new(
            &internal::UNEXPECTED,
            "the operation engine refused the request for a reason this build cannot name",
        ),
    }
}

/// The error body of a failed engine call other than `begin`.
#[must_use]
pub fn engine_error(error: &EngineError) -> ErrorBody {
    match error {
        EngineError::NotReady => starting(subsystem::OPERATIONS),
        EngineError::ShuttingDown => shutting_down(),
        EngineError::NotFound(id) => *not_found(*id),
        EngineError::RevisionMismatch { expected, actual } => revision_mismatch(*expected, *actual),
        EngineError::NotResumable {
            operation: id,
            reason,
        } => ErrorBody::new(&operation::NOT_RESUMABLE, "the operation cannot be resumed")
            .with_detail("operationId", id.to_string())
            .with_detail("reason", reason.as_code())
            .with_operation_id(id.as_uuid()),
        EngineError::NotAbandonable {
            operation: id,
            state,
            reason,
        } => ErrorBody::new(
            &operation::NOT_ABANDONABLE,
            "the operation cannot be abandoned",
        )
        .with_detail("operationId", id.to_string())
        .with_detail("state", state.as_code())
        .with_detail("reason", reason.as_code())
        .with_operation_id(id.as_uuid()),
        EngineError::JournalFull { kind, max } => {
            ErrorBody::new(&operation::JOURNAL_FULL, "the operation journal is full")
                .with_detail("kind", *kind)
                .with_detail("max", max.to_string())
        }
        EngineError::PayloadTooLarge { field, max } => payload_too_large(field, *max),
        EngineError::Registry(error) => map::registry_error(error),
        other => {
            tracing::error!(error = %other, "the operation engine failed inside the daemon");
            ErrorBody::new(
                &internal::UNEXPECTED,
                "the operation engine failed inside the daemon",
            )
        }
    }
}

/// A feature's precheck refusal, through the catalog like a stored error.
fn rejected(error: &dyn stackly_core::operation::OperationError) -> ErrorBody {
    let code = error.code();
    let Some(spec) = stackly_protocol::codes::lookup(code.as_str()) else {
        tracing::error!(
            code = code.as_str(),
            "a precheck refused with a code the catalog does not know"
        );
        return ErrorBody::new(
            &internal::UNEXPECTED,
            "the request was refused for a reason this daemon cannot name",
        );
    };
    let mut body = ErrorBody::new(spec, error.to_string());
    for (key, value) in error.details() {
        if spec.details.contains(&key.as_str()) {
            body = body.with_detail(key.as_str(), value);
        }
    }
    match error.rule() {
        Some(rule) => body.with_rule(rule),
        None => body,
    }
}

/// `daemon.starting` naming the subsystem the method waits for.
#[must_use]
pub fn starting(name: &str) -> ErrorBody {
    ErrorBody::new(
        &daemon::STARTING,
        "the daemon is starting; try again in a moment",
    )
    .with_detail("subsystem", name)
}

fn shutting_down() -> ErrorBody {
    ErrorBody::new(&daemon::SHUTTING_DOWN, "the daemon is stopping")
}

fn payload_too_large(field: &str, max: usize) -> ErrorBody {
    ErrorBody::new(
        &operation::PAYLOAD_TOO_LARGE,
        "a journal payload is above its limit",
    )
    .with_detail("field", field)
    .with_detail("max", max.to_string())
}

fn revision_mismatch(expected: ResourceRevision, actual: ResourceRevision) -> ErrorBody {
    ErrorBody::new(
        &resource::REVISION_MISMATCH,
        "the resource changed since it was read",
    )
    .with_detail("expected", expected.to_string())
    .with_detail("actual", actual.to_string())
    .with_hint("refresh the resource, then try again")
}

fn not_found(id: OperationId) -> Box<ErrorBody> {
    boxed(
        ErrorBody::new(&resource::NOT_FOUND, "the operation does not exist").with_resource(
            ResourceRef {
                kind: ResourceKind::Operation,
                id: id.to_string(),
            },
        ),
    )
}

/// A wire operation ID as the domain's: a UUID v7. Another UUID names no
/// operation, so it is not found.
fn operation_id(id: Uuid) -> Result<OperationId, Box<ErrorBody>> {
    OperationId::from_uuid(id).map_err(|_| {
        boxed(
            ErrorBody::new(&resource::NOT_FOUND, "the operation does not exist").with_resource(
                ResourceRef {
                    kind: ResourceKind::Operation,
                    id: id.to_string(),
                },
            ),
        )
    })
}

fn expected_revision(revision: Revision, field: &str) -> Result<ResourceRevision, Box<ErrorBody>> {
    ResourceRevision::new(revision.get()).map_err(|_| {
        boxed(
            ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
                .with_field(FieldError::new(field, FieldCode::OutOfRange)),
        )
    })
}

/// The registry query of `operation.list`. `Validate` already checked the
/// limit, the cursor's text, and the resource kind; what is left is the
/// resource ID.
fn query(params: &ListOperationsParams) -> Result<OperationQuery, Box<ErrorBody>> {
    let invalid = |path: &str| {
        boxed(
            ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
                .with_detail("method", "operation.list")
                .with_field(FieldError::new(path, FieldCode::InvalidFormat)),
        )
    };
    let resource = match &params.resource {
        None => None,
        Some(resource) => Some(match resource.kind {
            ResourceKind::Runtime => {
                OperationResource::Runtime(resource.id.parse().map_err(|_| invalid("resource.id"))?)
            }
            ResourceKind::Instance => OperationResource::Instance(
                resource.id.parse().map_err(|_| invalid("resource.id"))?,
            ),
            ResourceKind::Environment => OperationResource::Environment(
                resource.id.parse().map_err(|_| invalid("resource.id"))?,
            ),
            ResourceKind::Manager if resource.id == ResourceRef::MANAGER_ID => {
                OperationResource::Manager
            }
            _ => return Err(invalid("resource.id")),
        }),
    };
    let before = match &params.before {
        None => None,
        Some(text) => {
            let cursor = Cursor::parse(text).ok_or_else(|| invalid("before"))?;
            Some(OperationCursor {
                created_at: cursor.created_at(),
                id: OperationId::from_uuid(cursor.id()).map_err(|_| invalid("before"))?,
            })
        }
    };
    Ok(OperationQuery {
        active_only: params.active_only,
        resource,
        limit: u16::try_from(params.limit_or_default()).map_err(|_| invalid("limit"))?,
        before,
    })
}

const fn wire_cursor(cursor: OperationCursor) -> Cursor {
    Cursor::new(cursor.created_at, cursor.id.as_uuid())
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    boxed(ErrorBody::new(&internal::UNEXPECTED, message))
}

fn boxed(body: ErrorBody) -> Box<ErrorBody> {
    Box::new(body)
}
