//! `env.preview_down` and `env.down` (spec 0047, *Precheck Down*). The
//! preview is a read: `down::read_view` and `down::select`, with whether an
//! Up would be cancelled. `env.down` replays a repeated request, reads the
//! same view, and begins one `environment_down` coordinator whose begin
//! gate refuses `env.down_changed` when the stop set is not the one the
//! client confirmed. Right after the begin, a task of its own cancels every
//! `environment_up` of the environment that has not ended, so a client that
//! goes away does not leave Down queued behind a running Up.

use std::sync::Arc;

use serde_json::json;
use stackly_core::environment::down::{
    DownDeps, DownEntry, DownError, DownRequest, DownSelection, DownView, down_work, read_view,
    select,
};
use stackly_core::operation::{
    BeginRequest, BusyPolicy, OperationEngine, Origin, Precheck, PrecheckOutcome,
};
use stackly_core::supervisor::EnvironmentHolder;
use stackly_domain::{
    EnvironmentId, InstanceId, JournalPayload, LockKey, OperationKind, OperationQuery,
    OperationResource, OperationTrigger,
};
use stackly_protocol::env::{
    DownBlocked, DownInstance, DownKeep, EnvDownParams, EnvDownPreview, EnvDownReply,
    EnvPreviewDownParams, EnvironmentRef, InstanceEnvironment,
};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::message::Call;
use stackly_protocol::{Count, ErrorBody};
use uuid::Uuid;

use super::{not_found, registry_error, registry_of, unexpected, with_env_hint};
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::instance::dto::lifecycle;
use crate::ipc::operation::{
    StartOutcome, StartRequest as OperationStart, replay_operation, sender, start_operation,
};
use crate::membership::env_ownership;
use crate::state::DaemonCore;

fn environment_id(id: Uuid) -> Result<EnvironmentId, Box<ErrorBody>> {
    EnvironmentId::from_uuid(id).map_err(|_| not_found())
}

/// The view of `environment` now, or `env.not_found`.
async fn view_of(
    core: &DaemonCore,
    environment: EnvironmentId,
) -> Result<DownView, Box<ErrorBody>> {
    let registry = registry_of(core)?;
    tokio::task::spawn_blocking(move || read_view(&registry, environment))
        .await
        .map_err(|_| unexpected("a down read task failed"))?
        .map_err(|error| registry_error(&error))?
        .ok_or_else(not_found)
}

/// The `environment_up` operations of `environment` that have not ended.
async fn open_ups(
    engine: &OperationEngine,
    environment: EnvironmentId,
) -> Vec<stackly_domain::OperationId> {
    let query = OperationQuery {
        active_only: true,
        resource: Some(OperationResource::Environment(environment)),
        limit: OperationQuery::LIMIT_MAX,
        before: None,
    };
    let Ok(page) = engine.list(query).await else {
        return Vec::new();
    };
    page.items
        .iter()
        .filter(|view| view.operation.kind() == OperationKind::EnvironmentUp)
        .map(|view| view.operation.id())
        .collect()
}

fn instance(entry: &DownEntry) -> DownInstance {
    DownInstance {
        instance_id: entry.id.as_uuid(),
        name: entry.name.to_string(),
        alias: entry.member.as_ref().map(|member| member.alias.to_string()),
        ownership: entry
            .member
            .as_ref()
            .map(|member| env_ownership(member.ownership)),
        lifecycle: lifecycle(entry.lifecycle),
    }
}

/// An environment holder as an instance's environment entry; a holder that
/// has no member row for the instance is left out, its demand still shows.
fn holder(holder: &EnvironmentHolder) -> Option<InstanceEnvironment> {
    Some(InstanceEnvironment {
        environment_id: holder.environment.as_uuid(),
        name: holder.name.to_string(),
        alias: holder.alias.as_ref()?.to_string(),
        ownership: env_ownership(holder.ownership?),
        lease_active: true,
    })
}

fn demand_kind(
    source: stackly_core::instance::stop_all::DemandSource,
) -> stackly_protocol::stop_all::DemandSourceKind {
    use stackly_core::instance::stop_all::DemandSource;
    use stackly_protocol::stop_all::DemandSourceKind;
    match source {
        DemandSource::Manual => DemandSourceKind::Manual,
        DemandSource::Startup => DemandSourceKind::Startup,
        _ => DemandSourceKind::Environment,
    }
}

fn preview_of(view: &DownView, selection: &DownSelection, cancels_up: bool) -> EnvDownPreview {
    EnvDownPreview {
        environment: EnvironmentRef {
            id: view.environment.as_uuid(),
            name: view.name.to_string(),
            revision: crate::ipc::operation::dto::revision(view.revision),
        },
        stop: selection.stop.iter().map(instance).collect(),
        keep: selection
            .keep
            .iter()
            .map(|kept| DownKeep {
                instance: instance(&kept.entry),
                demand: kept.demand.iter().copied().map(demand_kind).collect(),
                environments: kept.environments.iter().filter_map(holder).collect(),
            })
            .collect(),
        idle: selection.idle.iter().map(instance).collect(),
        blocked: selection
            .blocked
            .iter()
            .map(|blocked| DownBlocked {
                instance: instance(&blocked.entry),
                code: blocked.code.to_string(),
            })
            .collect(),
        lease_count: Count::saturating(view.lease_count as u64),
        cancels_up,
    }
}

