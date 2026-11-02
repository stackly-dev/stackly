//! `stackly instance endpoint set` (spec 0029, AC-14, AC-17): changes the
//! port request of IP endpoints, with the checks of create.

use clap::{ArgAction, Args, Subcommand};
use stackly_protocol::codes::lookup;
use stackly_protocol::instance::EndpointRequest;
use stackly_protocol::instance::config::{
    ChangeEndpointsParams, ChangeEndpointsReply, EndpointValidation,
};
use stackly_protocol::{Call, ErrorBody, Reply};

use super::config::{accepted_apply, mapped, read_view};
use super::resolve::TargetArg;
use super::{InstanceAction, Verb, conclude};
use crate::args::PortArg;
use crate::commands::{RunCommand, execute};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::escape::escape_display;
use crate::output::human::{HumanWriter, Render, wire_name};
use crate::session::CallSpec;
use crate::wait::{Accepted, AsyncArgs};

/// The verbs of `stackly instance endpoint`.
#[derive(Debug, Subcommand)]
pub enum EndpointCommand {
    /// Change the port of one or more endpoints, then restart a running instance
    #[command(
        after_help = "Examples:\n  stackly instance endpoint set orders-db --port postgres=5433 --wait\n  stackly instance endpoint set orders-db --port postgres=auto --dry-run --json"
    )]
    Set(SetArgs),
}

impl EndpointCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Set(args) => execute(args, cx).await,
        }
    }
}

/// `stackly instance endpoint set`.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// A port for an endpoint: <endpoint>=<port> or <endpoint>=auto; repeat for more endpoints
    #[arg(long = "port", value_name = "ENDPOINT=PORT", action = ArgAction::Append, required = true)]
    pub ports: Vec<PortArg>,
    /// Only judge the ports: print the verdict and change nothing
    #[arg(long)]
    pub dry_run: bool,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// The result of `set`: the verdict of `--dry-run`, or the operation.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum SetOutput {
    /// `--dry-run`.
    DryRun(EndpointValidation),
    /// A change.
    Changed(Box<InstanceAction>),
}

fn extract(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceChangeEndpoints(reply) => match *reply {
            ChangeEndpointsReply::Applied(applied) => Some(accepted_apply(&applied)),
            _ => None,
        },
        _ => None,
    }
}

/// The error of the first request a dry run refuses, with the code and the
/// free port the daemon found, so the exit code is the one `set` would
/// answer with.
fn first_problem(verdict: &EndpointValidation) -> Option<CliFailure> {
    let problem = verdict.problems.first()?;
    let spec = lookup(&problem.code)?;
    let body = ErrorBody::new(spec, "a port request cannot be applied");
    let body = if spec.details.contains(&"endpoint") {
        body.with_detail("endpoint", problem.endpoint.clone())
    } else {
        body
    };
    let body = match problem.suggested_port {
        Some(port) if spec.details.contains(&"suggestedPort") => {
            body.with_detail("suggestedPort", port.to_string())
        }
        _ => body,
    };
    Some(CliFailure::daemon(Box::new(body), None))
}

impl RunCommand for SetArgs {
    type Output = SetOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<SetOutput> {
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let view = match read_view(cx, id).await {
            Ok(view) => view,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let endpoints: Vec<EndpointRequest> = self
            .ports
            .iter()
            .map(|port| EndpointRequest {
                name: port.endpoint.clone(),
                port: port.port,
            })
            .collect();
        let params = ChangeEndpointsParams {
            instance_id: id,
            base_revision: view.revision.number,
            endpoints,
            dry_run: self.dry_run,
        };
        if self.dry_run {
            let reply = cx
                .call(Call::InstanceChangeEndpoints(params), CallSpec::control())
                .await;
            let verdict = match reply {
                Ok(Reply::InstanceChangeEndpoints(reply)) => match *reply {
                    ChangeEndpointsReply::DryRun(verdict) => verdict,
                    _ => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
                },
                Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
                Err(failure) => return CommandOutput::failed(failure),
            };
            return match first_problem(&verdict) {
                Some(failure) => CommandOutput::partial(SetOutput::DryRun(verdict), failure),
                None => CommandOutput::ok(SetOutput::DryRun(verdict)),
            };
        }
        match cx
            .submit(Call::InstanceChangeEndpoints(params), &self.run, extract)
            .await
        {
            Ok(submission) => {
                let output = conclude(cx, submission, Verb::EndpointSet, Some(id)).await;
                mapped(output, |action| SetOutput::Changed(Box::new(action)))
            }
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for SetOutput {
    fn render(&self, out: &mut HumanWriter) {
        match self {
            Self::Changed(action) => action.render(out),
            Self::DryRun(verdict) => {
                if !verdict.valid {
                    out.line("not applicable");
                    for problem in &verdict.problems {
                        let suggestion = problem
                            .suggested_port
                            .map_or_else(String::new, |port| format!(", try port {port}"));
                        out.line(&format!(
                            "  {}: {}{suggestion}",
                            escape_display(&problem.endpoint),
                            problem.code
                        ));
                    }
                    return;
                }
                if verdict.changes.is_empty() {
                    out.line("valid, nothing changes");
                    return;
                }
                out.line(&format!("valid, impact {}", wire_name(&verdict.impact)));
                for change in &verdict.changes {
                    out.line(&format!(
                        "  {}: {} -> {}",
                        escape_display(&change.name),
                        wire_name(&change.old),
                        wire_name(&change.new)
                    ));
                }
            }
        }
    }
}
