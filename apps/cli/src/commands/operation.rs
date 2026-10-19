//! `stackly operation list`, `inspect`, `cancel`, `resume`, and `abandon`
//! (spec 0008, *`stackly operation list`, `inspect`, `cancel`*, AC-7,
//! AC-8, AC-15). They only talk to the daemon and never read the registry.

use clap::{Args, Subcommand};
use serde::Serialize;
use stackly_protocol::operation::{
    AbandonOperationParams, AbandonReply, CancelDisposition, CancelOperationParams, CancelReply,
    CreatedItemInfo, GetOperationParams, ListOperationsParams, OPERATION_LIST_LIMIT_DEFAULT,
    OPERATION_LIST_LIMIT_MAX, OperationDetail, OperationProgress, OperationSummary,
    ResumeOperationParams,
};
use stackly_protocol::{Call, ResourceKind, ResourceRef, Revision};
use uuid::Uuid;

use super::{RunCommand, execute};
use crate::args;
use crate::confirm::{Confirm, ConfirmArgs, ConfirmRequest, ConfirmRole, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::{Tone, operation_tone};
use crate::session::CallSpec;
use crate::wait::{WaitArgs, WaitMode, Waiter, settled};

/// The verbs of `stackly operation`.
#[derive(Debug, Subcommand)]
pub enum OperationCommand {
    /// List operations, newest first: the latest 50 in every state by
    /// default
    #[command(
        after_help = "Examples:\n  stackly operation list\n  stackly operation list --active --json\n  stackly operation list --resource instance:0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e --limit 20"
    )]
    List(ListArgs),
    /// Show one operation with its steps and what it created
    #[command(
        after_help = "Examples:\n  stackly operation inspect 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e\n  stackly operation inspect 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e --wait --timeout 5m"
    )]
    Inspect(InspectArgs),
    /// Ask the daemon to cancel an operation; a step past its point of no
    /// return still runs to its end
    #[command(
        after_help = "Examples:\n  stackly operation cancel 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e\n  stackly operation cancel 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e --wait"
    )]
    Cancel(CancelArgs),
    /// Resume an operation that needs recovery and says it can go on
    #[command(
        after_help = "Examples:\n  stackly operation resume 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e --wait"
    )]
    Resume(ResumeArgs),
    /// Give up an operation that needs recovery; what it created stays as
    /// leftovers
    #[command(
        after_help = "Examples:\n  stackly operation abandon 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e\n  stackly operation abandon 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e --yes --json"
    )]
    Abandon(AbandonArgs),
}

impl OperationCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::List(args) => execute(args, cx).await,
            Self::Inspect(args) => execute(args, cx).await,
            Self::Cancel(args) => execute(args, cx).await,
            Self::Resume(args) => execute(args, cx).await,
            Self::Abandon(args) => execute(args, cx).await,
        }
    }
}

/// `stackly operation list`.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only operations that are not final yet, needs_recovery included
    #[arg(long)]
    pub active: bool,
    /// Only operations of one resource: manager, or runtime, instance, or
    /// environment then :<uuid>
    #[arg(long, value_name = "RESOURCE", value_parser = args::resource_filter)]
    pub resource: Option<ResourceRef>,
    /// How many operations, from 1 to 200 (default 50)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=i64::from(OPERATION_LIST_LIMIT_MAX)))]
    pub limit: Option<u32>,
    /// Go on after the page that printed this next value
    #[arg(long, value_name = "CURSOR", value_parser = args::cursor)]
    pub before: Option<String>,
}

/// The `operation list` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationList {
    /// The operations, newest first.
    pub items: Vec<OperationSummary>,
    /// The cursor of the next page, or null on the last.
    pub next: Option<String>,
}

impl RunCommand for ListArgs {
    type Output = OperationList;

