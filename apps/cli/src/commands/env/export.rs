//! `stackly env export [--file <path> | <environment>] [--format
//! dotenv|shell]` (spec 0045, *CLI*, AC-14, AC-15, AC-17): asks the
//! daemon's read only `env.export` and prints exactly its `text` to
//! stdout, the same text the GUI copies, then one `skipped:` line per
//! service or endpoint left out to stderr. The daemon builds every value
//! and never sends a secret, a path, or a PID. Exit 0, also when every
//! service is skipped; an environment that is not registered exits 10.

use std::path::PathBuf;

use clap::{Args, ValueEnum};
use serde::Serialize;
use stackly_protocol::env::{EnvExport, EnvExportParams, EnvSource, ExportFormat};
use stackly_protocol::{Call, Reply};

use super::plan::file_source;
use super::target::EnvironmentTarget;
use crate::commands::RunCommand;
use crate::commands::provider::local::{esc, shows_text, to_stderr};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render, wire_name};
use crate::session::CallSpec;

/// How `env export` writes the variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum FormatArg {
    /// `NAME=value` per line
    #[default]
    Dotenv,
    /// `export NAME='value'` per line
    Shell,
}

impl From<FormatArg> for ExportFormat {
    fn from(format: FormatArg) -> Self {
        match format {
            FormatArg::Dotenv => Self::Dotenv,
            FormatArg::Shell => Self::Shell,
        }
    }
}

/// `stackly env export`.
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// The manifest of a registered environment; a relative path starts at
    /// the current folder [default: ./stackly.yaml]
    #[arg(long, value_name = "PATH", conflicts_with = "environment")]
    pub file: Option<PathBuf>,
    /// A registered environment, by name or whole ID
    #[arg(value_name = "ENVIRONMENT")]
    pub environment: Option<EnvironmentTarget>,
    /// How each variable is written
    #[arg(long, value_enum, default_value_t = FormatArg::Dotenv)]
    pub format: FormatArg,
}

/// The `env export` result: the daemon's export as JSON, and exactly its
/// text in text mode.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Exported {
    export: EnvExport,
    #[serde(skip)]
    written: bool,
}

impl Render for Exported {
    fn render(&self, out: &mut HumanWriter) {
        if self.written {
            return;
        }
        for line in self.export.text.lines() {
            out.line(line);
        }
    }
}

/// `skipped: <service>[.<endpoint>]: <reason>` for each entry left out.
fn skipped_lines(out: &mut HumanWriter, export: &EnvExport) {
    for skipped in &export.skipped {
        let endpoint = skipped
            .endpoint
            .as_deref()
            .map(|endpoint| format!(".{}", esc(endpoint)))
            .unwrap_or_default();
        out.line(&format!(
            "skipped: {}{endpoint}: {}",
            esc(&skipped.service),
            wire_name(&skipped.reason)
        ));
    }
}

impl RunCommand for ExportArgs {
    type Output = Exported;

    async fn run(self, cx: &mut Context) -> CommandOutput<Exported> {
        let source = match self.environment {
            Some(target) => match target.resolve(cx).await {
                Ok(summary) => EnvSource::Registered(summary.id),
                Err(failure) => return CommandOutput::failed(failure),
            },
            None => match file_source(self.file) {
                Ok(source) => source,
                Err(failure) => return CommandOutput::failed(failure),
            },
        };
        let call = Call::EnvExport(EnvExportParams {
            source,
            format: self.format.into(),
        });
        let export = match cx.call(call, CallSpec::control()).await {
            Ok(Reply::EnvExport(export)) => *export,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let mut exported = Exported {
            export,
            written: false,
        };
        if shows_text(cx, false) {
            cx.output.write_result_now(&exported);
            exported.written = true;
            let export = &exported.export;
            to_stderr(cx, |out| skipped_lines(out, export));
        }
        CommandOutput::ok(exported)
    }
}
