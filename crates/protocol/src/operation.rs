//! The `operation` area (spec 0007, *Permukaan API*): the DTOs of the six
//! operation methods and of the two operation events, the reply every
//! method that starts an operation carries, and the `operation.list`
//! cursor.
//!
//! Every method here needs a handshake. An operation is never sent with
//! its journal payloads: no step checkpoint and no custom payload ever
//! reaches the wire.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::{ErrorBody, FieldError};
use crate::handshake::ClientKind;
use crate::json::{FieldCode, Validate};
use crate::wire::{Count, ResourceKind, ResourceRef, Revision, Timestamp};

/// The page size of `operation.list` when the params name none.
pub const OPERATION_LIST_LIMIT_DEFAULT: u32 = 50;
/// The smallest `ListOperationsParams.limit`.
pub const OPERATION_LIST_LIMIT_MIN: u32 = 1;
/// The largest page size `operation.list` accepts.
pub const OPERATION_LIST_LIMIT_MAX: u32 = 200;
/// The longest `before` cursor `operation.list` accepts, in bytes.
pub const OPERATION_CURSOR_MAX: usize = 80;
/// The most JSON bytes the summaries of one `operation.list` page add up
/// to (900 KiB), so the reply always fits in a control frame.
pub const OPERATION_PAGE_BYTES_MAX: usize = 900 * 1024;

/// Every method whose reply carries an [`OperationAccepted`] and that
/// needs an `idempotencyKey` (spec 0007, *Idempotency*). The feature that
/// owns such a method adds it here when it adds the method.
pub const STARTING_METHODS: &[&str] = &[
    "instance.create",
    "instance.start",
    "instance.stop",
    "runtime.install",
];

/// Whether `method` starts an operation, so a request without an
/// `idempotencyKey` gets `operation.idempotency_key_required`.
#[must_use]
pub fn starts_operation(method: &str) -> bool {
    STARTING_METHODS.contains(&method)
}

/// One operation as every client sees it, in replies and in both
/// operation events.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationSummary {
    /// The operation ID, a lowercase UUID v7.
    pub id: Uuid,
    /// What the operation does.
    pub kind: OperationKind,
    /// The resource the operation works on; null for `update_manager`
    /// before it names the manager.
    #[serde(default)]
    pub resource: Option<ResourceRef>,
    /// The state, one of the seven of PRD section 18.2.
    pub state: OperationState,
    /// What the operation is doing now, such as `downloading`.
    #[serde(default)]
    pub stage: Option<String>,
    /// How far the operation got.
    pub progress: OperationProgress,
    /// The operation this one waits for while queued: the oldest one that
    /// holds or waits for one of its locks.
    #[serde(default)]
    pub waiting_on: Option<Uuid>,
    /// When a cancel was asked for.
    #[serde(default)]
    pub cancel_requested_at: Option<Timestamp>,
    /// When the first step that cannot be undone started. From then on a
    /// cancel no longer stops the work.
    #[serde(default)]
    pub point_of_no_return_at: Option<Timestamp>,
    /// Why the operation failed; only in state `failed`.
    #[serde(default)]
    pub failure: Option<OperationFailureInfo>,
    /// What needs the person's action; only in state `needs_recovery`.
    #[serde(default)]
    pub recovery: Option<RecoveryInfo>,
    /// What the operation produced, once it ended or was abandoned.
    #[serde(default)]
    pub result: Option<OperationResult>,
    /// Who started the operation.
    pub trigger: OperationTriggerInfo,
    /// When the operation was created.
    pub created_at: Timestamp,
    /// When the operation first started running.
    #[serde(default)]
    pub started_at: Option<Timestamp>,
    /// When the operation last changed.
    pub updated_at: Timestamp,
    /// The operation's revision; every committed change raises it.
    pub revision: Revision,
}

