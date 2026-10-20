//! The DTOs of the `operation` area (spec 0007, *Permukaan API*) built
//! from the engine's views. Journal payloads never reach the wire: no step
//! checkpoint, no custom payload, and no input.

use serde::de::DeserializeOwned;
use stackly_core::operation::{
    CancelDisposition as CoreDisposition, EngineStats, OperationDetailView, OperationOutcome,
    OperationView, PartialOutcome as CorePartial, RecoveryAction as CoreAction,
};
use stackly_domain::{
    CreatedRecord, FailureCode, FailureError, JournalStep, OperationId, OperationResource,
    OperationState as DomainState, Progress, ResourceRevision,
};
use stackly_protocol::codes::{self, internal, operation};
use stackly_protocol::handshake::ClientKind;
use stackly_protocol::operation::{
    CancelDisposition, CountedProgress, CreatedItemInfo, OperationDetail, OperationFailureInfo,
    OperationProgress, OperationResult, OperationState, OperationStats, OperationStepInfo,
    OperationSummary, OperationTriggerInfo, PartialItem, PartialOutcome, RecoveryAction,
    RecoveryInfo,
};
use stackly_protocol::{Count, ErrorBody, ResourceKind, ResourceRef, Revision, Timestamp};

use crate::map;

/// One operation as every client sees it.
#[must_use]
pub fn summary(view: &OperationView) -> OperationSummary {
    let operation = &view.operation;
    let journal = &view.journal;
    let id = operation.id();
    let state = state(operation.state());
    let failure = (state == OperationState::Failed).then(|| failure_info(view));
    let recovery = (state == OperationState::NeedsRecovery).then(|| RecoveryInfo {
        hint: operation.recovery_hint().map_or(
            stackly_protocol::operation::RecoveryHint::Unrecognized,
            |hint| by_code(hint.as_code()),
        ),
        cause: journal.recovery_cause().map_or(
            stackly_protocol::operation::RecoveryCause::Unrecognized,
            |cause| by_code(cause.as_code()),
        ),
        error: journal.failure_error().map(|error| failure_body(error, id)),
        actions: view.recovery_actions.iter().map(|a| action(*a)).collect(),
    });
    OperationSummary {
        id: id.as_uuid(),
        kind: by_code(operation.kind().as_code()),
        resource: resource(operation.resource()),
        state,
        stage: operation.stage().map(|stage| stage.as_str().to_owned()),
        progress: progress(operation.progress()),
        waiting_on: journal.waiting_on().map(|id| id.as_uuid()),
        cancel_requested_at: journal.cancel_requested_at().map(Timestamp::from_jiff),
        point_of_no_return_at: journal.point_of_no_return_at().map(Timestamp::from_jiff),
        failure,
        recovery,
        result: journal.result().and_then(|payload| {
            OperationOutcome::from_payload(payload)
                .inspect_err(|_| {
                    tracing::warn!(operation_id = %id, "a stored operation result does not decode");
                })
                .ok()
                .map(|outcome| result(&outcome))
        }),
        trigger: OperationTriggerInfo {
            origin: by_code(journal.trigger().origin().as_code()),
            client: journal
                .trigger()
                .client()
                .and_then(|client| client_kind(client.as_str())),
        },
        created_at: Timestamp::from_jiff(operation.created_at()),
        started_at: operation.started_at().map(Timestamp::from_jiff),
        updated_at: Timestamp::from_jiff(operation.updated_at()),
        revision: revision(operation.revision()),
    }
}

/// One operation with its steps and created items.
#[must_use]
pub fn detail(detail: &OperationDetailView) -> OperationDetail {
    OperationDetail {
        operation: summary(&detail.view),
        steps: detail.steps.iter().map(step_info).collect(),
        created: detail.created.iter().map(created_info).collect(),
    }
}

