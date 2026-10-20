//! The operation journal (spec 0007, "Model domain"): what an operation
//! was asked to do, the steps it ran, and what it created, so the engine
//! can resume, compensate, or report it after a crash.
//!
//! Every value checks the rules of the registry's journal tables in its
//! constructor, so a value that exists can always be stored.

use std::fmt;

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{
    InvariantDetail, InvariantError, JournalError, Machine, TransitionError, ValueError, ValueKind,
};
use crate::id::{EnvironmentId, InstanceId, OperationId};
use crate::operation::lock::LockKey;
use crate::rule::Rule;
use crate::secret::SecretRef;
use crate::value::{
    ClientCode, CompensatorName, CustomItemKey, DetailKey, ErrorCodeName, MethodName, ReasonCode,
    ResourceRevision, Sha256Digest, StageName,
};

/// The text of one JSON object kept in the journal: an operation's input
/// or result, a step checkpoint, or a custom item's payload. It never holds
/// a secret value.
///
/// It checks only the shape: 2 or more bytes, starting with `{` and ending
/// with `}`, within the limit the caller picks. The code that serializes
/// it guarantees valid JSON, and the SQLite registry checks again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JournalPayload(String);

impl JournalPayload {
    /// Wraps the JSON object text `text`, at most `max` bytes long.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when the text is not braced, shorter than 2
    /// bytes, or longer than `max`.
    pub fn new(text: impl Into<String>, max: usize) -> Result<Self, ValueError> {
        let text = text.into();
        if (2..=max).contains(&text.len()) && text.starts_with('{') && text.ends_with('}') {
            Ok(Self(text))
        } else {
            Err(ValueError::new(
                ValueKind::JournalPayload,
                "must be the text of a JSON object within its byte limit",
            ))
        }
    }

    /// The empty object `{}`.
    #[must_use]
    pub fn empty() -> Self {
        Self("{}".to_owned())
    }