    async fn run(self, cx: &mut Context) -> CommandOutput<OperationList> {
        let params = ListOperationsParams {
            active_only: self.active,
            resource: self.resource.clone(),
            limit: Some(self.limit.unwrap_or(OPERATION_LIST_LIMIT_DEFAULT)),
            before: self.before.clone(),
        };
        let page = match cx
            .call(Call::OperationList(params), CallSpec::control())
            .await
        {
            Ok(stackly_protocol::Reply::OperationList(page)) => page,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let footer = page.next.as_ref().map(|next| self.footer(next));
        let output = CommandOutput::ok(OperationList {
            items: page.items,
            next: page.next,
        });
        match footer {
            Some(footer) => output.with_hint(footer),
            None => output,
        }
    }
}

impl ListArgs {
    /// The hint that repeats this command's flags for the next page.
    fn footer(&self, next: &str) -> String {
        let mut text = "more operations: stackly operation list".to_owned();
        if self.active {
            text.push_str(" --active");
        }
        if let Some(resource) = &self.resource {
            text.push_str(&format!(" --resource {}", resource_text(Some(resource))));
        }
        if let Some(limit) = self.limit {
            text.push_str(&format!(" --limit {limit}"));
        }
        text.push_str(&format!(" --before {next}"));
        text
    }
}

impl Render for OperationList {
    fn render(&self, out: &mut HumanWriter) {
        if self.items.is_empty() {
            out.line("No operations.");
            return;
        }
        let rows: Vec<Vec<Cell>> = self
            .items
            .iter()
            .map(|operation| {
                vec![
                    Cell::fixed(operation.id.to_string()),
                    Cell::text(&wire_name(&operation.kind)),
                    Cell::toned(&wire_name(&operation.state), operation_tone(operation)),
                    Cell::optional(operation.stage.as_deref()),
                    Cell::fixed(progress_text(operation.progress)),
                    Cell::text(&resource_text(operation.resource.as_ref())),
                    Cell::fixed(out.age(operation.created_at)),
                ]
            })
            .collect();
        out.table(
            &[
                "id", "kind", "state", "stage", "progress", "resource", "age",
            ],
            &rows,
        );
    }
}

/// An operation ID argument.
#[derive(Debug, Args)]
pub struct IdArg {
    /// The operation ID, a whole UUID
    #[arg(value_name = "ID", value_parser = args::operation_id)]
    pub id: Uuid,
}

/// `stackly operation inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// The operation.
    #[command(flatten)]
    pub operation: IdArg,
    /// `--wait` and `--timeout`.
    #[command(flatten)]
    pub wait: WaitArgs,
}

impl RunCommand for InspectArgs {
    type Output = OperationDetail;

