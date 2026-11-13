//! `stackly env status [target]` (spec 0046, *CLI*, AC-18): reads
//! `env.get` with `checkManifest` true and prints the head line, one row per
//! service with the ports that are really bound, why each service that is
//! not ready is not, and the manifest state when it is not `in_sync`. The
//! JSON result is the `EnvironmentDetail` the Environments screen shows.
//! It exits 0 whatever the status is; an environment that is not known
//! exits 10 (`cli.environment_not_found`). A path is first planned, for the
//! ID of the environment registered at it.

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{
    EnvGetParams, EnvSource, EnvironmentDetail, ManifestState, PlanRegistration,
};
use stackly_protocol::{Call, Reply};

use super::plan::plan;
use super::render::{counts, findings};
use super::services::{not_ready_lines, ready_line, status_table, status_title};
use super::target::{Resolved, TargetArgs};
use crate::commands::RunCommand;
use crate::commands::path::home_masked;
use crate::commands::provider::local::{esc, shows_text, to_stderr};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render, wire_name};
use crate::session::CallSpec;

/// `stackly env status`.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// The manifest or the environment.
    #[command(flatten)]
    pub target: TargetArgs,
}

/// The `env status` result: the detail as the daemon sent it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Status(pub EnvironmentDetail);

impl Render for Status {
    fn render(&self, out: &mut HumanWriter) {
        let detail = &self.0;
        let summary = &detail.summary;
        let mut head = format!(
            "{}  {}  {}",
            esc(&summary.name),
            status_title(summary.status),
            ready_line(detail).trim_end_matches('.')
        );
        if let Some(file) = &summary.manifest_location {
            head.push_str("  ");
            head.push_str(&esc(&home_masked(file, std::env::home_dir().as_deref())));
        }
        out.line(&head);
        if !detail.services.is_empty() {
            out.line("");
            status_table(out, detail);
        }
        if detail
            .services
            .iter()
            .any(|service| service.not_ready.is_some())
        {
            out.line("");
            not_ready_lines(out, detail);
        }
        if let Some(state) = detail
            .manifest_state
            .filter(|state| *state != ManifestState::InSync)
        {
            out.line(&format!("manifest: {}", wire_name(&state)));
        }
    }
}

impl RunCommand for StatusArgs {
    type Output = Status;

    async fn run(self, cx: &mut Context) -> CommandOutput<Status> {
        let id = match self.target.resolve(cx).await {
            Ok(Resolved::Registered(summary)) => summary.id,
            Ok(Resolved::File(file)) => {
                let plan = match plan(cx, EnvSource::File(file.clone()), false).await {
                    Ok(plan) => plan,
                    Err(failure) => return CommandOutput::failed(failure),
                };
                if !plan.validation.valid {
                    if shows_text(cx, false) {
                        to_stderr(cx, |out| findings(out, &plan.validation));
                    }
                    let (errors, warnings) = counts(&plan.validation);
                    let failure = CliFailure::env_invalid(errors, warnings);
                    return CommandOutput::failed(failure).with_hint(
                        "Name the environment instead: stackly env status <environment>.",
                    );
                }
                match (plan.registration, plan.environment) {
                    (Some(PlanRegistration::Registered), Some(environment)) => environment.id,
                    _ => return CommandOutput::failed(CliFailure::environment_not_imported(&file)),
                }
            }
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::EnvGet(EnvGetParams {
            id,
            check_manifest: true,
        });
        match cx.call(call, CallSpec::control()).await {
            Ok(Reply::EnvGet(detail)) => CommandOutput::ok(Status(*detail)),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
