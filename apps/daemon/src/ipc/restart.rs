//! The planned daemon restart (spec 0030): `daemon.restart_preview`, the
//! `restart` mode of `daemon.shutdown`, and the DTO of a restart plan.
//!
//! A restart is one `daemon_restart` operation. The handler computes the
//! plan, quiesces the operation engine, waits for the operations in
//! flight, and begins the operation, which writes the plan and stops the
//! instances the plan stops. The handler's only reply is
//! [`ShutdownAccepted`]; the plan's progress follows as `daemon.restart_plan`
//! events, and the daemon exits 75 once the plan is handed over.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use stackly_core::operation::{ActiveOperation, BeginRequest, BusyPolicy, OperationEngine, Origin};
use stackly_core::supervisor::{
    DaemonRestartRequest, PlanEntry, PlanPrepared, PlanRefusal, RestartHandover, plan_digest,
    plan_entries,
};
use stackly_domain::{
    InstanceFilter, JournalPayload, LockKey, OperationId, OperationKind, OperationResource,
    PlanDecision, RestartPlan, RestartPlanId, Sha256Digest,
};
use stackly_protocol::codes::daemon;
use stackly_protocol::daemon::{LaunchContextKind, ShutdownAccepted, ShutdownMode, ShutdownParams};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::message::Call;
use stackly_protocol::restart::{
    BlockingOperation, RestartPlanInstance, RestartPlanView, RestartPreview, RestartPreviewInstance,
};
use stackly_protocol::{ErrorBody, ErrorItem, Timestamp};
use tokio::sync::oneshot;
use tokio::time::Instant;
use uuid::Uuid;

use super::instance::dto::lifecycle;
use super::operation::{StartRequest, engine_error, replay_operation, start_operation};
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::state::{DaemonCore, ShutdownTrigger};
use crate::supervisor::Availability;

/// How long a finished plan stays in `daemon.get_info` (spec 0030).
pub const RESTART_SUMMARY_TTL: Duration = Duration::from_secs(10 * 60);
/// How often the wait for operations looks at the engine.
const POLL: Duration = Duration::from_millis(100);
/// Time the handler gives the operation to write the plan after the quiesce
/// wait, for an operation that waits for the lock of an instance.
const PREPARE_GRACE: Duration = Duration::from_secs(10);

/// The wire value whose code is `code`; every open enum of this module
/// reads an unknown code as `unrecognized`.
fn by_code<T: DeserializeOwned>(code: &str) -> T {
    serde_json::from_value(serde_json::Value::String(code.to_owned()))
        .expect("every open wire enum reads any string, an unknown one as unrecognized")
}

fn boxed(body: ErrorBody) -> Box<ErrorBody> {
    Box::new(body)
}

/// The launch context of this daemon.
#[must_use]
pub fn launch_context(core: &DaemonCore) -> LaunchContextKind {
    core.start_info()
        .launch
        .as_ref()
        .map_or(LaunchContextKind::Manual, |launch| launch.context)
}

/// A restart plan as clients see it.
#[must_use]
pub fn plan_view(plan: &RestartPlan) -> RestartPlanView {
    let stored = plan.stored();
    RestartPlanView {
        id: stored.id.as_uuid(),
        state: by_code(stored.state.as_code()),
        created_at: Timestamp::from_jiff(stored.created_at),
        handed_over_at: stored.handed_over_at.map(Timestamp::from_jiff),
        reconciled_at: stored.reconciled_at.map(Timestamp::from_jiff),
        finished_at: stored.finished_at.map(Timestamp::from_jiff),
        close_reason: stored.close_reason.map(|reason| by_code(reason.as_code())),
        instances: plan
            .instances()
            .iter()
            .map(|entry| RestartPlanInstance {
                instance_id: entry.instance_id.as_uuid(),
                name: entry.name.to_string(),
                decision: by_code(entry.decision.as_code()),
                reason: by_code(entry.reason.as_code()),
                stopped_at: entry.stopped_at.map(Timestamp::from_jiff),
                ready_at: entry.ready_at.map(Timestamp::from_jiff),
                outcome: entry.outcome.map(|outcome| by_code(outcome.as_code())),
                failure_code: entry.failure_code.as_ref().map(ToString::to_string),
            })
            .collect(),
    }
}

fn blocking_operation(active: &ActiveOperation) -> BlockingOperation {
    BlockingOperation {
        operation_id: active.id.as_uuid(),
        kind: by_code(active.kind.as_code()),
        resumable: active.resumable,
    }
}

/// The entries of a plan when no supervisor serves: every instance the
/// registry still lists is kept, for `supervisor_unavailable`.
async fn recovery_entries(core: &DaemonCore) -> Vec<PlanEntry> {
    let Some(registry) = core.registry() else {
        return Vec::new();
    };
    let read = tokio::task::spawn_blocking(move || {
        stackly_domain::Registry::instances(
            &registry,
            InstanceFilter {
                include_retained: false,
            },
        )
    })
    .await;
    match read {
        Ok(Ok(versioned)) => plan_entries(&versioned.value, false, &[]),
        _ => Vec::new(),
    }
}

