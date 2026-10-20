//! The `operation` area of the daemon (spec 0007 build step 14): the six
//! operation methods, [`start_operation`] for every method that starts an
//! operation, the errors of the engine as error bodies, and the event sink
//! that publishes each committed change.

pub mod dto;

use std::sync::Arc;

use stackly_core::operation::{
    BeginError, BeginOutcome, BeginRequest, EngineError, LockHolder, NoOpOutcome, OperationEngine,
    OperationEvents, OperationView, Origin, RequestOrigin,
};
use stackly_core::secret::Redactor;
use stackly_domain::{
    ClientCode, IdempotencyKey, JournalPayload, MethodName, OperationCursor, OperationId,
    OperationQuery, OperationResource, ResourceRevision, Sha256Digest,
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
    redactor: Arc<Redactor>,
}

impl BusEvents {
    /// A sink that publishes on `bus`, with the text of each summary
    /// passed through `redactor` first (spec 0011 AC-14).
    #[must_use]
    pub const fn new(bus: Arc<EventBus>, redactor: Arc<Redactor>) -> Self {
        Self { bus, redactor }
    }

    fn publish(&self, view: &OperationView, kind: fn(Box<OperationSummary>) -> EventKind) {
        let mut summary = dto::summary(view);
        map::redact::summary(&self.redactor, &mut summary);
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
#[non_exhaustive]
pub struct StartRequest<'a> {
    /// The wire method.
    pub method: &'static str,
    /// The request's `idempotencyKey`, already checked for its format.
    pub idempotency_key: Option<&'a str>,
    /// `request_fingerprint` of the request's call (spec 0007,
    /// *Idempotency*).
    pub fingerprint: [u8; 32],
}

impl<'a> StartRequest<'a> {
    /// Bundles a request of `method` with its key and fingerprint.
    #[must_use]
    pub const fn new(
        method: &'static str,
        idempotency_key: Option<&'a str>,
        fingerprint: [u8; 32],
    ) -> Self {
        Self {
            method,
            idempotency_key,
            fingerprint,
        }
    }
}

/// What [`start_operation`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
    let (engine, origin) = request_parts(core, context, &start)?;
    let outcome = engine
        .begin(build(Origin::Request(origin)))
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

/// What [`replay_operation`] found: the operation a repeated request
/// names, and the input it recorded.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub struct Replay {
    /// The reply [`start_operation`] gives for the same request, with
    /// `replayed` true. An answer without an operation that was recorded
    /// is told by its stage, [`NO_OP_STAGE`](stackly_core::operation::NO_OP_STAGE).
    pub accepted: OperationAccepted,
    /// The input of the journal of that operation, or of the first
    /// request for a recorded answer without an operation: what the
    /// handler needs to build its own reply without doing the work again.
    pub input: JournalPayload,
}

impl Replay {
    fn new(view: &OperationView) -> Self {
        Self {
            accepted: OperationAccepted {
                operation: dto::summary(view),
                replayed: true,
            },
            input: view.journal.input().clone(),
        }
    }

    /// The operation's ID, for the `operation_id` of the request log line.
    #[must_use]
    pub fn operation_id(&self) -> Uuid {
        self.accepted.operation.id
    }
}

impl std::fmt::Debug for Replay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Replay")
            .field("accepted", &self.accepted)
            .finish_non_exhaustive()
    }
}

/// Finds what [`start_operation`] would replay for the same request,
/// before the handler does expensive work or reads state that changed
/// since the first request (spec 0007, *Idempotency*, `replay`). It shares
/// the key checks, the origin, the engine access, and the error map with
/// [`start_operation`], so a request gets the same answer through both.
/// `None` means no operation holds the key; the handler then builds its
/// request and calls [`start_operation`], which stays authoritative.
///
/// # Errors
///
/// Returns the error body of the refusal, as [`start_operation`] does for
/// the key checks and the engine access, and [`begin_error`] for a
/// [`BeginError`] of `replay`: `daemon.starting`, `daemon.shutting_down`,
/// `operation.idempotency_key_reused`, or a registry error.
pub async fn replay_operation(
    core: &DaemonCore,
    context: &RequestContext,
    start: StartRequest<'_>,
) -> Result<Option<Replay>, Box<ErrorBody>> {
    let (engine, origin) = request_parts(core, context, &start)?;
    let found = engine
        .replay(&origin)
        .await
        .map_err(|error| boxed(begin_error(&error, start.method)))?;
    replay_result(found)
}