/// What an operation does: the kind codes of spec 0004. Open: a newer
/// daemon may add kinds, which older clients read as `unrecognized`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// Install a managed runtime.
    InstallRuntime,
    /// Register a runtime in a folder the user owns.
    RegisterExternalRuntime,
    /// Remove a runtime.
    RemoveRuntime,
    /// Create an instance.
    CreateInstance,
    /// Start an instance.
    StartInstance,
    /// Stop an instance.
    StopInstance,
    /// Restart an instance.
    RestartInstance,
    /// Reload an instance's configuration.
    ReloadInstance,
    /// Rename an instance.
    RenameInstance,
    /// Remove an instance and keep its data.
    RemoveInstance,
    /// Delete an instance and its data.
    PurgeInstance,
    /// Move an instance to another runtime.
    RebindRuntime,
    /// Move an instance to another provider revision.
    UpdateProviderRevision,
    /// Back up an instance.
    BackupInstance,
    /// Restore an instance.
    RestoreInstance,
    /// Bring an environment up.
    EnvironmentUp,
    /// Take an environment down.
    EnvironmentDown,
    /// Update Stackly itself; its resource is the manager.
    UpdateManager,
    /// Migrate an instance's data to another line.
    MigrateData,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The state of an operation (PRD section 18.2). Closed: a new state is a
/// new protocol major.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Waiting to run, either new or resumed.
    Queued,
    /// Running.
    Running,
    /// A cancel was asked for; the operation winds down.
    CancelRequested,
    /// Finished with success; final.
    Succeeded,
    /// Finished with a failure; final.
    Failed,
    /// Cancelled; final.
    Cancelled,
    /// Stopped in a way it cannot recover from on its own; see `recovery`.
    NeedsRecovery,
}

impl OperationState {
    /// Every state, in the order of PRD section 18.2.
    pub const ALL: [Self; 7] = [
        Self::Queued,
        Self::Running,
        Self::CancelRequested,
        Self::Succeeded,
        Self::Failed,
        Self::Cancelled,
        Self::NeedsRecovery,
    ];

    /// Whether the state is final: `succeeded`, `failed`, or `cancelled`.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    /// Whether `operation.list` with `activeOnly` lists the state:
    /// `queued`, `running`, `cancel_requested`, or `needs_recovery`.
    #[must_use]
    pub const fn is_active(self) -> bool {
        !self.is_final()
    }
}

/// How far an operation got. Closed.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum OperationProgress {
    /// The work does not know how much is left.
    Indeterminate,
    /// The work counted what it did and what there is.
    Counted(CountedProgress),
}

/// Content of a `counted` progress.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CountedProgress {
    /// What is done, such as bytes downloaded.
    pub done: Count,
    /// What there is in all.
    pub total: Count,
}

/// Why an operation failed; only in state `failed`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationFailureInfo {
    /// The engine's reason code, such as `step_failed`, `interrupted`, or
    /// `abandoned`.
    pub reason: String,
    /// True when a new operation with a new idempotency key may succeed.
    /// It differs from `error.retryable`, which is about sending the same
    /// request again.
    pub retryable: bool,
    /// The error that ended the operation, with `operationId` set.
    pub error: ErrorBody,
}

/// What an operation in state `needs_recovery` needs.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryInfo {
    /// What the engine suggests.
    pub hint: RecoveryHint,
    /// Why the operation needs recovery.
    pub cause: RecoveryCause,
    /// The error behind the cause, if one was recorded.
    #[serde(default)]
    pub error: Option<ErrorBody>,
    /// The recovery methods a client may call now.
    #[serde(default)]
    pub actions: Vec<RecoveryAction>,
}

/// What the engine suggests for an operation that needs recovery. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryHint {
    /// The engine undoes the work on its own after the daemon starts.
    AutoCompensate,
    /// `operation.resume` continues the work.
    Resume,
    /// `operation.abandon` cleans up what it can and closes the operation.
    Abandon,
    /// The person has to look; nothing automatic is safe.
    Manual,
    /// A hint this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Why an operation needs recovery. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryCause {
    /// The daemon stopped in the middle of the operation.
    Interrupted,
    /// A step failed after the point of no return.
    StepFailed,
    /// Undoing what the operation created failed.
    CompensationFailed,
    /// The work panicked after the point of no return.
    Panicked,
    /// A cause this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// A recovery method a client may call for an operation. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    /// `operation.resume`.
    Resume,
    /// `operation.abandon`.
    Abandon,
    /// An action this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What an operation produced.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    /// The resources the operation created or changed.
    #[serde(default)]
    pub resources: Vec<ResourceRef>,
    /// The items that did not all go to plan; a client shows the operation
    /// as partial when this is not empty.
    #[serde(default)]
    pub partial: Vec<PartialItem>,
}