/// The real `daemon.restart_preview`: the plan a restart asked for now
/// would write, and what would hold it up.
pub fn preview(core: &DaemonCore) -> HandlerFuture<'_, RestartPreview> {
    Box::pin(async move {
        let (entries, supervisor_available) = match core.supervision().availability() {
            Availability::Ready(supervisor) => (
                supervisor
                    .restart_plan_entries()
                    .await
                    .map_err(|error| boxed(super::operation::operation_error_body(&error)))?,
                true,
            ),
            Availability::Starting => {
                return Err(boxed(super::operation::starting(
                    stackly_protocol::daemon::subsystem::SUPERVISOR,
                )));
            }
            Availability::Unavailable => (recovery_entries(core).await, false),
        };
        let blocking_operations = match core.operations().engine() {
            Some(engine) => engine
                .active_operations()
                .await
                .map_err(|error| boxed(engine_error(&error)))?
                .iter()
                .map(blocking_operation)
                .collect(),
            None => Vec::new(),
        };
        Ok(RestartPreview {
            digest: plan_digest(&entries).to_string(),
            launch_context: launch_context(core),
            supervisor_available,
            instances: entries
                .iter()
                .map(|entry| RestartPreviewInstance {
                    instance_id: entry.instance_id.as_uuid(),
                    name: entry.name.to_string(),
                    provider: entry.provider.clone(),
                    lifecycle: lifecycle(entry.lifecycle),
                    decision: by_code(entry.decision.as_code()),
                    reason: by_code(entry.reason.as_code()),
                })
                .collect(),
            blocking_operations,
        })
    })
}

/// Lifts the quiesce when a restart ends early, unless it was handed
/// over.
struct Quiesce {
    engine: OperationEngine,
    armed: bool,
}

impl Quiesce {
    fn on(engine: OperationEngine) -> Self {
        engine.set_quiesce(true);
        Self {
            engine,
            armed: true,
        }
    }

    /// The plan is written: the work lifts the quiesce itself if it aborts.
    fn keep(mut self) {
        self.armed = false;
    }
}

impl Drop for Quiesce {
    fn drop(&mut self) {
        if self.armed {
            self.engine.set_quiesce(false);
        }
    }
}

/// Starts the daemon's stop order once the plan is handed over.
struct DaemonHandover {
    trigger: ShutdownTrigger,
}

impl RestartHandover for DaemonHandover {
    fn handed_over(&self) {
        let _ = self.trigger.request(ShutdownMode::Restart);
    }
}

fn restart_busy(blocking: &[ActiveOperation]) -> Box<ErrorBody> {
    let items: Vec<ErrorItem> = blocking
        .iter()
        .map(|active| {
            ErrorItem::new("operation")
                .with_field("operationId", active.id.to_string())
                .with_field("kind", active.kind.as_code())
        })
        .collect();
    boxed(
        ErrorBody::new(
            &daemon::RESTART_BUSY,
            "an operation that cannot resume still runs, so the daemon was not restarted",
        )
        .with_items(items)
        .with_hint("wait for the operations to finish, or restart with force"),
    )
}

fn plan_changed(expected: &str, actual: &str) -> Box<ErrorBody> {
    boxed(
        ErrorBody::new(
            &daemon::RESTART_PLAN_CHANGED,
            "the restart plan differs from the one that was confirmed, so nothing was stopped",
        )
        .with_detail("expected", expected)
        .with_detail("actual", actual)
        .with_hint("read the plan again with daemon.restart_preview"),
    )
}

/// `daemon.restart_pending`: a restart is quiescing or stopping instances.
#[must_use]
pub fn restart_pending(plan: Option<Uuid>) -> Box<ErrorBody> {
    let body = ErrorBody::new(
        &daemon::RESTART_PENDING,
        "a daemon restart is in progress, so the daemon takes no new request that starts an operation",
    );
    boxed(match plan {
        Some(plan) => body.with_detail("planId", plan.to_string()),
        None => body,
    })
}

