//! `instance.preview_stop_all` and `instance.stop_all` (spec 0031, *Pilihan
//! stop all* and *Operasi `stop_all_instances`*). Both choose instances
//! through `stop_all::select`. The preview only reads. The method starts one
//! operation `stop_all_instances` that holds the lock keys of its instances,
//! asks the open starts and restarts of those instances to cancel, and
//! answers once the operation ended, from its end state, so a repeated
//! request gets the same answer.

use std::time::Duration;

use stackly_core::instance::stop_all::{
    DemandSource, StopAllEntry as CoreEntry, confirmed_steps, read_view, select,
};
use stackly_core::operation::{
    BeginRequest, BusyPolicy, OperationDetailView, OperationOutcome, Origin, PartialOutcome,
};
use stackly_core::supervisor::{StopAllRequest, Supervisor};
use stackly_domain::{
    InstanceId, JournalPayload, LockKey, OperationId, OperationKind, OperationResource,
    OperationState,
};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::{InstanceAutostart, StartupPolicyKind};
use stackly_protocol::message::Call;
use stackly_protocol::stop_all::{
    DemandSourceKind, InstanceRef, StopAllEntry, StopAllItem, StopAllParams, StopAllPreview,
    StopAllResult, StopAllSkip,
};
use stackly_protocol::{Count, ErrorBody};
use uuid::Uuid;

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::instance::dto::lifecycle;
use crate::ipc::operation::dto::failure_body;
use crate::ipc::operation::{
    StartRequest as OperationStart, payload_too_large, replay_operation, start_operation,
};
use crate::map::detail::startup_kind;
use crate::session::autostart_of;
use crate::state::DaemonCore;

/// How often the handler looks at the operation it waits for.
const POLL: Duration = Duration::from_millis(20);

/// The method name the idempotency record keeps for `instance.stop_all`.
const METHOD: &str = "instance.stop_all";

pub(crate) fn unexpected(detail: &'static str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(
        &stackly_protocol::codes::internal::UNEXPECTED,
        detail,
    ))
}

/// `instance.stop_all_changed`: what the person saw is not what would stop.
fn changed() -> Box<ErrorBody> {
    Box::new(ErrorBody::new(
        &stackly_protocol::codes::instance::STOP_ALL_CHANGED,
        "the instances to stop changed since the preview",
    ))
}

fn demand_kind(source: DemandSource) -> DemandSourceKind {
    match source {
        DemandSource::Manual => DemandSourceKind::Manual,
        DemandSource::Startup => DemandSourceKind::Startup,
        _ => DemandSourceKind::Environment,
    }
}

fn item(entry: &CoreEntry) -> StopAllItem {
    StopAllItem {
        instance_id: entry.id.as_uuid(),
        name: entry.name.to_string(),
        lifecycle: lifecycle(entry.lifecycle),
        demand: entry.demand.iter().copied().map(demand_kind).collect(),
        autostart: InstanceAutostart {
            policy: startup_kind(entry.startup),
            session: autostart_of(entry.session_lease),
        },
    }
}

/// The selection the registry gives now.
pub(crate) async fn current_selection(
    core: &DaemonCore,
) -> Result<stackly_core::instance::stop_all::StopAllSelection, Box<ErrorBody>> {
    let registry = core.registry().ok_or_else(|| {
        Box::new(ErrorBody::new(
            &stackly_protocol::codes::registry::UNAVAILABLE,
            "the registry is not available",
        ))
    })?;
    let session = core.session().login().map(|login| login.id.clone());
    let view = tokio::task::spawn_blocking(move || read_view(&registry, session.as_ref()))
        .await
        .map_err(|_| unexpected("the stop all read task failed"))?
        .map_err(|error| Box::new(crate::map::registry_error(&error)))?;
    Ok(select(&view))
}

