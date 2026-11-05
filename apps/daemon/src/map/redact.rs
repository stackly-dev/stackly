//! Redaction of everything the daemon sends (spec 0011 AC-14): the text of
//! an error envelope, and the text an operation summary carries in its
//! progress, failure, recovery, and result, whether it travels in a reply
//! or in an event. The one `Redactor` of the run does the work; only
//! `secret.reveal` sends a value, and its reply is not touched here.

use stackly_core::secret::Redactor;
use stackly_protocol::instance::config::ChangeEndpointsReply;
use stackly_protocol::operation::{
    CreatedItemInfo, OperationAccepted, OperationDetail, OperationSummary,
};
use stackly_protocol::runtime::InstallOutcome;
use stackly_protocol::{ErrorBody, Reply};

/// `body` with every free text redacted: message, hint, detail values, and
/// item field values.
#[must_use]
pub fn error(redactor: &Redactor, body: ErrorBody) -> ErrorBody {
    body.redacted_with(|text| redactor.redact(text).into_owned())
}

/// `summary` with its stage, the errors of its failure and recovery, and
/// the item and code of each item of its partial result redacted.
pub fn summary(redactor: &Redactor, summary: &mut OperationSummary) {
    let text = |value: &mut String| {
        if let std::borrow::Cow::Owned(redacted) = redactor.redact(value) {
            *value = redacted;
        }
    };
    if let Some(stage) = &mut summary.stage {
        text(stage);
    }
    if let Some(failure) = summary.failure.take() {
        let mut failure = failure;
        failure.error = error(redactor, failure.error);
        summary.failure = Some(failure);
    }
    if let Some(recovery) = &mut summary.recovery
        && let Some(body) = recovery.error.take()
    {
        recovery.error = Some(error(redactor, body));
    }
    if let Some(result) = &mut summary.result {
        for item in &mut result.partial {
            text(&mut item.item);
            if let Some(code) = &mut item.code {
                text(code);
            }
        }
    }
}

fn created(redactor: &Redactor, items: &mut [CreatedItemInfo]) {
    for item in items {
        if let std::borrow::Cow::Owned(redacted) = redactor.redact(&item.reference) {
            item.reference = redacted;
        }
    }
}

/// The summary of an accepted operation redacted, if anything ran.
fn accepted(redactor: &Redactor, accepted: Option<&mut OperationAccepted>) {
    if let Some(accepted) = accepted {
        summary(redactor, &mut accepted.operation);
    }
}

