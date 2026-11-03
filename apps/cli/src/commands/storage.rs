//! `stackly storage locations` and `set-location` (spec 0032, AC-19): where
//! the data, the logs, and the downloads of what you create next go. A change
//! applies to new instances only and moves no data.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use stackly_protocol::codes::resource;
use stackly_protocol::settings::{
    LocationKind, LocationList, LocationTarget, SetLocationParams, StorageLocationView,
};
use stackly_protocol::{Call, ErrorBody, Reply};

use super::execute;
use crate::args::IdempotencyKeyArg;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render};
use crate::session::{CallSpec, RequestTimeout};

/// The verbs of `stackly storage`.
#[derive(Debug, Subcommand)]
pub enum StorageCommand {
    /// List the storage locations and which one is the default for each kind
    #[command(
        after_help = "Examples:\n  stackly storage locations\n  stackly storage locations --json"
    )]
    Locations(LocationsArgs),
    /// Choose the folder new instances, logs, or downloads go to
    #[command(
        after_help = "Examples:\n  stackly storage set-location data ~/Work\n  stackly storage set-location logs /Users/me/StacklyLogs\n  stackly storage set-location cache --default"
    )]
    SetLocation(SetLocationArgs),
}

impl StorageCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Locations(args) => execute(args, cx).await,
            Self::SetLocation(args) => execute(args, cx).await,
        }
    }
}

/// `stackly storage locations`.
#[derive(Debug, Args)]
pub struct LocationsArgs {}

/// The kinds of location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum KindArg {
    /// The data of new instances
    Data,
    /// The logs of new instances
    Logs,
    /// New downloads of runtimes
    Cache,
}

impl KindArg {
    const fn kind(self) -> LocationKind {
        match self {
            Self::Data => LocationKind::Data,
            Self::Logs => LocationKind::Logs,
            Self::Cache => LocationKind::Cache,
        }
    }
}

/// `stackly storage set-location`.
#[derive(Debug, Args)]
pub struct SetLocationArgs {
    /// What the folder holds
    #[arg(value_enum)]
    pub kind: KindArg,
    /// The folder; a relative path is made absolute from the current folder
    #[arg(
        value_name = "PATH",
        required_unless_present = "default",
        conflicts_with = "default"
    )]
    pub path: Option<PathBuf>,
    /// Go back to the location that comes with Stackly
    #[arg(long = "default")]
    pub default: bool,
    /// Send this key so a repeated request gets the same answer; by default a new one is made
    #[arg(long, value_name = "KEY")]
    pub idempotency_key: Option<IdempotencyKeyArg>,
}

/// The `result` of `locations`.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct LocationsResult {
    list: LocationList,
}

impl RunCommand for LocationsArgs {
    type Output = LocationsResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<LocationsResult> {
        match cx
            .call(Call::StorageListLocations, CallSpec::control())
            .await
        {
            Ok(Reply::StorageListLocations(list)) => CommandOutput::ok(LocationsResult { list }),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

fn kind_text(kind: LocationKind) -> &'static str {
    match kind {
        LocationKind::Data => "data",
        LocationKind::Logs => "logs",
        LocationKind::Cache => "cache",
        LocationKind::Unrecognized => "unknown",
    }
}

impl Render for LocationsResult {
    fn render(&self, out: &mut HumanWriter) {
        let rows: Vec<Vec<Cell>> = self
            .list
            .items
            .iter()
            .map(|item| {
                vec![
                    Cell::fixed(kind_text(item.kind)),
                    Cell::text(&item.path),
                    Cell::fixed(if item.is_default { "default" } else { "" }),
                    Cell::fixed(if item.kind == LocationKind::Cache {
                        String::new()
                    } else {
                        item.instance_count.get().to_string()
                    }),
                    Cell::fixed(if item.reachable { "yes" } else { "no" }),
                ]
            })
            .collect();
        out.table(&["KIND", "PATH", "USE", "INSTANCES", "REACHABLE"], &rows);
    }
}

/// The `result` of `set-location`: the default location of the kind.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SetLocationResult {
    location: StorageLocationView,
}

/// A path made absolute from the current folder, without touching the
/// filesystem.
fn absolute(path: PathBuf) -> Result<PathBuf, CliFailure> {
    if path.is_absolute() {
        return Ok(path);
    }
    let cwd = std::env::current_dir().map_err(|error| {
        CliFailure::usage(
            "invalid_value",
            &format!("cannot read the current folder: {error}"),
        )
    })?;
    Ok(cwd.join(path))
}

/// The failure of a revision that went stale between the read and the
/// write.
fn changed_while_saving(failure: CliFailure) -> CliFailure {
    if failure.body.code() != resource::REVISION_MISMATCH.code {
        return failure;
    }
    let mut body = ErrorBody::new(
        &resource::REVISION_MISMATCH,
        "Settings changed while saving. Run the command again.",
    );
    for detail in failure.body.details() {
        body = body.with_detail(detail.key(), detail.value());
    }
    CliFailure::daemon(body, failure.request_id)
}

impl RunCommand for SetLocationArgs {
    type Output = SetLocationResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<SetLocationResult> {
        let target = match (self.default, self.path) {
            (true, _) => LocationTarget::BuiltIn,
            (false, Some(path)) => match absolute(path) {
                Ok(path) => LocationTarget::Path(path.display().to_string()),
                Err(failure) => return CommandOutput::failed(failure),
            },
            (false, None) => {
                return CommandOutput::failed(CliFailure::usage(
                    "missing_argument",
                    "give a folder, or --default",
                ));
            }
        };
        let revision = match cx.call(Call::SettingsGet, CallSpec::control()).await {
            Ok(Reply::SettingsGet(view)) => view.revision,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let key = self.idempotency_key.as_ref().map_or_else(
            || uuid::Uuid::now_v7().to_string(),
            |key| key.as_str().to_owned(),
        );
        let call = Call::StorageSetLocation(SetLocationParams {
            kind: self.kind.kind(),
            target,
            expected_revision: revision,
        });
        let spec = CallSpec {
            timeout: RequestTimeout::Interactive,
            idempotency_key: Some(key),
        };
        match cx.call(call, spec).await {
            Ok(Reply::StorageSetLocation(location)) => CommandOutput::ok(SetLocationResult {
                location: *location,
            }),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(changed_while_saving(failure)),
        }
    }
}

impl Render for SetLocationResult {
    fn render(&self, out: &mut HumanWriter) {
        let location = &self.location;
        let what = kind_text(location.kind);
        out.line(&format!("New {what} goes to:"));
        out.kv(&[("", Cell::text(&location.path))]);
        out.line("Only what you create from now on uses it. Existing data stays where it is.");
    }
}
