//! What the CLI shows when an instance fails to start or keeps crashing
//! (spec 0033, milestone 2): the commands to run next (*Perintah lanjutan
//! CLI*), the related operation of an instance (*Operasi terkait*, the same
//! rule the GUI banner uses), the `Last operation` line of `inspect` and
//! `get`, and the backoff line of `--wait`.
//!
//! Every text here is fixed text, catalog details, or a name, never output
//! of the process; the error block and the result writer escape it.

use jiff::tz::TimeZone;
use stackly_protocol::codes::supervisor;
use stackly_protocol::instance::{InstanceLifecycle, InstanceSummary};
use stackly_protocol::operation::{
    ListOperationsParams, OperationKind, OperationState, OperationSummary,
};
use stackly_protocol::{Call, ErrorBody, ErrorCategory, Reply, ResourceKind, ResourceRef};
use uuid::Uuid;

use super::{InstanceAction, read_instance};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::human::wire_name;
use crate::output::time::format_local;
use crate::output::{CommandOutput, OutputMode};
use crate::session::CallSpec;

/// Whether `stackly doctor` (spec 0034) exists, so the step that names it
/// is written (spec 0033, AC-16).
// provisional: F34
const DOCTOR_COMMAND: bool = false;

/// How many of the newest operations of an instance `inspect` reads to
/// find its related operation.
pub const RELATED_LIMIT: u32 = 10;

/// The stage a start or restart is in while it waits to spawn again
/// (spec 0033, *Daemon dan core*).
pub const BACKOFF_STAGE: &str = "backoff";

/// The backoff line when the restart count or the next attempt is not
/// known.
pub const RESTARTING: &str = "crashed, restarting";

/// What the next steps after a failure are made from.
#[derive(Debug, Clone, Copy)]
pub struct Failure<'a> {
    /// The name of the instance, as the target resolved to it.
    pub name: &'a str,
    /// The error of the failed start or restart, when there is one.
    pub error: Option<&'a ErrorBody>,
    /// `lastFailure.code` of the instance, when it has one.
    pub last_failure: Option<&'a str>,
    /// The lifecycle of the instance, when it could be read.
    pub lifecycle: Option<InstanceLifecycle>,
    /// A port change succeeded after the failure, so its ports are new.
    pub ports_changed: bool,
}

/// The commands to run next, in order (*Perintah lanjutan CLI*): a port
/// step for a port failure, the logs, the doctor once it exists, and always
/// last the start. The port step is left out after a port change and for a
/// lifecycle whose ports `endpoint set` cannot save (spec 0029, AC-6).
#[must_use]
pub fn next_steps(failure: &Failure<'_>) -> Vec<String> {
    let name = failure.name;
    let mut steps = Vec::new();
    steps.extend(port_step(failure));
    steps.push(format!("read its logs: stackly instance logs {name}"));
    if DOCTOR_COMMAND {
        steps.push(format!("check this Mac: stackly doctor --instance {name}"));
    }
    steps.push(format!("start it again: stackly instance start {name}"));
    steps
}

/// `use a free port: ...` with the suggested port, `pick another port:
/// ...` without one, or `None` when the failure is not about a port.
fn port_step(failure: &Failure<'_>) -> Option<String> {
    let editable = failure.lifecycle.is_none_or(|lifecycle| {
        matches!(
            lifecycle,
            InstanceLifecycle::Stopped | InstanceLifecycle::Failed
        )
    });
    if failure.ports_changed || !editable {
        return None;
    }
    let port_error = failure
        .error
        .filter(|error| Cause::of_error(error.code()).is_port());
    if port_error.is_none() && failure.last_failure != Some("endpoint_in_use") {
        return None;
    }
    let name = failure.name;
    let endpoint = port_error.and_then(|error| error.detail("endpoint"));
    let suggested = port_error.and_then(|error| error.detail("suggestedPort"));
    Some(match (endpoint, suggested) {
        (Some(endpoint), Some(port)) => {
            format!(
                "use a free port: stackly instance endpoint set {name} --port {endpoint}={port}"
            )
        }
        (Some(endpoint), None) => format!(
            "pick another port: stackly instance endpoint set {name} --port {endpoint}=<port>"
        ),
        (None, _) => format!(
            "pick another port: stackly instance endpoint set {name} --port <endpoint>=<port>"
        ),
    })
}

