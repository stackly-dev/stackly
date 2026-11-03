//! The part of `daemon.shutdown` `agent_off` that stops the instances
//! (spec 0031, *Matikan agent*). The handler of the method calls
//! [`agent_off`] and builds `ShutdownAccepted` from its outcome.
//!
//! In order: a repeated request with the same key waits for the operation
//! it started and gets the same answer; a request that cannot be taken is
//! refused with no effect (`registry.recovery_mode`, `daemon.restart_pending`,
//! `daemon.shutting_down`); the quiesce gate closes to every new client
//! operation; one `stop_all_instances` stops the instances `stop_all::select`
//! chose, together with the ones that are `already_stopping`, so the daemon
//! does not exit while a stop still runs; and the selection runs once more.
//! The daemon exits only when that last selection is empty, otherwise it
//! stays up and lifts the gate.

use stackly_core::operation::{BeginRequest, BusyPolicy, Origin};
use stackly_core::supervisor::StopAllRequest;
use stackly_domain::{InstanceId, JournalPayload, LockKey, OperationKind, OperationResource};
use stackly_protocol::codes::{daemon, registry};
use stackly_protocol::daemon::{ShutdownAccepted, ShutdownMode, ShutdownParams};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::stop_all::StopAllResult;
use stackly_protocol::{Call, ErrorBody};

use super::operation::{
    StartRequest as OperationStart, payload_too_large, replay_operation,
    start_operation_bypassing_quiesce,
};
use super::restart::restart_pending;
use super::stop_all::{StopAllInput, current_selection, reply_of, unexpected, wait_final};
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::state::DaemonCore;

/// What `agent_off` found out, for `ShutdownAccepted`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentOffOutcome {
    /// What the operation stopped, failed on, and skipped.
    pub stop_all: StopAllResult,
    /// Whether the daemon goes on to exit. When true the stop of the daemon
    /// was requested already, and the gate stays closed.
    pub exiting: bool,
}

/// Closes the gate and the claim again unless the daemon is exiting.
struct Gate<'a> {
    core: &'a DaemonCore,
    engine: stackly_core::operation::OperationEngine,
    keep: bool,
}

impl<'a> Gate<'a> {
    fn close(core: &'a DaemonCore, engine: stackly_core::operation::OperationEngine) -> Self {
        engine.set_quiesce(true);
        Self {
            core,
            engine,
            keep: false,
        }
    }

    /// The daemon exits: the gate stays closed.
    fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for Gate<'_> {
    fn drop(&mut self) {
        if !self.keep {
            self.engine.set_quiesce(false);
            self.core.end_agent_off();
        }
    }
}

/// The real `daemon.shutdown` with `mode: agent_off` (spec 0031, *Matikan
/// agent*): the same in a launchd daemon and a manual one.
pub fn shutdown<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ShutdownParams,
) -> HandlerFuture<'a, ShutdownAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let fingerprint = request_fingerprint(&Call::DaemonShutdown(params));
        let outcome = agent_off(core, &context, "daemon.shutdown", fingerprint).await?;
        Ok(ShutdownAccepted {
            server_instance_id: core.start_info().server_instance_id,
            mode: ShutdownMode::AgentOff,
            plan_id: None,
            exiting: outcome.exiting,
            stop_all: Some(outcome.stop_all),
        })
    })
}

/// Runs `agent_off` for the request `context` with the idempotency
/// fingerprint `fingerprint` of the whole `daemon.shutdown` call.
///
/// # Errors
///
/// Returns `registry.recovery_mode` in recovery mode,
/// `daemon.restart_pending` while a planned restart quiesces or stops,
/// `daemon.shutting_down` when a stop was accepted or another `agent_off`
/// runs, and the error of the operation when it failed.
pub async fn agent_off(
    core: &DaemonCore,
    context: &RequestContext,
    method: &'static str,
    fingerprint: [u8; 32],
) -> Result<AgentOffOutcome, Box<ErrorBody>> {
    let start = || OperationStart::new(method, context.idempotency_key.as_deref(), fingerprint);
    // A key that came again waits for the operation it started, and gets its
    // answer. The daemon is exiting exactly when the first answer said so.
    if let Some(replay) = replay_operation(core, context, start()).await? {
        let detail = wait_final(core, replay.accepted.operation.id).await?;
        return Ok(AgentOffOutcome {
            stop_all: reply_of(&detail)?,
            exiting: core.shutdown_pending(),
        });
    }
    if core.in_recovery() {
        return Err(Box::new(ErrorBody::new(
            &registry::RECOVERY_MODE,
            "the registry is in recovery mode, so the daemon cannot choose or stop instances",
        )));
    }
    let supervisor = core.supervision().supervisor()?;
    let engine = core.operations().writer()?;
    if core.shutdown_pending() {
        return Err(shutting_down());
    }
    if engine.is_quiesced() && !core.agent_off_active() {
        return Err(restart_pending(None));
    }
    if !core.begin_agent_off() {
        return Err(shutting_down());
    }
    let mut gate = Gate::close(core, engine);

    let selection = current_selection(core).await?;
    let mut steps: Vec<_> = selection
        .stop
        .iter()
        .map(|entry| (entry.name.clone(), entry.id))
        .chain(
            selection
                .already_stopping
                .iter()
                .map(|entry| (entry.name.clone(), entry.id)),
        )
        .collect();
    steps.sort();
    let steps: Vec<InstanceId> = steps.into_iter().map(|(_, id)| id).collect();
    let input = StopAllInput {
        source: "agent_off",
        steps: steps.iter().map(|id| id.as_uuid()).collect(),
        skipped: selection
            .skipped
            .iter()
            .map(|entry| (entry.id.as_uuid(), entry.code.to_string()))
            .collect(),
    };
    let input = JournalPayload::new(input.to_json(), stackly_core::operation::limits::INPUT_MAX)
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
        busy: BusyPolicy::Queue,
        expected_revision: None,
        origin,
        input,
        work,
        precheck: None,
    };
    let started = start_operation_bypassing_quiesce(core, context, start(), request).await?;
    let Some(operation) = started.operation_id() else {
        return Err(unexpected("agent off started no operation"));
    };
    supervisor.cancel_open_starts(&steps).await;
    let detail = wait_final(core, operation).await?;
    let stop_all = reply_of(&detail)?;

    // The daemon exits only when nothing is left that could run without a
    // supervisor: no failure, no skip, and a last selection that finds
    // nothing to stop, nothing still stopping, and nothing it had to skip.
    let mut exiting = false;
    if stop_all.failed.is_empty() && stop_all.skipped.is_empty() {
        let last = current_selection(core).await?;
        exiting =
            last.stop.is_empty() && last.already_stopping.is_empty() && last.skipped.is_empty();
    }
    if exiting {
        // The first request counts; this one is the request that stops it.
        let _ = core.request_shutdown(ShutdownMode::Stop);
        gate.keep();
    }
    Ok(AgentOffOutcome { stop_all, exiting })
}

fn shutting_down() -> Box<ErrorBody> {
    Box::new(ErrorBody::new(
        &daemon::SHUTTING_DOWN,
        "the daemon is already stopping",
    ))
}