    /// Returns the JSON text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the length in bytes.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for JournalPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Fails when `payload` is longer than `field` allows.
fn check_len(
    field: &'static str,
    payload: &JournalPayload,
    max: usize,
) -> Result<(), JournalError> {
    if payload.byte_len() <= max {
        Ok(())
    } else {
        Err(JournalError::PayloadTooLarge {
            field,
            len: payload.byte_len(),
            max,
        })
    }
}

/// Defines a sequence number newtype over `u16`, from 1 to `$max`.
macro_rules! seq_value {
    ($(#[$meta:meta])* $name:ident, $kind:ident, $max:literal, $rule:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u16);

        impl $name {
            /// The first number.
            pub const FIRST: Self = Self(1);
            /// The largest number.
            pub const MAX: u16 = $max;

            /// Wraps a number.
            ///
            /// # Errors
            ///
            /// Returns a [`ValueError`] outside 1 to `MAX`.
            pub const fn new(number: u16) -> Result<Self, ValueError> {
                if number >= 1 && number <= $max {
                    Ok(Self(number))
                } else {
                    Err(ValueError::new(ValueKind::$kind, $rule))
                }
            }

            /// Returns the number.
            #[must_use]
            pub const fn get(self) -> u16 {
                self.0
            }

            /// Returns the next number, or `None` after the largest.
            #[must_use]
            pub const fn next(self) -> Option<Self> {
                if self.0 < $max { Some(Self(self.0 + 1)) } else { None }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

seq_value! {
    /// The number of a step in its operation, 1 to 256, in the order the
    /// steps began.
    StepSeq, StepSeq, 256, "must be 1 to 256"
}

seq_value! {
    /// The number of a created item in its operation, 1 to 1024, in the
    /// order the operation recorded them.
    CreatedSeq, CreatedSeq, 1024, "must be 1 to 1024"
}

code_enum! {
    /// Who started an operation.
    pub enum TriggerOrigin: TriggerOrigin {
        /// A client request.
        User => "user",
        /// The daemon itself, for example a restart policy.
        Background => "background",
    }
}

/// Who started an operation and whether it may ask the user anything.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationTrigger {
    origin: TriggerOrigin,
    client: Option<ClientCode>,
    interactive: bool,
}

impl OperationTrigger {
    /// A trigger from a client request: the client's kind from its
    /// handshake, and whether it can answer a prompt.
    #[must_use]
    pub const fn user(client: Option<ClientCode>, interactive: bool) -> Self {
        Self {
            origin: TriggerOrigin::User,
            client,
            interactive,
        }
    }

    /// A trigger from the daemon itself, which never prompts.
    #[must_use]
    pub const fn background() -> Self {
        Self {
            origin: TriggerOrigin::Background,
            client: None,
            interactive: false,
        }
    }

    /// Rebuilds a stored trigger.
    #[must_use]
    pub const fn new(origin: TriggerOrigin, client: Option<ClientCode>, interactive: bool) -> Self {
        Self {
            origin,
            client,
            interactive,
        }
    }

    /// Returns who started the operation.
    #[must_use]
    pub const fn origin(&self) -> TriggerOrigin {
        self.origin
    }

    /// Returns the client's kind, if a client started it.
    #[must_use]
    pub const fn client(&self) -> Option<&ClientCode> {
        self.client.as_ref()
    }

    /// Returns whether the client said it can answer a prompt.
    #[must_use]
    pub const fn interactive(&self) -> bool {
        self.interactive
    }

    /// Returns whether the work may show a prompt (spec 0007 calls it
    /// `interaction()`): only for a user request from an interactive
    /// client. Spec 0011 maps it to its `Interaction` value.
    #[must_use]
    pub const fn allows_prompt(&self) -> bool {
        matches!(self.origin, TriggerOrigin::User) && self.interactive
    }
}

code_enum! {
    /// Why an operation entered `needs_recovery`.
    pub enum RecoveryCause: RecoveryCause {
        /// The daemon stopped while it ran.
        Interrupted => "interrupted",
        /// A step failed after the point of no return.
        StepFailed => "step_failed",
        /// Undoing what it created failed.
        CompensationFailed => "compensation_failed",
        /// The work panicked after the point of no return.
        Panicked => "panicked",
    }
}

/// One entry of a [`FailureError`] list, such as one missing secret or one
/// invalid setting: a reason kind and up to [`FailureItem::FIELDS_MAX`]
/// fields. It has the shape of the wire `ErrorItem` of spec 0006.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FailureItem {
    kind: ReasonCode,
    fields: Vec<(DetailKey, String)>,
}

impl FailureItem {
    /// The most fields one item holds.
    pub const FIELDS_MAX: usize = 8;

    /// Bundles an item. The caller redacts the field values first.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when there are more than
    /// [`FailureItem::FIELDS_MAX`] fields or a field value is longer than
    /// [`FailureError::DETAIL_VALUE_MAX`] bytes.
    pub fn new(kind: ReasonCode, fields: Vec<(DetailKey, String)>) -> Result<Self, ValueError> {
        if fields.len() > Self::FIELDS_MAX {
            return Err(ValueError::new(
                ValueKind::FailureItem,
                "an item holds at most 8 fields",
            ));
        }
        if fields
            .iter()
            .any(|(_, value)| value.len() > FailureError::DETAIL_VALUE_MAX)
        {
            return Err(ValueError::new(
                ValueKind::FailureItem,
                "a field value must be at most 512 bytes",
            ));
        }
        Ok(Self { kind, fields })
    }

    /// Returns the item kind.
    #[must_use]
    pub const fn kind(&self) -> &ReasonCode {
        &self.kind
    }

    /// Returns the fields, in order.
    #[must_use]
    pub fn fields(&self) -> &[(DetailKey, String)] {
        &self.fields
    }
}

/// The error an operation stored: a catalog code, a redacted message,
/// details, and the items and business rule the error carried. It never
/// holds a hint: `stacklyd` computes that from the code and details.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FailureError {
    code: ErrorCodeName,
    message: String,
    details: Vec<(DetailKey, String)>,
    items: Vec<FailureItem>,
    rule: Option<Rule>,
}

impl FailureError {
    /// The longest message, in characters.
    pub const MESSAGE_MAX: usize = 200;
    /// The longest detail value, in bytes.
    pub const DETAIL_VALUE_MAX: usize = 512;
    /// The most items one error holds.
    pub const ITEMS_MAX: usize = 100;

    /// Bundles an error without items or rule. The caller redacts `message`
    /// and the detail values first.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when the message is empty or longer than
    /// [`FailureError::MESSAGE_MAX`] characters, or a detail value is
    /// longer than [`FailureError::DETAIL_VALUE_MAX`] bytes.
    pub fn new(
        code: ErrorCodeName,
        message: impl Into<String>,
        details: Vec<(DetailKey, String)>,
    ) -> Result<Self, ValueError> {
        let message = message.into();
        if !(1..=Self::MESSAGE_MAX).contains(&message.chars().count()) {
            return Err(ValueError::new(
                ValueKind::FailureError,
                "the message must be 1 to 200 characters",
            ));
        }
        if details
            .iter()
            .any(|(_, value)| value.len() > Self::DETAIL_VALUE_MAX)
        {
            return Err(ValueError::new(
                ValueKind::FailureError,
                "a detail value must be at most 512 bytes",
            ));
        }
        Ok(Self {
            code,
            message,
            details,
            items: Vec::new(),
            rule: None,
        })
    }

    /// Adds the items the error carried.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when there are more than
    /// [`FailureError::ITEMS_MAX`] items.
    pub fn with_items(mut self, items: Vec<FailureItem>) -> Result<Self, ValueError> {
        if items.len() > Self::ITEMS_MAX {
            return Err(ValueError::new(
                ValueKind::FailureError,
                "an error holds at most 100 items",
            ));
        }
        self.items = items;
        Ok(self)
    }

    /// Adds the business rule the error was rejected under.
    #[must_use]
    pub fn with_rule(mut self, rule: Rule) -> Self {
        self.rule = Some(rule);
        self
    }

    /// Returns the catalog code.
    #[must_use]
    pub const fn code(&self) -> &ErrorCodeName {
        &self.code
    }

    /// Returns the redacted message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the details, in order.
    #[must_use]
    pub fn details(&self) -> &[(DetailKey, String)] {
        &self.details
    }

    /// Returns the items, in order.
    #[must_use]
    pub fn items(&self) -> &[FailureItem] {
        &self.items
    }

    /// Returns the business rule, if the error carried one.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        self.rule
    }
}

/// The values a new [`OperationJournal`] starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJournal {
    /// The operation the journal belongs to.
    pub operation_id: OperationId,
    /// The wire method and request fingerprint of a client request, or
    /// `None` for a background operation.
    pub request: Option<(MethodName, Sha256Digest)>,
    /// Who started it.
    pub trigger: OperationTrigger,
    /// [`OperationKind::resumable`](crate::OperationKind::resumable) when
    /// it was created, frozen for the operation.
    pub resumable: bool,
    /// What the work needs to be rebuilt, without secret values.
    pub input: JournalPayload,
    /// The resource revision the request expects, if any.
    pub expected_revision: Option<ResourceRevision>,
}

/// Every field of an [`OperationJournal`] as the registry stores it, for
/// [`OperationJournal::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJournal {
    /// The operation.
    pub operation_id: OperationId,
    /// The wire method.
    pub method: Option<MethodName>,
    /// The request fingerprint.
    pub fingerprint: Option<Sha256Digest>,
    /// Who started it.
    pub trigger: OperationTrigger,
    /// Whether it can resume.
    pub resumable: bool,
    /// Its input.
    pub input: JournalPayload,
    /// The expected resource revision.
    pub expected_revision: Option<ResourceRevision>,
    /// The operation it waits on.
    pub waiting_on: Option<OperationId>,
    /// When its first irreversible step began.
    pub point_of_no_return_at: Option<Timestamp>,
    /// When a cancel was asked for.
    pub cancel_requested_at: Option<Timestamp>,
    /// When compensation began.
    pub compensation_started_at: Option<Timestamp>,
    /// Why it is in `needs_recovery`.
    pub recovery_cause: Option<RecoveryCause>,
    /// Its stored error.
    pub failure_error: Option<FailureError>,
    /// Its result.
    pub result: Option<JournalPayload>,
}