/// The cause of a failure, one row of *Pemetaan sebab*: `lastFailure.code`
/// has no area, an operation error carries a catalog code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    CrashLoop,
    PortInUse,
    PortConflict,
    PortUnavailable,
    DataInUse,
    Other,
}

impl Cause {
    /// The row of an operation error code.
    fn of_error(code: &str) -> Self {
        [
            (&supervisor::RETRY_BUDGET_EXHAUSTED, Self::CrashLoop),
            (&supervisor::ENDPOINT_IN_USE, Self::PortInUse),
            (&supervisor::ENDPOINT_CONFLICT, Self::PortConflict),
            (&supervisor::ENDPOINT_UNAVAILABLE, Self::PortUnavailable),
            (&supervisor::DATA_IN_USE, Self::DataInUse),
        ]
        .into_iter()
        .find(|(spec, _)| spec.code == code)
        .map_or(Self::Other, |(_, cause)| cause)
    }

    /// The row of a `lastFailure.code`; the conflict and unavailable rows
    /// have none, since they only happen before the journal.
    fn of_failure(code: &str) -> Self {
        match code {
            "crash_loop" => Self::CrashLoop,
            "endpoint_in_use" => Self::PortInUse,
            "data_in_use" => Self::DataInUse,
            _ => Self::Other,
        }
    }

    /// One of the three port rows.
    const fn is_port(self) -> bool {
        matches!(
            self,
            Self::PortInUse | Self::PortConflict | Self::PortUnavailable
        )
    }
}

/// The related operation of an instance (*Operasi terkait*), from its
/// newest operations as `operation.list` orders them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Related {
    /// The newest start, restart, stop, or port change; other kinds are
    /// passed over.
    pub latest: Option<OperationSummary>,
    /// The failed start or restart that explains a `stopped` or `failed`
    /// instance, when one does.
    pub error: Option<OperationSummary>,
    /// A port change succeeded on the `failed` instance after its failure.
    pub ports_changed: bool,
}

impl Related {
    /// Applies the rule to `operations` (newest first) of `instance`.
    ///
    /// `latest` is the first start, restart, stop, or port change. It is
    /// the error only when it is a failed start or restart with its error
    /// on a `stopped` or `failed` instance, and for a `failed` instance
    /// only when it also matches the failure: it ended at
    /// or after `lastFailure.at`, or with `at` null its code is in the same
    /// row of *Pemetaan sebab* as `lastFailure.code`. On a `failed`
    /// instance, succeeded port changes newer than the failure set
    /// `ports_changed` and are passed over, and the same test runs on the
    /// next related operation. On a `stopped` instance a newer port change
    /// hides the error (spec 0033, AC-11).
    #[must_use]
    pub fn of(operations: &[OperationSummary], instance: &InstanceSummary) -> Self {
        let mut related = operations.iter().filter(|operation| {
            matches!(
                operation.kind,
                OperationKind::StartInstance
                    | OperationKind::RestartInstance
                    | OperationKind::StopInstance
                    | OperationKind::ChangeEndpoints
            )
        });
        let Some(latest) = related.next() else {
            return Self::default();
        };
        let failure_at = instance
            .last_failure
            .as_ref()
            .and_then(|failure| failure.at);
        let mut candidate = Some(latest);
        let mut ports_changed = false;
        if instance.lifecycle == InstanceLifecycle::Failed {
            while let Some(operation) = candidate
                && operation.kind == OperationKind::ChangeEndpoints
                && operation.state == OperationState::Succeeded
                && failure_at.is_none_or(|at| operation.updated_at > at)
            {
                ports_changed = true;
                candidate = related.next();
            }
        }
        Self {
            latest: Some(latest.clone()),
            error: candidate
                .filter(|operation| explains(operation, instance))
                .cloned(),
            ports_changed,
        }
    }

