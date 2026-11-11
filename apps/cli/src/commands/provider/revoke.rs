//! `stackly provider revoke <provider> [--yes]` (spec 0037, *CLI*,
//! AC-14): reads the entry, prints what revoking does, asks, then sends
//! `provider.revoke`. Revoking only closes new installs and instances;
//! the daemon refuses an entry without any approval with
//! `provider.not_approved`.

use clap::Args;
use stackly_protocol::provider::ProviderTargetParams;
use stackly_protocol::{Call, Reply};

use super::copy;
use super::local::{confirm_action, esc, get_detail, local_of, operation_call};
use super::outcome::Outcome;
use crate::commands::RunCommand;
use crate::confirm::ConfirmArgs;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;

/// `stackly provider revoke`.
#[derive(Debug, Args)]
pub struct RevokeArgs {
    /// The imported provider ID with its namespace, such as acme/demo
    #[arg(value_name = "PROVIDER")]
    pub provider: String,
    /// `--yes`.
    #[command(flatten)]
    pub confirm: ConfirmArgs,
}

impl RunCommand for RevokeArgs {
    type Output = Outcome;

    async fn run(self, cx: &mut Context) -> CommandOutput<Outcome> {
        let detail = match get_detail(cx, &self.provider).await {
            Ok(detail) => detail,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if let Err(failure) = local_of(&detail) {
            return CommandOutput::failed(failure);
        }
        let name = esc(&detail.summary.display_name);
        let question = copy::revoke_question(&name);
        if let Err(failure) =
            confirm_action(cx, &question, copy::REVOKE_CONSEQUENCE, self.confirm.yes)
        {
            return CommandOutput::failed(failure);
        }
        let call = Call::ProviderRevoke(ProviderTargetParams {
            provider: detail.summary.provider,
        });
        match cx.call(call, operation_call()).await {
            Ok(Reply::ProviderRevoke(detail)) => CommandOutput::ok(Outcome::new(
                format!("Revoked the approval for {name}. Existing instances can still start."),
                *detail,
            )),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
