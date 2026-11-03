//! `stackly settings get` and `set` (spec 0032, AC-16, AC-17): the settings
//! the daemon keeps for every client, so the GUI and the command line see the
//! same values.
//!
//! `set` reads the revision first and sends the change with it. It does not
//! try again when the revision went stale: it says so and exits 3.

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use stackly_protocol::codes::resource;
use stackly_protocol::instance::StartupPolicyKind;
use stackly_protocol::settings::{
    DaemonSettingsView, SettingsPatch, StorageLocationView, UpdateChannel, UpdateSettingsParams,
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

/// The verbs of `stackly settings`.
#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// Show the daemon settings and where new instances go
    #[command(after_help = "Examples:\n  stackly settings get\n  stackly settings get --json")]
    Get(GetArgs),
    /// Change one daemon setting
    #[command(
        after_help = "Examples:\n  stackly settings set default-autostart on\n  stackly settings set update-channel beta\n  stackly settings set telemetry off"
    )]
    Set(SetArgs),
}

impl SettingsCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Get(args) => execute(args, cx).await,
            Self::Set(args) => execute(args, cx).await,
        }
    }
}

/// `stackly settings get`.
#[derive(Debug, Args)]
pub struct GetArgs {}

/// The settings `set` changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SettingKey {
    /// Whether instances you create start at login unless they say otherwise (on or off)
    DefaultAutostart,
    /// Which update channel to follow (stable or beta)
    UpdateChannel,
    /// Whether you agree to share usage statistics (on or off); nothing is sent today
    Telemetry,
}

/// `stackly settings set`.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// The setting
    #[arg(value_enum)]
    pub key: SettingKey,
    /// The value: on or off, or stable or beta
    #[arg(value_name = "VALUE")]
    pub value: String,
    /// Send this key so a repeated request gets the same answer; by default a new one is made
    #[arg(long, value_name = "KEY")]
    pub idempotency_key: Option<IdempotencyKeyArg>,
}

/// The `result` of `get` and `set`: the settings as the daemon answers.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SettingsResult {
    settings: DaemonSettingsView,
}

async fn read_settings(cx: &mut Context) -> Result<DaemonSettingsView, CliFailure> {
    match cx.call(Call::SettingsGet, CallSpec::control()).await? {
        Reply::SettingsGet(view) => Ok(*view),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

impl RunCommand for GetArgs {
    type Output = SettingsResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<SettingsResult> {
        match read_settings(cx).await {
            Ok(settings) => CommandOutput::ok(SettingsResult { settings }),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

/// The patch a key and a value stand for, or the usage failure.
fn patch_of(key: SettingKey, value: &str) -> Result<SettingsPatch, CliFailure> {
    let switch = || match value {
        "on" => Ok(true),
        "off" => Ok(false),
        other => Err(CliFailure::usage(
            "invalid_value",
            &format!("“{other}” is not on or off"),
        )),
    };
    Ok(match key {
        SettingKey::DefaultAutostart => SettingsPatch {
            default_startup: Some(if switch()? {
                StartupPolicyKind::AtLogin
            } else {
                StartupPolicyKind::Manual
            }),
            ..SettingsPatch::default()
        },
        SettingKey::Telemetry => SettingsPatch {
            telemetry: Some(switch()?),
            ..SettingsPatch::default()
        },
        SettingKey::UpdateChannel => SettingsPatch {
            update_channel: Some(match value {
                "stable" => UpdateChannel::Stable,
                "beta" => UpdateChannel::Beta,
                other => {
                    return Err(CliFailure::usage(
                        "invalid_value",
                        &format!("“{other}” is not stable or beta"),
                    ));
                }
            }),
            ..SettingsPatch::default()
        },
    })
}

/// The failure of a revision that went stale between the read and the
/// write: the same code, in the words of the spec.
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

impl RunCommand for SetArgs {
    type Output = SettingsResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<SettingsResult> {
        let patch = match patch_of(self.key, &self.value) {
            Ok(patch) => patch,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let current = match read_settings(cx).await {
            Ok(settings) => settings,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let key = self.idempotency_key.as_ref().map_or_else(
            || uuid::Uuid::now_v7().to_string(),
            |key| key.as_str().to_owned(),
        );
        let call = Call::SettingsUpdate(UpdateSettingsParams {
            expected_revision: current.revision,
            patch,
        });
        let spec = CallSpec {
            timeout: RequestTimeout::Interactive,
            idempotency_key: Some(key),
        };
        match cx.call(call, spec).await {
            Ok(Reply::SettingsUpdate(settings)) => CommandOutput::ok(SettingsResult {
                settings: *settings,
            }),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(changed_while_saving(failure)),
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn mebibytes(bytes: u64) -> String {
    format!("{} MiB", bytes / (1024 * 1024))
}

fn location_text(location: &StorageLocationView) -> String {
    let mut text = location.path.clone();
    if location.built_in {
        text.push_str(" (built in)");
    }
    if !location.reachable {
        text.push_str(" (not reachable)");
    }
    text
}

impl Render for SettingsResult {
    fn render(&self, out: &mut HumanWriter) {
        let settings = &self.settings;
        let autostart = on_off(settings.default_startup == StartupPolicyKind::AtLogin);
        let channel = match settings.update_channel {
            UpdateChannel::Stable => "stable",
            UpdateChannel::Beta => "beta",
            UpdateChannel::Unrecognized => "unknown",
        };
        let retention = format!(
            "{} files × {} per stream",
            settings.log_retention.segments_per_stream.get(),
            mebibytes(settings.log_retention.segment_max_bytes.get())
        );
        out.kv(&[
            ("default-autostart", Cell::fixed(autostart)),
            ("update-channel", Cell::fixed(channel)),
            ("telemetry", Cell::fixed(on_off(settings.telemetry))),
            (
                "data location",
                Cell::text(&location_text(&settings.locations.data)),
            ),
            (
                "logs location",
                Cell::text(&location_text(&settings.locations.logs)),
            ),
            (
                "cache location",
                Cell::text(&location_text(&settings.locations.cache)),
            ),
            ("log retention", Cell::fixed(retention)),
            ("revision", Cell::fixed(settings.revision.get().to_string())),
        ]);
    }
}
