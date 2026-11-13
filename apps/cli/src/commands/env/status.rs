//! `stackly env status [target]` (spec 0046, *CLI*, AC-18): reads
//! `env.get` with `checkManifest` true and prints the head line, one row per
//! service with the ports that are really bound, why each service that is
//! not ready is not, and the manifest state when it is not `in_sync`. The
//! JSON result is the `EnvironmentDetail` the Environments screen shows.
//! It exits 0 whatever the status is; an environment that is not known
//! exits 10 (`cli.environment_not_found`). A path is first planned, for the
//! ID of the environment registered at it. Under the head line it prints
//! `lease: held on <n> instances since <time>` or `lease: none` (spec 0047
//! AC-22), and nothing for a daemon that does not send the lease.

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{
    EnvGetParams, EnvLeaseInfo, EnvSource, EnvironmentDetail, ManifestState, PlanRegistration,
};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

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

/// The `lease:` line of `env status` (spec 0047 AC-22).
#[must_use]
pub fn lease_line(lease: &EnvLeaseInfo) -> String {
    let count = lease.active_count.get();
    if count == 0 {
        return "lease: none".to_owned();
    }
    let instances = if count == 1 {
        "1 instance".to_owned()
    } else {
        format!("{count} instances")
    };
    match &lease.held_since {
        Some(since) => format!("lease: held on {instances} since {since}"),
        None => format!("lease: held on {instances}"),
    }
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
        if let Some(lease) = &detail.lease {
            out.line(&lease_line(lease));
        }
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

/// The ID of the environment registered at the manifest `file`, from
/// `env.plan` (spec 0046, *Target*). A manifest that is not valid ends with
/// `cli.env_invalid` and the hint to name the environment for `verb`
/// instead; a file no environment is registered at with
/// `cli.environment_not_found`.
///
/// # Errors
///
/// The output the command ends with.
pub async fn planned_id<T>(
    cx: &mut Context,
    file: String,
    verb: &str,
) -> Result<Uuid, Box<CommandOutput<T>>> {
    let plan = match plan(cx, EnvSource::File(file.clone()), false).await {
        Ok(plan) => plan,
        Err(failure) => return Err(Box::new(CommandOutput::failed(failure))),
    };
    if !plan.validation.valid {
        if shows_text(cx, false) {
            to_stderr(cx, |out| findings(out, &plan.validation));
        }
        let (errors, warnings) = counts(&plan.validation);
        let failure = CliFailure::env_invalid(errors, warnings);
        return Err(Box::new(CommandOutput::failed(failure).with_hint(format!(
            "Name the environment instead: stackly env {verb} <environment>."
        ))));
    }
    match (plan.registration, plan.environment) {
        (Some(PlanRegistration::Registered), Some(environment)) => Ok(environment.id),
        _ => Err(Box::new(CommandOutput::failed(
            CliFailure::environment_not_imported(&file),
        ))),
    }
}

impl RunCommand for StatusArgs {
    type Output = Status;

    async fn run(self, cx: &mut Context) -> CommandOutput<Status> {
        let id = match self.target.resolve(cx).await {
            Ok(Resolved::Registered(summary)) => summary.id,
            Ok(Resolved::File(file)) => match planned_id(cx, file, "status").await {
                Ok(id) => id,
                Err(output) => return *output,
            },
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

#[cfg(test)]
mod tests {
    use stackly_protocol::{Count, Timestamp};

    use super::*;

    #[test]
    fn the_lease_line_counts_instances_and_says_since_when() {
        let since: Timestamp = "2026-11-14T08:30:00.000Z".parse().unwrap();
        let lease = |count: u64| EnvLeaseInfo {
            active_count: Count::saturating(count),
            held_since: (count > 0).then_some(since),
        };
        assert_eq!(lease_line(&lease(0)), "lease: none");
        assert_eq!(
            lease_line(&lease(1)),
            format!("lease: held on 1 instance since {since}")
        );
        assert_eq!(
            lease_line(&lease(3)),
            format!("lease: held on 3 instances since {since}")
        );
    }
}