/// The journal row of one operation (1:1 with it): the request it came
/// from, its input, and the times and causes the engine records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationJournal {
    operation_id: OperationId,
    request: Option<(MethodName, Sha256Digest)>,
    trigger: OperationTrigger,
    resumable: bool,
    input: JournalPayload,
    expected_revision: Option<ResourceRevision>,
    waiting_on: Option<OperationId>,
    point_of_no_return_at: Option<Timestamp>,
    cancel_requested_at: Option<Timestamp>,
    compensation_started_at: Option<Timestamp>,
    recovery_cause: Option<RecoveryCause>,
    failure_error: Option<FailureError>,
    result: Option<JournalPayload>,
}

impl OperationJournal {
    /// The longest input, in bytes.
    pub const INPUT_MAX: usize = 16_384;
    /// The longest result, in bytes.
    pub const RESULT_MAX: usize = 16_384;

    /// Starts the journal of a new operation, with every recorded time and
    /// cause empty.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::PayloadTooLarge`] when the input is longer
    /// than [`OperationJournal::INPUT_MAX`].
    pub fn new(new: NewJournal) -> Result<Self, JournalError> {
        check_len("input", &new.input, Self::INPUT_MAX)?;
        Ok(Self {
            operation_id: new.operation_id,
            request: new.request,
            trigger: new.trigger,
            resumable: new.resumable,
            input: new.input,
            expected_revision: new.expected_revision,
            waiting_on: None,
            point_of_no_return_at: None,
            cancel_requested_at: None,
            compensation_started_at: None,
            recovery_cause: None,
            failure_error: None,
            result: None,
        })
    }

    /// Rebuilds a journal the registry stored. Only the registry and test
    /// fakes call it.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] when only one of method and
    /// fingerprint is set, or the input or result is over its limit.
    pub fn restore(stored: StoredJournal) -> Result<Self, InvariantError> {
        let request = match (stored.method, stored.fingerprint) {
            (Some(method), Some(fingerprint)) => Some((method, fingerprint)),
            (None, None) => None,
            _ => {
                return Err(inconsistent(
                    "operation journal",
                    "method and fingerprint are not set together",
                ));
            }
        };
        if stored.input.byte_len() > Self::INPUT_MAX
            || stored
                .result
                .as_ref()
                .is_some_and(|result| result.byte_len() > Self::RESULT_MAX)
        {
            return Err(inconsistent(
                "operation journal",
                "the input or result is over its limit",
            ));
        }
        Ok(Self {
            operation_id: stored.operation_id,
            request,
            trigger: stored.trigger,
            resumable: stored.resumable,
            input: stored.input,
            expected_revision: stored.expected_revision,
            waiting_on: stored.waiting_on,
            point_of_no_return_at: stored.point_of_no_return_at,
            cancel_requested_at: stored.cancel_requested_at,
            compensation_started_at: stored.compensation_started_at,
            recovery_cause: stored.recovery_cause,
            failure_error: stored.failure_error,
            result: stored.result,
        })
    }

    /// Returns the operation.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Returns the wire method of the request, if a client sent one.
    #[must_use]
    pub fn method(&self) -> Option<&MethodName> {
        self.request.as_ref().map(|(method, _)| method)
    }

    /// Returns the fingerprint of the request, if a client sent one.
    #[must_use]
    pub fn fingerprint(&self) -> Option<&Sha256Digest> {
        self.request.as_ref().map(|(_, fingerprint)| fingerprint)
    }

    /// Returns who started it.
    #[must_use]
    pub const fn trigger(&self) -> &OperationTrigger {
        &self.trigger
    }

    /// Returns whether it can resume, as frozen when it was created.
    #[must_use]
    pub const fn resumable(&self) -> bool {
        self.resumable
    }

    /// Returns its input.
    #[must_use]
    pub const fn input(&self) -> &JournalPayload {
        &self.input
    }

    /// Returns the resource revision the request expects, if any.
    #[must_use]
    pub const fn expected_revision(&self) -> Option<ResourceRevision> {
        self.expected_revision
    }

    /// Returns the oldest operation it waits on, while it waits for a lock.
    #[must_use]
    pub const fn waiting_on(&self) -> Option<OperationId> {
        self.waiting_on
    }

    /// Returns when its first irreversible step began.
    #[must_use]
    pub const fn point_of_no_return_at(&self) -> Option<Timestamp> {
        self.point_of_no_return_at
    }

    /// Returns when a cancel was first asked for.
    #[must_use]
    pub const fn cancel_requested_at(&self) -> Option<Timestamp> {
        self.cancel_requested_at
    }

    /// Returns when compensation began.
    #[must_use]
    pub const fn compensation_started_at(&self) -> Option<Timestamp> {
        self.compensation_started_at
    }

    /// Returns why it is in `needs_recovery`.
    #[must_use]
    pub const fn recovery_cause(&self) -> Option<RecoveryCause> {
        self.recovery_cause
    }

    /// Returns its stored error: the final error of a `failed` operation,
    /// or the cause of `needs_recovery`.
    #[must_use]
    pub const fn failure_error(&self) -> Option<&FailureError> {
        self.failure_error.as_ref()
    }

    /// Returns its result.
    #[must_use]
    pub const fn result(&self) -> Option<&JournalPayload> {
        self.result.as_ref()
    }

    /// Sets the operation it waits on, or clears it.
    pub const fn set_waiting_on(&mut self, waiting_on: Option<OperationId>) {
        self.waiting_on = waiting_on;
    }

    /// Records that its first irreversible step began at `at`. A later call
    /// keeps the first time.
    pub fn mark_point_of_no_return(&mut self, at: Timestamp) {
        self.point_of_no_return_at.get_or_insert(at);
    }

    /// Records that a cancel was asked for at `at`. A later call keeps the
    /// first time.
    pub fn mark_cancel_requested(&mut self, at: Timestamp) {
        self.cancel_requested_at.get_or_insert(at);
    }

    /// Records that compensation began at `at`. A later call keeps the
    /// first time, so a compensated operation never resumes.
    pub fn mark_compensation_started(&mut self, at: Timestamp) {
        self.compensation_started_at.get_or_insert(at);
    }

    /// Sets why it is in `needs_recovery`, or clears it on resume.
    pub const fn set_recovery_cause(&mut self, cause: Option<RecoveryCause>) {
        self.recovery_cause = cause;
    }

