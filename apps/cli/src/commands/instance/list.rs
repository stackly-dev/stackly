//! `stackly instance list` (spec 0022, AC-10). It reads every page of
//! `instance.list`, so the counts and the `--status` filter cover all
//! instances, unlike the paged `operation list`.

use clap::Args;
use serde::Serialize;
use stackly_protocol::instance::{InstanceHealthState, InstanceReadinessState, InstanceSummary};

use super::group::{self, InstanceCounts, InstanceGroup, StatusFilter};
use super::render::endpoint_text;
use super::resolve::all_instances;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::Tone;

/// The reason that says an instance has no process to observe.
const NOT_RUNNING: &str = "not_running";

/// `stackly instance list`.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only running (ready or not ready), stopped, or failed instances;
    /// the counts still cover all of them
    #[arg(long, value_enum, value_name = "STATUS")]
    pub status: Option<StatusFilter>,
}

/// The `instance list` result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceList {
    /// The instances that pass `--status`, ordered by name.
    pub items: Vec<InstanceSummary>,
    /// The groups of all instances, whatever `--status` says.
    pub counts: InstanceCounts,
    #[serde(skip)]
    status: Option<StatusFilter>,
}

impl RunCommand for ListArgs {
    type Output = InstanceList;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceList> {
        let all = match all_instances(cx).await {
            Ok(all) => all,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let counts = InstanceCounts::of(&all);
        let items = all
            .into_iter()
            .filter(|instance| {
                group::of(instance)
                    .is_some_and(|group| self.status.is_none_or(|status| status.matches(group)))
            })
            .collect();
        CommandOutput::ok(InstanceList {
            items,
            counts,
            status: self.status,
        })
    }
}

impl Render for InstanceList {
    fn render(&self, out: &mut HumanWriter) {
        if self.counts.total == 0 {
            out.line("No instances.");
            return;
        }
        if self.items.is_empty() {
            if let Some(status) = self.status {
                out.line(&format!("No {} instances.", status.word()));
            }
        } else {
            let rows: Vec<Vec<Cell>> = self.items.iter().map(row).collect();
            out.table(
                &[
                    "name",
                    "provider",
                    "version",
                    "endpoints",
                    "lifecycle",
                    "readiness",
                    "health",
                    "ownership",
                ],
                &rows,
            );
            out.line("");
        }
        out.line(&self.counts.summary());
    }
}

/// One row of the table.
fn row(instance: &InstanceSummary) -> Vec<Cell> {
    let endpoints: Vec<String> = instance.endpoints.iter().map(endpoint_text).collect();
    let group = group::of(instance);
    vec![
        Cell::text(&instance.name),
        Cell::text(&instance.provider.name),
        Cell::text(&instance.runtime_version),
        Cell::text(&endpoints.join(", ")),
        Cell::toned(&wire_name(&instance.lifecycle), group_tone(group)),
        observed(
            &wire_name(&instance.readiness.state),
            &instance.readiness.reason,
            readiness_tone(instance.readiness.state),
        ),
        observed(
            &wire_name(&instance.health.state),
            &instance.health.reason,
            health_tone(instance.health.state),
        ),
        Cell::text(&wire_name(&instance.ownership)),
    ]
}

/// `<state>` alone for a good state, `<state> (<reason>)` for the rest, and
/// `-` when there is no process to observe.
fn observed(state: &str, reason: &str, tone: Tone) -> Cell {
    if reason == NOT_RUNNING {
        return Cell::fixed("");
    }
    if tone == Tone::Positive || reason.is_empty() {
        return Cell::toned(state, tone);
    }
    Cell::toned(&format!("{state} ({reason})"), tone)
}

/// The color of a lifecycle, by group.
const fn group_tone(group: Option<InstanceGroup>) -> Tone {
    match group {
        Some(InstanceGroup::Ready) => Tone::Positive,
        Some(InstanceGroup::NotReady | InstanceGroup::Unknown) => Tone::Caution,
        Some(InstanceGroup::Failed) => Tone::Critical,
        Some(InstanceGroup::Stopped) | None => Tone::Neutral,
    }
}

/// The color of a readiness state.
const fn readiness_tone(state: InstanceReadinessState) -> Tone {
    match state {
        InstanceReadinessState::Ready => Tone::Positive,
        InstanceReadinessState::NotReady | InstanceReadinessState::Unknown => Tone::Caution,
    }
}

/// The color of a health state.
const fn health_tone(state: InstanceHealthState) -> Tone {
    match state {
        InstanceHealthState::Healthy => Tone::Positive,
        InstanceHealthState::Degraded | InstanceHealthState::Unknown => Tone::Caution,
        InstanceHealthState::Unhealthy => Tone::Critical,
    }
}