    async fn run(self, cx: &mut Context) -> CommandOutput<OperationDetail> {
        let id = self.operation.id;
        if self.wait.wait {
            let outcome = Waiter::wait(cx, id, &self.wait, WaitMode::Observe).await;
            return waited(outcome.last, outcome.failure);
        }
        match get(cx, id).await {
            Ok(detail) => CommandOutput::ok(detail),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for OperationDetail {
    fn render(&self, out: &mut HumanWriter) {
        let operation = &self.operation;
        let mut rows = vec![
            ("ID", Cell::fixed(operation.id.to_string())),
            ("Kind", Cell::text(&wire_name(&operation.kind))),
            (
                "Resource",
                Cell::text(&resource_text(operation.resource.as_ref())),
            ),
            (
                "State",
                Cell::toned(&wire_name(&operation.state), operation_tone(operation)),
            ),
            ("Stage", Cell::optional(operation.stage.as_deref())),
            ("Progress", Cell::fixed(progress_text(operation.progress))),
        ];
        if let Some(waiting_on) = operation.waiting_on {
            rows.push(("Waiting on", Cell::fixed(waiting_on.to_string())));
        }
        if let Some(at) = operation.cancel_requested_at {
            rows.push(("Cancel requested", Cell::fixed(out.time(at))));
        }
        if let Some(at) = operation.point_of_no_return_at {
            rows.push(("Point of no return", Cell::fixed(out.time(at))));
        }
        let mut trigger = wire_name(&operation.trigger.origin);
        if let Some(client) = &operation.trigger.client {
            trigger.push_str(&format!(" ({})", wire_name(client)));
        }
        rows.push(("Trigger", Cell::text(&trigger)));
        rows.push((
            "Created",
            Cell::fixed(out.time_and_age(operation.created_at)),
        ));
        rows.push((
            "Started",
            Cell::fixed(
                operation
                    .started_at
                    .map(|at| out.time(at))
                    .unwrap_or_default(),
            ),
        ));
        rows.push(("Updated", Cell::fixed(out.time(operation.updated_at))));
        rows.push((
            "Revision",
            Cell::fixed(operation.revision.get().to_string()),
        ));
        if let Some(failure) = &operation.failure {
            rows.push(("Reason", Cell::text(&failure.reason)));
        }
        if let Some(recovery) = &operation.recovery {
            rows.push((
                "Recovery",
                Cell::toned(
                    &format!(
                        "{} after {}",
                        wire_name(&recovery.hint),
                        wire_name(&recovery.cause)
                    ),
                    Tone::Critical,
                ),
            ));
            if !recovery.actions.is_empty() {
                let actions: Vec<String> = recovery.actions.iter().map(wire_name).collect();
                rows.push(("Actions", Cell::text(&actions.join(", "))));
            }
        }
        let error = operation
            .failure
            .as_ref()
            .map(|failure| &failure.error)
            .or_else(|| {
                operation
                    .recovery
                    .as_ref()
                    .and_then(|recovery| recovery.error.as_ref())
            });
        if let Some(error) = error {
            rows.push((
                "Error",
                Cell::text(&format!("{}: {}", error.code(), error.message())),
            ));
            if let Some(hint) = error.hint() {
                rows.push(("Hint", Cell::text(hint)));
            }
        }
        out.kv(&rows);
        if !self.steps.is_empty() {
            out.line("");
            out.line("Steps:");
            let steps: Vec<Vec<Cell>> = self
                .steps
                .iter()
                .map(|step| {
                    vec![
                        Cell::fixed(step.seq.to_string()),
                        Cell::text(&step.name),
                        Cell::text(&wire_name(&step.state)),
                        Cell::fixed(step.attempts.to_string()),
                        Cell::fixed(out.time(step.started_at)),
                        Cell::fixed(step.finished_at.map(|at| out.time(at)).unwrap_or_default()),
                        Cell::optional(step.error_code.as_deref()),
                    ]
                })
                .collect();
            out.indented_table(
                2,
                &[
                    "seq", "name", "state", "attempts", "started", "finished", "error",
                ],
                &steps,
            );
        }
        if !self.created.is_empty() {
            out.line("");
            out.line("Created:");
            created_table(out, &self.created);
        }
        if let Some(result) = operation
            .result
            .as_ref()
            .filter(|result| !result.partial.is_empty())
        {
            out.line("");
            out.line("Partial:");
            let rows: Vec<Vec<Cell>> = result
                .partial
                .iter()
                .map(|item| {
                    vec![
                        Cell::text(&item.item),
                        Cell::text(&resource_text(item.resource.as_ref())),
                        Cell::text(&wire_name(&item.outcome)),
                        Cell::optional(item.code.as_deref()),
                    ]
                })
                .collect();
            out.indented_table(2, &["item", "resource", "outcome", "code"], &rows);
        }
    }
}

/// `stackly operation cancel`. It takes no `--yes`: cancelling an
/// operation needs no confirmation (spec 0012).
#[derive(Debug, Args)]
pub struct CancelArgs {
    /// The operation.
    #[command(flatten)]
    pub operation: IdArg,
    /// `--wait` and `--timeout`.
    #[command(flatten)]
    pub wait: WaitArgs,
}

impl RunCommand for CancelArgs {
    type Output = CancelReply;

    async fn run(self, cx: &mut Context) -> CommandOutput<CancelReply> {
        let id = self.operation.id;
        let call = Call::OperationCancel(CancelOperationParams { operation_id: id });
        let reply = match cx.call(call, CallSpec::control()).await {
            Ok(stackly_protocol::Reply::OperationCancel(reply)) => *reply,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        if self.wait.wait {
            if settled(reply.operation.state) {
                let failure = crate::wait::settle(
                    WaitMode::Cancel,
                    &OperationDetail {
                        operation: reply.operation.clone(),
                        steps: Vec::new(),
                        created: Vec::new(),
                    },
                );
                return with_failure(reply, failure);
            }
            let outcome = Waiter::wait(cx, id, &self.wait, WaitMode::Cancel).await;
            let reply = CancelReply {
                operation: outcome
                    .last
                    .map_or(reply.operation, |detail| detail.operation),
                disposition: reply.disposition,
            };
            return with_failure(reply, outcome.failure);
        }
        let failure = match reply.disposition {
            CancelDisposition::AlreadyFinal => Some(CliFailure::cancel_not_applied(
                id,
                &wire_name(&reply.operation.state),
            )),
            CancelDisposition::RecoveryRequired => Some(
                reply
                    .operation
                    .recovery
                    .as_ref()
                    .and_then(|recovery| recovery.error.clone())
                    .map_or_else(
                        || CliFailure::cancel_not_applied(id, &wire_name(&reply.operation.state)),
                        |error| CliFailure::daemon(error, None),
                    ),
            ),
            CancelDisposition::CancelledBeforeStart
            | CancelDisposition::Requested
            | CancelDisposition::ContinuingPastPointOfNoReturn
            | CancelDisposition::Unrecognized => None,
        };
        let waits = failure.is_none() && !settled(reply.operation.state);
        let output = with_failure(reply, failure);
        if waits {
            output.with_hint(format!("wait with: stackly operation inspect {id} --wait"))
        } else {
            output
        }
    }
}

impl Render for CancelReply {
    fn render(&self, out: &mut HumanWriter) {
        let id = self.operation.id;
        let state = wire_name(&self.operation.state);
        match self.disposition {
            CancelDisposition::AlreadyFinal => {
                out.line(&format!("operation {id} already ended (state {state})"));
            }
            CancelDisposition::RecoveryRequired => {
                out.line(&format!("operation {id} needs recovery (state {state})"));
                out.line("resume or abandon it instead");
            }
            disposition => {
                out.line(&format!(
                    "cancel requested for operation {id} (state {state})"
                ));
                match disposition {
                    CancelDisposition::CancelledBeforeStart => {
                        out.line("it had not started yet, so nothing of it ran");
                    }
                    CancelDisposition::ContinuingPastPointOfNoReturn => {
                        out.line(
                            "the current step is past its point of no return and runs to its end; the operation stops after it",
                        );
                    }
                    CancelDisposition::Unrecognized => {
                        out.line(
                            "the daemon answered with a disposition this stackly does not know",
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

/// `stackly operation resume`.
#[derive(Debug, Args)]
pub struct ResumeArgs {
    /// The operation.
    #[command(flatten)]
    pub operation: IdArg,
    /// `--wait` and `--timeout`.
    #[command(flatten)]
    pub wait: WaitArgs,
}

impl RunCommand for ResumeArgs {
    type Output = OperationSummary;

    async fn run(self, cx: &mut Context) -> CommandOutput<OperationSummary> {
        let id = self.operation.id;
        let revision = match get(cx, id).await {
            Ok(detail) => detail.operation.revision,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::OperationResume(ResumeOperationParams {
            operation_id: id,
            expected_revision: Some(revision),
        });
        let summary = match cx.call(call, CallSpec::control()).await {
            Ok(stackly_protocol::Reply::OperationResume(summary)) => *summary,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        if self.wait.wait {
            let outcome = Waiter::wait(cx, id, &self.wait, WaitMode::Observe).await;
            let summary = outcome.last.map_or(summary, |detail| detail.operation);
            return with_failure(summary, outcome.failure);
        }
        let waits = !settled(summary.state);
        let output = CommandOutput::ok(summary);
        if waits {
            output.with_hint(format!("wait with: stackly operation inspect {id} --wait"))
        } else {
            output
        }
    }
}

impl Render for OperationSummary {
    fn render(&self, out: &mut HumanWriter) {
        out.line(&format!(
            "resumed operation {} (state {})",
            self.id,
            wire_name(&self.state)
        ));
    }
}

/// `stackly operation abandon`, a destructive confirmation: `--yes` is
/// needed when stackly runs noninteractive, and the default answer is no.
#[derive(Debug, Args)]
pub struct AbandonArgs {
    /// The operation.
    #[command(flatten)]
    pub operation: IdArg,
    /// `--yes`.
    #[command(flatten)]
    pub confirm: ConfirmArgs,
}

impl RunCommand for AbandonArgs {
    type Output = AbandonReply;

    async fn run(self, cx: &mut Context) -> CommandOutput<AbandonReply> {
        let id = self.operation.id;
        let question = format!("abandon operation {id}?");
        let request = ConfirmRequest {
            role: ConfirmRole::Destructive,
            question: &question,
            consequence: "it ends failed with operation.abandoned, and what it created stays behind as leftovers",
        };
        if let Err(failure) =
            Confirm::ask(cx, request, self.confirm.approval(), OnNoninteractive::Fail)
        {
            return CommandOutput::failed(failure);
        }
        let revision: Revision = match get(cx, id).await {
            Ok(detail) => detail.operation.revision,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::OperationAbandon(AbandonOperationParams {
            operation_id: id,
            expected_revision: revision,
        });
        match cx.call(call, CallSpec::control()).await {
            Ok(stackly_protocol::Reply::OperationAbandon(reply)) => CommandOutput::ok(*reply),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for AbandonReply {
    fn render(&self, out: &mut HumanWriter) {
        out.line(&format!(
            "abandoned operation {} (state {})",
            self.operation.id,
            wire_name(&self.operation.state)
        ));
        if self.leftovers.is_empty() {
            out.line("No leftovers.");
        } else {
            out.line("Leftovers:");
            created_table(out, &self.leftovers);
        }
    }
}

/// `operation.get` for `id`.
async fn get(cx: &mut Context, id: Uuid) -> Result<OperationDetail, CliFailure> {
    let call = Call::OperationGet(GetOperationParams { operation_id: id });
    match cx.call(call, CallSpec::control()).await? {
        stackly_protocol::Reply::OperationGet(detail) => Ok(*detail),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// The output of a wait that ended with `last` and `failure`.
fn waited<T>(last: Option<T>, failure: Option<CliFailure>) -> CommandOutput<T> {
    match (last, failure) {
        (Some(last), None) => CommandOutput::ok(last),
        (Some(last), Some(failure)) => CommandOutput::partial(last, failure),
        (None, Some(failure)) => CommandOutput::failed(failure),
        (None, None) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
    }
}

fn with_failure<T>(result: T, failure: Option<CliFailure>) -> CommandOutput<T> {
    waited(Some(result), failure)
}

/// What an operation created, as an indented table.
fn created_table(out: &mut HumanWriter, items: &[CreatedItemInfo]) {
    let rows: Vec<Vec<Cell>> = items
        .iter()
        .map(|item| {
            vec![
                Cell::text(&item.kind),
                Cell::text(&item.reference),
                Cell::text(&wire_name(&item.state)),
                Cell::optional(item.detail.as_deref()),
            ]
        })
        .collect();
    out.indented_table(2, &["kind", "reference", "state", "detail"], &rows);
}

/// `done/total` for counted progress, `-` otherwise.
fn progress_text(progress: OperationProgress) -> String {
    match progress {
        OperationProgress::Counted(counted) => {
            format!("{}/{}", counted.done.get(), counted.total.get())
        }
        OperationProgress::Indeterminate => String::new(),
    }
}

/// `<kind>:<id>`, `manager`, or empty when there is none.
fn resource_text(resource: Option<&ResourceRef>) -> String {
    match resource {
        Some(ResourceRef {
            kind: ResourceKind::Manager,
            ..
        }) => "manager".to_owned(),
        Some(resource) => format!("{}:{}", wire_name(&resource.kind), resource.id),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_footer_repeats_the_flags_given_and_only_those() {
        let args = ListArgs {
            active: true,
            resource: Some(ResourceRef {
                kind: ResourceKind::Instance,
                id: "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e".to_owned(),
            }),
            limit: Some(20),
            before: Some("old".to_owned()),
        };
        assert_eq!(
            args.footer("next"),
            "more operations: stackly operation list --active --resource instance:0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e --limit 20 --before next"
        );
        let bare = ListArgs {
            active: false,
            resource: None,
            limit: None,
            before: None,
        };
        assert_eq!(
            bare.footer("n"),
            "more operations: stackly operation list --before n"
        );
    }

    #[test]
    fn resources_read_as_kind_and_id_or_manager() {
        assert_eq!(
            resource_text(Some(&ResourceRef {
                kind: ResourceKind::Manager,
                id: "self".to_owned()
            })),
            "manager"
        );
        assert_eq!(
            resource_text(Some(&ResourceRef {
                kind: ResourceKind::Runtime,
                id: "r".to_owned()
            })),
            "runtime:r"
        );
        assert_eq!(resource_text(None), "");
    }
}