    /// Sets its stored error, or clears it.
    pub fn set_failure_error(&mut self, error: Option<FailureError>) {
        self.failure_error = error;
    }

    /// Sets its result, or clears it.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::PayloadTooLarge`] when the result is longer
    /// than [`OperationJournal::RESULT_MAX`]; the journal does not change.
    pub fn set_result(&mut self, result: Option<JournalPayload>) -> Result<(), JournalError> {
        if let Some(result) = &result {
            check_len("result", result, Self::RESULT_MAX)?;
        }
        self.result = result;
        Ok(())
    }
}

code_enum! {
    /// Where one step of an operation stands.
    pub enum StepState: StepState {
        /// It runs now.
        Running => "running",
        /// It finished.
        Done => "done",
        /// It failed.
        Failed => "failed",
        /// The daemon stopped while it ran.
        Interrupted => "interrupted",
    }
}

/// The values a step starts from in [`JournalStep::begin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStep {
    /// Its number, the next one in the operation.
    pub seq: StepSeq,
    /// Its name, unique in the operation.
    pub name: StageName,
    /// Whether it is safe to run again from the start after it stopped.
    pub repeatable: bool,
    /// Whether what it does can be undone; the first step that cannot is
    /// the point of no return.
    pub reversible: bool,
}

/// Every field of a [`JournalStep`] as the registry stores it, for
/// [`JournalStep::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStep {
    /// Its number.
    pub seq: StepSeq,
    /// Its name.
    pub name: StageName,
    /// Whether it can run again.
    pub repeatable: bool,
    /// Whether it can be undone.
    pub reversible: bool,
    /// Where it stands.
    pub state: StepState,
    /// How often it began.
    pub attempts: u32,
    /// What it handed to the next step.
    pub checkpoint: Option<JournalPayload>,
    /// The code it failed with.
    pub error_code: Option<ErrorCodeName>,
    /// When its last attempt began.
    pub started_at: Timestamp,
    /// When its last attempt ended.
    pub finished_at: Option<Timestamp>,
}

/// One step of an operation, as the journal records it.
///
/// It runs from `running` to `done`, `failed`, or `interrupted`, and only a
/// repeatable step runs again (`retry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalStep {
    seq: StepSeq,
    name: StageName,
    repeatable: bool,
    reversible: bool,
    state: StepState,
    attempts: u32,
    checkpoint: Option<JournalPayload>,
    error_code: Option<ErrorCodeName>,
    started_at: Timestamp,
    finished_at: Option<Timestamp>,
}

impl JournalStep {
    /// The longest checkpoint, in bytes.
    pub const CHECKPOINT_MAX: usize = 4096;

    /// Begins a step at `at`: `running`, first attempt.
    #[must_use]
    pub fn begin(new: NewStep, at: Timestamp) -> Self {
        Self {
            seq: new.seq,
            name: new.name,
            repeatable: new.repeatable,
            reversible: new.reversible,
            state: StepState::Running,
            attempts: 1,
            checkpoint: None,
            error_code: None,
            started_at: at,
            finished_at: None,
        }
    }

    /// Rebuilds a step the registry stored. Only the registry and test
    /// fakes call it.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] when `finished_at` is set
    /// while running or missing otherwise, an error code is set outside
    /// `failed`, a step that is not repeatable began more than once,
    /// `attempts` is 0, or the checkpoint is over its limit.
    pub fn restore(stored: StoredStep) -> Result<Self, InvariantError> {
        let running = stored.state == StepState::Running;
        let reason = if running == stored.finished_at.is_some() {
            Some("finished_at is set exactly when the step is not running")
        } else if stored.error_code.is_some() && stored.state != StepState::Failed {
            Some("an error code is set outside failed")
        } else if stored.attempts == 0 || (!stored.repeatable && stored.attempts > 1) {
            Some("a step began 0 times, or more than once without being repeatable")
        } else if stored
            .checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.byte_len() > Self::CHECKPOINT_MAX)
        {
            Some("the checkpoint is over its limit")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(inconsistent("operation step", reason));
        }
        Ok(Self {
            seq: stored.seq,
            name: stored.name,
            repeatable: stored.repeatable,
            reversible: stored.reversible,
            state: stored.state,
            attempts: stored.attempts,
            checkpoint: stored.checkpoint,
            error_code: stored.error_code,
            started_at: stored.started_at,
            finished_at: stored.finished_at,
        })
    }

    /// Returns its number.
    #[must_use]
    pub const fn seq(&self) -> StepSeq {
        self.seq
    }

    /// Returns its name.
    #[must_use]
    pub const fn name(&self) -> &StageName {
        &self.name
    }

    /// Returns whether it can run again from the start.
    #[must_use]
    pub const fn repeatable(&self) -> bool {
        self.repeatable
    }

    /// Returns whether it can be undone.
    #[must_use]
    pub const fn reversible(&self) -> bool {
        self.reversible
    }

    /// Returns where it stands.
    #[must_use]
    pub const fn state(&self) -> StepState {
        self.state
    }

    /// Returns how often it began: 1, more only for a repeatable step.
    #[must_use]
    pub const fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Returns what it handed on when it finished.
    #[must_use]
    pub const fn checkpoint(&self) -> Option<&JournalPayload> {
        self.checkpoint.as_ref()
    }

    /// Returns the code it failed with.
    #[must_use]
    pub const fn error_code(&self) -> Option<&ErrorCodeName> {
        self.error_code.as_ref()
    }

    /// Returns when its last attempt began.
    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// Returns when its last attempt ended, or `None` while it runs.
    #[must_use]
    pub const fn finished_at(&self) -> Option<Timestamp> {
        self.finished_at
    }

    /// Finishes the running step at `at` with what it hands on.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::Transition`] when it is not running, and
    /// [`JournalError::PayloadTooLarge`] for a checkpoint longer than
    /// [`JournalStep::CHECKPOINT_MAX`]. The step does not change.
    pub fn finish(
        &mut self,
        checkpoint: Option<JournalPayload>,
        at: Timestamp,
    ) -> Result<(), JournalError> {
        self.expect_running("finish")?;
        if let Some(checkpoint) = &checkpoint {
            check_len("checkpoint", checkpoint, Self::CHECKPOINT_MAX)?;
        }
        self.checkpoint = checkpoint;
        self.end(StepState::Done, at);
        Ok(())
    }

    /// Records that the running step failed at `at` with `code`.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::Transition`] when it is not running; the
    /// step does not change.
    pub fn fail(&mut self, code: ErrorCodeName, at: Timestamp) -> Result<(), JournalError> {
        self.expect_running("fail")?;
        self.error_code = Some(code);
        self.end(StepState::Failed, at);
        Ok(())
    }

    /// Records that the daemon stopped while the step ran.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::Transition`] when it is not running; the
    /// step does not change.
    pub fn interrupt(&mut self, at: Timestamp) -> Result<(), JournalError> {
        self.expect_running("interrupt")?;
        self.end(StepState::Interrupted, at);
        Ok(())
    }

    /// Runs a failed or interrupted step again from the start at `at`, as
    /// its next attempt. The checkpoint stays.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::NotRepeatable`] for a step that is not
    /// repeatable, and [`JournalError::Transition`] when it is running or
    /// done. The step does not change.
    pub fn retry(&mut self, at: Timestamp) -> Result<(), JournalError> {
        if !matches!(self.state, StepState::Failed | StepState::Interrupted) {
            return Err(self.refuse("retry"));
        }
        if !self.repeatable {
            return Err(JournalError::NotRepeatable {
                seq: self.seq.get(),
            });
        }
        self.state = StepState::Running;
        self.attempts = self.attempts.saturating_add(1);
        self.error_code = None;
        self.started_at = at;
        self.finished_at = None;
        Ok(())
    }

    fn expect_running(&self, event: &'static str) -> Result<(), JournalError> {
        if self.state == StepState::Running {
            Ok(())
        } else {
            Err(self.refuse(event))
        }
    }

    fn refuse(&self, event: &'static str) -> JournalError {
        TransitionError::new(Machine::JournalStep, self.state.as_code(), event).into()
    }

    const fn end(&mut self, state: StepState, at: Timestamp) {
        self.state = state;
        self.finished_at = Some(at);
    }
}