/// `env.preview_down` (AC-1): a read, not an operation.
pub fn preview_down(
    core: &DaemonCore,
    params: EnvPreviewDownParams,
) -> HandlerFuture<'_, Box<EnvDownPreview>> {
    Box::pin(async move {
        let environment = environment_id(params.id)?;
        let view = view_of(core, environment).await?;
        let selection = select(&view);
        let cancels_up = match core.operations().writer() {
            Ok(engine) => !open_ups(&engine, environment).await.is_empty(),
            Err(_) => false,
        };
        Ok(Box::new(preview_of(&view, &selection, cancels_up)))
    })
}

/// The environment and name the journal `input` of a Down records.
fn journal_environment(input: &JournalPayload) -> Option<(Uuid, String)> {
    let value: serde_json::Value = serde_json::from_str(input.as_str()).ok()?;
    let id = value.get("environmentId")?.as_str()?.parse().ok()?;
    let name = value.get("name")?.as_str()?.to_owned();
    Some((id, name))
}

/// `env.down` (AC-2, AC-3).
pub fn down<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: EnvDownParams,
) -> HandlerFuture<'a, Box<EnvDownReply>> {
    let context = context.clone();
    Box::pin(async move {
        down_inner(core, &context, params)
            .await
            .map_err(|error| Box::new(with_env_hint(*error)))
    })
}

async fn down_inner(
    core: &DaemonCore,
    context: &RequestContext,
    params: EnvDownParams,
) -> Result<Box<EnvDownReply>, Box<ErrorBody>> {
    let fingerprint = request_fingerprint(&Call::EnvDown(params.clone()));
    let key = context.idempotency_key.as_deref();
    let start = OperationStart::new("env.down", key, fingerprint);

    // A repeated request gets its first reply before anything is read.
    if let Some(replay) = replay_operation(core, context, start).await? {
        let (environment_id, name) = journal_environment(&replay.input)
            .ok_or_else(|| unexpected("the journal of the Down holds no environment"))?;
        return Ok(Box::new(EnvDownReply {
            environment_id,
            name,
            operation: replay.accepted,
        }));
    }

    let environment = environment_id(params.id)?;
    let view = view_of(core, environment).await?;
    let selection = select(&view);
    let stop = selection.stop_ids();
    let expected: Vec<InstanceId> = params
        .expected_stop_instance_ids
        .iter()
        .filter_map(|id| InstanceId::from_uuid(*id).ok())
        .collect();
    let all_parse = expected.len() == params.expected_stop_instance_ids.len();
    let name = view.name.clone();
    let input = JournalPayload::new(
        json!({
            "environmentId": environment.to_string(),
            "name": name.as_str(),
            "stop": stop.iter().map(ToString::to_string).collect::<Vec<_>>(),
        })
        .to_string(),
        stackly_domain::OperationJournal::INPUT_MAX,
    )
    .map_err(|_| unexpected("the input of the Down does not fit the journal"))?;

    let ports = core
        .env_ports()
        .ok_or_else(|| unexpected("the ports of Down are not set"))?;
    let engine = core.operations().writer()?;
    let work = down_work(
        DownRequest {
            environment,
            name: name.clone(),
            stop,
            caller: crate::ipc::secret::caller(context),
        },
        DownDeps {
            engine: engine.clone(),
            children: Arc::clone(&ports) as Arc<dyn stackly_core::environment::down::DownChildren>,
        },
    );
    let precheck: Box<dyn Precheck> = Box::new(move || {
        if all_parse && selection.matches(&expected) {
            PrecheckOutcome::Proceed
        } else {
            PrecheckOutcome::Reject(Box::new(DownError::Changed))
        }
    });
    let lock = name.clone();
    let request = move |origin: Origin| BeginRequest {
        kind: OperationKind::EnvironmentDown,
        resource: OperationResource::Environment(environment),
        creates_resource: false,
        extra_locks: vec![LockKey::EnvironmentName(lock)],
        // Down waits for the operation that holds the environment (AC-3).
        busy: BusyPolicy::Queue,
        expected_revision: None,
        origin,
        input,
        work,
        precheck: Some(precheck),
    };
    let outcome = start_operation(core, context, start, request).await?;
    let StartOutcome::Accepted(accepted) = outcome else {
        return Err(unexpected("a Down answered without an operation"));
    };
    // Cancel every Up of the environment that has not ended, in a task of
    // its own, started before the reply goes out (AC-3).
    let by: OperationTrigger = sender(context);
    let down = accepted.operation.id;
    tokio::spawn(async move {
        for up in open_ups(&engine, environment).await {
            if let Err(error) = engine.cancel(up, by.clone()).await {
                tracing::debug!(%error, %up, %down, "an up a down cancels had ended");
            }
        }
    });
    ports.reload_membership_after(accepted.operation.id);
    Ok(Box::new(EnvDownReply {
        environment_id: environment.as_uuid(),
        name: name.to_string(),
        operation: *accepted,
    }))
}
