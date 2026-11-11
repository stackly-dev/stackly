//! `stackly provider remove <provider> [--yes]` (spec 0037, *CLI*,
//! AC-14, AC-9): reads the entry, prints what removing does, asks, then
//! sends `provider.remove`. The result is the daemon's `RemovedProvider`.
//! An invalid local entry can be removed too.

use clap::Args;
use serde::Serialize;
use stackly_protocol::provider::{ProviderTargetParams, RemovedProvider};
use stackly_protocol::{Call, Reply};

use super::copy;
use super::local::{confirm_action, esc, get_detail, local_of, operation_call};
use crate::commands::RunCommand;
use crate::confirm::ConfirmArgs;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// `stackly provider remove`.
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// The imported provider ID with its namespace, such as acme/demo
    #[arg(value_name = "PROVIDER")]
    pub provider: String,
    /// `--yes`.
    #[command(flatten)]
    pub confirm: ConfirmArgs,
}

/// The `provider remove` result: the daemon's answer.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Removed(pub RemovedProvider);

impl Render for Removed {
    fn render(&self, out: &mut HumanWriter) {
        out.line(&format!(
            "Removed {} from the catalog. Existing instances can still start.",
            esc(&self.0.provider)
        ));
    }
}

impl RunCommand for RemoveArgs {
    type Output = Removed;

    async fn run(self, cx: &mut Context) -> CommandOutput<Removed> {
        let detail = match get_detail(cx, &self.provider).await {
            Ok(detail) => detail,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if let Err(failure) = local_of(&detail) {
            return CommandOutput::failed(failure);
        }
        let question = copy::remove_question(&esc(&detail.summary.display_name));
        if let Err(failure) =
            confirm_action(cx, &question, copy::REMOVE_CONSEQUENCE, self.confirm.yes)
        {
            return CommandOutput::failed(failure);
        }
        let call = Call::ProviderRemove(ProviderTargetParams {
            provider: detail.summary.provider,
        });
        match cx.call(call, operation_call()).await {
            Ok(Reply::ProviderRemove(removed)) => CommandOutput::ok(Removed(removed)),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
