//! The audit rows of operations (spec 0035, *Baris audit operasi*): the
//! engine writes one in the same batch as every commit that brings an
//! operation to a final state or to `needs_recovery`, and one for each cancel,
//! resume, or abandon that changed it. [`row`] builds every one of them.
//!
//! Nothing here can hold a secret value: every detail value goes through the
//! daemon's [`Redact`] before it is trimmed, and a work's [`AuditNote`] holds
//! only names, numbers, ports, and references. Building a row never fails: a
//! detail set that still breaks the rules is logged and stored empty, so the
//! state commit always goes through.

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    AuditAction, AuditActor, AuditEventId, AuditEventRecord, AuditResource, AuditResult,
    ConfigRevisionNumber, DetailKey, Operation, OperationJournal, OperationResource,
    OperationState, OperationTrigger, SafeDetails, TriggerOrigin,
};

use super::state::truncate_bytes;
use super::{CancelDisposition, Redact};

/// The longest error message an audit row keeps, in characters.
const MESSAGE_MAX_CHARS: usize = 200;

/// What a work wants the audit rows of its operation to carry (spec 0035,
/// *Catatan audit pekerjaan*): the instance's config revision number and up
/// to [`AuditNote::DETAILS_MAX`] details, such as `ports` or `runtimeId`.
///
/// The engine keeps the note in memory only, so an operation that `recover`
/// closes after a daemon restart has rows without it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditNote {
    /// The number of a config revision the registry already holds.
    pub config_revision: Option<ConfigRevisionNumber>,
    /// Details in order; [`AuditNote::with`] keeps at most
    /// [`AuditNote::DETAILS_MAX`]. A key the engine writes itself, or one
    /// given twice, is dropped, and the row's limit of 20 keys applies.
    pub details: Vec<(DetailKey, String)>,
}

impl AuditNote {
    /// The most details a note keeps.
    pub const DETAILS_MAX: usize = 12;

    /// A note with `config_revision` and no details.
    #[must_use]
    pub const fn new(config_revision: Option<ConfigRevisionNumber>) -> Self {
        Self {
            config_revision,
            details: Vec::new(),
        }
    }

    /// The note with one more detail; once it holds
    /// [`AuditNote::DETAILS_MAX`] details, more are ignored.
    ///
    /// # Panics
    ///
    /// Panics when `key` is not a camelCase detail key. Keys are fixed in
    /// the source, so a wrong one fails the first test that writes it.
    #[must_use]
    pub fn with(mut self, key: &'static str, value: impl ToString) -> Self {
        if self.details.len() < Self::DETAILS_MAX {
            self.details.push((
                key.parse()
                    .expect("audit note keys are fixed camelCase words"),
                value.to_string(),
            ));
        }
        self
    }

    /// The value of `key`, if the note holds it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.details
            .iter()
            .find(|(stored, _)| stored.as_str() == key)
            .map(|(_, value)| value.as_str())
    }
}

/// What an audit row of an operation records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuditEvent {
    /// It reached `succeeded`, `failed`, or `cancelled`
    /// (`operation.finished`).
    Finished,
    /// It entered `needs_recovery` (`operation.needs_recovery`).
    NeedsRecovery,
    /// A cancel changed it (`operation.cancel`).
    Cancel(CancelDisposition),
    /// It was resumed (`operation.resume`).
    Resume,
    /// It was abandoned, leaving `leftovers` items (`operation.abandon`).
    Abandon {
        /// How many created items were not undone.
        leftovers: usize,
    },
}

impl AuditEvent {
    /// The row's action.
    #[must_use]
    pub const fn action(self) -> AuditAction {
        match self {
            Self::Finished => AuditAction::OPERATION_FINISHED,
            Self::NeedsRecovery => AuditAction::OPERATION_NEEDS_RECOVERY,
            Self::Cancel(_) => AuditAction::OPERATION_CANCEL,
            Self::Resume => AuditAction::OPERATION_RESUME,
            Self::Abandon { .. } => AuditAction::OPERATION_ABANDON,
        }
    }
}

/// The event a commit from `before` to `after` owes a row for: a change
/// into a final state is [`AuditEvent::Finished`], into `needs_recovery`
/// [`AuditEvent::NeedsRecovery`], and anything else none.
#[must_use]
pub fn transition(before: OperationState, after: OperationState) -> Option<AuditEvent> {
    if before == after {
        None
    } else if after.is_final() {
        Some(AuditEvent::Finished)
    } else if after == OperationState::NeedsRecovery {
        Some(AuditEvent::NeedsRecovery)
    } else {
        None
    }
}