/// The reply of an operation method, or of a method that starts one (a
/// replay of a stored summary included), with the text of every summary it
/// carries redacted; any other reply is left as it is, since its DTOs hold
/// no free text a value could reach.
pub fn operation_reply(redactor: &Redactor, reply: &mut Reply) {
    match reply {
        Reply::InstanceCreate(reply)
        | Reply::InstanceStart(reply)
        | Reply::InstanceStop(reply)
        | Reply::InstanceRestart(reply)
        | Reply::InstanceReload(reply)
        | Reply::InstanceForceStop(reply)
        | Reply::InstanceConfigKeepRevision(reply)
        | Reply::InstanceRename(reply) => accepted(redactor, reply.accepted.as_mut()),
        Reply::InstanceConfigApply(reply) => accepted(redactor, reply.accepted.as_mut()),
        Reply::InstanceChangeEndpoints(reply) => {
            if let ChangeEndpointsReply::Applied(reply) = reply.as_mut() {
                accepted(redactor, reply.accepted.as_mut());
            }
        }
        Reply::RuntimeInstall(reply) => {
            if let InstallOutcome::Started(started) = &mut reply.outcome {
                accepted(redactor, Some(started.as_mut()));
            }
        }
        Reply::OperationGet(detail) => {
            let OperationDetail {
                operation,
                created: items,
                ..
            } = detail.as_mut();
            summary(redactor, operation);
            created(redactor, items);
        }
        Reply::OperationList(page) => {
            for item in &mut page.items {
                summary(redactor, item);
            }
        }
        Reply::OperationCancel(reply) => summary(redactor, &mut reply.operation),
        Reply::OperationResume(operation) => summary(redactor, operation),
        Reply::OperationAbandon(reply) => {
            summary(redactor, &mut reply.operation);
            created(redactor, &mut reply.leftovers);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use stackly_platform::SecretValue;
    use stackly_protocol::codes::request;

    use super::*;

    fn redactor() -> Redactor {
        let redactor = Redactor::new();
        redactor.learn(&SecretValue::new("fixture-secret-0001".to_owned()).unwrap());
        redactor
    }

    #[test]
    fn an_error_loses_a_known_value_in_every_text() {
        let body = ErrorBody::new(&request::INVALID_PARAMS, "saw fixture-secret-0001 here")
            .with_hint("not fixture-secret-0001")
            .with_detail("method", "fixture-secret-0001");

        let redacted = error(&redactor(), body);

        let json = serde_json::to_string(&redacted).unwrap();
        assert!(!json.contains("fixture-secret-0001"), "{json}");
        assert_eq!(redacted.message(), "saw **** here");
    }

    fn progress_summary(stage: &str) -> OperationSummary {
        serde_json::from_value(serde_json::json!({
            "id": "0192f3a4-0000-7000-8000-000000000001",
            "kind": "install_runtime",
            "resource": null,
            "state": "failed",
            "stage": stage,
            "progress": { "type": "indeterminate" },
            "waitingOn": null,
            "cancelRequestedAt": null,
            "pointOfNoReturnAt": null,
            "failure": {
                "reason": "step_failed",
                "retryable": false,
                "error": {
                    "code": "internal.unexpected",
                    "category": "internal",
                    "message": "step failed with fixture-secret-0001",
                    "retryable": false,
                    "details": [],
                    "fields": [],
                    "items": []
                }
            },
            "recovery": null,
            "result": {
                "resources": [],
                "partial": [{
                    "item": "file fixture-secret-0001",
                    "outcome": "failed",
                    "code": "fixture-secret-0001"
                }]
            },
            "trigger": { "origin": "user", "client": "cli" },
            "createdAt": "2026-10-19T08:00:00.000Z",
            "startedAt": null,
            "updatedAt": "2026-10-19T08:00:06.500Z",
            "revision": 5
        }))
        .expect("the summary decodes")
    }

    #[test]
    fn an_operation_summary_loses_a_known_value_in_its_stage_failure_and_partial_result() {
        let redactor = redactor();
        let mut summary_one = progress_summary("copying fixture-secret-0001");

        summary(&redactor, &mut summary_one);

        let json = serde_json::to_string(&summary_one).unwrap();
        assert!(!json.contains("fixture-secret-0001"), "{json}");
        assert_eq!(summary_one.stage.as_deref(), Some("copying ****"));
    }

    #[test]
    fn every_operation_reply_is_redacted_and_another_reply_is_left_alone() {
        let redactor = redactor();
        let mut page = Reply::OperationList(stackly_protocol::operation::OperationPage {
            items: vec![progress_summary("copying fixture-secret-0001")],
            next: None,
        });
        let mut other = Reply::EventsUnsubscribe;

        operation_reply(&redactor, &mut page);
        operation_reply(&redactor, &mut other);

        let json = serde_json::to_string(&page).unwrap();
        assert!(!json.contains("fixture-secret-0001"), "{json}");
        assert_eq!(other, Reply::EventsUnsubscribe);
    }

    /// Every `operation` object in `value` swapped for the summary of
    /// [`progress_summary`].
    fn with_secret_summaries(value: &mut serde_json::Value, found: &mut usize) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, inner) in map.iter_mut() {
                    if key == "operation" && inner.is_object() {
                        *inner =
                            serde_json::to_value(progress_summary("copying fixture-secret-0001"))
                                .unwrap();
                        *found += 1;
                    } else {
                        with_secret_summaries(inner, found);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for inner in items {
                    with_secret_summaries(inner, found);
                }
            }
            _ => {}
        }
    }

    /// The reply of the response frame in `protocols/fixtures/ipc/<name>`,
    /// with every operation summary in it carrying a known value.
    fn fixture_reply_with_secret(name: &str) -> Reply {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/ipc")
            .join(name);
        let mut frame: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut found = 0;
        with_secret_summaries(&mut frame, &mut found);
        assert_eq!(found, 1, "{name} carries one operation summary");
        let reply = frame["content"]["reply"].take();
        serde_json::from_value(reply).unwrap()
    }

    #[test]
    fn the_summary_of_every_reply_that_starts_an_operation_is_redacted() {
        let redactor = redactor();
        for name in [
            "frame-response-instance-start.json",
            "frame-response-instance-config-apply.json",
            "frame-response-instance-change-endpoints-applied.json",
            "frame-response-runtime-install-started.json",
        ] {
            let mut reply = fixture_reply_with_secret(name);
            let before = serde_json::to_string(&reply).unwrap();
            assert!(before.contains("fixture-secret-0001"), "{name}: {before}");

            operation_reply(&redactor, &mut reply);

            let json = serde_json::to_string(&reply).unwrap();
            assert!(!json.contains("fixture-secret-0001"), "{name}: {json}");
            assert!(json.contains("copying ****"), "{name}: {json}");
        }
    }

    #[test]
    fn the_code_of_a_partial_item_is_redacted_when_it_holds_a_known_value() {
        let redactor = redactor();
        let mut summary_one = progress_summary("copying");

        summary(&redactor, &mut summary_one);

        let partial = &summary_one.result.as_ref().unwrap().partial[0];
        assert_eq!(partial.code.as_deref(), Some("****"));
        assert_eq!(partial.item, "file ****");
    }
}