/// The pure mapping of what the engine's `replay` found.
fn replay_result(found: Option<BeginOutcome>) -> Result<Option<Replay>, Box<ErrorBody>> {
    match found {
        None => Ok(None),
        Some(BeginOutcome::Replayed(view)) => Ok(Some(Replay::new(&view))),
        Some(BeginOutcome::NoOp(NoOpOutcome {
            recorded: Some(view),
            ..
        })) => Ok(Some(Replay::new(&view))),
        Some(BeginOutcome::Created(_)) => {
            tracing::error!("the engine replayed a created operation");
            Err(unexpected("the operation engine answered a replay wrongly"))
        }
        Some(BeginOutcome::NoOp(_)) => {
            tracing::error!("the engine replayed an answer without a recorded operation");
            Err(unexpected("the operation engine answered a replay wrongly"))
        }
    }
}

/// The checks and parts both [`start_operation`] and [`replay_operation`]
/// begin with: the key checks, then the engine and the request's origin.
fn request_parts(
    core: &DaemonCore,
    context: &RequestContext,
    start: &StartRequest<'_>,
) -> Result<(OperationEngine, RequestOrigin), Box<ErrorBody>> {
    let (key, method) = request_key(start)?;
    let engine = core.operations().writer()?;
    let origin = RequestOrigin {
        method,
        key,
        fingerprint: Sha256Digest::from_bytes(start.fingerprint),
        client: client_code(context.client.kind),
        interactive: context.client.is_interactive(),
    };
    Ok((engine, origin))
}

/// The key checks that come before anything is read: the key is required,
/// then it must parse, and the method must be a valid name.
fn request_key(start: &StartRequest<'_>) -> Result<(IdempotencyKey, MethodName), Box<ErrorBody>> {
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
    Ok((key, method))
}

fn accepted(view: &OperationView, replayed: bool) -> StartOutcome {
    StartOutcome::Accepted(Box::new(OperationAccepted {
        operation: dto::summary(view),
        replayed,
    }))
}