/// The time of the `index`th row of one batch: `now` plus `index`
/// nanoseconds, so the rows of one batch always order the same way by
/// `(occurred_at, id)`.
#[must_use]
pub fn nth(now: Timestamp, index: usize) -> Timestamp {
    let nanos = i64::try_from(index).unwrap_or(i64::MAX);
    now.checked_add(SignedDuration::from_nanos(nanos))
        .unwrap_or(now)
}

/// Builds the audit row of `event` for `operation` as `journal` stands
/// after the commit, on behalf of `origin` (the operation's trigger, or the
/// sender of a cancel, resume, or abandon), with the work's `note`, at
/// `occurred_at`.
///
/// The engine's keys come first (*Kunci `safe_details`*), then the note's
/// details whose key is not taken yet. Every value goes through `redactor`
/// before it is trimmed to 512 bytes on a character boundary, and past 20
/// keys the last ones are dropped and counted in `omitted`, which is one of
/// the 20.
#[must_use]
pub fn row(
    operation: &Operation,
    journal: &OperationJournal,
    event: AuditEvent,
    origin: &OperationTrigger,
    note: Option<&AuditNote>,
    redactor: &dyn Redact,
    occurred_at: Timestamp,
) -> AuditEventRecord {
    let actor = match origin.origin() {
        TriggerOrigin::User => AuditActor::User,
        _ => AuditActor::Daemon,
    };
    let state = operation.state();
    let result = match event {
        AuditEvent::Finished => match state {
            OperationState::Succeeded => AuditResult::Succeeded,
            OperationState::Cancelled => AuditResult::Cancelled,
            OperationState::NeedsRecovery => AuditResult::NeedsRecovery,
            _ => AuditResult::Failed,
        },
        AuditEvent::NeedsRecovery => AuditResult::NeedsRecovery,
        AuditEvent::Cancel(_) | AuditEvent::Resume | AuditEvent::Abandon { .. } => {
            AuditResult::Succeeded
        }
    };
    let (resource, instance) = match operation.resource() {
        OperationResource::Instance(id) => (Some(AuditResource::Instance(id)), Some(id)),
        OperationResource::Runtime(id) => (Some(AuditResource::Runtime(id)), None),
        OperationResource::Environment(id) => (Some(AuditResource::Environment(id)), None),
        _ => (None, None),
    };
    let mut record = AuditEventRecord::new(
        AuditEventId::generate(),
        actor,
        event.action(),
        result,
        occurred_at,
    );
    record.resource = resource;
    record.operation = Some(operation.id());
    record.operation_kind = Some(operation.kind());
    record.instance = instance;
    // The table refuses a revision number without an instance; dropping it
    // keeps the state commit going.
    record.config_revision = instance.and(note.and_then(|note| note.config_revision));
    record.client = origin.client().cloned();
    record.safe_details = details(operation, journal, event, note, redactor);
    record
}

