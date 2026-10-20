//! `stackly instance create`, `start`, `stop`, and `get` (spec 0014,
//! *CLI*, AC-5 to AC-8). They only talk to the daemon: the CLI never reads
//! a provider package or starts a process.
//!
//! `create`, `start`, and `stop` start an operation through
//! [`Context::submit`]. Their result is `{ operation, idempotencyKey,
//! instance }`, where `instance` is read with one `instance.get` once
//! nothing is left to wait for (AC-6).

mod action;
mod create;
mod get;
pub mod render;

use clap::{Args, Subcommand};
use serde::Serialize;
use stackly_protocol::instance::{GetInstanceParams, InstanceActionAccepted, InstanceSummary};
use stackly_protocol::operation::OperationSummary;
use stackly_protocol::{Call, Reply, ResourceKind};
use uuid::Uuid;

use self::action::{StartArgs, StopArgs};
use self::create::CreateArgs;
use self::get::GetArgs;
use crate::args::InstanceIdArg;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render, wire_name};
use crate::session::CallSpec;
use crate::wait::{Accepted, Submission};

use super::execute;

/// The verbs of `stackly instance`.
#[derive(Debug, Subcommand)]
pub enum InstanceCommand {
    /// Create an instance from a provider and an installed runtime
    // provisional: F20 (--runtime takes software@version, --provider and
    // --revision turn optional, --set follows the field kind)
    #[command(
        after_help = "Examples:\n  stackly instance create demo --runtime 0192f3a5-0000-7000-8000-0000000000a1 --provider fake --revision 0.2.0 --wait\n  stackly instance create demo --runtime 0192f3a5-0000-7000-8000-0000000000a1 --provider fake --revision 0.2.0 --port http=auto --set-json readyDelayMs=1500 --json"
    )]
    Create(CreateArgs),
    /// Start an instance
    #[command(
        after_help = "Examples:\n  stackly instance start 0192f3a5-0000-7000-8000-000000000001\n  stackly instance start 0192f3a5-0000-7000-8000-000000000001 --wait --timeout 5m"
    )]
    Start(StartArgs),
    /// Stop an instance
    #[command(
        after_help = "Examples:\n  stackly instance stop 0192f3a5-0000-7000-8000-000000000001\n  stackly instance stop 0192f3a5-0000-7000-8000-000000000001 --wait --json"
    )]
    Stop(StopArgs),
    /// Show one instance with its status and endpoints
    #[command(
        after_help = "Examples:\n  stackly instance get 0192f3a5-0000-7000-8000-000000000001\n  stackly instance get 0192f3a5-0000-7000-8000-000000000001 --json"
    )]
    Get(GetArgs),
}

impl InstanceCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Create(args) => execute(args, cx).await,
            Self::Start(args) => execute(args, cx).await,
            Self::Stop(args) => execute(args, cx).await,
            Self::Get(args) => execute(args, cx).await,
        }
    }
}

/// An instance ID argument.
#[derive(Debug, Args)]
pub struct InstanceArg {
    /// The instance ID, a whole UUID
    #[arg(value_name = "INSTANCE_ID")]
    pub id: InstanceIdArg,
}

/// Which verb an [`InstanceAction`] answers, for its text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verb {
    /// `create`, with the name that was asked for.
    Create { name: String },
    /// `start`.
    Start,
    /// `stop`.
    Stop,
}

impl Verb {
    /// The lifecycle an instance already in the asked state is in, for the
    /// line `already <lifecycle>` when `instance.get` could not say.
    const fn already(&self) -> &'static str {
        match self {
            Self::Create { .. } | Self::Start => "running",
            Self::Stop => "stopped",
        }
    }
}

/// The `result` of `create`, `start`, and `stop`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceAction {
    /// The operation: as accepted, or with `--wait` as it stands at the
    /// end. Null when the instance was already in the asked state.
    pub operation: Option<OperationSummary>,
    /// The key the request carried.
    pub idempotency_key: String,
    /// The instance, read once after the wait or after `alreadyInState`;
    /// null otherwise, and null when that read failed.
    pub instance: Option<InstanceSummary>,
    #[serde(skip)]
    verb: Verb,
    #[serde(skip)]
    instance_id: Option<Uuid>,
}