/// One item of a partial result, such as one leftover of an abandon.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartialItem {
    /// What the item is, such as a created item's reference.
    pub item: String,
    /// The resource the item belongs to, if any.
    #[serde(default)]
    pub resource: Option<ResourceRef>,
    /// What happened to the item.
    pub outcome: PartialOutcome,
    /// A catalog code that explains the outcome, if any.
    #[serde(default)]
    pub code: Option<String>,
}

/// What happened to one item of a partial result. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartialOutcome {
    /// Done, or cleaned up.
    Done,
    /// Failed, or cleaning it up failed.
    Failed,
    /// Left alone on purpose, such as an item another operation owns.
    Skipped,
    /// Still there; the person decides what to do with it.
    LeftOver,
    /// An outcome this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Who started an operation.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationTriggerInfo {
    /// A request, or the daemon itself.
    pub origin: TriggerOrigin,
    /// The kind of client that sent the request; null for the daemon.
    #[serde(default)]
    pub client: Option<ClientKind>,
}

/// Where an operation came from. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TriggerOrigin {
    /// A client request.
    User,
    /// The daemon, such as a restart policy.
    Background,
    /// An origin this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One step of an operation's journal. The step's checkpoint is never
/// sent.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationStepInfo {
    /// The step's position, from 1.
    pub seq: u32,
    /// The step's name.
    pub name: String,
    /// The step's state.
    pub state: StepState,
    /// Whether the engine may run the step again after an interruption.
    pub repeatable: bool,
    /// Whether the engine can undo the step.
    pub reversible: bool,
    /// How many times the step started, at least 1.
    pub attempts: u32,
    /// When the step last started.
    pub started_at: Timestamp,
    /// When the step ended; null while it runs.
    #[serde(default)]
    pub finished_at: Option<Timestamp>,
    /// The catalog code of the step's failure; only in state `failed`.
    #[serde(default)]
    pub error_code: Option<String>,
}

/// The state of one step. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Running.
    Running,
    /// Finished.
    Done,
    /// Failed.
    Failed,
    /// The daemon stopped while the step ran.
    Interrupted,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One thing an operation created or claimed, which a cancel or an abandon
/// may undo. A custom item's payload is never sent.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedItemInfo {
    /// The item kind, such as `instance_tree` or `secret_ref`.
    pub kind: String,
    /// The item's reference: an ID, a secret reference, or
    /// `<compensator>:<key>`.
    pub reference: String,
    /// Where the item stands.
    pub state: CreatedState,
    /// A reason code, such as `created_by_other_operation`.
    #[serde(default)]
    pub detail: Option<String>,
}

/// Where one created item stands. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CreatedState {
    /// The operation recorded that it is about to create the item.
    Intended,
    /// The item exists and belongs to the operation.
    Created,
    /// The item was undone.
    Compensated,
    /// The item stays, as part of the operation's result.
    Kept,
    /// Undoing the item failed.
    CompensationFailed,
    /// The item turned out to belong to someone else and was left alone.
    NotOwned,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What a cancel did. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelDisposition {
    /// The operation had not started and is now `cancelled`.
    CancelledBeforeStart,
    /// The work stops at its next checkpoint, then undoes what it did.
    Requested,
    /// The work already passed a step that cannot be undone, so it runs
    /// to its end.
    ContinuingPastPointOfNoReturn,
    /// The operation had already ended; nothing changed.
    AlreadyFinal,
    /// The operation needs recovery; resume or abandon it instead.
    RecoveryRequired,
    /// A disposition this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Params of `operation.get`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetOperationParams {
    /// The operation to read.
    pub operation_id: Uuid,
}

impl Validate for GetOperationParams {}

/// Reply of `operation.get`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationDetail {
    /// The operation.
    pub operation: OperationSummary,
    /// Its steps, in order.
    #[serde(default)]
    pub steps: Vec<OperationStepInfo>,
    /// What it created or claimed, in order.
    #[serde(default)]
    pub created: Vec<CreatedItemInfo>,
}

