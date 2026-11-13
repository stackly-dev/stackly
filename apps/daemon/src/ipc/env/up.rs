//! `env.up` (spec 0046, *Precheck Up*): replay, read and plan the manifest
//! with its lock and local overrides, then begin one `environment_up`
//! coordinator whose precheck refuses an invalid manifest, a blocked plan,
//! a plan that changed, and references that have no value, before any
//! operation or mutation exists. The reply comes as soon as the operation
//! is accepted; clients follow it through the operation events.

use std::sync::Arc;

use serde_json::json;
use stackly_core::environment::up::{DriverDeps, PlanRequest, UpRequest, check, up_work};
use stackly_core::environment::{EnvOpError, Registration};
use stackly_core::operation::{BeginRequest, BusyPolicy, Origin, Precheck, PrecheckOutcome};
use stackly_domain::{
    EnvironmentId, JournalPayload, LockKey, OperationKind, OperationResource, Sha256Digest,
};
use stackly_protocol::ErrorBody;
use stackly_protocol::env::{EnvUpParams, EnvUpReply};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::message::Call;
use uuid::Uuid;

use super::children::planned_up;
use super::{plan_source, registry_of, unexpected, with_env_hint};
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::{
    StartOutcome, StartRequest as OperationStart, operation_error_body, replay_operation,
    start_operation,
};
use crate::state::DaemonCore;

/// `env.up` (AC-1 to AC-3).
pub fn up<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: EnvUpParams,
) -> HandlerFuture<'a, Box<EnvUpReply>> {
    let context = context.clone();
    Box::pin(async move {
        up_inner(core, &context, params)
            .await
            .map_err(|error| Box::new(with_env_hint(*error)))
    })
}

/// The environment and name a journal `input` of an Up records.
fn journal_environment(input: &JournalPayload) -> Option<(Uuid, String)> {
    let value: serde_json::Value = serde_json::from_str(input.as_str()).ok()?;
    let id = value.get("environmentId")?.as_str()?.parse().ok()?;
    let name = value.get("name")?.as_str()?.to_owned();
    Some((id, name))
}

async fn up_inner(
    core: &DaemonCore,
    context: &RequestContext,
    params: EnvUpParams,
) -> Result<Box<EnvUpReply>, Box<ErrorBody>> {
    let fingerprint = request_fingerprint(&Call::EnvUp(params.clone()));
    let key = context.idempotency_key.as_deref();
    let start = OperationStart::new("env.up", key, fingerprint);

    // A repeated request gets its first reply before anything is read
    // (spec 0007, *Idempotency*).
    if let Some(replay) = replay_operation(core, context, start).await? {
        let (environment_id, name) = journal_environment(&replay.input)
            .ok_or_else(|| unexpected("the journal of the Up holds no environment"))?;
        return Ok(Box::new(EnvUpReply {
            environment_id,
            name,
            operation: replay.accepted,
        }));
    }

    let registry = registry_of(core)?;
    let (file, registered) = plan_source(&registry, &params.source).await?;
    let expected: Sha256Digest = params
        .expected_plan_digest
        .parse()
        .map_err(|_| unexpected("a validated plan digest does not parse"))?;
    let plan_request = PlanRequest {
        path: file.clone().into(),
        registered,
        locked: params.locked,
    };
    let planned = planned_up(core, &plan_request)
        .await
        .map_err(super::PlanFailure::into_body)?;

    // A manifest without a valid name has nothing to lock or run on: the
    // precheck's own answer, given before the begin.
    let Some(name) = planned.validation.name.clone() else {
        let refusal = check(&planned, expected).err().unwrap_or(
            stackly_core::environment::up::UpError::Environment(EnvOpError::Invalid { errors: 0 }),
        );
        return Err(Box::new(operation_error_body(&refusal)));
    };
    let known = planned
        .plan
        .environment
        .as_ref()
        .filter(|_| planned.plan.registration == Some(Registration::Registered));
    let (environment, creates, expected_revision) = match known {
        Some(reference) => (reference.id, false, Some(reference.revision)),
        None => (EnvironmentId::generate(), true, None),
    };
    let manifest_sha256 = planned.validation.sha256;
    let source = serde_json::to_value(&params.source)
        .map_err(|_| unexpected("the source of the Up does not encode"))?;
    let input = JournalPayload::new(
        json!({
            "source": source,
            "environmentId": environment.to_string(),
            "name": name.as_str(),
            "manifestSha256": manifest_sha256.to_string(),
            "planDigest": expected.to_string(),
            "locked": params.locked,
        })
        .to_string(),
        stackly_domain::OperationJournal::INPUT_MAX,
    )
    .map_err(|_| unexpected("the input of the Up does not fit the journal"))?;

    let ports = core
        .env_ports()
        .ok_or_else(|| unexpected("the ports of Up are not set"))?;
    let engine = core.operations().writer()?;
    let work = up_work(
        UpRequest {
            environment,
            name: name.clone(),
            plan: plan_request,
            manifest_sha256,
            caller: crate::ipc::secret::caller(context),
            home: core.home_mask().clone(),
        },
        DriverDeps {
            engine,
            children: Arc::clone(&ports) as Arc<dyn stackly_core::environment::up::UpChildren>,
            reads: Arc::clone(&ports) as Arc<dyn stackly_core::environment::up::UpReads>,
            board: Arc::clone(core.up_board()),
        },
    );
    let reply_name = name.to_string();
    let precheck: Box<dyn Precheck> = Box::new(move || match check(&planned, expected) {
        Ok(()) => PrecheckOutcome::Proceed,
        Err(error) => PrecheckOutcome::Reject(Box::new(error)),
    });
    let request = move |origin: Origin| BeginRequest {
        kind: OperationKind::EnvironmentUp,
        resource: OperationResource::Environment(environment),
        creates_resource: creates,
        extra_locks: vec![LockKey::EnvironmentName(name)],
        // A second Up of the same environment is refused with the first's
        // ID (AC-3).
        busy: BusyPolicy::Reject,
        expected_revision,
        origin,
        input,
        work,
        precheck: Some(precheck),
    };
    let outcome = start_operation(core, context, start, request).await?;
    let StartOutcome::Accepted(accepted) = outcome else {
        return Err(unexpected("an Up answered without an operation"));
    };
    ports.reload_membership_after(accepted.operation.id);
    Ok(Box::new(EnvUpReply {
        environment_id: environment.as_uuid(),
        name: reply_name,
        operation: *accepted,
    }))
}