/// Something an operation created and may have to undo. The engine
/// derives every path to delete from the IDs here, never from a stored
/// path.
///
/// Its stored form is a kind (`instance_tree`, `environment_tree`,
/// `secret_ref`, `custom`) and a reference: the ID, the reference text, or
/// `<compensator>:<key>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum CreatedItem {
    /// An instance's folders, by its ID.
    InstanceTree(InstanceId),
    /// An environment's folder, by its ID.
    EnvironmentTree(EnvironmentId),
    /// A secret reference and the store item behind it (spec 0011), which
    /// the `secret_ref` compensator the daemon registers removes.
    SecretRef(SecretRef),
    /// Anything a feature's own compensator undoes, by its key.
    Custom {
        /// The compensator that undoes it.
        compensator: CompensatorName,
        /// What the compensator undoes.
        key: CustomItemKey,
    },
}

impl CreatedItem {
    /// Returns the stored kind, for example `instance_tree`.
    #[must_use]
    pub const fn kind_code(&self) -> &'static str {
        match self {
            Self::InstanceTree(_) => "instance_tree",
            Self::EnvironmentTree(_) => "environment_tree",
            Self::SecretRef(_) => "secret_ref",
            Self::Custom { .. } => "custom",
        }
    }

    /// Returns the stored reference: the ID, the reference text, or
    /// `<compensator>:<key>`.
    #[must_use]
    pub fn item_ref(&self) -> String {
        match self {
            Self::InstanceTree(id) => id.to_string(),
            Self::EnvironmentTree(id) => id.to_string(),
            Self::SecretRef(reference) => reference.to_string(),
            Self::Custom { compensator, key } => format!("{compensator}:{key}"),
        }
    }

    /// Rebuilds an item from its stored kind and reference.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] for an unknown kind or a reference that
    /// does not fit it.
    pub fn from_parts(kind: &str, item_ref: &str) -> Result<Self, ValueError> {
        let invalid = || {
            ValueError::new(
                ValueKind::CreatedItem,
                "must be a known kind with a matching reference",
            )
        };
        match kind {
            "instance_tree" => Ok(Self::InstanceTree(item_ref.parse().map_err(|_| invalid())?)),
            "environment_tree" => Ok(Self::EnvironmentTree(
                item_ref.parse().map_err(|_| invalid())?,
            )),
            "secret_ref" => Ok(Self::SecretRef(
                SecretRef::parse(item_ref).map_err(|_| invalid())?,
            )),
            "custom" => {
                let (compensator, key) = item_ref.split_once(':').ok_or_else(invalid)?;
                Ok(Self::Custom {
                    compensator: compensator.parse()?,
                    key: key.parse()?,
                })
            }
            _ => Err(invalid()),
        }
    }
}

code_enum! {
    /// Where one created item stands.
    pub enum CreatedState: CreatedState {
        /// The operation is about to create it.
        Intended => "intended",
        /// The operation created it.
        Created => "created",
        /// Compensation undid it, or found it already gone.
        Compensated => "compensated",
        /// The operation succeeded and it stays.
        Kept => "kept",
        /// Undoing it failed.
        CompensationFailed => "compensation_failed",
        /// It turned out not to be this operation's, so it was left alone.
        NotOwned => "not_owned",
    }
}

impl CreatedState {
    /// Returns whether the item is settled: anything but `intended` and
    /// `created`.
    #[must_use]
    pub const fn is_settled(self) -> bool {
        !matches!(self, Self::Intended | Self::Created)
    }
}

/// Every field of a [`CreatedRecord`] as the registry stores it, for
/// [`CreatedRecord::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCreatedRecord {
    /// Its number.
    pub seq: CreatedSeq,
    /// The step that created it.
    pub step_seq: StepSeq,
    /// The item.
    pub item: CreatedItem,
    /// The custom compensator's payload.
    pub payload: Option<JournalPayload>,
    /// Where it stands.
    pub state: CreatedState,
    /// Why it settled as it did.
    pub detail: Option<ReasonCode>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
    /// When it settled.
    pub settled_at: Option<Timestamp>,
}

