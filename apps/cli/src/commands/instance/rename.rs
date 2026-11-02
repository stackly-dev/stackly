//! `stackly instance rename` (spec 0029, AC-13, AC-17): changes the name and
//! nothing else, on any lifecycle but `initializing`.

use clap::Args;
use stackly_protocol::Call;
use stackly_protocol::Reply;
use stackly_protocol::instance::config::RenameInstanceParams;

use super::resolve::TargetArg;
use super::{InstanceAction, Verb, accepted, conclude};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::wait::{Accepted, AsyncArgs};

/// `stackly instance rename`.
#[derive(Debug, Args)]
pub struct RenameArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// The new name: lowercase letters, digits, and single hyphens, starting with a letter
    #[arg(value_name = "NEW_NAME")]
    pub new_name: String,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

fn extract(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceRename(reply) => Some(accepted(&reply)),
        _ => None,
    }
}

impl RunCommand for RenameArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::InstanceRename(RenameInstanceParams {
            instance_id: id,
            name: self.new_name,
            expected_revision: None,
        });
        match cx.submit(call, &self.run, extract).await {
            Ok(submission) => conclude(cx, submission, Verb::Rename, Some(id)).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