/// Params of `operation.list`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListOperationsParams {
    /// Only operations that have not ended: `queued`, `running`,
    /// `cancel_requested`, and `needs_recovery`. Absent means false.
    #[serde(default)]
    pub active_only: bool,
    /// Only operations on this resource: a runtime, an instance, an
    /// environment, or the manager (`{ kind: manager, id: "self" }`).
    #[serde(default)]
    pub resource: Option<ResourceRef>,
    /// Most operations on the page, 1 to 200; absent means 50.
    #[serde(default)]
    pub limit: Option<u32>,
    /// The `next` cursor of the previous page, sent back unchanged.
    #[serde(default)]
    pub before: Option<String>,
}

impl ListOperationsParams {
    /// The page size: `limit`, or [`OPERATION_LIST_LIMIT_DEFAULT`].
    #[must_use]
    pub fn limit_or_default(&self) -> u32 {
        self.limit.unwrap_or(OPERATION_LIST_LIMIT_DEFAULT)
    }
}

impl Validate for ListOperationsParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if self.limit.is_some_and(|limit| {
            !(OPERATION_LIST_LIMIT_MIN..=OPERATION_LIST_LIMIT_MAX).contains(&limit)
        }) {
            fields.push(FieldError::new("limit", FieldCode::OutOfRange));
        }
        if let Some(before) = &self.before {
            if before.len() > OPERATION_CURSOR_MAX {
                fields.push(FieldError::new("before", FieldCode::TooLong));
            } else if OperationCursor::parse(before).is_none() {
                fields.push(FieldError::new("before", FieldCode::InvalidFormat));
            }
        }
        if let Some(resource) = &self.resource
            && !matches!(
                resource.kind,
                ResourceKind::Runtime
                    | ResourceKind::Instance
                    | ResourceKind::Environment
                    | ResourceKind::Manager
            )
        {
            fields.push(FieldError::new("resource.kind", FieldCode::UnknownValue));
        }
        fields
    }
}

/// Reply of `operation.list`: newest first, by creation time then ID.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationPage {
    /// The operations on this page.
    #[serde(default)]
    pub items: Vec<OperationSummary>,
    /// The cursor of the next page, to send back as `before`; null on the
    /// last page.
    #[serde(default)]
    pub next: Option<String>,
}

/// The position an `operation.list` cursor names: the creation time and
/// the ID of the last operation on a page.
///
/// Its text is `<created_at>_<id>`: the time in UTC with nine fractional
/// digits (30 characters, as the registry stores it), then the lowercase
/// UUID. Clients treat the text as opaque and send it back unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationCursor {
    created_at: jiff::Timestamp,
    id: Uuid,
}

impl OperationCursor {
    /// The length of the time part of the text.
    const TIME_LEN: usize = 30;

    /// The cursor after the operation created at `created_at` with `id`.
    #[must_use]
    pub const fn new(created_at: jiff::Timestamp, id: Uuid) -> Self {
        Self { created_at, id }
    }

    /// Reads the text of a cursor, or `None` when it is not exactly the
    /// text [`Display`](fmt::Display) writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (time, id) = text.split_once('_')?;
        if time.len() != Self::TIME_LEN {
            return None;
        }
        let cursor = Self::new(time.parse().ok()?, id.parse().ok()?);
        (cursor.to_string() == text).then_some(cursor)
    }

    /// The creation time of the last operation on the page.
    #[must_use]
    pub const fn created_at(&self) -> jiff::Timestamp {
        self.created_at
    }

    /// The ID of the last operation on the page.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
}

impl fmt::Display for OperationCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.9}_{}", self.created_at, self.id)
    }
}

/// Params of `operation.cancel`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelOperationParams {
    /// The operation to cancel.
    pub operation_id: Uuid,
}

impl Validate for CancelOperationParams {}

/// Reply of `operation.cancel`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelReply {
    /// The operation after the cancel.
    pub operation: OperationSummary,
    /// What the cancel did.
    pub disposition: CancelDisposition,
}

/// Params of `operation.resume`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResumeOperationParams {
    /// The operation to resume.
    pub operation_id: Uuid,
    /// The revision the client saw; a newer one gets
    /// `resource.revision_mismatch`.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
}

impl Validate for ResumeOperationParams {}

/// Params of `operation.abandon`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AbandonOperationParams {
    /// The operation to abandon.
    pub operation_id: Uuid,
    /// The revision the person saw when they confirmed; required, so an
    /// abandon only applies to the state they looked at.
    pub expected_revision: Revision,
}