/// The client code a request's operation records, from its handshake.
pub(crate) fn client_code(kind: ClientKind) -> ClientCode {
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
    dto::catalog_body(
        spec,
        &error.to_string(),
        &error.details(),
        &error.items(),
        error.rule(),
        None,
    )
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

#[cfg(test)]
mod tests {
    use stackly_core::operation::{NotAbandonableReason, NotResumableReason, Rejection};
    use stackly_domain::{
        DetailKey, ErrorCodeName, LockKey, OperationState as DomainState, RegistryError,
    };

    use super::*;

    /// A body that the catalog fully explains: a known code, its category
    /// and `retryable`, and details only from its entry.
    fn checked(body: &ErrorBody) -> &ErrorBody {
        let spec = body.spec().expect("the code is in the catalog");
        assert_eq!(body.category(), spec.category, "{}", body.code());
        assert_eq!(body.retryable(), spec.retryable, "{}", body.code());
        for detail in body.details() {
            assert!(
                spec.details.contains(&detail.key()),
                "{} carries {}",
                body.code(),
                detail.key()
            );
        }
        body
    }

    fn revision(number: u64) -> ResourceRevision {
        ResourceRevision::new(number).unwrap()
    }

    #[derive(Debug, thiserror::Error)]
    #[error("instance pg-main is already running")]
    struct Refused {
        code: &'static str,
    }

    impl stackly_core::operation::OperationError for Refused {
        fn code(&self) -> ErrorCodeName {
            self.code.parse().unwrap()
        }

        fn retryable(&self) -> bool {
            false
        }

        fn details(&self) -> Vec<(DetailKey, String)> {
            vec![
                ("operationId".parse().unwrap(), "x".to_owned()),
                ("notInCatalog".parse().unwrap(), "y".to_owned()),
            ]
        }

        fn rule(&self) -> Option<stackly_domain::Rule> {
            Some(stackly_domain::Rule::Br008)
        }
    }

    #[test]
    fn every_begin_refusal_becomes_its_catalog_error() {
        let id = OperationId::generate();
        let cases = [
            (BeginError::NotReady, "daemon.starting"),
            (BeginError::ShuttingDown, "daemon.shutting_down"),
            (
                BeginError::InvalidRequest { reason: "bug" },
                "internal.unexpected",
            ),
            (
                BeginError::PayloadTooLarge {
                    field: "input",
                    max: 16_384,
                },
                "operation.payload_too_large",
            ),
            (
                BeginError::KeyReused { existing: id },
                "operation.idempotency_key_reused",
            ),
            (BeginError::ResourceNotFound, "resource.not_found"),
            (
                BeginError::RevisionMismatch {
                    expected: revision(2),
                    actual: revision(3),
                },
                "resource.revision_mismatch",
            ),
            (
                BeginError::RecoveryRequired {
                    operation: id,
                    key: LockKey::Manager,
                },
                "operation.recovery_required",
            ),
            (
                BeginError::Busy {
                    holder: LockHolder::Operation {
                        id,
                        state: DomainState::Running,
                    },
                },
                "resource.busy",
            ),
            (
                BeginError::Busy {
                    holder: LockHolder::Internal("supervisor"),
                },
                "resource.busy",
            ),
            (BeginError::QueueFull { max: 64 }, "operation.queue_full"),
            (
                BeginError::Registry(RegistryError::NoSpace),
                "registry.no_space",
            ),
        ];

        for (error, code) in cases {
            let body = begin_error(&error, "instance.start");
            assert_eq!(checked(&body).code(), code, "{error:?}");
        }
    }

    #[test]
    fn a_reused_key_names_the_first_operation_and_the_method() {
        let id = OperationId::generate();

        let body = begin_error(&BeginError::KeyReused { existing: id }, "instance.start");

        assert_eq!(body.detail("operationId"), Some(id.to_string().as_str()));
        assert_eq!(body.detail("method"), Some("instance.start"));
        assert!(body.hint().is_some());
    }

    #[test]
    fn a_busy_resource_names_its_holder_unless_the_holder_is_internal() {
        let id = OperationId::generate();
        let by_operation = begin_error(
            &BeginError::Busy {
                holder: LockHolder::Operation {
                    id,
                    state: DomainState::Running,
                },
            },
            "instance.stop",
        );
        let by_internal = begin_error(
            &BeginError::Busy {
                holder: LockHolder::Internal("supervisor"),
            },
            "instance.stop",
        );

        assert_eq!(
            by_operation.detail("operationId"),
            Some(id.to_string().as_str())
        );
        assert!(by_operation.retryable());
        assert_eq!(by_internal.detail("operationId"), None);
    }

    #[test]
    fn recovery_required_names_the_operation_and_the_lock_key() {
        let id = OperationId::generate();

        let body = begin_error(
            &BeginError::RecoveryRequired {
                operation: id,
                key: LockKey::Manager,
            },
            "manager.update",
        );

        assert_eq!(body.detail("operationId"), Some(id.to_string().as_str()));
        assert_eq!(body.detail("lockKey"), Some("manager:self"));
        assert_eq!(body.operation_id(), Some(id.as_uuid()));
    }

    #[test]
    fn begin_while_the_engine_recovers_is_daemon_starting_for_operations() {
        let body = begin_error(&BeginError::NotReady, "instance.start");

        assert_eq!(body.detail("subsystem"), Some("operations"));
        assert!(body.retryable());
    }

    #[test]
    fn a_precheck_refusal_keeps_its_code_rule_and_only_the_details_its_entry_lists() {
        let refused = Rejection::new(Box::new(Refused {
            code: "resource.busy",
        }));

        let body = begin_error(&BeginError::Rejected(refused), "instance.start");

        assert_eq!(checked(&body).code(), "resource.busy");
        assert_eq!(body.message(), "instance pg-main is already running");
        assert_eq!(body.rule(), Some("BR-008"));
        assert_eq!(body.detail("operationId"), Some("x"));
        assert_eq!(body.detail("notInCatalog"), None);
    }

    #[derive(Debug, thiserror::Error)]
    #[error("the settings are not valid")]
    struct Listed;

    impl stackly_core::operation::OperationError for Listed {
        fn code(&self) -> ErrorCodeName {
            "provider.invalid_settings".parse().unwrap()
        }

        fn retryable(&self) -> bool {
            false
        }

        fn details(&self) -> Vec<(DetailKey, String)> {
            vec![("issueCount".parse().unwrap(), "1".to_owned())]
        }

        fn items(&self) -> Vec<stackly_domain::FailureItem> {
            vec![
                stackly_domain::FailureItem::new(
                    "issue".parse().unwrap(),
                    vec![("location".parse().unwrap(), "/port".to_owned())],
                )
                .unwrap(),
            ]
        }

        fn rule(&self) -> Option<stackly_domain::Rule> {
            Some(stackly_domain::Rule::Br002)
        }
    }

    #[test]
    fn a_precheck_refusal_keeps_its_items_and_rule_through_the_stored_error_mapping() {
        let refused = Rejection::new(Box::new(Listed));

        let body = begin_error(&BeginError::Rejected(refused), "environment.create");

        assert_eq!(checked(&body).code(), "provider.invalid_settings");
        assert_eq!(body.rule(), Some("BR-002"));
        assert_eq!(body.detail("issueCount"), Some("1"));
        assert_eq!(body.items().len(), 1);
        assert_eq!(body.items()[0].field("location"), Some("/port"));
        assert_eq!(body.operation_id(), None, "a refusal has no operation");
    }

    fn start(key: Option<&str>) -> StartRequest<'_> {
        StartRequest::new("instance.start", key, [7; 32])
    }

    #[test]
    fn a_request_without_a_key_is_refused_before_anything_is_read() {
        let body = request_key(&start(None)).unwrap_err();

        assert_eq!(checked(&body).code(), "operation.idempotency_key_required");
        assert_eq!(body.detail("method"), Some("instance.start"));
        assert!(body.hint().is_some());
    }

    #[test]
    fn a_key_with_a_bad_format_is_a_malformed_request_naming_the_field() {
        let body = request_key(&start(Some("has spaces"))).unwrap_err();

        assert_eq!(checked(&body).code(), "request.malformed");
        assert_eq!(body.fields().len(), 1);
        assert_eq!(body.fields()[0].path(), "idempotencyKey");
    }

    #[test]
    fn a_good_key_and_method_pass_the_shared_checks() {
        let (key, method) = request_key(&start(Some("cli-install-1"))).unwrap();

        assert_eq!(key.as_str(), "cli-install-1");
        assert_eq!(method.as_str(), "instance.start");
    }

    fn view(stage: Option<&str>, input: &str) -> OperationView {
        let mut builder = stackly_testkit::OperationBuilder::new().state(DomainState::Succeeded);
        if let Some(stage) = stage {
            builder = builder.stage(stage);
        }
        let operation = builder.build();
        let journal = stackly_domain::OperationJournal::new(stackly_domain::NewJournal {
            operation_id: operation.id(),
            request: None,
            trigger: stackly_domain::OperationTrigger::background(),
            resumable: false,
            input: JournalPayload::new(input.to_owned(), 16_384).unwrap(),
            expected_revision: None,
        })
        .unwrap();
        OperationView {
            operation,
            journal,
            recovery_actions: Vec::new(),
            auto_compensating: false,
        }
    }

    #[test]
    fn a_replayed_operation_is_accepted_with_replayed_true_and_its_journal_input() {
        let found = view(None, r#"{"version":"17.6"}"#);

        let replay = replay_result(Some(BeginOutcome::Replayed(found.clone())))
            .unwrap()
            .expect("a hit");

        assert!(replay.accepted.replayed);
        assert_eq!(replay.accepted.operation.id, found.operation.id().as_uuid());
        assert_eq!(replay.operation_id(), found.operation.id().as_uuid());
        assert_eq!(replay.input.as_str(), r#"{"version":"17.6"}"#);
        assert!(
            !format!("{replay:?}").contains("17.6"),
            "Debug hides the input"
        );
    }

    #[test]
    fn a_recorded_no_op_answer_is_a_replay_told_by_its_stage() {
        let found = view(Some("no_op"), r#"{"name":"first"}"#);

        let replay = replay_result(Some(BeginOutcome::NoOp(NoOpOutcome {
            recorded: Some(found),
            replayed: true,
        })))
        .unwrap()
        .expect("a hit");

        assert!(replay.accepted.replayed);
        assert_eq!(replay.accepted.operation.stage.as_deref(), Some("no_op"));
        assert_eq!(replay.input.as_str(), r#"{"name":"first"}"#);
    }

    #[test]
    fn a_miss_is_none_and_the_outcomes_replay_never_returns_are_internal_unexpected() {
        assert_eq!(replay_result(None).unwrap(), None);

        for impossible in [
            BeginOutcome::Created(view(None, "{}")),
            BeginOutcome::NoOp(NoOpOutcome {
                recorded: None,
                replayed: false,
            }),
        ] {
            let body = replay_result(Some(impossible)).unwrap_err();
            assert_eq!(checked(&body).code(), "internal.unexpected");
        }
    }

    #[test]
    fn a_precheck_refusal_with_a_code_outside_the_catalog_is_internal_unexpected() {
        let refused = Rejection::new(Box::new(Refused {
            code: "feature.not_built",
        }));

        let body = begin_error(&BeginError::Rejected(refused), "instance.start");

        assert_eq!(body.code(), "internal.unexpected");
        assert!(!body.message().contains("pg-main"));
    }

    #[test]
    fn every_engine_error_becomes_its_catalog_error() {
        let id = OperationId::generate();
        let cases = [
            (EngineError::NotReady, "daemon.starting"),
            (EngineError::ShuttingDown, "daemon.shutting_down"),
            (EngineError::NotFound(id), "resource.not_found"),
            (
                EngineError::RevisionMismatch {
                    expected: revision(4),
                    actual: revision(5),
                },
                "resource.revision_mismatch",
            ),
            (
                EngineError::NotResumable {
                    operation: id,
                    reason: NotResumableReason::NoFactory,
                },
                "operation.not_resumable",
            ),
            (
                EngineError::NotAbandonable {
                    operation: id,
                    state: DomainState::Running,
                    reason: NotAbandonableReason::WrongState,
                },
                "operation.not_abandonable",
            ),
            (
                EngineError::JournalFull {
                    kind: "steps",
                    max: 256,
                },
                "operation.journal_full",
            ),
            (
                EngineError::PayloadTooLarge {
                    field: "checkpoint",
                    max: 4096,
                },
                "operation.payload_too_large",
            ),
            (
                EngineError::InvalidUse { reason: "bug" },
                "internal.unexpected",
            ),
            (
                EngineError::Internal { reason: "bug" },
                "internal.unexpected",
            ),
            (EngineError::Registry(RegistryError::Busy), "registry.busy"),
        ];

        for (error, code) in cases {
            assert_eq!(checked(&engine_error(&error)).code(), code, "{error:?}");
        }
    }

    #[test]
    fn a_refused_resume_or_abandon_names_the_operation_and_the_reason() {
        let id = OperationId::generate();
        let resume = engine_error(&EngineError::NotResumable {
            operation: id,
            reason: NotResumableReason::StepNotRepeatable,
        });
        let abandon = engine_error(&EngineError::NotAbandonable {
            operation: id,
            state: DomainState::NeedsRecovery,
            reason: NotAbandonableReason::AutoCompensating,
        });

        assert_eq!(resume.detail("reason"), Some("step_not_repeatable"));
        assert_eq!(resume.operation_id(), Some(id.as_uuid()));
        assert_eq!(abandon.detail("state"), Some("needs_recovery"));
        assert_eq!(abandon.detail("reason"), Some("auto_compensating"));
        assert_eq!(abandon.operation_id(), Some(id.as_uuid()));
    }

    #[test]
    fn an_unknown_operation_is_not_found_with_the_operation_as_its_resource() {
        let id = OperationId::generate();

        let body = engine_error(&EngineError::NotFound(id));

        assert_eq!(
            body.resource(),
            Some(&ResourceRef {
                kind: ResourceKind::Operation,
                id: id.to_string()
            })
        );
    }

    #[test]
    fn a_uuid_that_is_not_v7_names_no_operation() {
        let v4: Uuid = "6f1c5c1e-3f7a-4b8e-9c2d-1a2b3c4d5e6f".parse().unwrap();

        let error = operation_id(v4).unwrap_err();

        assert_eq!(error.code(), "resource.not_found");
    }

    #[test]
    fn an_expected_revision_of_zero_is_out_of_range() {
        let error = expected_revision(Revision::new(0).unwrap(), "expectedRevision").unwrap_err();

        assert_eq!(checked(&error).code(), "request.invalid_params");
        assert_eq!(error.fields()[0].path(), "expectedRevision");
        assert_eq!(error.fields()[0].code(), "out_of_range");
        assert_eq!(
            expected_revision(Revision::new(7).unwrap(), "expectedRevision").unwrap(),
            revision(7)
        );
    }

    #[test]
    fn the_list_query_reads_the_manager_only_as_self_and_resource_ids_as_uuids() {
        let manager = query(&ListOperationsParams {
            resource: Some(ResourceRef::manager()),
            ..ListOperationsParams::default()
        })
        .unwrap();
        let other_manager = query(&ListOperationsParams {
            resource: Some(ResourceRef {
                kind: ResourceKind::Manager,
                id: "other".to_owned(),
            }),
            ..ListOperationsParams::default()
        })
        .unwrap_err();
        let bad_runtime = query(&ListOperationsParams {
            resource: Some(ResourceRef {
                kind: ResourceKind::Runtime,
                id: "not-a-uuid".to_owned(),
            }),
            ..ListOperationsParams::default()
        })
        .unwrap_err();

        assert_eq!(manager.resource, Some(OperationResource::Manager));
        assert_eq!(manager.limit, 50);
        assert!(!manager.active_only);
        for error in [other_manager, bad_runtime] {
            assert_eq!(checked(&error).code(), "request.invalid_params");
            assert_eq!(error.fields()[0].path(), "resource.id");
        }
    }

    #[test]
    fn a_list_cursor_keeps_the_registry_nanoseconds_both_ways() {
        let id = OperationId::generate();
        let text = format!("2026-10-19T08:00:00.123456789Z_{id}");

        let read = query(&ListOperationsParams {
            before: Some(text.clone()),
            limit: Some(200),
            active_only: true,
            ..ListOperationsParams::default()
        })
        .unwrap();
        let before = read.before.unwrap();

        assert_eq!(before.id, id);
        assert_eq!(before.created_at.subsec_nanosecond(), 123_456_789);
        assert_eq!(wire_cursor(before).to_string(), text);
        assert_eq!(read.limit, 200);
        assert!(read.active_only);
    }

    #[test]
    fn each_client_kind_records_its_own_client_code() {
        assert_eq!(client_code(ClientKind::Cli).as_str(), "cli");
        assert_eq!(client_code(ClientKind::Gui).as_str(), "gui");
        assert_eq!(
            client_code(ClientKind::Unrecognized).as_str(),
            "unrecognized"
        );
    }
}