    /// Whether `inspect` shows `Last operation` and `Next steps:` for
    /// `lifecycle` (spec 0033, AC-7): always for `failed` and
    /// `recovering`, and for `stopped` only after a failed start or
    /// restart. `None` is a related operation that could not be read.
    #[must_use]
    pub fn shown(lifecycle: InstanceLifecycle, related: Option<&Self>) -> bool {
        match lifecycle {
            InstanceLifecycle::Failed | InstanceLifecycle::Recovering => true,
            InstanceLifecycle::Stopped => related.is_some_and(|related| related.error.is_some()),
            _ => false,
        }
    }
}

/// Whether `operation` is a failed start or restart, with its error, that
/// explains `instance`: any such operation on a `stopped` instance, and on a
/// `failed` one only when it matches `lastFailure`. Other lifecycles have
/// none, the same as `RelatedOperation` in the app.
fn explains(operation: &OperationSummary, instance: &InstanceSummary) -> bool {
    let failed_start = matches!(
        operation.kind,
        OperationKind::StartInstance | OperationKind::RestartInstance
    ) && operation.state == OperationState::Failed;
    let Some(info) = operation.failure.as_ref().filter(|_| failed_start) else {
        return false;
    };
    match instance.lifecycle {
        InstanceLifecycle::Stopped => true,
        InstanceLifecycle::Failed => {
            instance
                .last_failure
                .as_ref()
                .is_some_and(|failure| match failure.at {
                    Some(at) => operation.updated_at >= at,
                    None => Cause::of_error(info.error.code()) == Cause::of_failure(&failure.code),
                })
        }
        _ => false,
    }
}

/// The `Last operation` value (spec 0033, AC-7): `<kind> failed, <code>,
/// <uuid>` for a failed operation, `<kind> <state>, <uuid>` for any other,
/// `-` without a related operation, and `unavailable` when the read
/// failed (`None`).
#[must_use]
pub fn last_operation_line(related: Option<&Related>) -> String {
    let Some(related) = related else {
        return "unavailable".to_owned();
    };
    let Some(operation) = &related.latest else {
        return "-".to_owned();
    };
    let kind = wire_name(&operation.kind);
    let state = wire_name(&operation.state);
    match &operation.failure {
        Some(info) if operation.state == OperationState::Failed => {
            format!("{kind} {state}, {}, {}", info.error.code(), operation.id)
        }
        _ => format!("{kind} {state}, {}", operation.id),
    }
}

/// The next steps `inspect` writes for `instance` with what `related`
/// says, or with the instance alone when the read failed.
#[must_use]
pub fn for_inspect(instance: &InstanceSummary, related: Option<&Related>) -> Vec<String> {
    next_steps(&Failure {
        name: &instance.name,
        error: related
            .and_then(|related| related.error.as_ref())
            .and_then(|operation| operation.failure.as_ref())
            .map(|info| &info.error),
        last_failure: instance
            .last_failure
            .as_ref()
            .map(|failure| failure.code.as_str()),
        lifecycle: Some(instance.lifecycle),
        ports_changed: related.is_some_and(|related| related.ports_changed),
    })
}

/// Reads the related operation of instance `id` with one `operation.list
/// { resource, limit: 10 }`; `None` when that read failed, which never
/// fails `inspect`.
pub async fn read_related(cx: &mut Context, instance: &InstanceSummary) -> Option<Related> {
    let call = Call::OperationList(ListOperationsParams {
        active_only: false,
        resource: Some(ResourceRef {
            kind: ResourceKind::Instance,
            id: instance.id.to_string(),
        }),
        limit: Some(RELATED_LIMIT),
        before: None,
    });
    match cx.call(call, CallSpec::control()).await {
        Ok(Reply::OperationList(page)) => Some(Related::of(&page.items, instance)),
        _ => None,
    }
}