/// The real `instance.preview_stop_all`.
pub fn preview(core: &DaemonCore) -> HandlerFuture<'_, Box<StopAllPreview>> {
    Box::pin(async move {
        core.supervision().supervisor()?;
        let selection = current_selection(core).await?;
        let items: Vec<StopAllItem> = selection.stop.iter().map(item).collect();
        let at_login = items
            .iter()
            .filter(|item| item.autostart.policy == StartupPolicyKind::AtLogin)
            .count();
        Ok(Box::new(StopAllPreview {
            items,
            already_stopping: selection
                .already_stopping
                .iter()
                .map(|entry| InstanceRef {
                    instance_id: entry.id.as_uuid(),
                    name: entry.name.to_string(),
                })
                .collect(),
            skipped: selection
                .skipped
                .iter()
                .map(|entry| StopAllSkip {
                    instance_id: entry.id.as_uuid(),
                    name: entry.name.to_string(),
                    code: entry.code.to_string(),
                })
                .collect(),
            at_login_count: Count::saturating(at_login as u64),
        }))
    })
}

/// What the journal input of a `stop_all_instances` holds.
pub(crate) struct StopAllInput {
    /// Who asks: `stop_all` or `agent_off`. A parsed input leaves it empty.
    pub(crate) source: &'static str,
    pub(crate) steps: Vec<Uuid>,
    pub(crate) skipped: Vec<(Uuid, String)>,
}

impl StopAllInput {
    pub(crate) fn to_json(&self) -> String {
        serde_json::json!({
            "source": self.source,
            "steps": self.steps.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "preSkipped": self
                .skipped
                .iter()
                .map(|(id, code)| serde_json::json!({ "instanceId": id.to_string(), "code": code }))
                .collect::<Vec<_>>(),
        })
        .to_string()
    }

