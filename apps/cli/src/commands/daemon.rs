//! `stackly daemon status` (spec 0008, *`stackly daemon status`*), which
//! replaces the provisional one of spec 0006. Any health status exits 0.

use clap::{Args, Subcommand};
use serde::Serialize;
use stackly_protocol::daemon::{DaemonHealth, DaemonInfo, SubsystemStatus};
use stackly_protocol::{CONTRACT_DIGEST, Call, ProtocolVersion, Reply};

use super::{RunCommand, execute};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::warning;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::{Tone, health_tone, subsystem_tone};
use crate::session::CallSpec;

/// The recovery hint of a registry in recovery mode.
const RECOVERY_HINT: &str = "restore the registry from a snapshot or backup, then restart stacklyd";

/// The verbs of `stackly daemon`.
#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Show which daemon runs for you, its health, versions, and where it
    /// keeps things.
    #[command(after_help = "Examples:\n  stackly daemon status\n  stackly daemon status --json")]
    Status(StatusArgs),
}

impl DaemonCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Status(args) => execute(args, cx).await,
        }
    }
}

/// `stackly daemon status` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct StatusArgs {}

/// The `daemon status` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    /// `daemon.get_info`.
    pub info: DaemonInfo,
    /// `daemon.get_health`, null when it failed.
    pub health: Option<DaemonHealth>,
    /// This client's side of the contract.
    pub client: ClientContract,
}

/// This client's protocol and contract digest, and whether the daemon saw
/// drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientContract {
    /// [`ProtocolVersion::CURRENT`].
    pub protocol: ProtocolVersion,
    /// [`CONTRACT_DIGEST`].
    pub contract_digest: &'static str,
    /// The handshake's `contractDrift`.
    pub contract_drift: bool,
}

impl RunCommand for StatusArgs {
    type Output = DaemonStatus;

    async fn run(self, cx: &mut Context) -> CommandOutput<DaemonStatus> {
        let info = match cx.call(Call::DaemonGetInfo, CallSpec::control()).await {
            Ok(Reply::DaemonGetInfo(info)) => info,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let drift = match cx.handshake().await {
            Ok(accepted) => accepted.contract_drift,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if info.roots.root_override {
            cx.warn(
                warning::ROOT_OVERRIDE,
                format!(
                    "the STACKLY_ROOT override is active, with the data root at {}",
                    info.roots.data
                ),
            );
        }
        if drift {
            cx.warn(
                warning::CONTRACT_DRIFT,
                format!(
                    "the daemon has another contract digest of protocol {}.{}; update stackly or stacklyd so both match",
                    ProtocolVersion::CURRENT.major,
                    ProtocolVersion::CURRENT.minor
                ),
            );
        }
        let health = match cx.call(Call::DaemonGetHealth, CallSpec::control()).await {
            Ok(Reply::DaemonGetHealth(health)) => Ok(health),
            Ok(_) => Err(CliFailure::unexpected_reply(None)),
            Err(failure) => Err(failure),
        };
        let recovery = health
            .as_ref()
            .is_ok_and(|health| health.recovery.is_some());
        let status = DaemonStatus {
            info,
            health: health.as_ref().ok().cloned(),
            client: ClientContract {
                protocol: ProtocolVersion::CURRENT,
                contract_digest: CONTRACT_DIGEST,
                contract_drift: drift,
            },
        };
        let output = match health {
            Ok(_) => CommandOutput::ok(status),
            Err(failure) => CommandOutput::partial(status, failure),
        };
        if recovery {
            output.with_hint(RECOVERY_HINT)
        } else {
            output
        }
    }
}

impl Render for DaemonStatus {
    fn render(&self, out: &mut HumanWriter) {
        let info = &self.info;
        let status = self.health.as_ref().map_or_else(
            || Cell::toned("unknown", Tone::Caution),
            |health| Cell::toned(&wire_name(&health.status), health_tone(health.status)),
        );
        let mut rows = vec![
            ("Status", status),
            (
                "Version",
                Cell::text(&format!(
                    "{} (protocol {}.{})",
                    info.version, info.protocol.major, info.protocol.minor
                )),
            ),
            ("PID", Cell::fixed(info.pid.to_string())),
        ];
        if let Some(uid) = info.uid {
            rows.push(("UID", Cell::fixed(uid.to_string())));
        }
        rows.push(("Started", Cell::fixed(out.time_and_age(info.started_at))));
        rows.push((
            "Server ID",
            Cell::fixed(info.server_instance_id.to_string()),
        ));
        let subsystems = self
            .health
            .iter()
            .flat_map(|health| &health.subsystems)
            .filter(|subsystem| subsystem.status != SubsystemStatus::Ok);
        for (index, subsystem) in subsystems.enumerate() {
            let mut text = format!("{}: {}", subsystem.name, wire_name(&subsystem.status));
            if let Some(detail) = &subsystem.detail {
                text.push_str(&format!(" ({detail})"));
            }
            let label = if index == 0 { "Subsystems" } else { "" };
            rows.push((label, Cell::toned(&text, subsystem_tone(subsystem.status))));
        }
        rows.push(("Data", Cell::text(&info.roots.data)));
        rows.push(("Logs", Cell::text(&info.roots.logs)));
        rows.push(("Cache", Cell::text(&info.roots.cache)));
        rows.push(("Runtime", Cell::text(&info.roots.runtime)));
        rows.push(("Socket", Cell::text(&info.socket_path)));
        if let Some(recovery) = self
            .health
            .as_ref()
            .and_then(|health| health.recovery.as_ref())
        {
            rows.push(("Recovery", Cell::toned(&recovery.reason, Tone::Critical)));
            rows.push(("Report", Cell::text(&recovery.report_path)));
            let schema = recovery.db_schema_version.map_or_else(
                || "unreadable".to_owned(),
                |db| format!("{db} > {}", recovery.app_schema_version),
            );
            rows.push(("Schema", Cell::fixed(schema)));
        }
        rows.push((
            "Override",
            Cell::fixed(if info.roots.root_override {
                "yes (STACKLY_ROOT)"
            } else {
                "no"
            }),
        ));
        out.kv(&rows);
    }
}
