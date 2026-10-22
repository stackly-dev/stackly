//! `stackly instance inspect` (spec 0022, AC-11): one instance by name or
//! ID. `get` of spec 0014 runs the same code, so both write the same text
//! and the same `{ instance }`.

use clap::Args;
use serde::Serialize;
use stackly_protocol::instance::InstanceSummary;

use super::read_instance;
use super::render::render_instance;
use super::resolve::TargetArg;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// `stackly instance inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
}

/// The `result` of `inspect` and `get`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInspect {
    /// The instance, as `instance.get` shows it.
    pub instance: InstanceSummary,
}

impl RunCommand for InspectArgs {
    type Output = InstanceInspect;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceInspect> {
        inspect(cx, &self.instance).await
    }
}

/// Reads the instance `arg` names with `instance.get`.
pub async fn inspect(cx: &mut Context, arg: &TargetArg) -> CommandOutput<InstanceInspect> {
    let id = match arg.target.resolve(cx).await {
        Ok(id) => id,
        Err(failure) => return CommandOutput::failed(failure),
    };
    match read_instance(cx, id).await {
        Ok(instance) => CommandOutput::ok(InstanceInspect { instance }),
        Err(failure) => CommandOutput::failed(failure),
    }
}

impl Render for InstanceInspect {
    fn render(&self, out: &mut HumanWriter) {
        render_instance(out, &self.instance);
    }
}
