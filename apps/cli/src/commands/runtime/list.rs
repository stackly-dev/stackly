//! `stackly runtime list` (spec 0018, AC-16). It reads every page of
//! `runtime.list` and the installs that have not ended.

use clap::Args;
use serde::Serialize;
use stackly_protocol::runtime::{InstallingRuntime, RuntimeState, RuntimeSummary};

use super::all_runtimes;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::Tone;

/// The characters of the artifact digest that are shown.
const DIGEST_SHOWN: usize = 12;

/// `stackly runtime list`.
#[derive(Debug, Args)]
pub struct ListArgs {}

/// The `runtime list` result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeList {
    /// The runtimes, ordered by ID.
    pub items: Vec<RuntimeSummary>,
    /// The installs that have not ended.
    pub installing: Vec<InstallingRuntime>,
}

impl RunCommand for ListArgs {
    type Output = RuntimeList;

    async fn run(self, cx: &mut Context) -> CommandOutput<RuntimeList> {
        match all_runtimes(cx).await {
            Ok(page) => CommandOutput::ok(RuntimeList {
                items: page.items,
                installing: page.installing,
            }),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for RuntimeList {
    fn render(&self, out: &mut HumanWriter) {
        if self.items.is_empty() && self.installing.is_empty() {
            out.line("No runtimes.");
            return;
        }
        if !self.items.is_empty() {
            let rows: Vec<Vec<Cell>> = self.items.iter().map(row).collect();
            out.table(
                &[
                    "id", "software", "version", "target", "source", "state", "support", "digest",
                    "used by",
                ],
                &rows,
            );
        }
        if !self.installing.is_empty() {
            if !self.items.is_empty() {
                out.line("");
            }
            for install in &self.installing {
                out.line(&format!(
                    "installing {} {} (operation {})",
                    crate::output::escape::escape_display(&install.display_name),
                    install.version,
                    install.operation_id
                ));
            }
        }
    }
}

/// One row of the table.
fn row(runtime: &RuntimeSummary) -> Vec<Cell> {
    let digest = runtime
        .artifact_sha256
        .as_deref()
        .map_or_else(String::new, |sha| {
            sha.chars().take(DIGEST_SHOWN).collect::<String>()
        });
    let users: Vec<String> = runtime
        .used_by
        .iter()
        .map(|user| {
            if user.retained {
                format!("{} (retained)", user.name)
            } else {
                user.name.clone()
            }
        })
        .collect();
    vec![
        Cell::fixed(runtime.id.to_string()),
        Cell::text(&runtime.software),
        Cell::text(&runtime.version),
        Cell::fixed(format!(
            "{}-{}",
            wire_name(&runtime.target.os),
            wire_name(&runtime.target.arch)
        )),
        Cell::fixed(wire_name(&runtime.install_mode)),
        Cell::toned(&wire_name(&runtime.state), state_tone(runtime.state)),
        Cell::fixed(wire_name(&runtime.support)),
        Cell::fixed(digest),
        Cell::text(&users.join(", ")),
    ]
}

/// The color of a runtime state.
pub const fn state_tone(state: RuntimeState) -> Tone {
    match state {
        RuntimeState::Installed => Tone::Positive,
        RuntimeState::Failed | RuntimeState::Missing => Tone::Critical,
        RuntimeState::Drifted | RuntimeState::Unrecognized => Tone::Caution,
        RuntimeState::Resolving
        | RuntimeState::Downloading
        | RuntimeState::Verifying
        | RuntimeState::Installing => Tone::Informative,
        RuntimeState::Absent => Tone::Neutral,
    }
}
