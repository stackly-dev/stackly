//! What a feature hands the engine to run (spec 0007, "API engine"): the
//! work, its errors, its result, and how it is rebuilt after a restart.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde_json::{Map, Value};
use stackly_domain::{
    CreatedRecord, CreatedSeq, CreatedState, DetailKey, ErrorCodeName, Generation, JournalPayload,
    JournalView, Operation, ReasonCode, Registry, ResourceRef, StageName, WriteOp,
};

use super::EngineError;
use super::context::OperationContext;
use super::limits::RESULT_MAX;

/// A boxed future that can move between threads.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What [`OperationWork::run`] returns.
pub type WorkResult = Result<WorkOutcome, WorkError>;

/// The work of one operation. The feature that owns the operation kind
/// builds it; the engine runs it once, on its own task.
pub trait OperationWork: Send + 'static {
    /// Runs the work with its context until it finishes, fails, or stops.
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult>;
}

/// What a successful work returns.
pub struct WorkOutcome {
    /// What the operation made, reported in its result.
    pub result: OperationOutcome,
    /// Builds the final registry batch. The engine runs it on a blocking
    /// thread, appends its own writes, and runs it again after a generation
    /// conflict, at most [`FINALIZE_RETRIES`](super::limits::FINALIZE_RETRIES)
    /// times.
    pub finalize: Option<Finalize>,
}

impl fmt::Debug for WorkOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkOutcome")
            .field("result", &self.result)
            .field("finalize", &self.finalize.is_some())
            .finish()
    }
}

/// Builds the final batch from what the registry holds now.
pub type Finalize = Box<dyn FnMut(&dyn Registry) -> Result<FinalBatch, WorkError> + Send>;

/// Writes a work commits, at the end or in the middle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FinalBatch {
    /// The generation the writes depend on, if any.
    pub expected_generation: Option<Generation>,
    /// The writes.
    pub ops: Vec<WriteOp>,
}

/// How an item of a partial result ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartialOutcome {
    /// It was done, or undone as asked.
    Done,
    /// It failed.
    Failed,
    /// It was left alone, for example because it was not the operation's.
    Skipped,
    /// It is still there and nobody undid it.
    LeftOver,
}

impl PartialOutcome {
    /// Returns the stable snake_case code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::LeftOver => "left_over",
        }
    }

    /// The outcome an abandoned item reports for its state (spec 0007,
    /// AC-11).
    #[must_use]
    pub const fn of_abandoned(state: CreatedState) -> Self {
        match state {
            CreatedState::Compensated | CreatedState::Kept => Self::Done,
            CreatedState::CompensationFailed => Self::Failed,
            CreatedState::NotOwned => Self::Skipped,
            CreatedState::Intended | CreatedState::Created => Self::LeftOver,
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [Self::Done, Self::Failed, Self::Skipped, Self::LeftOver]
            .into_iter()
            .find(|outcome| outcome.as_code() == code)
    }
}

/// One item of a partial result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialItem {
    /// What the item is, for example `instance_tree:<id>`.
    pub item: String,
    /// The resource it belongs to, if any.
    pub resource: Option<ResourceRef>,
    /// How it ended.
    pub outcome: PartialOutcome,
    /// A reason or error code, if any.
    pub code: Option<String>,
}

impl PartialItem {
    /// The partial item of a created record, with the outcome its state
    /// maps to.
    #[must_use]
    pub fn of_record(record: &CreatedRecord) -> Self {
        Self {
            item: format!("{}:{}", record.item().kind_code(), record.item().item_ref()),
            resource: match record.item() {
                stackly_domain::CreatedItem::InstanceTree(id) => Some(ResourceRef::Instance(*id)),
                stackly_domain::CreatedItem::EnvironmentTree(id) => {
                    Some(ResourceRef::Environment(*id))
                }
                _ => None,
            },
            outcome: PartialOutcome::of_abandoned(record.state()),
            code: record.detail().map(ToString::to_string),
        }
    }
}

/// What an operation made, stored as its journal result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationOutcome {
    /// The resources it made or changed in the end.
    pub resources: Vec<ResourceRef>,
    /// What it did only in part, for example the leftovers of an abandon.
    pub partial: Vec<PartialItem>,
}