/// Whether `failure` means the start or restart itself failed, so its
/// error block gets the next steps (spec 0033, AC-6): the waited
/// `operation` ended `failed`, or with no operation the daemon refused the
/// request for the instance's state (a `conflict` or `failed` error, or a
/// port or data code). A wait that stopped early, a cancel, a recovery, a
/// transport error, a missing instance, and the CLI's own errors get none.
#[must_use]
pub fn start_failed(failure: &CliFailure, operation: Option<&OperationSummary>) -> bool {
    if let Some(operation) = operation {
        return operation.state == OperationState::Failed;
    }
    let body = &failure.body;
    let area = body.code().split('.').next().unwrap_or_default();
    if matches!(area, "cli" | "ipc" | "request" | "operation") {
        return false;
    }
    let cause = Cause::of_error(body.code());
    matches!(
        body.category(),
        ErrorCategory::Conflict | ErrorCategory::Failed
    ) || cause != Cause::Other
}

/// Adds the next steps to the error block of a failed `start` or `restart`
/// of instance `id` (spec 0033, AC-6). The name comes from the instance the
/// command read after the wait, or from one more `instance.get` for a
/// refused request; without either, the name the target resolved to, else
/// the ID. `--json` and `--quiet` never show the hints, so nothing is read
/// for them.
pub async fn explain(
    cx: &mut Context,
    mut output: CommandOutput<InstanceAction>,
    id: Uuid,
    resolved_name: Option<&str>,
) -> CommandOutput<InstanceAction> {
    let Some(failure) = output.failure.take() else {
        return output;
    };
    let operation = output
        .result
        .as_ref()
        .and_then(|action| action.operation.as_ref());
    let shown = cx.output.mode == OutputMode::Human && !cx.output.quiet;
    if !shown || !start_failed(&failure, operation) {
        output.failure = Some(failure);
        return output;
    }
    let instance = match output.result.as_ref() {
        Some(action) => action.instance.clone(),
        None => read_instance(cx, id).await.ok(),
    };
    let fallback = id.to_string();
    let name = instance
        .as_ref()
        .map(|instance| instance.name.as_str())
        .or(resolved_name)
        .unwrap_or(&fallback);
    let from_daemon = !failure.body.code().starts_with("cli.");
    let steps = next_steps(&Failure {
        name,
        error: from_daemon.then_some(&*failure.body),
        last_failure: instance
            .as_ref()
            .and_then(|instance| instance.last_failure.as_ref())
            .map(|failure| failure.code.as_str()),
        lifecycle: instance.as_ref().map(|instance| instance.lifecycle),
        ports_changed: false,
    });
    output.failure = Some(failure.with_hints(steps));
    output
}

/// The backoff line of `--wait` (spec 0033, AC-8) for an operation that
/// just entered the `backoff` stage: one `instance.get` of the instance it
/// works on, then `crashed, restart {attempts} of {budget} at {time}` with
/// the time as `inspect` writes the Restarts row, or `crashed, restarting`
/// when the next attempt is not known or the read failed.
pub async fn backoff_line(cx: &mut Context, operation: &OperationSummary) -> String {
    let Some(id) = operation
        .resource
        .as_ref()
        .filter(|resource| resource.kind == ResourceKind::Instance)
        .and_then(|resource| Uuid::try_parse(&resource.id).ok())
    else {
        return RESTARTING.to_owned();
    };
    let call = Call::InstanceGet(stackly_protocol::instance::GetInstanceParams { instance_id: id });
    match cx.call_once(call, CallSpec::control()).await {
        Ok(Reply::InstanceGet(instance)) => restart_text(&instance, &cx.output.tz),
        _ => RESTARTING.to_owned(),
    }
}

/// `crashed, restart {attempts} of {budget} at {time}`, or `crashed,
/// restarting` without a next attempt.
#[must_use]
pub fn restart_text(instance: &InstanceSummary, tz: &TimeZone) -> String {
    let restart = &instance.restart;
    restart.next_attempt_at.map_or_else(
        || RESTARTING.to_owned(),
        |at| {
            format!(
                "crashed, restart {} of {} at {}",
                restart.attempts,
                restart.budget,
                format_local(at.as_jiff(), tz)
            )
        },
    )
}

