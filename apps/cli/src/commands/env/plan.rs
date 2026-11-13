//! `stackly env plan [--file <path> | <environment>]` (spec 0044, *CLI*,
//! AC-16): asks the daemon's `env.plan` for a file or for the manifest of
//! a registered environment, and prints one table row per step with the
//! summary line. The table goes to stdout first; then blockers, secrets,
//! differences, and members no longer in the manifest go to stderr. Exit
//! 0 when the plan can be applied, 2 with `cli.env_invalid` for a
//! manifest that is not valid, 3 with `cli.env_plan_blocked` otherwise;
//! the plan is the result each time.

use std::path::PathBuf;

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{EnvPlan, EnvPlanParams, EnvSource};
use stackly_protocol::{Call, Reply};

use super::DEFAULT_FILE;
use super::render::{counts, findings, plan_sections, plan_table};
use super::target::EnvironmentTarget;
use crate::commands::RunCommand;
use crate::commands::path::absolute;
use crate::commands::provider::local::{shows_text, to_stderr};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};
use crate::session::CallSpec;

/// `stackly env plan`.
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// The manifest; a relative path starts at the current folder
    /// [default: ./stackly.yaml]
    #[arg(long, value_name = "PATH", conflicts_with = "environment")]
    pub file: Option<PathBuf>,
    /// A registered environment, by name or whole ID; its manifest is read
    #[arg(value_name = "ENVIRONMENT")]
    pub environment: Option<EnvironmentTarget>,
}

/// The `env plan` result: the plan as the daemon sent it, as JSON, and
/// its table in text unless the command already wrote it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Planned {
    plan: EnvPlan,
    #[serde(skip)]
    written: bool,
}

impl Planned {
    /// The table of `plan`, not written yet.
    #[must_use]
    pub const fn new(plan: EnvPlan) -> Self {
        Self {
            plan,
            written: false,
        }
    }
}

impl Render for Planned {
    fn render(&self, out: &mut HumanWriter) {
        if self.plan.validation.valid && !self.written {
            plan_table(out, &self.plan);
        }
    }
}

/// `env.plan` for `source`.
///
/// # Errors
///
/// The daemon's error, such as `env.manifest_unreadable` or
/// `env.not_found`.
pub async fn plan(cx: &mut Context, source: EnvSource) -> Result<EnvPlan, CliFailure> {
    let call = Call::EnvPlan(EnvPlanParams {
        source,
        locked: false,
    });
    match cx.call(call, CallSpec::control()).await? {
        Reply::EnvPlan(plan) => Ok(*plan),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// The file source of `file`, `./stackly.yaml` when it is left out.
///
/// # Errors
///
/// `cli.usage` when the current folder cannot be read.
pub fn file_source(file: Option<PathBuf>) -> Result<EnvSource, CliFailure> {
    let file = file.unwrap_or_else(|| PathBuf::from(DEFAULT_FILE));
    Ok(EnvSource::File(absolute(&file)?.display().to_string()))
}

impl RunCommand for PlanArgs {
    type Output = Planned;

    async fn run(self, cx: &mut Context) -> CommandOutput<Planned> {
        let source = match self.environment {
            Some(target) => match target.resolve(cx).await {
                Ok(summary) => EnvSource::Registered(summary.id),
                Err(failure) => return CommandOutput::failed(failure),
            },
            None => match file_source(self.file) {
                Ok(source) => source,
                Err(failure) => return CommandOutput::failed(failure),
            },
        };
        let plan = match plan(cx, source).await {
            Ok(plan) => plan,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let mut planned = Planned::new(plan);
        if shows_text(cx, false) {
            cx.output.write_result_now(&planned);
            planned.written = true;
            let plan = &planned.plan;
            to_stderr(cx, |out| {
                findings(out, &plan.validation);
                plan_sections(out, plan);
            });
        }
        let plan = &planned.plan;
        if !plan.validation.valid {
            let (errors, warnings) = counts(&plan.validation);
            return CommandOutput::partial(planned, CliFailure::env_invalid(errors, warnings));
        }
        if !plan.applicable {
            let blockers = plan.blockers.len() as u64;
            return CommandOutput::partial(planned, CliFailure::env_plan_blocked(blockers));
        }
        CommandOutput::ok(planned)
    }
}