    fn parse(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let uuids = |key: &str| -> Option<Vec<Uuid>> {
            value
                .get(key)?
                .as_array()?
                .iter()
                .map(|id| id.as_str()?.parse().ok())
                .collect()
        };
        let steps = uuids("steps")?;
        let skipped = value
            .get("preSkipped")?
            .as_array()?
            .iter()
            .map(|row| {
                Some((
                    row.get("instanceId")?.as_str()?.parse().ok()?,
                    row.get("code")?.as_str()?.to_owned(),
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            source: "",
            steps,
            skipped,
        })
    }
}

/// The reply of an operation that ended, from its state, its input, and its
/// result: the same for the first answer and for a replay (spec 0031).
pub(crate) fn reply_of(detail: &OperationDetailView) -> Result<StopAllResult, Box<ErrorBody>> {
    let journal = &detail.view.journal;
    let id = detail.view.operation.id();
    let input = StopAllInput::parse(journal.input().as_str())
        .ok_or_else(|| unexpected("the stop all input cannot be read"))?;
    let entry = |instance_id: Uuid, code: &str| StopAllEntry {
        instance_id,
        code: code.to_owned(),
    };
    let mut result = StopAllResult {
        stopped: Vec::new(),
        failed: Vec::new(),
        skipped: input
            .skipped
            .iter()
            .map(|(instance, code)| entry(*instance, code))
            .collect(),
    };
    match detail.view.operation.state() {
        OperationState::Succeeded => {
            let outcome = journal
                .result()
                .map(OperationOutcome::from_payload)
                .transpose()
                .map_err(|_| unexpected("the stop all result cannot be read"))?
                .unwrap_or_default();
            for resource in &outcome.resources {
                if let stackly_domain::ResourceRef::Instance(instance) = resource {
                    result.stopped.push(instance.as_uuid());
                }
            }
            for part in &outcome.partial {
                let Some(stackly_domain::ResourceRef::Instance(instance)) = &part.resource else {
                    continue;
                };
                let code = part.code.clone().unwrap_or_default();
                match part.outcome {
                    PartialOutcome::Failed => result.failed.push(entry(instance.as_uuid(), &code)),
                    _ => result.skipped.push(entry(instance.as_uuid(), &code)),
                }
            }
        }
        OperationState::Cancelled => {
            for instance in &input.steps {
                result.skipped.push(entry(*instance, "operation.cancelled"));
            }
        }
        _ => {
            return Err(Box::new(match journal.failure_error() {
                Some(error) => failure_body(error, id),
                None => ErrorBody::new(
                    &stackly_protocol::codes::internal::UNEXPECTED,
                    "stop all failed without a recorded error",
                ),
            }));
        }
    }
    Ok(result)
}

/// Waits until the operation is final and reads it.
pub(crate) async fn wait_final(
    core: &DaemonCore,
    operation: Uuid,
) -> Result<OperationDetailView, Box<ErrorBody>> {
    let engine = core.operations().writer()?;
    let id = OperationId::from_uuid(operation)
        .map_err(|_| unexpected("the operation ID is not a version 7 UUID"))?;
    loop {
        match engine.get(id).await {
            Ok(Some(detail)) if detail.view.operation.state().is_final() => return Ok(detail),
            Ok(Some(_)) => {}
            Ok(None) => return Err(unexpected("the operation vanished")),
            Err(_) => return Err(unexpected("the operation cannot be read")),
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The real `instance.stop_all`.
pub fn stop_all<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: StopAllParams,
) -> HandlerFuture<'a, Box<StopAllResult>> {
    let context = context.clone();
    Box::pin(async move {
        // The expected set is a set: its order and repeats do not make
        // another request.
        let mut params = params;
        if let Some(expected) = params.expected_instance_ids.as_mut() {
            expected.sort_unstable();
            expected.dedup();
        }
        let expected = params.expected_instance_ids.clone();
        let fingerprint = request_fingerprint(&Call::InstanceStopAll(params));
        let start = || OperationStart::new(METHOD, context.idempotency_key.as_deref(), fingerprint);
        // A request that came again with its key answers from the operation
        // it started, also while that still runs.
        if let Some(replay) = replay_operation(core, &context, start()).await? {
            let detail = wait_final(core, replay.accepted.operation.id).await?;
            return reply_of(&detail).map(Box::new);
        }
        let supervisor: Supervisor = core.supervision().supervisor()?;
        let selection = current_selection(core).await?;
        // An ID that is no instance ID can match nothing the rule chose.
        let expected = expected
            .map(|ids| {
                ids.into_iter()
                    .map(InstanceId::from_uuid)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
            .map_err(|_| changed())?;
        let steps = confirmed_steps(&selection, expected.as_deref()).map_err(|_| changed())?;
        let input = StopAllInput {
            source: "stop_all",
            steps: steps.iter().map(|id| id.as_uuid()).collect(),
            skipped: selection
                .skipped
                .iter()
                .map(|entry| (entry.id.as_uuid(), entry.code.to_string()))
                .collect(),
        };
        let input =
            JournalPayload::new(input.to_json(), stackly_core::operation::limits::INPUT_MAX)
                .map_err(|_| {
                    Box::new(payload_too_large(
                        "input",
                        stackly_core::operation::limits::INPUT_MAX,
                    ))
                })?;
        let work = supervisor.stop_all_work(StopAllRequest::new(steps.clone()));
        let locks: Vec<LockKey> = steps
            .iter()
            .map(|id| LockKey::for_resource(OperationResource::Instance(*id)))
            .collect();
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::StopAllInstances,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: locks,
            // The operation waits for what still holds an instance.
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work,
            precheck: None,
        };
        let started = start_operation(core, &context, start(), request).await?;
        let Some(operation) = started.operation_id() else {
            return Err(unexpected("stop all started no operation"));
        };
        supervisor.cancel_open_starts(&steps).await;
        let detail = wait_final(core, operation).await?;
        reply_of(&detail).map(Box::new)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_round_trips_through_its_json() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let input = StopAllInput {
            source: "stop_all",
            steps: vec![first],
            skipped: vec![(second, "supervisor.identity_unproven".to_owned())],
        };

        let parsed = StopAllInput::parse(&input.to_json()).expect("the input parses");

        assert_eq!(parsed.steps, vec![first]);
        assert_eq!(
            parsed.skipped,
            vec![(second, "supervisor.identity_unproven".to_owned())]
        );
    }
}