/// One step, without its checkpoint.
#[must_use]
pub fn step_info(step: &JournalStep) -> OperationStepInfo {
    OperationStepInfo {
        seq: u32::from(step.seq().get()),
        name: step.name().as_str().to_owned(),
        state: by_code(step.state().as_code()),
        repeatable: step.repeatable(),
        reversible: step.reversible(),
        attempts: step.attempts(),
        started_at: Timestamp::from_jiff(step.started_at()),
        finished_at: step.finished_at().map(Timestamp::from_jiff),
        error_code: step.error_code().map(|code| code.as_str().to_owned()),
    }
}

/// One created item, without its custom payload.
#[must_use]
pub fn created_info(record: &CreatedRecord) -> CreatedItemInfo {
    CreatedItemInfo {
        kind: record.item().kind_code().to_owned(),
        reference: record.item().item_ref(),
        state: by_code(record.state().as_code()),
        detail: record.detail().map(|detail| detail.as_str().to_owned()),
    }
}

/// The engine's counters and limits.
#[must_use]
pub fn stats(stats: &EngineStats) -> OperationStats {
    OperationStats {
        running: stats.running,
        queued: stats.queued,
        needs_recovery: stats.needs_recovery,
        max_running: stats.max_running,
        max_queued: stats.max_queued,
        interrupted_at_start: stats.interrupted_at_start,
        resumed_at_start: stats.resumed_at_start,
        auto_compensated_at_start: stats.auto_compensated_at_start,
        rejected_queue_full: Count::saturating(stats.rejected_queue_full),
    }
}

/// What a cancel did.
#[must_use]
pub fn disposition(disposition: CoreDisposition) -> CancelDisposition {
    by_code(disposition.as_code())
}

/// The error an operation stored, as the wire carries it (spec 0007,
/// *Kegagalan dan error*): the catalog gives the category and `retryable`,
/// and `operationId` names the operation. A detail key the catalog entry
/// does not list is dropped, and a code this build does not know becomes
/// `internal.unexpected`; both are bugs, so each logs an error.
#[must_use]
pub fn failure_body(error: &FailureError, operation_id: OperationId) -> ErrorBody {
    let code = error.code().as_str();
    let Some(spec) = codes::lookup(code) else {
        tracing::error!(
            operation_id = %operation_id,
            code,
            "an operation stored an error code the catalog does not know",
        );
        return ErrorBody::new(
            &internal::UNEXPECTED,
            "the operation failed with an error this daemon cannot name",
        )
        .with_operation_id(operation_id.as_uuid());
    };
    let mut body = ErrorBody::new(spec, error.message()).with_operation_id(operation_id.as_uuid());
    for (key, value) in error.details() {
        if spec.details.contains(&key.as_str()) {
            body = body.with_detail(key.as_str(), value.as_str());
        } else {
            tracing::error!(
                operation_id = %operation_id,
                code,
                key = key.as_str(),
                "an operation stored a detail its catalog entry does not list; it was dropped",
            );
        }
    }
    body
}

/// The failure of a `failed` operation. An operation closed before the
/// journal existed has no stored error, so one follows from its reason.
fn failure_info(view: &OperationView) -> OperationFailureInfo {
    let id = view.operation.id();
    let (reason, retryable) = view.operation.failure().map_or_else(
        || ("unknown".to_owned(), false),
        |failure| (failure.code().as_str().to_owned(), failure.retryable()),
    );
    let error = view.journal.failure_error().map_or_else(
        || fallback_error(&reason).with_operation_id(id.as_uuid()),
        |error| failure_body(error, id),
    );
    OperationFailureInfo {
        reason,
        retryable,
        error,
    }
}

/// The error of a `failed` operation that stored none, from its reason.
fn fallback_error(reason: &str) -> ErrorBody {
    let spec = match reason {
        _ if reason == FailureCode::INTERRUPTED.as_str() => &operation::INTERRUPTED,
        _ if reason == FailureCode::DAEMON_STOPPING.as_str() => &operation::DAEMON_STOPPING,
        _ if reason == FailureCode::ABANDONED.as_str() => &operation::ABANDONED,
        _ => &internal::UNEXPECTED,
    };
    ErrorBody::new(spec, "the operation failed")
}

