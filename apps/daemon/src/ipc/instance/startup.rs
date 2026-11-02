//! `instance.set_startup` (spec 0031, AC-16): the policy that decides
//! whether an instance starts at login. It follows `instance.rename`: a
//! precheck answers what the state decides, and only a real change begins
//! the operation `set_instance_startup`. Unlike rename it replies with the
//! instance, so it waits for the operation to end.

use std::time::Duration;

use stackly_core::operation::{
    BeginRequest, BusyPolicy, NoOpReply, Origin, Precheck, PrecheckOutcome,
};
use stackly_core::supervisor::{SetStartupRequest, SupervisorError};
use stackly_domain::{
    InstanceLifecycle, JournalPayload, OperationId, OperationJournal, OperationKind,
    OperationResource, StartupPolicy,
};
use stackly_protocol::ErrorBody;
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::{InstanceSummary, SetStartupParams, StartupPolicyKind};
use stackly_protocol::message::Call;

use super::{
    expected_revision, instance_id, read_for_precheck, read_instance, registry_of, reject,
    summary_of, unexpected,
};
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::dto::failure_body;
use crate::ipc::operation::{StartOutcome, StartRequest as OperationStart, start_operation};
use crate::state::DaemonCore;

/// How often the handler looks at the operation it waits for.
const POLL: Duration = Duration::from_millis(20);

/// The domain policy of a wire policy, or `None` for one this build does
/// not know.
const fn policy_of(kind: StartupPolicyKind) -> Option<StartupPolicy> {
    match kind {
        StartupPolicyKind::Manual => Some(StartupPolicy::Manual),
        StartupPolicyKind::AtLogin => Some(StartupPolicy::AtLogin),
        StartupPolicyKind::Unrecognized => None,
    }
}

/// The real `instance.set_startup`.
pub fn set_startup<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: SetStartupParams,
) -> HandlerFuture<'a, InstanceSummary> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let expected = expected_revision(params.expected_revision)?;
        let startup = policy_of(params.startup).ok_or_else(|| {
            Box::new(
                ErrorBody::new(
                    &stackly_protocol::codes::request::INVALID_PARAMS,
                    "startup is not a startup policy",
                )
                .with_field(stackly_protocol::FieldError::new(
                    "startup",
                    stackly_protocol::json::FieldCode::InvalidValue,
                )),
            )
        })?;
        let session = core.session().login().map(|login| login.id.clone());
        let fingerprint = request_fingerprint(&Call::InstanceSetStartup(params));
        let work = supervisor.set_startup_work(SetStartupRequest::new(id, startup, session));
        let input = JournalPayload::new(
            serde_json::json!({ "startup": startup.as_code() }).to_string(),
            OperationJournal::INPUT_MAX,
        )
        .unwrap_or_else(|_| JournalPayload::empty());
        let precheck: Box<dyn Precheck> =
            Box::new(move || startup_precheck(&registry, id, expected, startup));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::SetInstanceStartup,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "instance.set_startup",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        if let StartOutcome::Accepted(accepted) = &outcome {
            wait_final(core, accepted.operation.id).await?;
        }
        let instance = read_instance(core, id).await?;
        // A policy of `manual` released the lease of this session; the
        // event that follows tells clients the new state.
        if startup == StartupPolicy::Manual {
            core.supervision()
                .autostart()
                .set(id, stackly_protocol::instance::AutostartSession::None);
        }
        if matches!(outcome, StartOutcome::Accepted(_))
            && let Some(events) = core.supervision().events()
        {
            stackly_core::supervisor::SupervisorEvents::instance_changed(&*events, &instance);
        }
        Ok(summary_of(core, &supervisor, &instance))
    })
}

/// Refuses what the state decides and answers the same policy with nothing
/// to do. The commit checks again.
fn startup_precheck(
    registry: &stackly_storage::SqliteRegistry,
    id: stackly_domain::InstanceId,
    expected: Option<stackly_domain::ResourceRevision>,
    startup: StartupPolicy,
) -> PrecheckOutcome {
    let instance = match read_for_precheck(registry, id, expected) {
        Ok(instance) => instance,
        Err(rejected) => return rejected,
    };
    if instance.lifecycle() == InstanceLifecycle::Retained {
        return reject(SupervisorError::InstanceRetained);
    }
    if instance.startup() == startup {
        return PrecheckOutcome::NoOp(NoOpReply::nothing());
    }
    PrecheckOutcome::Proceed
}

/// Waits until the operation is final; a failed one answers with its own
/// error.
async fn wait_final(core: &DaemonCore, operation: uuid::Uuid) -> Result<(), Box<ErrorBody>> {
    let engine = core.operations().writer()?;
    let id = OperationId::from_uuid(operation)
        .map_err(|_| unexpected("the operation ID is not a version 7 UUID"))?;
    loop {
        match engine.get(id).await {
            Ok(Some(detail)) if detail.view.operation.state().is_final() => {
                return match detail.view.journal.failure_error() {
                    Some(error) => Err(Box::new(failure_body(error, id))),
                    None => Ok(()),
                };
            }
            Ok(Some(_)) => {}
            Ok(None) => return Err(unexpected("the operation vanished")),
            Err(_) => return Err(unexpected("the operation cannot be read")),
        }
        tokio::time::sleep(POLL).await;
    }
}