impl Render for InstanceAction {
    fn render(&self, out: &mut HumanWriter) {
        let Some(operation) = &self.operation else {
            let lifecycle = self.instance.as_ref().map_or_else(
                || self.verb.already().to_owned(),
                |i| wire_name(&i.lifecycle),
            );
            out.line(&format!("already {lifecycle}"));
            return;
        };
        if let (true, Some(instance)) = (
            operation.state == stackly_protocol::operation::OperationState::Succeeded,
            &self.instance,
        ) {
            let name = escaped(&instance.name);
            match self.verb {
                Verb::Create { .. } => out.line(&format!("created {name} ({})", instance.id)),
                Verb::Start | Verb::Stop => out.line(&format!(
                    "{name} {}, readiness {}",
                    wire_name(&instance.lifecycle),
                    wire_name(&instance.readiness.state)
                )),
            }
            return;
        }
        if matches!(self.verb, Verb::Create { .. })
            && let Some(id) = self.instance_id
        {
            out.line(&format!("instance {id}"));
        }
        out.line(&format!(
            "operation {} {}",
            operation.id,
            wire_name(&operation.state)
        ));
    }
}

/// `text` from the daemon or an argument, escaped for the terminal.
fn escaped(text: &str) -> String {
    crate::output::escape::escape_display(text).into_owned()
}

/// The ID of the instance an operation works on, from its `resource`.
fn instance_of(operation: Option<&OperationSummary>) -> Option<Uuid> {
    operation
        .and_then(|operation| operation.resource.as_ref())
        .filter(|resource| resource.kind == ResourceKind::Instance)
        .and_then(|resource| Uuid::try_parse(&resource.id).ok())
}

/// The operation a reply of `instance.create`, `start`, or `stop` holds.
fn accepted(reply: &InstanceActionAccepted) -> Accepted {
    Accepted {
        operation: reply
            .accepted
            .as_ref()
            .map(|accepted| accepted.operation.clone()),
        replayed: reply
            .accepted
            .as_ref()
            .is_some_and(|accepted| accepted.replayed),
    }
}

/// `instance.get` for `id`.
async fn read_instance(cx: &mut Context, id: Uuid) -> Result<InstanceSummary, CliFailure> {
    let call = Call::InstanceGet(GetInstanceParams { instance_id: id });
    match cx.call(call, CallSpec::control()).await? {
        Reply::InstanceGet(instance) => Ok(*instance),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// Shapes the result of a submitted request (AC-6): once nothing is left to
/// wait for, one `instance.get` fills `instance`, even next to an error of
/// the operation; when that read fails, `instance` stays null and the exit
/// code and error stay the operation's.
async fn conclude(
    cx: &mut Context,
    submission: Submission,
    verb: Verb,
    instance_id: Option<Uuid>,
) -> CommandOutput<InstanceAction> {
    let instance_id = instance_id.or_else(|| instance_of(submission.operation.as_ref()));
    let instance = match instance_id {
        Some(id) if submission.finished() => read_instance(cx, id).await.ok(),
        _ => None,
    };
    let hint = submission
        .failure
        .is_none()
        .then(|| submission.wait_hint())
        .flatten();
    let replayed = submission.replayed;
    let action = InstanceAction {
        operation: submission.operation,
        idempotency_key: submission.idempotency_key,
        instance,
        verb,
        instance_id,
    };
    let output = match submission.failure {
        Some(failure) => CommandOutput::partial(action, failure),
        None => CommandOutput::ok(action),
    };
    let output = if replayed {
        output.with_hint("the idempotency key matched an earlier request, so this is its operation")
    } else {
        output
    };
    match hint {
        Some(hint) => output.with_hint(hint),
        None => output,
    }
}