fn result(outcome: &OperationOutcome) -> OperationResult {
    OperationResult {
        resources: outcome
            .resources
            .iter()
            .filter_map(map::resource_ref)
            .collect(),
        partial: outcome
            .partial
            .iter()
            .map(|item| PartialItem {
                item: item.item.clone(),
                resource: item.resource.as_ref().and_then(map::resource_ref),
                outcome: partial_outcome(item.outcome),
                code: item.code.clone(),
            })
            .collect(),
    }
}

/// The wire state; the seven states of PRD section 18.2 on both sides.
#[must_use]
pub const fn state(state: DomainState) -> OperationState {
    match state {
        DomainState::Queued => OperationState::Queued,
        DomainState::Running => OperationState::Running,
        DomainState::CancelRequested => OperationState::CancelRequested,
        DomainState::Succeeded => OperationState::Succeeded,
        DomainState::Failed => OperationState::Failed,
        DomainState::Cancelled => OperationState::Cancelled,
        DomainState::NeedsRecovery => OperationState::NeedsRecovery,
    }
}

/// The resource of an operation; null for the manager (spec 0007,
/// `OperationSummary.resource`).
#[must_use]
pub fn resource(resource: OperationResource) -> Option<ResourceRef> {
    let (kind, id) = match resource {
        OperationResource::Runtime(id) => (ResourceKind::Runtime, id.to_string()),
        OperationResource::Instance(id) => (ResourceKind::Instance, id.to_string()),
        OperationResource::Environment(id) => (ResourceKind::Environment, id.to_string()),
        OperationResource::Manager => return None,
    };
    Some(ResourceRef { kind, id })
}

const fn progress(progress: Progress) -> OperationProgress {
    match progress {
        Progress::Indeterminate => OperationProgress::Indeterminate,
        Progress::Counted { done, total } => OperationProgress::Counted(CountedProgress {
            done: Count::saturating(done),
            total: Count::saturating(total),
        }),
    }
}

/// The wire revision; a registry revision never reaches 2^53.
#[must_use]
pub const fn revision(revision: ResourceRevision) -> Revision {
    Revision::saturating(revision.get())
}

const fn action(action: CoreAction) -> RecoveryAction {
    match action {
        CoreAction::Resume => RecoveryAction::Resume,
        CoreAction::Abandon => RecoveryAction::Abandon,
    }
}

fn partial_outcome(outcome: CorePartial) -> PartialOutcome {
    by_code(outcome.as_code())
}

/// The client kind a trigger names; a code this build does not know is
/// left out rather than sent as `unrecognized`.
fn client_kind(code: &str) -> Option<ClientKind> {
    match by_code::<ClientKind>(code) {
        ClientKind::Unrecognized => None,
        kind => Some(kind),
    }
}

