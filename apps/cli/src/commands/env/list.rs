//! `stackly env list` (spec 0044, *CLI*, AC-17): one row per registered
//! environment from `env.list`, with its status, how many services are
//! ready, and its manifest with the home folder as `~`.

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{EnvironmentList, EnvironmentStatus};

use super::target::list;
use crate::commands::RunCommand;
use crate::commands::path::home_masked;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::Tone;

/// `stackly env list`.
#[derive(Debug, Args)]
pub struct ListArgs {}

/// The `env list` result: `{ items }` as the daemon sent it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Listed(pub EnvironmentList);

impl Render for Listed {
    fn render(&self, out: &mut HumanWriter) {
        if self.0.items.is_empty() {
            out.line("No environments.");
            return;
        }
        let home = std::env::home_dir();
        let rows: Vec<Vec<Cell>> = self
            .0
            .items
            .iter()
            .map(|item| {
                let file = item
                    .manifest_location
                    .as_deref()
                    .map(|file| home_masked(file, home.as_deref()));
                vec![
                    Cell::text(&item.name),
                    Cell::toned(&wire_name(&item.status), status_tone(item.status)),
                    Cell::fixed(format!(
                        "{}/{}",
                        u64::from(item.ready_count),
                        u64::from(item.service_count)
                    )),
                    Cell::optional(file.as_deref()),
                ]
            })
            .collect();
        out.table(&["name", "status", "ready", "file"], &rows);
    }
}

/// The color of an environment status.
const fn status_tone(status: EnvironmentStatus) -> Tone {
    match status {
        EnvironmentStatus::Ready => Tone::Positive,
        EnvironmentStatus::PartiallyReady => Tone::Caution,
        EnvironmentStatus::Down | EnvironmentStatus::Unrecognized => Tone::Neutral,
    }
}

impl RunCommand for ListArgs {
    type Output = Listed;

    async fn run(self, cx: &mut Context) -> CommandOutput<Listed> {
        match list(cx).await {
            Ok(list) => CommandOutput::ok(Listed(list)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