/// The `safe_details` of a row, redacted, trimmed, and capped.
fn details(
    operation: &Operation,
    journal: &OperationJournal,
    event: AuditEvent,
    note: Option<&AuditNote>,
    redactor: &dyn Redact,
) -> SafeDetails {
    let mut entries: Vec<(&'static str, String)> = Vec::new();
    let state = operation.state();
    let stage = || operation.stage().map(ToString::to_string);
    let error = |entries: &mut Vec<(&'static str, String)>| {
        if let Some(error) = journal.failure_error() {
            entries.push(("errorCode", error.code().to_string()));
            entries.push((
                "message",
                error.message().chars().take(MESSAGE_MAX_CHARS).collect(),
            ));
        }
    };
    match event {
        AuditEvent::Finished => {
            entries.push(("state", state.as_code().to_owned()));
            match state {
                OperationState::Succeeded => {
                    if journal.cancel_requested_at().is_some() {
                        entries.push(("cancelRequested", "true".to_owned()));
                    }
                }
                OperationState::Failed => {
                    if let Some(failure) = operation.failure() {
                        entries.push(("reason", failure.code().to_string()));
                    }
                    error(&mut entries);
                    entries.extend(stage().map(|stage| ("stage", stage)));
                }
                _ => entries.extend(stage().map(|stage| ("stage", stage))),
            }
        }
        AuditEvent::NeedsRecovery => {
            entries.push(("state", state.as_code().to_owned()));
            error(&mut entries);
            entries.extend(stage().map(|stage| ("stage", stage)));
            if let Some(cause) = journal.recovery_cause() {
                entries.push(("cause", cause.as_code().to_owned()));
            }
            if let Some(hint) = operation.recovery_hint() {
                entries.push(("hint", hint.as_code().to_owned()));
            }
        }
        AuditEvent::Cancel(disposition) => {
            entries.push(("disposition", disposition.as_code().to_owned()));
        }
        AuditEvent::Resume => {}
        AuditEvent::Abandon { leftovers } => {
            entries.push(("leftovers", leftovers.to_string()));
        }
    }
    let mut keyed: Vec<(DetailKey, String)> = entries
        .into_iter()
        .map(|(key, value)| {
            (
                key.parse()
                    .expect("the engine's audit keys are fixed camelCase words"),
                value,
            )
        })
        .collect();
    for (key, value) in note.map(|note| note.details.as_slice()).unwrap_or_default() {
        if key.as_str() != "omitted" && !keyed.iter().any(|(taken, _)| taken == key) {
            keyed.push((key.clone(), value.clone()));
        }
    }
    let mut keyed: Vec<(DetailKey, String)> = keyed
        .into_iter()
        .map(|(key, value)| {
            let value = truncate_bytes(&redactor.redact(&value), SafeDetails::VALUE_MAX);
            (key, value)
        })
        .collect();
    if keyed.len() > SafeDetails::MAX_ENTRIES {
        let kept = SafeDetails::MAX_ENTRIES - 1;
        let omitted = keyed.len() - kept;
        keyed.truncate(kept);
        keyed.push((
            "omitted"
                .parse()
                .expect("omitted is a fixed camelCase word"),
            omitted.to_string(),
        ));
    }
    SafeDetails::new(keyed).unwrap_or_else(|error| {
        tracing::error!(
            operation_id = %operation.id(),
            %error,
            "an audit row's details broke the rules and were left out"
        );
        SafeDetails::empty()
    })
}

#[cfg(test)]
mod tests {
    use stackly_domain::{
        ClientCode, ErrorCodeName, FailureCode, FailureError, InstanceId, JournalPayload,
        NewJournal, NewOperation, OperationEvent, OperationFailure, OperationKind, RecoveryCause,
        RecoveryHint, RuntimeId,
    };

    use super::*;
    use stackly_platform::SecretValue;

    use crate::secret::Redactor;

    const SENTINEL: &str = "fixture-credential-7f3a9c2e1b";

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn key(text: &str) -> DetailKey {
        text.parse().unwrap()
    }

    fn cli() -> OperationTrigger {
        OperationTrigger::user(Some("cli".parse::<ClientCode>().unwrap()), true)
    }

    fn running(kind: OperationKind, resource: OperationResource) -> (Operation, OperationJournal) {
        let mut operation = Operation::new(NewOperation {
            id: stackly_domain::OperationId::generate(),
            kind,
            resource,
            idempotency_key: None,
            created_at: at("2026-11-10T08:00:00Z"),
        });
        operation
            .apply(OperationEvent::Start, at("2026-11-10T08:00:01Z"))
            .unwrap();
        operation
            .set_stage("render".parse().unwrap(), at("2026-11-10T08:00:02Z"))
            .unwrap();
        let journal = OperationJournal::new(NewJournal {
            operation_id: operation.id(),
            request: None,
            trigger: cli(),
            resumable: false,
            input: JournalPayload::empty(),
            expected_revision: None,
            parent: None,
        })
        .unwrap();
        (operation, journal)
    }

    fn redactor() -> Redactor {
        let redactor = Redactor::new();
        redactor.learn(&SecretValue::new(SENTINEL.to_owned()).unwrap());
        redactor
    }

    fn values(record: &AuditEventRecord) -> Vec<(String, String)> {
        record
            .safe_details
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_owned()))
            .collect()
    }

    // covers: AC-1 (spec 0035)
    #[test]
    fn a_failed_operation_row_carries_its_trigger_kind_instance_revision_and_error() {
        let instance = InstanceId::generate();
        let (mut operation, mut journal) = running(
            OperationKind::StartInstance,
            OperationResource::Instance(instance),
        );
        operation
            .apply(
                OperationEvent::Fail(OperationFailure::new(FailureCode::STEP_FAILED, true)),
                at("2026-11-10T08:00:03Z"),
            )
            .unwrap();
        journal.set_failure_error(Some(
            FailureError::new(
                "supervisor.endpoint_in_use"
                    .parse::<ErrorCodeName>()
                    .unwrap(),
                "port 5432 is in use",
                Vec::new(),
            )
            .unwrap(),
        ));
        let note = AuditNote::new(Some(ConfigRevisionNumber::new(3).unwrap()))
            .with("ports", "main 5432")
            .with("state", "not the engine's");

        let row = row(
            &operation,
            &journal,
            AuditEvent::Finished,
            journal.trigger(),
            Some(&note),
            &Redactor::new(),
            at("2026-11-10T08:00:03Z"),
        );

        assert_eq!(row.action, AuditAction::OPERATION_FINISHED);
        assert_eq!(row.actor, AuditActor::User);
        assert_eq!(row.client.as_ref().map(ClientCode::as_str), Some("cli"));
        assert_eq!(row.operation, Some(operation.id()));
        assert_eq!(row.operation_kind, Some(OperationKind::StartInstance));
        assert_eq!(row.resource, Some(AuditResource::Instance(instance)));
        assert_eq!(row.instance, Some(instance));
        assert_eq!(row.config_revision.map(ConfigRevisionNumber::get), Some(3));
        assert_eq!(row.result, AuditResult::Failed);
        assert_eq!(
            values(&row),
            [
                ("errorCode", "supervisor.endpoint_in_use"),
                ("message", "port 5432 is in use"),
                ("ports", "main 5432"),
                ("reason", "step_failed"),
                ("stage", "render"),
                ("state", "failed"),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
        );
    }

    // covers: AC-4 (spec 0035)
    #[test]
    fn a_secret_value_never_reaches_a_row_and_a_long_value_is_redacted_before_it_is_cut() {
        let (mut operation, mut journal) = running(
            OperationKind::CreateInstance,
            OperationResource::Instance(InstanceId::generate()),
        );
        operation
            .apply(
                OperationEvent::Fail(OperationFailure::new(FailureCode::STEP_FAILED, false)),
                at("2026-11-10T08:00:03Z"),
            )
            .unwrap();
        journal.set_failure_error(Some(
            FailureError::new(
                "provider.init_failed".parse::<ErrorCodeName>().unwrap(),
                format!("init said {SENTINEL} is wrong"),
                Vec::new(),
            )
            .unwrap(),
        ));
        // The sentinel crosses byte 512: cut first, its head would stay.
        let long = format!("{}{SENTINEL}{}", "x".repeat(500), "y".repeat(100));
        assert!(long.len() > 600);
        let note = AuditNote::new(None)
            .with("settings", format!("password={SENTINEL}"))
            .with("long", &long);

        let row = row(
            &operation,
            &journal,
            AuditEvent::Finished,
            journal.trigger(),
            Some(&note),
            &redactor(),
            at("2026-11-10T08:00:03Z"),
        );

        let head = &SENTINEL[..8];
        for (key, value) in values(&row) {
            assert!(!value.contains(head), "{key} holds the sentinel: {value}");
            assert!(value.len() <= SafeDetails::VALUE_MAX, "{key}");
        }
        let long_value = row.safe_details.get("long").unwrap();
        assert!(long_value.starts_with(&"x".repeat(500)));
        assert!(row.safe_details.get("message").unwrap().contains("****"));
    }

    // covers: AC-4 (spec 0035)
    #[test]
    fn past_20_keys_the_last_are_dropped_and_counted_in_omitted() {
        let (operation, journal) = running(
            OperationKind::ApplyConfig,
            OperationResource::Instance(InstanceId::generate()),
        );
        let mut note = AuditNote::new(None);
        for n in 0..30 {
            note.details.push((key(&format!("k{n:02}")), n.to_string()));
        }
        let row = row(
            &operation,
            &journal,
            AuditEvent::Cancel(CancelDisposition::Requested),
            &cli(),
            Some(&note),
            &Redactor::new(),
            at("2026-11-10T08:00:03Z"),
        );
        assert_eq!(row.safe_details.len(), SafeDetails::MAX_ENTRIES);
        assert_eq!(row.safe_details.get("disposition"), Some("requested"));
        // disposition and k00..k17 are kept: 19 keys, then omitted.
        assert_eq!(row.safe_details.get("k17"), Some("17"));
        assert_eq!(row.safe_details.get("k18"), None);
        assert_eq!(row.safe_details.get("omitted"), Some("12"));
        assert_eq!(row.result, AuditResult::Succeeded);
    }

    #[test]
    fn a_note_holds_at_most_12_details() {
        let mut note = AuditNote::new(None);
        for n in 0..20 {
            note = note.with("k", n);
        }
        assert_eq!(note.details.len(), AuditNote::DETAILS_MAX);
        assert_eq!(note.get("k"), Some("0"));
    }

    // covers: AC-1 (spec 0035)
    #[test]
    fn a_revision_without_an_instance_is_dropped_and_the_manager_has_no_resource() {
        let (operation, journal) = running(
            OperationKind::InstallRuntime,
            OperationResource::Runtime(RuntimeId::generate()),
        );
        let note = AuditNote::new(Some(ConfigRevisionNumber::FIRST));
        let row = row(
            &operation,
            &journal,
            AuditEvent::Resume,
            &OperationTrigger::background(),
            Some(&note),
            &Redactor::new(),
            at("2026-11-10T08:00:03Z"),
        );
        assert_eq!(row.config_revision, None);
        assert_eq!(row.instance, None);
        assert!(matches!(row.resource, Some(AuditResource::Runtime(_))));
        assert_eq!(row.actor, AuditActor::Daemon);
        assert_eq!(row.client, None);
        assert!(row.safe_details.is_empty());

        let (manager, journal) = running(OperationKind::UpdateManager, OperationResource::Manager);
        let row = super::row(
            &manager,
            &journal,
            AuditEvent::Abandon { leftovers: 2 },
            &cli(),
            None,
            &Redactor::new(),
            at("2026-11-10T08:00:03Z"),
        );
        assert_eq!(row.resource, None);
        assert_eq!(row.safe_details.get("leftovers"), Some("2"));
        assert_eq!(row.action, AuditAction::OPERATION_ABANDON);
    }

    // covers: AC-1 (spec 0035)
    #[test]
    fn a_needs_recovery_row_carries_cause_hint_and_error() {
        let (mut operation, mut journal) = running(
            OperationKind::InstallRuntime,
            OperationResource::Runtime(RuntimeId::generate()),
        );
        operation
            .apply(
                OperationEvent::CompensationFailed,
                at("2026-11-10T08:00:03Z"),
            )
            .unwrap();
        operation
            .set_recovery_hint(RecoveryHint::Abandon, at("2026-11-10T08:00:03Z"))
            .unwrap();
        journal.set_recovery_cause(Some(RecoveryCause::CompensationFailed));
        journal.set_failure_error(Some(
            FailureError::new(
                "operation.compensation_failed"
                    .parse::<ErrorCodeName>()
                    .unwrap(),
                "undoing failed",
                Vec::new(),
            )
            .unwrap(),
        ));
        let row = row(
            &operation,
            &journal,
            AuditEvent::NeedsRecovery,
            journal.trigger(),
            None,
            &Redactor::new(),
            at("2026-11-10T08:00:03Z"),
        );
        assert_eq!(row.result, AuditResult::NeedsRecovery);
        assert_eq!(
            values(&row),
            [
                ("cause", "compensation_failed"),
                ("errorCode", "operation.compensation_failed"),
                ("hint", "abandon"),
                ("message", "undoing failed"),
                ("stage", "render"),
                ("state", "needs_recovery"),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
        );
    }

    // covers: AC-1 (spec 0035)
    #[test]
    fn a_success_after_a_cancel_request_says_so() {
        let (mut operation, mut journal) = running(
            OperationKind::InstallRuntime,
            OperationResource::Runtime(RuntimeId::generate()),
        );
        operation
            .apply(OperationEvent::RequestCancel, at("2026-11-10T08:00:03Z"))
            .unwrap();
        journal.mark_cancel_requested(at("2026-11-10T08:00:03Z"));
        operation
            .apply(OperationEvent::Succeed, at("2026-11-10T08:00:04Z"))
            .unwrap();
        let row = row(
            &operation,
            &journal,
            AuditEvent::Finished,
            journal.trigger(),
            None,
            &Redactor::new(),
            at("2026-11-10T08:00:04Z"),
        );
        assert_eq!(row.result, AuditResult::Succeeded);
        assert_eq!(row.safe_details.get("cancelRequested"), Some("true"));
        assert_eq!(row.safe_details.get("state"), Some("succeeded"));
    }

    #[test]
    fn only_a_change_into_a_final_state_or_needs_recovery_owes_a_row() {
        use OperationState as S;
        for before in S::ALL.iter().copied() {
            for after in S::ALL.iter().copied() {
                let expected = if before == after {
                    None
                } else if after.is_final() {
                    Some(AuditEvent::Finished)
                } else if after == S::NeedsRecovery {
                    Some(AuditEvent::NeedsRecovery)
                } else {
                    None
                };
                assert_eq!(transition(before, after), expected, "{before} -> {after}");
            }
        }
        let now = at("2026-11-10T08:00:00Z");
        assert_eq!(nth(now, 0), now);
        assert_eq!(
            nth(now, 2),
            now.checked_add(SignedDuration::from_nanos(2)).unwrap()
        );
    }
}