/// The open wire enum whose code is `code`. Every open enum of the area
/// reads a code it does not know as `unrecognized`; the tests below check
/// that no domain code falls there.
fn by_code<T: DeserializeOwned>(code: &str) -> T {
    serde_json::from_value(serde_json::Value::String(code.to_owned()))
        .expect("every open wire enum reads any string, an unknown one as unrecognized")
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use stackly_domain::{
        CreatedState, OperationKind, RecoveryCause, RecoveryHint, StepState, TriggerOrigin,
    };

    use super::*;

    fn code_of<T: Serialize>(value: &T) -> String {
        serde_json::to_value(value)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn round_trips<W: DeserializeOwned + Serialize>(codes: impl IntoIterator<Item = &'static str>) {
        for code in codes {
            let wire: W = by_code(code);
            assert_eq!(code_of(&wire), code, "{code} has no wire value");
        }
    }

    #[test]
    fn every_domain_code_has_a_known_wire_value() {
        round_trips::<stackly_protocol::operation::OperationKind>(
            OperationKind::ALL.iter().map(|kind| kind.as_code()),
        );
        round_trips::<stackly_protocol::operation::StepState>(
            StepState::ALL.iter().map(|state| state.as_code()),
        );
        round_trips::<stackly_protocol::operation::CreatedState>(
            CreatedState::ALL.iter().map(|state| state.as_code()),
        );
        round_trips::<stackly_protocol::operation::RecoveryHint>(
            RecoveryHint::ALL.iter().map(|hint| hint.as_code()),
        );
        round_trips::<stackly_protocol::operation::RecoveryCause>(
            RecoveryCause::ALL.iter().map(|cause| cause.as_code()),
        );
        round_trips::<stackly_protocol::operation::TriggerOrigin>(
            TriggerOrigin::ALL.iter().map(|origin| origin.as_code()),
        );
        round_trips::<CancelDisposition>(
            [
                CoreDisposition::CancelledBeforeStart,
                CoreDisposition::Requested,
                CoreDisposition::ContinuingPastPointOfNoReturn,
                CoreDisposition::AlreadyFinal,
                CoreDisposition::RecoveryRequired,
            ]
            .map(CoreDisposition::as_code),
        );
        round_trips::<PartialOutcome>(
            [
                CorePartial::Done,
                CorePartial::Failed,
                CorePartial::Skipped,
                CorePartial::LeftOver,
            ]
            .map(CorePartial::as_code),
        );
    }

    #[test]
    fn every_domain_state_has_its_wire_state() {
        for domain in DomainState::ALL {
            assert_eq!(code_of(&state(*domain)), domain.as_code());
        }
    }

    #[test]
    fn a_client_code_this_build_does_not_know_is_left_out() {
        assert_eq!(client_kind("cli"), Some(ClientKind::Cli));
        assert_eq!(client_kind("gui"), Some(ClientKind::Gui));
        assert_eq!(client_kind("unrecognized"), None);
        assert_eq!(client_kind("watch_app"), None);
    }

    #[test]
    fn a_stored_error_takes_its_category_from_the_catalog_and_names_the_operation() {
        let id = OperationId::generate();
        let error = FailureError::new(
            "operation.recovery_required".parse().unwrap(),
            "operation x needs recovery",
            vec![
                ("operationId".parse().unwrap(), "x".to_owned()),
                ("lockKey".parse().unwrap(), "manager:self".to_owned()),
            ],
        )
        .unwrap();

        let body = failure_body(&error, id);

        assert_eq!(body.code(), "operation.recovery_required");
        assert_eq!(body.category(), operation::RECOVERY_REQUIRED.category);
        assert_eq!(body.retryable(), operation::RECOVERY_REQUIRED.retryable);
        assert_eq!(body.operation_id(), Some(id.as_uuid()));
        assert_eq!(body.detail("lockKey"), Some("manager:self"));
        assert_eq!(body.message(), "operation x needs recovery");
    }

    #[test]
    fn a_stored_code_the_catalog_does_not_know_becomes_internal_unexpected() {
        let id = OperationId::generate();
        let error = FailureError::new(
            "feature.not_built".parse().unwrap(),
            "secret text",
            Vec::new(),
        )
        .unwrap();

        let body = failure_body(&error, id);

        assert_eq!(body.code(), "internal.unexpected");
        assert_eq!(body.operation_id(), Some(id.as_uuid()));
        assert!(!body.message().contains("secret text"));
    }

    #[test]
    fn a_detail_the_catalog_entry_does_not_list_is_dropped() {
        let error = FailureError::new(
            "operation.interrupted".parse().unwrap(),
            "the daemon stopped",
            vec![("path".parse().unwrap(), "/tmp/x".to_owned())],
        )
        .unwrap();

        let body = failure_body(&error, OperationId::generate());

        assert!(body.details().is_empty());
    }

    #[test]
    fn the_manager_travels_as_no_resource() {
        assert_eq!(resource(OperationResource::Manager), None);
        let id = stackly_domain::InstanceId::generate();
        assert_eq!(
            resource(OperationResource::Instance(id)),
            Some(ResourceRef {
                kind: ResourceKind::Instance,
                id: id.to_string()
            })
        );
    }
}