/// One item an operation recorded as created, written before the item is
/// made (`intended`) so a crash in between never loses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedRecord {
    seq: CreatedSeq,
    step_seq: StepSeq,
    item: CreatedItem,
    payload: Option<JournalPayload>,
    state: CreatedState,
    detail: Option<ReasonCode>,
    recorded_at: Timestamp,
    settled_at: Option<Timestamp>,
}

impl CreatedRecord {
    /// The longest custom payload, in bytes.
    pub const PAYLOAD_MAX: usize = 4096;

    /// Records at `at` that step `step_seq` is about to create `item`.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::PayloadNotAllowed`] for a payload on an item
    /// that is not custom, and [`JournalError::PayloadTooLarge`] for one
    /// longer than [`CreatedRecord::PAYLOAD_MAX`].
    pub fn intend(
        seq: CreatedSeq,
        step_seq: StepSeq,
        item: CreatedItem,
        payload: Option<JournalPayload>,
        at: Timestamp,
    ) -> Result<Self, JournalError> {
        if let Some(payload) = &payload {
            if !matches!(item, CreatedItem::Custom { .. }) {
                return Err(JournalError::PayloadNotAllowed);
            }
            check_len("payload", payload, Self::PAYLOAD_MAX)?;
        }
        Ok(Self {
            seq,
            step_seq,
            item,
            payload,
            state: CreatedState::Intended,
            detail: None,
            recorded_at: at,
            settled_at: None,
        })
    }

    /// Rebuilds a record the registry stored. Only the registry and test
    /// fakes call it.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] when `settled_at` does not
    /// match the state, or a payload is on an item that is not custom or
    /// over its limit.
    pub fn restore(stored: StoredCreatedRecord) -> Result<Self, InvariantError> {
        let reason = if stored.state.is_settled() != stored.settled_at.is_some() {
            Some("settled_at is set exactly when the item is settled")
        } else if stored.payload.as_ref().is_some_and(|payload| {
            !matches!(stored.item, CreatedItem::Custom { .. })
                || payload.byte_len() > Self::PAYLOAD_MAX
        }) {
            Some("a payload is on an item that is not custom, or over its limit")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(inconsistent("created item", reason));
        }
        Ok(Self {
            seq: stored.seq,
            step_seq: stored.step_seq,
            item: stored.item,
            payload: stored.payload,
            state: stored.state,
            detail: stored.detail,
            recorded_at: stored.recorded_at,
            settled_at: stored.settled_at,
        })
    }

    /// Returns its number.
    #[must_use]
    pub const fn seq(&self) -> CreatedSeq {
        self.seq
    }

    /// Returns the step that created it.
    #[must_use]
    pub const fn step_seq(&self) -> StepSeq {
        self.step_seq
    }

    /// Returns the item.
    #[must_use]
    pub const fn item(&self) -> &CreatedItem {
        &self.item
    }

    /// Returns the custom compensator's payload.
    #[must_use]
    pub const fn payload(&self) -> Option<&JournalPayload> {
        self.payload.as_ref()
    }

    /// Returns where it stands.
    #[must_use]
    pub const fn state(&self) -> CreatedState {
        self.state
    }

    /// Returns why it settled as it did, for example
    /// `created_by_other_operation`.
    #[must_use]
    pub const fn detail(&self) -> Option<&ReasonCode> {
        self.detail.as_ref()
    }

    /// Returns when it was recorded.
    #[must_use]
    pub const fn recorded_at(&self) -> Timestamp {
        self.recorded_at
    }

    /// Returns when it settled, or `None` while `intended` or `created`.
    #[must_use]
    pub const fn settled_at(&self) -> Option<Timestamp> {
        self.settled_at
    }

    /// Confirms that the intended item now exists.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::Transition`] unless it is `intended`; the
    /// record does not change.
    pub fn confirm(&mut self) -> Result<(), JournalError> {
        if self.state != CreatedState::Intended {
            return Err(self.refuse("confirm"));
        }
        self.state = CreatedState::Created;
        Ok(())
    }

    /// Settles it at `at` as `compensated`, `kept`, `compensation_failed`,
    /// or `not_owned`, with why. An item that is `intended` or `created`,
    /// or whose compensation failed, may settle; only a `created` one may
    /// be kept, and a failed compensation may be tried again.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::Transition`] for a state it cannot reach
    /// from where it stands; the record does not change.
    pub fn settle(
        &mut self,
        state: CreatedState,
        detail: Option<ReasonCode>,
        at: Timestamp,
    ) -> Result<(), JournalError> {
        let allowed = match state {
            CreatedState::Intended | CreatedState::Created => false,
            CreatedState::Kept => self.state == CreatedState::Created,
            CreatedState::Compensated
            | CreatedState::CompensationFailed
            | CreatedState::NotOwned => matches!(
                self.state,
                CreatedState::Intended | CreatedState::Created | CreatedState::CompensationFailed
            ),
        };
        if !allowed {
            return Err(self.refuse(state.as_code()));
        }
        self.state = state;
        self.detail = detail;
        self.settled_at = Some(at);
        Ok(())
    }

    fn refuse(&self, event: &'static str) -> JournalError {
        TransitionError::new(Machine::CreatedRecord, self.state.as_code(), event).into()
    }
}

/// Everything the registry holds about one operation besides its row: the
/// journal, its steps and created items in `seq` order, and its lock keys
/// in text order. Read only.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct JournalView {
    /// The journal row.
    pub journal: OperationJournal,
    /// The steps, by `seq`.
    pub steps: Vec<JournalStep>,
    /// The created items, by `seq`.
    pub created: Vec<CreatedRecord>,
    /// The lock keys the operation needs, by their text.
    pub locks: Vec<LockKey>,
}

impl JournalView {
    /// Bundles what a registry read.
    #[must_use]
    pub const fn new(
        journal: OperationJournal,
        steps: Vec<JournalStep>,
        created: Vec<CreatedRecord>,
        locks: Vec<LockKey>,
    ) -> Self {
        Self {
            journal,
            steps,
            created,
            locks,
        }
    }
}

