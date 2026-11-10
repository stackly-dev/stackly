//! `stackly operation list`, `inspect`, `cancel`, `resume`, and `abandon`
//! (spec 0008, *`stackly operation list`, `inspect`, `cancel`*, AC-7,
//! AC-8, AC-15). They only talk to the daemon and never read the registry.
//!
//! Whether a cancel can still stop an operation safely comes from the
//! daemon's `cancel` field (spec 0035, *Ketersediaan cancel*); the CLI keeps
//! no table of kinds, and reads an older daemon that does not send it with
//! the fallback rule of that spec.

use clap::{Args, Subcommand};
use serde::Serialize;
use stackly_protocol::operation::{
    AbandonOperationParams, AbandonReply, CancelAvailability, CancelDisposition,
    CancelOperationParams, CancelReply, CreatedItemInfo, GetOperationParams, ListOperationsParams,
    OPERATION_LIST_LIMIT_DEFAULT, OPERATION_LIST_LIMIT_MAX, OperationDetail, OperationProgress,
    OperationState, OperationSummary, RecoveryAction, ResumeOperationParams,
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
    /// Ask the daemon to cancel an operation; one past a step that can't be
    /// undone continues to a safe end
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
                    Cell::fixed(cancel_column(cancel_of(operation))),
                    Cell::text(&resource_text(operation.resource.as_ref())),
                    Cell::fixed(out.age(operation.created_at)),
                ]
            })
            .collect();
        out.table(
            &[
                "id", "kind", "state", "stage", "progress", "cancel", "resource", "age",
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
        if let Some(sentence) = cancel_sentence(operation) {
            rows.push(("Cancel", Cell::fixed(sentence)));
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
                out.line(recovery_sentence(&self.operation));
            }
            disposition => {
                out.line(&format!(
                    "cancel requested for operation {id} (state {state})"
                ));
                out.line(&disposition_sentence(disposition, &self.operation));
            }
        }
    }
}

/// What a cancel the daemon took does, in the words of spec 0035, *CLI*.
fn disposition_sentence(disposition: CancelDisposition, operation: &OperationSummary) -> String {
    match disposition {
        CancelDisposition::CancelledBeforeStart => {
            "it had not started yet, so nothing of it ran".to_owned()
        }
        CancelDisposition::ContinuingPastPointOfNoReturn => format!(
            "it is past a step that can't be undone, so it continues to a safe end; check it with stackly operation inspect {}",
            operation.id
        ),
        // The engine answers `requested` past the point of no return only
        // for a kind that closes as interrupted, which stops safely.
        CancelDisposition::Requested if operation.point_of_no_return_at.is_some() => {
            STOPS_SAFELY.to_owned()
        }
        CancelDisposition::Requested => STOPS_AND_UNDOES.to_owned(),
        CancelDisposition::AlreadyFinal => "it had already ended, so nothing changed".to_owned(),
        CancelDisposition::RecoveryRequired => {
            format!("it needs recovery; {}", recovery_sentence(operation))
        }
        CancelDisposition::Unrecognized => {
            "the daemon answered with a disposition this stackly does not know".to_owned()
        }
    }
}

/// What `operation`, which needs recovery, lets you do instead of a cancel,
/// read only from its `recovery.actions` (spec 0035, AC-10): resume only
/// when it offers `resume`, abandon only when it offers `abandon`, and no
/// command at all when it offers neither or carries no recovery.
fn recovery_sentence(operation: &OperationSummary) -> &'static str {
    let actions = operation
        .recovery
        .as_ref()
        .map(|recovery| recovery.actions.as_slice())
        .unwrap_or_default();
    match (
        actions.contains(&RecoveryAction::Resume),
        actions.contains(&RecoveryAction::Abandon),
    ) {
        (true, true) => "resume or abandon it instead",
        (true, false) => "resume it instead",
        (false, true) => "abandon it instead",
        (false, false) => "it offers no recovery action now",
    }
}

/// What a cancel does before the point of no return.
const STOPS_AND_UNDOES: &str = "it stops at its next safe point and undoes what it did";
/// What a cancel does to a kind that closes as interrupted past its point
/// of no return.
const STOPS_SAFELY: &str = "it stops safely";