/// Whether `operation` is a start or restart waiting in the `backoff`
/// stage.
#[must_use]
pub fn in_backoff(operation: &OperationSummary) -> bool {
    matches!(
        operation.kind,
        OperationKind::StartInstance | OperationKind::RestartInstance
    ) && operation.stage.as_deref() == Some(BACKOFF_STAGE)
}

#[cfg(test)]
mod tests {
    use stackly_protocol::Timestamp;
    use stackly_protocol::instance::InstanceLastFailure;
    use stackly_protocol::operation::OperationFailureInfo;

    use super::*;

    const NAME: &str = "orders-db";

    fn port_error(suggested: Option<&str>) -> ErrorBody {
        let body = ErrorBody::new(&supervisor::ENDPOINT_IN_USE, "port 5432 is in use")
            .with_detail("endpoint", "postgres")
            .with_detail("port", "5432");
        match suggested {
            Some(port) => body.with_detail("suggestedPort", port),
            None => body,
        }
    }

    fn failure<'a>(
        error: Option<&'a ErrorBody>,
        last_failure: Option<&'a str>,
        lifecycle: Option<InstanceLifecycle>,
    ) -> Failure<'a> {
        Failure {
            name: NAME,
            error,
            last_failure,
            lifecycle,
            ports_changed: false,
        }
    }

    /// Spec 0033 AC-6, AC-16.
    #[test]
    fn a_port_failure_with_a_suggestion_names_the_free_port_first_and_the_start_last() {
        let error = port_error(Some("5433"));
        assert_eq!(
            next_steps(&failure(
                Some(&error),
                None,
                Some(InstanceLifecycle::Stopped)
            )),
            [
                "use a free port: stackly instance endpoint set orders-db --port postgres=5433",
                "read its logs: stackly instance logs orders-db",
                "start it again: stackly instance start orders-db",
            ]
        );
    }

    /// Spec 0033 AC-6.
    #[test]
    fn a_port_failure_without_a_suggestion_asks_for_another_port() {
        let error = port_error(None);
        let conflict = ErrorBody::new(&supervisor::ENDPOINT_CONFLICT, "taken")
            .with_detail("endpoint", "postgres");
        let unavailable = ErrorBody::new(&supervisor::ENDPOINT_UNAVAILABLE, "refused")
            .with_detail("endpoint", "postgres");
        for error in [&error, &conflict, &unavailable] {
            assert_eq!(
                next_steps(&failure(Some(error), None, None))[0],
                "pick another port: stackly instance endpoint set orders-db --port postgres=<port>",
                "{}",
                error.code()
            );
        }
        assert_eq!(
            next_steps(&failure(None, Some("endpoint_in_use"), None))[0],
            "pick another port: stackly instance endpoint set orders-db --port <endpoint>=<port>"
        );
    }

    /// Spec 0033 AC-6, AC-10, AC-14.
    #[test]
    fn the_port_step_is_left_out_after_a_port_change_and_where_ports_cannot_be_saved() {
        let error = port_error(Some("5433"));
        let mut changed = failure(Some(&error), None, Some(InstanceLifecycle::Failed));
        changed.ports_changed = true;
        assert_eq!(next_steps(&changed).len(), 2);
        let recovering = failure(Some(&error), None, Some(InstanceLifecycle::Recovering));
        assert_eq!(next_steps(&recovering).len(), 2);
        let crash = ErrorBody::new(&supervisor::RETRY_BUDGET_EXHAUSTED, "kept crashing");
        assert_eq!(
            next_steps(&failure(Some(&crash), Some("crash_loop"), None)),
            [
                "read its logs: stackly instance logs orders-db",
                "start it again: stackly instance start orders-db",
            ]
        );
    }

    /// Spec 0033 AC-16: no doctor step until `stackly doctor` exists.
    #[test]
    fn the_doctor_step_waits_for_the_doctor_command() {
        let steps = next_steps(&failure(None, None, None));
        assert!(
            !steps.iter().any(|step| step.contains("doctor")),
            "{steps:?}"
        );
    }

    const INSTANCE_ID: &str = "0192f3a5-0000-7000-8000-000000000001";

    fn ts(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn op(
        n: u128,
        kind: OperationKind,
        state: OperationState,
        updated: &str,
        code: Option<&str>,
    ) -> OperationSummary {
        let mut summary: OperationSummary = serde_json::from_value(serde_json::json!({
            "id": Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d00 + n),
            "kind": kind,
            "resource": { "kind": "instance", "id": INSTANCE_ID },
            "state": state,
            "stage": null,
            "progress": { "type": "indeterminate" },
            "waitingOn": null,
            "cancelRequestedAt": null,
            "pointOfNoReturnAt": null,
            "failure": null,
            "recovery": null,
            "result": null,
            "trigger": { "origin": "user", "client": "cli" },
            "createdAt": updated,
            "startedAt": null,
            "updatedAt": updated,
            "revision": 3,
        }))
        .unwrap();
        if let Some(code) = code {
            let spec = stackly_protocol::codes::lookup(code).unwrap();
            summary.failure = Some(OperationFailureInfo {
                reason: "step_failed".to_owned(),
                retryable: false,
                error: ErrorBody::new(spec, "it failed"),
            });
        }
        summary
    }

    fn instance(
        lifecycle: InstanceLifecycle,
        failure: Option<(&str, Option<&str>)>,
    ) -> InstanceSummary {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/ipc/frame-response-instance-get.json");
        let frame: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut instance: InstanceSummary =
            serde_json::from_value(frame["content"]["reply"]["content"].clone()).unwrap();
        instance.lifecycle = lifecycle;
        instance.last_failure = failure.map(|(code, at)| InstanceLastFailure {
            code: code.to_owned(),
            at: at.map(ts),
            exit_code: None,
            signal: None,
            excerpt: Vec::new(),
        });
        instance
    }

    const BEFORE: &str = "2026-11-10T08:00:00Z";
    const AT: &str = "2026-11-10T08:01:00Z";
    const AFTER: &str = "2026-11-10T08:02:00Z";

    fn start_failed_op(n: u128, updated: &str, code: &str) -> OperationSummary {
        op(
            n,
            OperationKind::StartInstance,
            OperationState::Failed,
            updated,
            Some(code),
        )
    }

    /// Spec 0033 *Operasi terkait*.
    #[test]
    fn other_kinds_are_passed_over_and_a_newer_rename_never_hides_the_error() {
        let rename = op(
            1,
            OperationKind::RenameInstance,
            OperationState::Succeeded,
            AFTER,
            None,
        );
        let start = start_failed_op(2, AT, supervisor::ENDPOINT_IN_USE.code);
        let stopped = instance(InstanceLifecycle::Stopped, None);
        let related = Related::of(&[rename, start.clone()], &stopped);
        assert_eq!(related.latest.as_ref(), Some(&start));
        assert_eq!(related.error.as_ref(), Some(&start));
        assert!(Related::shown(InstanceLifecycle::Stopped, Some(&related)));
        assert_eq!(Related::of(&[], &stopped), Related::default());
    }

    /// Spec 0033 AC-11.
    #[test]
    fn a_newer_stop_or_port_change_on_a_stopped_instance_hides_the_failed_start() {
        let start = start_failed_op(2, AT, supervisor::ENDPOINT_IN_USE.code);
        let stopped = instance(InstanceLifecycle::Stopped, None);
        for newer in [
            op(
                1,
                OperationKind::StopInstance,
                OperationState::Succeeded,
                AFTER,
                None,
            ),
            op(
                1,
                OperationKind::ChangeEndpoints,
                OperationState::Succeeded,
                AFTER,
                None,
            ),
        ] {
            let related = Related::of(&[newer.clone(), start.clone()], &stopped);
            assert_eq!(related.latest, Some(newer));
            assert_eq!(related.error, None);
            assert!(!related.ports_changed);
            assert!(!Related::shown(InstanceLifecycle::Stopped, Some(&related)));
        }
    }

    /// Spec 0033 *Operasi terkait*, AC-10.
    #[test]
    fn a_port_change_after_the_failure_of_a_failed_instance_keeps_the_earlier_error() {
        let failed = instance(
            InstanceLifecycle::Failed,
            Some(("endpoint_in_use", Some(AT))),
        );
        let change = op(
            1,
            OperationKind::ChangeEndpoints,
            OperationState::Succeeded,
            AFTER,
            None,
        );
        let start = start_failed_op(2, AT, supervisor::ENDPOINT_IN_USE.code);
        let related = Related::of(&[change.clone(), start.clone()], &failed);
        assert_eq!(related.latest, Some(change));
        assert_eq!(related.error, Some(start));
        assert!(related.ports_changed);
        assert_eq!(for_inspect(&failed, Some(&related)).len(), 2);
    }

    /// Spec 0033 *Operasi terkait*.
    #[test]
    fn a_failed_instance_uses_only_an_operation_that_matches_its_failure() {
        let at = instance(InstanceLifecycle::Failed, Some(("crash_loop", Some(AT))));
        let older = start_failed_op(1, BEFORE, supervisor::RETRY_BUDGET_EXHAUSTED.code);
        assert_eq!(Related::of(&[older], &at).error, None);
        let same = start_failed_op(1, AT, supervisor::RETRY_BUDGET_EXHAUSTED.code);
        assert!(Related::of(&[same], &at).error.is_some());

        let restarted = instance(InstanceLifecycle::Failed, Some(("crash_loop", None)));
        let budget = start_failed_op(1, BEFORE, supervisor::RETRY_BUDGET_EXHAUSTED.code);
        assert!(Related::of(&[budget], &restarted).error.is_some());
        let port = start_failed_op(1, BEFORE, supervisor::ENDPOINT_IN_USE.code);
        assert_eq!(Related::of(&[port], &restarted).error, None);
        let other = instance(InstanceLifecycle::Failed, Some(("process_exited", None)));
        let start = start_failed_op(1, BEFORE, supervisor::CONFIG_INVALID.code);
        assert!(Related::of(&[start], &other).error.is_some());
    }

    /// Spec 0033 *Operasi terkait*: the same edges as `RelatedOperation`
    /// in the app.
    #[test]
    fn no_error_while_recovering_without_a_failure_record_or_without_the_operation_error() {
        let start = start_failed_op(1, AT, supervisor::ENDPOINT_IN_USE.code);
        let recovering = instance(
            InstanceLifecycle::Recovering,
            Some(("process_exited", None)),
        );
        assert_eq!(
            Related::of(std::slice::from_ref(&start), &recovering).error,
            None
        );
        let unrecorded = instance(InstanceLifecycle::Failed, None);
        assert_eq!(
            Related::of(std::slice::from_ref(&start), &unrecorded).error,
            None
        );
        let mut bare = start;
        bare.failure = None;
        let stopped = instance(InstanceLifecycle::Stopped, None);
        assert_eq!(Related::of(&[bare], &stopped).error, None);
    }

    /// Spec 0033 AC-7.
    #[test]
    fn a_succeeded_respawn_is_the_latest_but_never_the_error() {
        let failed = instance(InstanceLifecycle::Failed, Some(("crash_loop", Some(AT))));
        let respawn = op(
            1,
            OperationKind::StartInstance,
            OperationState::Succeeded,
            BEFORE,
            None,
        );
        let related = Related::of(std::slice::from_ref(&respawn), &failed);
        assert_eq!(related.error, None);
        assert_eq!(
            last_operation_line(Some(&related)),
            format!("start_instance succeeded, {}", respawn.id)
        );
    }

    /// Spec 0033 AC-7.
    #[test]
    fn the_last_operation_line_names_kind_state_code_and_id_or_a_dash_or_unavailable() {
        let start = start_failed_op(4, AT, supervisor::RETRY_BUDGET_EXHAUSTED.code);
        let related = Related {
            latest: Some(start.clone()),
            error: Some(start),
            ports_changed: false,
        };
        assert_eq!(
            last_operation_line(Some(&related)),
            "start_instance failed, supervisor.retry_budget_exhausted, 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d04"
        );
        assert_eq!(last_operation_line(Some(&Related::default())), "-");
        assert_eq!(last_operation_line(None), "unavailable");
    }

    /// Spec 0033 AC-7.
    #[test]
    fn inspect_shows_the_failure_block_for_failed_recovering_and_a_refused_stopped_start() {
        let refused = Related {
            error: Some(start_failed_op(1, AT, supervisor::ENDPOINT_IN_USE.code)),
            ..Related::default()
        };
        assert!(Related::shown(InstanceLifecycle::Failed, None));
        assert!(Related::shown(InstanceLifecycle::Recovering, None));
        assert!(Related::shown(InstanceLifecycle::Stopped, Some(&refused)));
        assert!(!Related::shown(InstanceLifecycle::Stopped, None));
        assert!(!Related::shown(
            InstanceLifecycle::Stopped,
            Some(&Related::default())
        ));
        assert!(!Related::shown(InstanceLifecycle::Running, Some(&refused)));
    }

    /// Spec 0033 AC-6.
    #[test]
    fn only_a_failed_operation_or_a_refusal_for_the_state_gets_next_steps() {
        let daemon = |body: ErrorBody| CliFailure::daemon(body, None);
        let failed = start_failed_op(1, AT, supervisor::CONFIG_INVALID.code);
        let running = op(
            1,
            OperationKind::StartInstance,
            OperationState::Running,
            AT,
            None,
        );
        let wait =
            CliFailure::wait_timeout(failed.id, "running", std::time::Duration::from_secs(1));
        assert!(start_failed(
            &CliFailure::operation_failed(failed.id),
            Some(&failed)
        ));
        assert!(!start_failed(&wait, Some(&running)));
        assert!(start_failed(
            &daemon(ErrorBody::new(&supervisor::INSTANCE_RETAINED, "retained")),
            None
        ));
        assert!(start_failed(&daemon(port_error(None)), None));
        assert!(start_failed(
            &daemon(ErrorBody::new(&supervisor::ENDPOINT_UNAVAILABLE, "refused")),
            None
        ));
        for refused in [
            ErrorBody::new(&stackly_protocol::codes::resource::NOT_FOUND, "gone"),
            ErrorBody::new(&stackly_protocol::codes::ipc::CALL_TIMEOUT, "late"),
            ErrorBody::new(
                &stackly_protocol::codes::operation::RECOVERY_REQUIRED,
                "recover first",
            ),
        ] {
            assert!(!start_failed(&daemon(refused.clone()), None), "{refused:?}");
        }
        assert!(!start_failed(&CliFailure::confirmation_declined(), None));
    }

    /// Spec 0033 AC-8.
    #[test]
    fn the_backoff_line_names_the_attempt_the_budget_and_the_local_time() {
        let mut waiting = instance(InstanceLifecycle::Recovering, None);
        waiting.restart.attempts = 2;
        waiting.restart.budget = 5;
        waiting.restart.next_attempt_at = Some(ts("2026-11-10T03:04:05Z"));
        let jakarta = TimeZone::get("Asia/Jakarta").unwrap();
        assert_eq!(
            restart_text(&waiting, &jakarta),
            "crashed, restart 2 of 5 at 2026-11-10 10:04:05 +07:00"
        );
        waiting.restart.next_attempt_at = None;
        assert_eq!(restart_text(&waiting, &jakarta), "crashed, restarting");
    }

    /// Spec 0033 AC-8.
    #[test]
    fn only_a_start_or_restart_in_the_backoff_stage_is_in_backoff() {
        let mut start = op(
            1,
            OperationKind::StartInstance,
            OperationState::Running,
            AT,
            None,
        );
        assert!(!in_backoff(&start));
        start.stage = Some("backoff".to_owned());
        assert!(in_backoff(&start));
        start.kind = OperationKind::RestartInstance;
        assert!(in_backoff(&start));
        start.kind = OperationKind::StopInstance;
        assert!(!in_backoff(&start));
    }
}