fn inconsistent(resource: &'static str, reason: &'static str) -> InvariantError {
    InvariantDetail::InconsistentRecord { resource, reason }.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_767_225_600 + seconds).unwrap()
    }

    fn payload(text: &str) -> JournalPayload {
        JournalPayload::new(text, OperationJournal::INPUT_MAX).unwrap()
    }

    fn new_journal() -> NewJournal {
        NewJournal {
            operation_id: OperationId::generate(),
            request: Some((
                "instance.create".parse().unwrap(),
                "ab".repeat(32).parse().unwrap(),
            )),
            trigger: OperationTrigger::user(Some("cli".parse().unwrap()), true),
            resumable: false,
            input: payload(r#"{"name":"orders-db"}"#),
            expected_revision: None,
        }
    }

    fn step(repeatable: bool) -> JournalStep {
        JournalStep::begin(
            NewStep {
                seq: StepSeq::FIRST,
                name: "download".parse().unwrap(),
                repeatable,
                reversible: true,
            },
            at(1),
        )
    }

    #[test]
    fn a_payload_is_a_braced_json_object_within_its_limit() {
        assert_eq!(JournalPayload::empty().as_str(), "{}");
        assert!(JournalPayload::new("{}", 2).is_ok());
        for (text, max) in [("{}", 1), ("[]", 16), ("{", 16), ("", 16), ("{\"a\":1}", 4)] {
            assert!(
                JournalPayload::new(text, max).is_err(),
                "{text:?} max {max}"
            );
        }
    }

    #[test]
    fn sequence_numbers_stay_in_range() {
        assert_eq!(StepSeq::new(256).unwrap().next(), None);
        assert_eq!(StepSeq::FIRST.next().map(StepSeq::get), Some(2));
        assert!(StepSeq::new(0).is_err() && StepSeq::new(257).is_err());
        assert!(CreatedSeq::new(1024).is_ok() && CreatedSeq::new(1025).is_err());
    }

    #[test]
    fn only_an_interactive_user_request_may_prompt() {
        let cli = Some("cli".parse::<ClientCode>().unwrap());

        assert!(OperationTrigger::user(cli.clone(), true).allows_prompt());
        assert!(!OperationTrigger::user(cli, false).allows_prompt());
        assert!(!OperationTrigger::background().allows_prompt());
        assert!(!OperationTrigger::new(TriggerOrigin::Background, None, true).allows_prompt());
    }

    #[test]
    fn a_failure_error_has_a_short_message_and_short_detail_values() {
        let code: ErrorCodeName = "operation.queue_full".parse().unwrap();
        let key: DetailKey = "max".parse().unwrap();

        assert!(
            FailureError::new(code.clone(), "queue full", vec![(key.clone(), "64".into())]).is_ok()
        );
        assert!(FailureError::new(code.clone(), "", Vec::new()).is_err());
        assert!(FailureError::new(code.clone(), "é".repeat(201), Vec::new()).is_err());
        assert!(FailureError::new(code, "x", vec![(key, "v".repeat(513))]).is_err());
    }

    #[test]
    fn a_failure_item_holds_at_most_eight_short_fields() {
        let kind: ReasonCode = "issue".parse().unwrap();
        let key: DetailKey = "path".parse().unwrap();
        let fields = |n: usize| vec![(key.clone(), "v".to_owned()); n];

        assert!(FailureItem::new(kind.clone(), fields(8)).is_ok());
        assert!(FailureItem::new(kind.clone(), fields(9)).is_err());
        assert!(FailureItem::new(kind.clone(), vec![(key.clone(), "v".repeat(512))]).is_ok());
        assert!(FailureItem::new(kind, vec![(key, "v".repeat(513))]).is_err());
    }

    #[test]
    fn a_failure_error_keeps_up_to_one_hundred_items_and_a_rule() {
        let code: ErrorCodeName = "provider.invalid_settings".parse().unwrap();
        let item = FailureItem::new("issue".parse().unwrap(), Vec::new()).unwrap();
        let base = || FailureError::new(code.clone(), "invalid settings", Vec::new()).unwrap();

        let plain = base();
        assert!(plain.items().is_empty() && plain.rule().is_none());

        let full = base()
            .with_items(vec![item.clone(); FailureError::ITEMS_MAX])
            .unwrap()
            .with_rule(Rule::Br002);
        assert_eq!(full.items().len(), 100);
        assert_eq!(full.items()[0].kind().as_str(), "issue");
        assert_eq!(full.rule(), Some(Rule::Br002));

        assert!(
            base()
                .with_items(vec![item; FailureError::ITEMS_MAX + 1])
                .is_err()
        );
    }

    #[test]
    fn a_new_journal_records_only_what_it_was_asked_and_keeps_first_times() {
        let mut journal = OperationJournal::new(new_journal()).unwrap();

        assert_eq!(
            journal.method().map(MethodName::as_str),
            Some("instance.create")
        );
        assert!(journal.point_of_no_return_at().is_none() && journal.result().is_none());
        journal.mark_point_of_no_return(at(1));
        journal.mark_point_of_no_return(at(2));
        journal.mark_cancel_requested(at(3));
        journal.mark_compensation_started(at(4));
        journal.mark_compensation_started(at(5));

        assert_eq!(journal.point_of_no_return_at(), Some(at(1)));
        assert_eq!(journal.cancel_requested_at(), Some(at(3)));
        assert_eq!(journal.compensation_started_at(), Some(at(4)));
    }

    #[test]
    fn input_and_result_are_held_to_their_limits() {
        let big = format!("{{\"a\":\"{}\"}}", "x".repeat(OperationJournal::INPUT_MAX));
        let mut new = new_journal();
        new.input = JournalPayload::new(big.clone(), usize::MAX).unwrap();

        assert!(matches!(
            OperationJournal::new(new),
            Err(JournalError::PayloadTooLarge { field: "input", .. })
        ));
        let mut journal = OperationJournal::new(new_journal()).unwrap();
        assert!(
            journal
                .set_result(Some(JournalPayload::new(big, usize::MAX).unwrap()))
                .is_err()
        );
        assert_eq!(journal.result(), None);
    }

    #[test]
    fn a_stored_journal_needs_method_and_fingerprint_together() {
        let journal = OperationJournal::new(new_journal()).unwrap();
        let stored = StoredJournal {
            operation_id: journal.operation_id(),
            method: journal.method().cloned(),
            fingerprint: None,
            trigger: journal.trigger().clone(),
            resumable: journal.resumable(),
            input: journal.input().clone(),
            expected_revision: None,
            waiting_on: None,
            point_of_no_return_at: None,
            cancel_requested_at: None,
            compensation_started_at: None,
            recovery_cause: None,
            failure_error: None,
            result: None,
        };

        assert!(OperationJournal::restore(stored.clone()).is_err());
        let restored = OperationJournal::restore(StoredJournal {
            fingerprint: journal.fingerprint().copied(),
            ..stored
        })
        .unwrap();
        assert_eq!(restored, journal);
    }

    #[test]
    fn a_step_runs_once_unless_it_is_repeatable() {
        let mut once = step(false);
        once.interrupt(at(2)).unwrap();
        assert_eq!(
            once.retry(at(3)),
            Err(JournalError::NotRepeatable { seq: 1 })
        );

        let mut again = step(true);
        again
            .fail("download.failed".parse().unwrap(), at(2))
            .unwrap();
        again.retry(at(3)).unwrap();
        assert_eq!(
            (
                again.state(),
                again.attempts(),
                again.error_code(),
                again.finished_at()
            ),
            (StepState::Running, 2, None, None)
        );
        again
            .finish(
                Some(JournalPayload::new("{\"bytes\":3}", 64).unwrap()),
                at(4),
            )
            .unwrap();
        assert_eq!(again.finished_at(), Some(at(4)));
        assert!(matches!(
            again.retry(at(5)),
            Err(JournalError::Transition(_))
        ));
        assert!(matches!(
            again.finish(None, at(5)),
            Err(JournalError::Transition(_))
        ));
    }

    #[test]
    fn a_checkpoint_is_held_to_its_limit() {
        let mut running = step(true);
        let big = format!("{{\"a\":\"{}\"}}", "x".repeat(JournalStep::CHECKPOINT_MAX));

        let result = running.finish(Some(JournalPayload::new(big, usize::MAX).unwrap()), at(2));

        assert!(matches!(
            result,
            Err(JournalError::PayloadTooLarge {
                field: "checkpoint",
                ..
            })
        ));
        assert_eq!(running.state(), StepState::Running);
    }

    #[test]
    fn a_stored_step_must_be_consistent() {
        let done = {
            let mut step = step(false);
            step.finish(None, at(2)).unwrap();
            step
        };
        let stored = StoredStep {
            seq: done.seq(),
            name: done.name().clone(),
            repeatable: false,
            reversible: true,
            state: StepState::Done,
            attempts: 1,
            checkpoint: None,
            error_code: None,
            started_at: at(1),
            finished_at: Some(at(2)),
        };

        assert_eq!(JournalStep::restore(stored.clone()).unwrap(), done);
        for broken in [
            StoredStep {
                finished_at: None,
                ..stored.clone()
            },
            StoredStep {
                attempts: 2,
                ..stored.clone()
            },
            StoredStep {
                attempts: 0,
                repeatable: true,
                ..stored.clone()
            },
            StoredStep {
                error_code: Some("a.b".parse().unwrap()),
                ..stored.clone()
            },
            StoredStep {
                state: StepState::Running,
                ..stored
            },
        ] {
            assert!(JournalStep::restore(broken.clone()).is_err(), "{broken:?}");
        }
    }

    #[test]
    fn created_items_round_trip_through_kind_and_reference() {
        let items = [
            CreatedItem::InstanceTree(InstanceId::generate()),
            CreatedItem::EnvironmentTree(EnvironmentId::generate()),
            CreatedItem::Custom {
                compensator: "supervisor_spawned_process".parse().unwrap(),
                key: "pid:4242:start:1767225600".parse().unwrap(),
            },
            CreatedItem::SecretRef(
                SecretRef::parse("local:instance/0199f000-0000-7000-8000-000000000030/admin")
                    .unwrap(),
            ),
        ];

        for item in items {
            let (kind, reference) = (item.kind_code(), item.item_ref());
            assert_eq!(CreatedItem::from_parts(kind, &reference).unwrap(), item);
        }
        assert!(CreatedItem::from_parts("secret_ref", "local:x").is_err());
        assert!(CreatedItem::from_parts("custom", "no-colon").is_err());
    }

    #[test]
    fn a_created_record_goes_from_intended_to_settled() {
        let instance = CreatedItem::InstanceTree(InstanceId::generate());
        let mut record = CreatedRecord::intend(
            CreatedSeq::FIRST,
            StepSeq::FIRST,
            instance.clone(),
            None,
            at(1),
        )
        .unwrap();

        assert!(matches!(
            record.clone().settle(CreatedState::Kept, None, at(2)),
            Err(JournalError::Transition(_))
        ));
        record.confirm().unwrap();
        record
            .settle(CreatedState::CompensationFailed, None, at(2))
            .unwrap();
        record
            .settle(CreatedState::Compensated, None, at(3))
            .unwrap();
        assert_eq!(
            (record.state(), record.settled_at()),
            (CreatedState::Compensated, Some(at(3)))
        );
        assert!(record.settle(CreatedState::NotOwned, None, at(4)).is_err());
        assert!(record.confirm().is_err());

        assert_eq!(
            CreatedRecord::intend(
                CreatedSeq::FIRST,
                StepSeq::FIRST,
                instance,
                Some(JournalPayload::empty()),
                at(1)
            ),
            Err(JournalError::PayloadNotAllowed)
        );
    }

    #[test]
    fn a_stored_created_record_must_be_consistent() {
        let stored = StoredCreatedRecord {
            seq: CreatedSeq::FIRST,
            step_seq: StepSeq::FIRST,
            item: CreatedItem::InstanceTree(InstanceId::generate()),
            payload: None,
            state: CreatedState::Created,
            detail: None,
            recorded_at: at(1),
            settled_at: None,
        };

        assert!(CreatedRecord::restore(stored.clone()).is_ok());
        for broken in [
            StoredCreatedRecord {
                settled_at: Some(at(2)),
                ..stored.clone()
            },
            StoredCreatedRecord {
                state: CreatedState::Kept,
                ..stored.clone()
            },
            StoredCreatedRecord {
                payload: Some(JournalPayload::empty()),
                ..stored
            },
        ] {
            assert!(
                CreatedRecord::restore(broken.clone()).is_err(),
                "{broken:?}"
            );
        }
    }
}