impl OperationOutcome {
    /// Encodes the outcome as the journal stores it:
    /// `{"resources":[{"kind","id"}],"partial":[{"item","resource","outcome","code"}]}`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidUse`] for a resource kind a result
    /// cannot name, and [`EngineError::PayloadTooLarge`] past
    /// [`RESULT_MAX`].
    pub fn to_payload(&self) -> Result<JournalPayload, EngineError> {
        let resources = self
            .resources
            .iter()
            .map(resource_value)
            .collect::<Result<Vec<_>, _>>()?;
        let partial = self
            .partial
            .iter()
            .map(|item| {
                let mut row = Map::new();
                row.insert("item".to_owned(), Value::String(item.item.clone()));
                row.insert(
                    "resource".to_owned(),
                    item.resource
                        .as_ref()
                        .map(resource_value)
                        .transpose()?
                        .unwrap_or(Value::Null),
                );
                row.insert(
                    "outcome".to_owned(),
                    Value::String(item.outcome.as_code().to_owned()),
                );
                row.insert(
                    "code".to_owned(),
                    item.code.clone().map_or(Value::Null, Value::String),
                );
                Ok(Value::Object(row))
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let mut object = Map::new();
        object.insert("resources".to_owned(), Value::Array(resources));
        object.insert("partial".to_owned(), Value::Array(partial));
        let text = Value::Object(object).to_string();
        JournalPayload::new(text, RESULT_MAX).map_err(|_| EngineError::PayloadTooLarge {
            field: "result",
            max: RESULT_MAX,
        })
    }

    /// Decodes a stored result.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidUse`] when the payload is not one
    /// [`OperationOutcome::to_payload`] wrote.
    pub fn from_payload(payload: &JournalPayload) -> Result<Self, EngineError> {
        let invalid = || EngineError::InvalidUse {
            reason: "a stored result is not an operation outcome",
        };
        let value: Value = serde_json::from_str(payload.as_str()).map_err(|_| invalid())?;
        let resources = value
            .get("resources")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?
            .iter()
            .map(|value| resource_from(value).ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let partial = value
            .get("partial")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?
            .iter()
            .map(|row| {
                let item = row
                    .get("item")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?;
                let outcome = row
                    .get("outcome")
                    .and_then(Value::as_str)
                    .and_then(PartialOutcome::from_code)
                    .ok_or_else(invalid)?;
                let resource = match row.get("resource") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(resource_from(value).ok_or_else(invalid)?),
                };
                let code = row.get("code").and_then(Value::as_str).map(str::to_owned);
                Ok(PartialItem {
                    item: item.to_owned(),
                    resource,
                    outcome,
                    code,
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        Ok(Self { resources, partial })
    }
}

fn resource_value(resource: &ResourceRef) -> Result<Value, EngineError> {
    let (kind, id) = match resource {
        ResourceRef::Runtime(id) => ("runtime", id.to_string()),
        ResourceRef::Instance(id) => ("instance", id.to_string()),
        ResourceRef::Environment(id) => ("environment", id.to_string()),
        ResourceRef::Operation(id) => ("operation", id.to_string()),
        _ => {
            return Err(EngineError::InvalidUse {
                reason: "an operation result names only runtimes, instances, environments, and \
                         operations",
            });
        }
    };
    let mut row = Map::new();
    row.insert("kind".to_owned(), Value::String(kind.to_owned()));
    row.insert("id".to_owned(), Value::String(id));
    Ok(Value::Object(row))
}

fn resource_from(value: &Value) -> Option<ResourceRef> {
    let id = value.get("id")?.as_str()?;
    match value.get("kind")?.as_str()? {
        "runtime" => id.parse().ok().map(ResourceRef::Runtime),
        "instance" => id.parse().ok().map(ResourceRef::Instance),
        "environment" => id.parse().ok().map(ResourceRef::Environment),
        "operation" => id.parse().ok().map(ResourceRef::Operation),
        _ => None,
    }
}

/// An error a work returns from a failed step. Its code is a catalog code
/// of `stackly-protocol`, and its detail keys are in that code's entry.
pub trait OperationError: std::error::Error + Send + Sync + 'static {
    /// The catalog code, for example `supervisor.spawn_failed`.
    fn code(&self) -> ErrorCodeName;
    /// Whether a new operation with a new key may succeed.
    fn retryable(&self) -> bool;
    /// The details, by the keys of the catalog entry.
    fn details(&self) -> Vec<(DetailKey, String)>;
    /// The business rule the error enforces, if any.
    fn rule(&self) -> Option<&'static str> {
        None
    }
}

/// Why a work stopped without success.
pub enum WorkError {
    /// A cancel, or a shutdown stop of an operation that cannot resume,
    /// reached a checkpoint before the point of no return.
    Cancelled,
    /// Shutdown parked an operation that can resume.
    Parked,
    /// Shutdown left an operation that closes as interrupted
    /// ([`InterruptPolicy::CloseInterrupted`](stackly_domain::InterruptPolicy))
    /// to the next start: nothing more is written for it, and `recover`
    /// closes it then.
    Abandoned,
    /// A step failed.
    Failed(Box<dyn OperationError>),
    /// The engine itself failed, for example a journal write.
    Engine(EngineError),
}

impl fmt::Debug for WorkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Cancelled"),
            Self::Parked => f.write_str("Parked"),
            Self::Abandoned => f.write_str("Abandoned"),
            Self::Failed(error) => write!(f, "Failed({}: {error})", error.code()),
            Self::Engine(error) => write!(f, "Engine({error:?})"),
        }
    }
}

impl From<EngineError> for WorkError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

/// One step a work begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSpec {
    /// The step's name, unique in the operation.
    pub name: StageName,
    /// Whether it is safe to run again from its start after it stopped.
    pub repeatable: bool,
    /// Whether what it does can be undone. The first step that cannot is
    /// the point of no return.
    pub reversible: bool,
}

