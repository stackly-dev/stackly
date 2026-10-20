//! `stackly instance get` (spec 0014, AC-7). Its output is the `inspect`
//! of spec 0022, so `get` can become its alias there.

use clap::Args;
use serde::Serialize;
use stackly_protocol::instance::InstanceSummary;

use super::render::render_instance;
use super::{InstanceArg, read_instance};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// `stackly instance get`.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: InstanceArg,
}

/// The `result` of `get`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGet {
    /// The instance, as `instance.get` shows it.
    pub instance: InstanceSummary,
}

impl RunCommand for GetArgs {
    type Output = InstanceGet;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceGet> {
        match read_instance(cx, self.instance.id.get()).await {
            Ok(instance) => CommandOutput::ok(InstanceGet { instance }),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for InstanceGet {
    fn render(&self, out: &mut HumanWriter) {
        render_instance(out, &self.instance);
    }
}
