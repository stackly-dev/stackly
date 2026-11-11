//! `stackly provider approve <provider> [--yes]` (spec 0037, *CLI*,
//! AC-14, AC-11): reads the entry with `catalog.get_provider`, prints its
//! identity, permissions, and the approval statement exactly, asks
//! `Approve <provider>?`, then sends `provider.approve` bound to the
//! `packageSha256` of the detail it showed. Without a terminal, or with
//! `--json`, `--yes` is needed, and no mutation is sent without it.

use clap::Args;
use stackly_protocol::catalog::ProviderDetail;
use stackly_protocol::provider::{ApprovalState, DecideProviderParams};
use stackly_protocol::{Call, Reply};

use super::copy;
use super::local::{
    confirm, esc, get_detail, local_of, operation_call, permission_lines, shows_text, to_stderr,
    will_ask,
};
use super::outcome::Outcome;
use crate::commands::RunCommand;
use crate::confirm::{ConfirmArgs, ConfirmRequest, ConfirmRole};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;

/// `stackly provider approve`.
#[derive(Debug, Args)]
pub struct ApproveArgs {
    /// The imported provider ID with its namespace, such as acme/demo
    #[arg(value_name = "PROVIDER")]
    pub provider: String,
    /// `--yes`.
    #[command(flatten)]
    pub confirm: ConfirmArgs,
}

impl RunCommand for ApproveArgs {
    type Output = Outcome;

    async fn run(self, cx: &mut Context) -> CommandOutput<Outcome> {
        let detail = match get_detail(cx, &self.provider).await {
            Ok(detail) => detail,
            Err(failure) => return CommandOutput::failed(failure),
        };
        approve_detail(cx, detail, self.confirm.yes, None).await
    }
}

/// Shows `detail`, confirms, and approves exactly its stored bytes. An
/// entry already approved for those bytes is answered without a request.
/// `imported` is the line of an import that came first: a declined
/// approval then keeps the imported entry as the result.
pub async fn approve_detail(
    cx: &mut Context,
    detail: ProviderDetail,
    yes: bool,
    imported: Option<String>,
) -> CommandOutput<Outcome> {
    let local = match local_of(&detail) {
        Ok(local) => local.clone(),
        Err(failure) => return CommandOutput::failed(failure),
    };
    let name = esc(&detail.summary.display_name);
    if detail.summary.approval == Some(ApprovalState::Approved) {
        return CommandOutput::ok(Outcome::new(format!("{name} is already approved."), detail));
    }
    let asking = will_ask(cx, yes);
    if shows_text(cx, asking) {
        to_stderr(cx, |out| {
            out.line(&copy::identity_line(
                &esc(&detail.summary.provider),
                &esc(detail.summary.provider_revision.as_deref().unwrap_or("-")),
                &esc(detail.summary.publisher.as_deref().unwrap_or("-")),
            ));
            permission_lines(out, local.permissions.as_ref());
            if !local.changes.is_empty() {
                out.line(&copy::differs(&local.changes));
            }
            out.line("");
            out.line(copy::APPROVAL_STATEMENT);
            out.line("");
            out.line(copy::APPROVE_FOOTNOTE);
        });
    }
    let question = copy::approve_question(&esc(&detail.summary.provider));
    let request = ConfirmRequest {
        role: ConfirmRole::Standard,
        question: &question,
        consequence: "",
    };
    if let Err(failure) = confirm(cx, request, yes) {
        return match imported {
            Some(line) => CommandOutput::partial(Outcome::new(line, detail), failure),
            None => CommandOutput::failed(failure),
        };
    }
    let call = Call::ProviderApprove(DecideProviderParams {
        provider: detail.summary.provider.clone(),
        expected_package_sha256: local.package_sha256,
    });
    match cx.call(call, operation_call()).await {
        Ok(Reply::ProviderApprove(detail)) => {
            CommandOutput::ok(Outcome::new(format!("Approved {name}."), *detail))
        }
        Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
        // An import that came first is committed: a refused approval keeps
        // it as the result, as a declined confirmation does.
        Err(failure) => match imported {
            Some(line) => CommandOutput::partial(Outcome::new(line, detail), failure),
            None => CommandOutput::failed(failure),
        },
    }
}