/// Whether a cancel can stop `operation` safely now: the daemon's `cancel`,
/// or for an older daemon without it, `queued` or `running` as available,
/// `cancel_requested` as requested, and anything else as none (spec 0035,
/// *Ketersediaan cancel*). A value this build does not know is none.
fn cancel_of(operation: &OperationSummary) -> CancelAvailability {
    match operation.cancel {
        Some(CancelAvailability::Unrecognized) => CancelAvailability::None,
        Some(cancel) => cancel,
        None => match operation.state {
            OperationState::Queued | OperationState::Running => CancelAvailability::Available,
            OperationState::CancelRequested => CancelAvailability::Requested,
            OperationState::Succeeded
            | OperationState::Failed
            | OperationState::Cancelled
            | OperationState::NeedsRecovery => CancelAvailability::None,
        },
    }
}

/// The CANCEL column of `operation list`: `yes`, `no`, `requested`,
/// `finishing`, or empty.
const fn cancel_column(cancel: CancelAvailability) -> &'static str {
    match cancel {
        CancelAvailability::Available => "yes",
        CancelAvailability::PastPointOfNoReturn => "no",
        CancelAvailability::Requested => "requested",
        CancelAvailability::Continuing => "finishing",
        CancelAvailability::None | CancelAvailability::Unrecognized => "",
    }
}