/// Waits until no operation that can hold up the restart runs, for at most
/// `wait`. Returns the operations that still run then and cannot resume.
async fn wait_for_operations(
    engine: &OperationEngine,
    wait: Duration,
) -> Result<Vec<ActiveOperation>, Box<ErrorBody>> {
    let deadline = Instant::now() + wait;
    loop {
        let active = engine
            .active_operations()
            .await
            .map_err(|error| boxed(engine_error(&error)))?;
        if active.is_empty() {
            return Ok(active);
        }
        if Instant::now() >= deadline {
            return Ok(active.into_iter().filter(|op| !op.resumable).collect());
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The plan an operation recorded in its input, for a repeated request.
fn plan_of_input(input: &str) -> Option<Uuid> {
    serde_json::from_str::<serde_json::Value>(input)
        .ok()?
        .get("planId")?
        .as_str()?
        .parse()
        .ok()
}

/// The real `daemon.shutdown` with `mode: restart` (spec 0030, *Alur
/// restart terencana*, steps 2 to 5).
pub fn restart<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ShutdownParams,
) -> HandlerFuture<'a, ShutdownAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let accepted = |plan: Option<Uuid>| ShutdownAccepted {
            server_instance_id: core.start_info().server_instance_id,
            mode: ShutdownMode::Restart,
            plan_id: plan,
        };
        let force = params.force;
        let expected = params.expected_plan_digest.clone();
        let fingerprint = request_fingerprint(&Call::DaemonShutdown(params));
        let start = StartRequest::new(
            "daemon.shutdown",
            context.idempotency_key.as_deref(),
            fingerprint,
        );
        let shutting_down = || {
            boxed(ErrorBody::new(
                &daemon::SHUTTING_DOWN,
                "the daemon is already stopping",
            ))
        };
        match replay_operation(core, &context, start).await {
            Ok(Some(replay)) => return Ok(accepted(plan_of_input(replay.input.as_str()))),
            Ok(None) => {}
            Err(_) if core.shutdown_pending() => return Err(shutting_down()),
            Err(error) => return Err(error),
        }
        if core.shutdown_pending() {
            return Err(shutting_down());
        }
        let Availability::Ready(supervisor) = core.supervision().availability() else {
            return Err(boxed(super::operation::starting(
                stackly_protocol::daemon::subsystem::SUPERVISOR,
            )));
        };
        let engine = core.operations().writer()?;
        if engine.is_quiesced() {
            return Err(restart_pending(None));
        }

        let entries = supervisor
            .restart_plan_entries()
            .await
            .map_err(|error| boxed(super::operation::operation_error_body(&error)))?;
        let digest = plan_digest(&entries);
        let confirmed = match expected {
            None => digest,
            Some(text) => {
                let confirmed: Sha256Digest = text
                    .parse()
                    .map_err(|_| plan_changed(&text, &digest.to_string()))?;
                if confirmed != digest {
                    return Err(plan_changed(&text, &digest.to_string()));
                }
                confirmed
            }
        };

        let quiesce = Quiesce::on(engine.clone());
        let wait = supervisor.restart_quiesce_wait();
        let blocking = wait_for_operations(&engine, wait).await?;
        if !blocking.is_empty() && !force {
            return Err(restart_busy(&blocking));
        }

        let plan_id = RestartPlanId::generate();
        let (prepared_tx, prepared_rx) = oneshot::channel();
        let work = supervisor.daemon_restart_work(DaemonRestartRequest::new(
            plan_id,
            confirmed,
            core.start_info().server_instance_id,
            prepared_tx,
            Arc::new(DaemonHandover {
                trigger: core.shutdown_trigger(),
            }),
        ));
        let locks: Vec<LockKey> = entries
            .iter()
            .filter(|entry| entry.decision == PlanDecision::StopRestore)
            .map(|entry| LockKey::for_resource(OperationResource::Instance(entry.instance_id)))
            .collect();
        let input = JournalPayload::new(
            format!("{{\"planId\":\"{plan_id}\"}}"),
            stackly_core::operation::limits::INPUT_MAX,
        )
        .map_err(|_| {
            boxed(ErrorBody::new(
                &stackly_protocol::codes::internal::UNEXPECTED,
                "the restart input does not fit its journal",
            ))
        })?;
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::DaemonRestart,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: locks,
            // The operation waits for the instances it stops, which a
            // start in flight may still hold.
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work,
            precheck: None,
        };
        let start = StartRequest::new(
            "daemon.shutdown",
            context.idempotency_key.as_deref(),
            fingerprint,
        );
        let started = start_operation(core, &context, start, request).await?;

        let prepared = tokio::time::timeout(wait + PREPARE_GRACE, prepared_rx).await;
        if prepared.is_err()
            && let Some(operation) = started
                .operation_id()
                .and_then(|id| OperationId::from_uuid(id).ok())
        {
            // The operation still waits for an instance lock, and a
            // restart nobody waits for must not run later.
            let _ = engine.cancel(operation).await;
        }
        match prepared {
            Ok(Ok(PlanPrepared::Written(plan))) => {
                quiesce.keep();
                Ok(accepted(Some(plan.id().as_uuid())))
            }
            Ok(Ok(PlanPrepared::Refused(PlanRefusal::DigestChanged { expected, actual }))) => {
                Err(plan_changed(&expected.to_string(), &actual.to_string()))
            }
            Ok(Ok(PlanPrepared::Refused(PlanRefusal::OpenPlan(open)))) => {
                Err(restart_pending(Some(open.as_uuid())))
            }
            Ok(Ok(_)) | Ok(Err(_)) => Err(boxed(ErrorBody::new(
                &stackly_protocol::codes::internal::UNEXPECTED,
                "the restart plan could not be written",
            ))),
            Err(_) => Err(restart_busy(&blocking)),
        }
    })
}
