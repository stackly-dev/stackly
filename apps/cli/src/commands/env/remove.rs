//! `stackly env remove <environment> [--yes]` (spec 0044, *CLI*, AC-17):
//! resolves the name through `env.list`, asks with the `ConfirmArgs` rule
//! of spec 0008, then sends `env.remove` with the revision it showed, so
//! an environment that changed in between fails
//! `resource.revision_mismatch`. Instances and data are kept. With
//! `--yes` nothing goes to stderr: the result line on stdout already says
//! what is kept.

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{RemoveEnvironmentParams, RemovedEnvironment};
use stackly_protocol::{Call, Reply};

use super::target::EnvironmentTarget;
use crate::commands::RunCommand;
use crate::commands::provider::local::{confirm, esc, operation_call};
use crate::confirm::{ConfirmArgs, ConfirmRequest, ConfirmRole};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// What removing keeps, said with the question.
pub const REMOVE_CONSEQUENCE: &str = "Instances and data are kept.";

/// The question before `env.remove`.
#[must_use]
pub fn remove_question(name: &str) -> String {
    format!("Remove environment {name}?")
}

/// `stackly env remove`.
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// The environment name, or its whole ID
    #[arg(value_name = "ENVIRONMENT")]
    pub environment: EnvironmentTarget,
    /// `--yes`.
    #[command(flatten)]
    pub confirm: ConfirmArgs,
}

/// The `env remove` result: the daemon's answer.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Removed(pub RemovedEnvironment);

impl Render for Removed {
    fn render(&self, out: &mut HumanWriter) {
        out.line(&format!(
            "Removed environment {}. {REMOVE_CONSEQUENCE}",
            esc(&self.0.name)
        ));
    }
}

impl RunCommand for RemoveArgs {
    type Output = Removed;

    async fn run(self, cx: &mut Context) -> CommandOutput<Removed> {
        let summary = match self.environment.resolve(cx).await {
            Ok(summary) => summary,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let question = remove_question(&esc(&summary.name));
        let request = ConfirmRequest {
            role: ConfirmRole::Destructive,
            question: &question,
            consequence: REMOVE_CONSEQUENCE,
        };
        if let Err(failure) = confirm(cx, request, self.confirm.yes) {
            return CommandOutput::failed(failure);
        }
        let call = Call::EnvRemove(RemoveEnvironmentParams {
            id: summary.id,
            expected_revision: summary.revision,
        });
        match cx.call(call, operation_call()).await {
            Ok(Reply::EnvRemove(removed)) => CommandOutput::ok(Removed(removed)),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