/// The `Cancel` line of `operation inspect`, or none when a cancel changes
/// nothing. Past the point of no return, an operation the daemon still
/// lets cancel is of a kind that closes as interrupted, which stops safely;
/// the fallback for an older daemon does not know that, so it says what a
/// cancel does before that point.
fn cancel_sentence(operation: &OperationSummary) -> Option<String> {
    let stops_safely = operation.cancel.is_some() && operation.point_of_no_return_at.is_some();
    let effect = if stops_safely {
        STOPS_SAFELY
    } else {
        STOPS_AND_UNDOES
    };
    match cancel_of(operation) {
        CancelAvailability::Available => Some(format!("possible now, {effect}")),
        CancelAvailability::PastPointOfNoReturn => {
            Some("not possible now, the current step can't be undone safely".to_owned())
        }
        CancelAvailability::Requested => Some(format!("requested, {effect}")),
        CancelAvailability::Continuing => Some(
            "requested past a step that can't be undone, so it continues to a safe end".to_owned(),
        ),
        CancelAvailability::None | CancelAvailability::Unrecognized => None,
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

    fn operation(state: OperationState, cancel: Option<CancelAvailability>) -> OperationSummary {
        serde_json::from_value(serde_json::json!({
            "id": "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e",
            "kind": "install_runtime",
            "resource": null,
            "state": state,
            "stage": null,
            "progress": { "type": "indeterminate" },
            "trigger": { "origin": "user", "client": "cli" },
            "createdAt": "2026-11-10T08:00:00Z",
            "updatedAt": "2026-11-10T08:00:00Z",
            "revision": 2,
            "cancel": cancel,
        }))
        .unwrap()
    }

    #[test]
    fn the_cancel_column_reads_the_daemon_value_or_the_fallback_of_an_older_daemon() {
        use CancelAvailability as C;
        use OperationState as S;
        for (cancel, column) in [
            (C::Available, "yes"),
            (C::PastPointOfNoReturn, "no"),
            (C::Requested, "requested"),
            (C::Continuing, "finishing"),
            (C::None, ""),
            (C::Unrecognized, ""),
        ] {
            assert_eq!(
                cancel_column(cancel_of(&operation(S::Running, Some(cancel)))),
                column,
                "{cancel:?}"
            );
        }
        for (state, column) in [
            (S::Queued, "yes"),
            (S::Running, "yes"),
            (S::CancelRequested, "requested"),
            (S::Succeeded, ""),
            (S::Failed, ""),
            (S::Cancelled, ""),
            (S::NeedsRecovery, ""),
        ] {
            assert_eq!(
                cancel_column(cancel_of(&operation(state, None))),
                column,
                "{state:?}"
            );
        }
    }

    #[test]
    fn inspect_says_what_a_cancel_would_do_now_and_nothing_when_it_changes_nothing() {
        use CancelAvailability as C;
        use OperationState as S;
        let past = |mut operation: OperationSummary| {
            operation.point_of_no_return_at = Some("2026-11-10T08:00:01Z".parse().unwrap());
            operation
        };
        let sentence = |operation: OperationSummary| cancel_sentence(&operation);
        assert_eq!(
            sentence(operation(S::Running, Some(C::Available))).as_deref(),
            Some("possible now, it stops at its next safe point and undoes what it did")
        );
        assert_eq!(
            sentence(past(operation(S::Running, Some(C::Available)))).as_deref(),
            Some("possible now, it stops safely")
        );
        assert_eq!(
            sentence(past(operation(S::Running, Some(C::PastPointOfNoReturn)))).as_deref(),
            Some("not possible now, the current step can't be undone safely")
        );
        assert_eq!(
            sentence(operation(S::CancelRequested, Some(C::Requested))).as_deref(),
            Some("requested, it stops at its next safe point and undoes what it did")
        );
        assert_eq!(
            sentence(past(operation(S::CancelRequested, Some(C::Continuing)))).as_deref(),
            Some("requested past a step that can't be undone, so it continues to a safe end")
        );
        assert_eq!(
            sentence(past(operation(S::Running, None))).as_deref(),
            Some("possible now, it stops at its next safe point and undoes what it did"),
            "an older daemon does not say which kind stops safely"
        );
        assert_eq!(sentence(operation(S::Succeeded, Some(C::None))), None);
        assert_eq!(sentence(operation(S::NeedsRecovery, None)), None);
        assert_eq!(sentence(operation(S::Running, Some(C::Unrecognized))), None);
    }

    #[test]
    fn a_cancel_says_what_it_does_in_the_words_of_spec_0035() {
        let running = operation(OperationState::CancelRequested, None);
        assert_eq!(
            disposition_sentence(CancelDisposition::Requested, &running),
            "it stops at its next safe point and undoes what it did"
        );
        let mut past = running.clone();
        past.point_of_no_return_at = Some("2026-11-10T08:00:01Z".parse().unwrap());
        assert_eq!(
            disposition_sentence(CancelDisposition::Requested, &past),
            "it stops safely"
        );
        assert_eq!(
            disposition_sentence(CancelDisposition::ContinuingPastPointOfNoReturn, &past),
            "it is past a step that can't be undone, so it continues to a safe end; check it with stackly operation inspect 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e"
        );
        assert_eq!(
            disposition_sentence(CancelDisposition::CancelledBeforeStart, &running),
            "it had not started yet, so nothing of it ran"
        );
        assert_eq!(
            disposition_sentence(CancelDisposition::Unrecognized, &running),
            "the daemon answered with a disposition this stackly does not know"
        );
    }

    #[test]
    fn a_cancel_that_needs_recovery_suggests_only_the_actions_the_operation_offers() {
        use RecoveryAction::{Abandon, Resume, Unrecognized};
        let recovering = |actions: Option<Vec<RecoveryAction>>| {
            let mut operation = operation(OperationState::NeedsRecovery, None);
            operation.recovery = actions.map(|actions| {
                serde_json::from_value(serde_json::json!({
                    "hint": "abandon",
                    "cause": "step_failed",
                    "actions": actions,
                }))
                .unwrap()
            });
            operation
        };
        for (actions, sentence) in [
            (Some(vec![Resume, Abandon]), "resume or abandon it instead"),
            (Some(vec![Abandon, Resume]), "resume or abandon it instead"),
            (Some(vec![Resume]), "resume it instead"),
            (Some(vec![Abandon]), "abandon it instead"),
            (Some(vec![Unrecognized, Abandon]), "abandon it instead"),
            (Some(Vec::new()), "it offers no recovery action now"),
            (Some(vec![Unrecognized]), "it offers no recovery action now"),
            (None, "it offers no recovery action now"),
        ] {
            let operation = recovering(actions.clone());
            assert_eq!(recovery_sentence(&operation), sentence, "{actions:?}");
            assert_eq!(
                disposition_sentence(CancelDisposition::RecoveryRequired, &operation),
                format!("it needs recovery; {sentence}"),
                "{actions:?}"
            );
        }
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
