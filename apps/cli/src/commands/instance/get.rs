//! `stackly instance get` (spec 0014, AC-7), the alias of `inspect` that
//! spec 0022 makes of it: same arguments, same text, same `{ instance }`.

use clap::Args;

use super::inspect::{InstanceInspect, inspect};
use super::resolve::TargetArg;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;

/// `stackly instance get`.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
}

impl RunCommand for GetArgs {
    type Output = InstanceInspect;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceInspect> {
        inspect(cx, &self.instance).await
    }
}