impl StepSpec {
    /// A step that is reversible and not repeatable.
    #[must_use]
    pub const fn new(name: StageName) -> Self {
        Self {
            name,
            repeatable: false,
            reversible: true,
        }
    }

    /// Marks the step safe to run again from its start.
    #[must_use]
    pub const fn repeatable(mut self) -> Self {
        self.repeatable = true;
        self
    }

    /// Marks the step as one that cannot be undone.
    #[must_use]
    pub const fn irreversible(mut self) -> Self {
        self.reversible = false;
        self
    }
}

/// What [`OperationContext::begin_step`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStart {
    /// Run the step; `attempt` is 1, more when a repeatable step runs again.
    Run {
        /// Which attempt this is.
        attempt: u32,
    },
    /// The step already finished before a restart; skip it.
    AlreadyDone {
        /// What it handed on.
        checkpoint: Option<JournalPayload>,
    },
}

/// A created item the operation recorded, to confirm or decline later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CreatedHandle {
    pub(crate) seq: CreatedSeq,
}

/// Builds the work of an operation again from its journal, after a restart
/// or for a resume. Every operation kind registers one.
pub trait WorkFactory: Send + Sync + 'static {
    /// Rebuilds the work from the operation and its journal input.
    ///
    /// # Errors
    ///
    /// Returns [`RebuildError`] when the operation cannot be rebuilt, for
    /// example because its params held a secret value.
    fn rebuild(
        &self,
        operation: &Operation,
        journal: &JournalView,
    ) -> Result<Box<dyn OperationWork>, RebuildError>;
}

/// Why a work cannot be rebuilt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RebuildError {
    /// The operation can run only in the daemon run that created it.
    #[error("the operation cannot be rebuilt: {reason}")]
    NotRebuildable {
        /// Why.
        reason: ReasonCode,
    },
    /// The stored input does not decode.
    #[error("the stored input of the operation is invalid")]
    InvalidInput,
}

#[cfg(test)]
mod tests {
    use stackly_domain::{InstanceId, RuntimeId};

    use super::*;

    #[test]
    fn an_outcome_round_trips_through_its_payload() {
        let outcome = OperationOutcome {
            resources: vec![
                ResourceRef::Instance(InstanceId::generate()),
                ResourceRef::Runtime(RuntimeId::generate()),
            ],
            partial: vec![PartialItem {
                item: "instance_tree:x".to_owned(),
                resource: Some(ResourceRef::Instance(InstanceId::generate())),
                outcome: PartialOutcome::LeftOver,
                code: Some("permission_denied".to_owned()),
            }],
        };

        let payload = outcome.to_payload().unwrap();

        assert_eq!(OperationOutcome::from_payload(&payload).unwrap(), outcome);
        let empty: Value =
            serde_json::from_str(OperationOutcome::default().to_payload().unwrap().as_str())
                .unwrap();
        assert_eq!(empty, serde_json::json!({ "resources": [], "partial": [] }));
    }

    #[test]
    fn abandoned_items_map_their_state_to_an_outcome() {
        assert_eq!(
            PartialOutcome::of_abandoned(CreatedState::Compensated),
            PartialOutcome::Done
        );
        assert_eq!(
            PartialOutcome::of_abandoned(CreatedState::CompensationFailed),
            PartialOutcome::Failed
        );
        assert_eq!(
            PartialOutcome::of_abandoned(CreatedState::NotOwned),
            PartialOutcome::Skipped
        );
        assert_eq!(
            PartialOutcome::of_abandoned(CreatedState::Created),
            PartialOutcome::LeftOver
        );
    }
}
