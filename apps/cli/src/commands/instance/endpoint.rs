//! `stackly instance endpoint set` (spec 0029, AC-14, AC-17): changes the
//! port request of IP endpoints, with the checks of create. A change of a
//! port on an instance an environment holds a lease on is refused unless
//! `--override-leases` is given, and a dry run names those environments
//! (spec 0047 AC-19).

use clap::{ArgAction, Args, Subcommand};
use stackly_protocol::codes::lookup;
use stackly_protocol::instance::config::{
    ChangeEndpointsParams, ChangeEndpointsReply, EndpointValidation,
};
use stackly_protocol::instance::{EndpointRequest, PortRequest};
use stackly_protocol::{Call, ErrorBody, Reply};

use super::config::{accepted_apply, mapped, read_view, request_text};
use super::resolve::TargetArg;
use super::{InstanceAction, Verb, conclude, lease};
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
        after_help = "A port change on an instance that an environment holds a lease on exits 3 with instance.endpoints_require_override unless --override-leases is given; --dry-run names those environments.\n\nExamples:\n  stackly instance endpoint set orders-db --port postgres=5433 --wait\n  stackly instance endpoint set orders-db --port postgres=5433 --override-leases --wait\n  stackly instance endpoint set orders-db --port postgres=auto --dry-run --json"
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
    /// Change the ports even when an environment holds a lease on the
    /// instance; the environment keeps its lease
    #[arg(long)]
    pub override_leases: bool,
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

impl SetArgs {
    /// The command line that asks for the same ports, for the hint of a
    /// refusal.
    fn retry(&self) -> String {
        let mut line = format!(
            "stackly instance endpoint set {}",
            escape_display(&self.instance.target.to_string())
        );
        for port in &self.ports {
            let value = match port.port {
                PortRequest::Auto => "auto".to_owned(),
                PortRequest::Fixed(number) => number.to_string(),
            };
            line.push_str(&format!(
                " --port {}={value}",
                escape_display(&port.endpoint)
            ));
        }
        line
    }
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
            override_leases: self.override_leases,
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
            Err(failure) => {
                let name = self.instance.target.to_string();
                CommandOutput::failed(lease::explain(cx, failure, &name, &self.retry()))
            }
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
                            escape_display(&problem.code)
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
                        request_text(change.old),
                        request_text(change.new)
                    ));
                }
                for environment in &verdict.leases {
                    out.line(&format!(
                        "  used by {} ({}): applying it needs --override-leases",
                        escape_display(&environment.name),
                        escape_display(&environment.alias)
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(result: &impl Render) -> String {
        let mut out = HumanWriter::new(false, jiff::Timestamp::UNIX_EPOCH, jiff::tz::TimeZone::UTC);
        result.render(&mut out);
        out.into_string()
    }

    #[test]
    fn an_endpoint_problem_code_is_escaped_like_its_endpoint() {
        let verdict: EndpointValidation = serde_json::from_value(serde_json::json!({
            "valid": false,
            "problems": [{"endpoint": "pg\u{1b}", "code": "in_use\u{1b}[2J", "suggestedPort": 5433}],
            "changes": [],
            "impact": "none",
        }))
        .unwrap();
        assert_eq!(
            render(&SetOutput::DryRun(verdict)),
            "not applicable\n  pg\\u{1b}: in_use\\u{1b}[2J, try port 5433\n"
        );
    }

    #[test]
    fn a_valid_dry_run_names_the_old_and_new_port_requests_like_port_takes_them() {
        let verdict: EndpointValidation = serde_json::from_value(serde_json::json!({
            "valid": true,
            "problems": [],
            "changes": [
                {
                    "name": "postgres",
                    "old": {"type": "fixed", "content": 5432},
                    "new": {"type": "fixed", "content": 5433},
                },
                {"name": "admin", "old": {"type": "fixed", "content": 8080}, "new": {"type": "auto"}},
            ],
            "impact": "next_start",
        }))
        .unwrap();
        assert_eq!(
            render(&SetOutput::DryRun(verdict)),
            "valid, impact next_start\n  postgres: 5432 -> 5433\n  admin: 8080 -> auto\n"
        );
    }
}