impl Validate for AbandonOperationParams {}

/// Reply of `operation.abandon`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AbandonReply {
    /// The operation, now `failed` with reason `abandoned`.
    pub operation: OperationSummary,
    /// What the abandon did not undo; also in `operation.result.partial`.
    #[serde(default)]
    pub leftovers: Vec<CreatedItemInfo>,
}

/// Reply of `operation.get_stats`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationStats {
    /// Operations running now.
    pub running: u32,
    /// Operations queued now.
    pub queued: u32,
    /// Operations that need recovery now.
    pub needs_recovery: u32,
    /// The most operations that run at once.
    pub max_running: u32,
    /// The most operations that wait in the queue.
    pub max_queued: u32,
    /// Operations the daemon found interrupted when it started.
    pub interrupted_at_start: u32,
    /// Operations the daemon resumed on its own when it started.
    pub resumed_at_start: u32,
    /// Operations the daemon undid on its own when it started.
    pub auto_compensated_at_start: u32,
    /// Requests refused with `operation.queue_full` since the daemon
    /// started.
    pub rejected_queue_full: Count,
}

/// The reply, or the part of the reply, of every method that starts an
/// operation (spec 0007, *Idempotency*). The method's own reply may wrap
/// it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationAccepted {
    /// The operation, as created or as it stands now.
    pub operation: OperationSummary,
    /// True when the idempotency key named an operation an earlier request
    /// started, so nothing new ran.
    pub replayed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPERATION: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d04";

    fn uuid() -> Uuid {
        OPERATION.parse().unwrap()
    }

    fn field_codes(params: &ListOperationsParams) -> Vec<(String, String)> {
        params
            .validate()
            .iter()
            .map(|field| (field.path().to_owned(), field.code().to_owned()))
            .collect()
    }

    #[test]
    fn list_params_accept_limits_from_1_to_200_and_default_to_50() {
        for limit in [None, Some(1), Some(50), Some(OPERATION_LIST_LIMIT_MAX)] {
            let params = ListOperationsParams {
                limit,
                ..ListOperationsParams::default()
            };
            assert!(params.validate().is_empty(), "{limit:?}");
        }
        assert_eq!(ListOperationsParams::default().limit_or_default(), 50);
        for limit in [0, OPERATION_LIST_LIMIT_MAX + 1] {
            let params = ListOperationsParams {
                limit: Some(limit),
                ..ListOperationsParams::default()
            };
            assert_eq!(
                field_codes(&params),
                [("limit".to_owned(), "out_of_range".to_owned())]
            );
        }
    }

    #[test]
    fn list_params_take_a_resource_of_the_four_kinds_with_operations() {
        for kind in [
            ResourceKind::Runtime,
            ResourceKind::Instance,
            ResourceKind::Environment,
            ResourceKind::Manager,
        ] {
            let params = ListOperationsParams {
                resource: Some(ResourceRef {
                    kind,
                    id: "x".to_owned(),
                }),
                ..ListOperationsParams::default()
            };
            assert!(params.validate().is_empty(), "{kind:?}");
        }
        for kind in [
            ResourceKind::Operation,
            ResourceKind::Provider,
            ResourceKind::Backup,
            ResourceKind::Unrecognized,
        ] {
            let params = ListOperationsParams {
                resource: Some(ResourceRef {
                    kind,
                    id: "x".to_owned(),
                }),
                ..ListOperationsParams::default()
            };
            assert_eq!(
                field_codes(&params),
                [("resource.kind".to_owned(), "unknown_value".to_owned())],
                "{kind:?}"
            );
        }
    }

    #[test]
    fn list_params_take_only_a_cursor_the_daemon_wrote() {
        let cursor =
            OperationCursor::new("2026-10-18T03:04:05.123456789Z".parse().unwrap(), uuid());
        let params = ListOperationsParams {
            before: Some(cursor.to_string()),
            ..ListOperationsParams::default()
        };
        assert!(params.validate().is_empty());

        for before in [
            String::new(),
            OPERATION.to_owned(),
            format!("2026-10-18T03:04:05.123Z_{OPERATION}"),
            format!(
                "2026-10-18T03:04:05.123456789Z_{}",
                OPERATION.to_uppercase()
            ),
            format!("2026-10-18T10:04:05.123456789+07:00_{OPERATION}"),
            format!("2026-10-18T03:04:05.123456789Z-{OPERATION}"),
        ] {
            let params = ListOperationsParams {
                before: Some(before.clone()),
                ..ListOperationsParams::default()
            };
            assert_eq!(
                field_codes(&params),
                [("before".to_owned(), "invalid_format".to_owned())],
                "{before}"
            );
        }
        let params = ListOperationsParams {
            before: Some("x".repeat(OPERATION_CURSOR_MAX + 1)),
            ..ListOperationsParams::default()
        };
        assert_eq!(
            field_codes(&params),
            [("before".to_owned(), "too_long".to_owned())]
        );
    }

    #[test]
    fn a_cursor_is_thirty_characters_of_time_an_underscore_and_the_uuid() {
        let created_at: jiff::Timestamp = "2026-10-18T03:04:05.1Z".parse().unwrap();
        let cursor = OperationCursor::new(created_at, uuid());
        let text = cursor.to_string();
        assert_eq!(text, format!("2026-10-18T03:04:05.100000000Z_{OPERATION}"));
        assert!(text.len() <= OPERATION_CURSOR_MAX);
        let back = OperationCursor::parse(&text).unwrap();
        assert_eq!(back, cursor);
        assert_eq!((back.created_at(), back.id()), (created_at, uuid()));
    }

    #[test]
    fn list_params_decode_without_active_only_as_false() {
        let params: ListOperationsParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params, ListOperationsParams::default());
        assert!(!params.active_only);
    }

    #[test]
    fn final_states_are_the_three_that_accept_no_event() {
        let finals: Vec<_> = OperationState::ALL
            .into_iter()
            .filter(|state| state.is_final())
            .collect();
        assert_eq!(
            finals,
            [
                OperationState::Succeeded,
                OperationState::Failed,
                OperationState::Cancelled
            ]
        );
        assert!(OperationState::NeedsRecovery.is_active());
    }

    #[test]
    fn progress_is_indeterminate_or_counted_with_done_and_total() {
        let counted = OperationProgress::Counted(CountedProgress {
            done: Count::new(3).unwrap(),
            total: Count::new(10).unwrap(),
        });
        assert_eq!(
            serde_json::to_value(counted).unwrap(),
            serde_json::json!({ "type": "counted", "content": { "done": 3, "total": 10 } })
        );
        assert_eq!(
            serde_json::to_value(OperationProgress::Indeterminate).unwrap(),
            serde_json::json!({ "type": "indeterminate" })
        );
    }

    #[test]
    fn foreign_values_of_the_open_operation_enums_decode_as_unrecognized() {
        fn foreign<T: serde::de::DeserializeOwned>() -> T {
            serde_json::from_str("\"from_the_future\"").unwrap()
        }
        assert_eq!(foreign::<OperationKind>(), OperationKind::Unrecognized);
        assert_eq!(foreign::<RecoveryHint>(), RecoveryHint::Unrecognized);
        assert_eq!(foreign::<RecoveryCause>(), RecoveryCause::Unrecognized);
        assert_eq!(foreign::<RecoveryAction>(), RecoveryAction::Unrecognized);
        assert_eq!(foreign::<PartialOutcome>(), PartialOutcome::Unrecognized);
        assert_eq!(foreign::<TriggerOrigin>(), TriggerOrigin::Unrecognized);
        assert_eq!(foreign::<StepState>(), StepState::Unrecognized);
        assert_eq!(foreign::<CreatedState>(), CreatedState::Unrecognized);
        assert_eq!(
            foreign::<CancelDisposition>(),
            CancelDisposition::Unrecognized
        );
        assert!(serde_json::from_str::<OperationState>("\"from_the_future\"").is_err());
    }

    #[test]
    fn every_starting_method_is_a_known_method() {
        for method in STARTING_METHODS {
            assert!(crate::Call::METHODS.contains(method), "{method}");
            assert!(starts_operation(method));
        }
        assert!(!starts_operation("operation.cancel"));
    }

    #[test]
    fn the_page_byte_limit_leaves_room_in_a_control_frame() {
        const { assert!(OPERATION_PAGE_BYTES_MAX < crate::limits::CONTROL_FRAME_MAX as usize) };
    }
}
