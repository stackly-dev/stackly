//! `stackly instance inspect` (spec 0022, AC-11, extended by spec 0021
//! AC-7): one instance by name or ID, with the detail the instance page
//! shows. `get` of spec 0014 runs the same code, so both write the same text
//! and the same `{ instance, detail, dataUsage }`.

use clap::Args;
use serde::Serialize;
use stackly_protocol::instance::{DataUsage, InstanceDetail, InstanceSummary};

use super::render::render_inspect;
use super::resolve::TargetArg;
use super::{read_detail, read_instance, read_usage};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// `stackly instance inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// Measure the data folder now instead of showing the last size
    #[arg(long)]
    pub size: bool,
}

/// What the data folder size line says when there is no size to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageNote {
    /// The size is in `data_usage`.
    Measured,
    /// Nothing was measured yet, and none was asked for.
    NotMeasured,
    /// The daemon could not measure the folder.
    Unavailable,
}

/// The `result` of `inspect` and `get`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInspect {
    /// The instance, as `instance.get` shows it.
    pub instance: InstanceSummary,
    /// The detail page facts, as `instance.get_detail` shows them.
    pub detail: InstanceDetail,
    /// The size of the data folder; null when none was measured or it could
    /// not be.
    pub data_usage: Option<DataUsage>,
    #[serde(skip)]
    usage_note: UsageNote,
}

impl InstanceInspect {
    /// What the `Data size` line says when `data_usage` is null.
    #[must_use]
    pub const fn usage_note(&self) -> UsageNote {
        self.usage_note
    }
}

impl RunCommand for InspectArgs {
    type Output = InstanceInspect;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceInspect> {
        inspect(cx, &self.instance, self.size).await
    }
}

/// Reads the instance `arg` names with `instance.get`, `instance.get_detail`,
/// and `instance.get_data_usage` (`refresh` with `size`, else the cache
/// only). When the detail is of another revision than the summary, both are
/// read once more. A data usage that cannot be read does not stop the
/// command.
pub async fn inspect(
    cx: &mut Context,
    arg: &TargetArg,
    size: bool,
) -> CommandOutput<InstanceInspect> {
    let id = match arg.target.resolve(cx).await {
        Ok(id) => id,
        Err(failure) => return CommandOutput::failed(failure),
    };
    match read_all(cx, id, size).await {
        Ok(inspect) => CommandOutput::ok(inspect),
        Err(failure) => CommandOutput::failed(failure),
    }
}

async fn read_all(
    cx: &mut Context,
    id: uuid::Uuid,
    size: bool,
) -> Result<InstanceInspect, CliFailure> {
    let mut instance = read_instance(cx, id).await?;
    let mut detail = read_detail(cx, id).await?;
    if detail.revision != instance.revision {
        instance = read_instance(cx, id).await?;
        detail = read_detail(cx, id).await?;
    }
    let (data_usage, usage_note) = match read_usage(cx, id, size).await {
        Ok(Some(usage)) => (Some(usage), UsageNote::Measured),
        Ok(None) => (None, UsageNote::NotMeasured),
        Err(_) => (None, UsageNote::Unavailable),
    };
    Ok(InstanceInspect {
        instance,
        detail,
        data_usage,
        usage_note,
    })
}

impl Render for InstanceInspect {
    fn render(&self, out: &mut HumanWriter) {
        render_inspect(out, self);
    }
}
