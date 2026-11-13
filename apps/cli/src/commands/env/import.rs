//! `stackly env import [--file <path>] [--relink]` (spec 0044, *CLI*,
//! AC-17): `env.plan` for the file gives the `manifestSha256` the import
//! is bound to, then `env.import` registers it, so a file that changed in
//! between fails `env.manifest_changed`. Importing installs, creates, and
//! starts nothing, so it asks nothing. `--relink` moves the registered
//! environment of the same name to this file.

use std::path::PathBuf;

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{EnvironmentDetail, ImportEnvironmentParams, PlanRegistration};
use stackly_protocol::{Call, Reply};

use super::plan::{file_source, plan};
use super::render::{counts, findings};
use crate::commands::RunCommand;
use crate::commands::path::home_masked;
use crate::commands::provider::local::{esc, operation_call, shows_text, to_stderr};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};

/// The line of an import that changed nothing.
pub const ALREADY_IMPORTED: &str = "Already imported.";

/// `stackly env import`.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// The manifest; a relative path starts at the current folder
    /// [default: ./stackly.yaml]
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,
    /// Point the registered environment with this name at this file
    #[arg(long)]
    pub relink: bool,
}

/// The `env import` result: the environment after the import as JSON, and
/// one line about what happened above its rows in text.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Imported {
    #[serde(skip)]
    headline: String,
    detail: EnvironmentDetail,
}

impl Render for Imported {
    fn render(&self, out: &mut HumanWriter) {
        out.line(&self.headline);
        let summary = &self.detail.summary;
        let file = summary
            .manifest_location
            .as_deref()
            .map(|file| home_masked(file, std::env::home_dir().as_deref()));
        out.kv(&[
            ("Name", Cell::text(&summary.name)),
            ("ID", Cell::fixed(summary.id.to_string())),
            ("File", Cell::optional(file.as_deref())),
            ("Status", Cell::text(&wire_name(&summary.status))),
            (
                "Services",
                Cell::fixed(format!(
                    "{} ready of {}",
                    u64::from(summary.ready_count),
                    u64::from(summary.service_count)
                )),
            ),
        ]);
    }
}

impl RunCommand for ImportArgs {
    type Output = Imported;

    async fn run(self, cx: &mut Context) -> CommandOutput<Imported> {
        let source = match file_source(self.file) {
            Ok(source) => source,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let stackly_protocol::env::EnvSource::File(path) = source.clone() else {
            return CommandOutput::failed(CliFailure::unexpected_reply(None));
        };
        let plan = match plan(cx, source).await {
            Ok(plan) => plan,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let expected = match (&plan.validation.manifest_sha256, plan.validation.valid) {
            (Some(sha256), true) => sha256.clone(),
            _ => {
                if shows_text(cx, false) {
                    to_stderr(cx, |out| findings(out, &plan.validation));
                }
                let (errors, warnings) = counts(&plan.validation);
                return CommandOutput::failed(CliFailure::env_invalid(errors, warnings));
            }
        };
        let relink = (self.relink && plan.registration == Some(PlanRegistration::NameTaken))
            .then(|| plan.environment.as_ref().map(|environment| environment.id))
            .flatten();
        let before = plan
            .environment
            .as_ref()
            .map(|environment| environment.revision);
        let call = Call::EnvImport(ImportEnvironmentParams {
            path,
            expected_manifest_sha256: expected,
            relink_environment_id: relink,
        });
        let detail = match cx.call(call, operation_call()).await {
            Ok(Reply::EnvImport(detail)) => *detail,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let headline = if before == Some(detail.summary.revision) {
            ALREADY_IMPORTED.to_owned()
        } else {
            format!("Imported environment {}.", esc(&detail.summary.name))
        };
        CommandOutput::ok(Imported { headline, detail })
    }
}
