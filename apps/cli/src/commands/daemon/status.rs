//! `stackly daemon status` (spec 0008, *`stackly daemon status`*, and spec
//! 0015, AC-7): which daemon runs for you, its health, versions, and where
//! it keeps things, plus the background agent block that shows even when no
//! daemon answers. Any health status exits 0.

use clap::Args;
use serde::Serialize;
use stackly_platform::BackgroundAgentState;
use stackly_protocol::daemon::{
    DaemonHealth, DaemonInfo, DaemonLaunch, LaunchContextKind, SubsystemStatus,
};
use stackly_protocol::{CONTRACT_DIGEST, Call, ProtocolVersion, Reply};

use super::agent::{AgentReport, recovery_text};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::warning;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::{Tone, health_tone, subsystem_tone};
use crate::session::CallSpec;

/// The recovery hint of a registry in recovery mode.
const RECOVERY_HINT: &str = "restore the registry from a snapshot or backup, then restart stacklyd";

/// `stackly daemon status` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct StatusArgs {}

/// The `daemon status` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    /// `daemon.get_info`, null when no daemon answered.
    pub info: Option<DaemonInfo>,
    /// `daemon.get_health`, null when it failed.
    pub health: Option<DaemonHealth>,
    /// This client's side of the contract, null when no daemon answered.
    pub client: Option<ClientContract>,
    /// The background agent, always read.
    pub agent: AgentReport,
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
        let agent_env = cx.agent.clone();
        let (_, assessment) = agent_env.status().await;
        let without_daemon = |up: bool| agent_env.report(&assessment, up);
        let info = match cx.call(Call::DaemonGetInfo, CallSpec::control()).await {
            Ok(Reply::DaemonGetInfo(info)) => info,
            Ok(_) => {
                return no_daemon(without_daemon(false), CliFailure::unexpected_reply(None));
            }
            Err(failure) => return no_daemon(without_daemon(false), failure),
        };
        let drift = match cx.handshake().await {
            Ok(accepted) => accepted.contract_drift,
            Err(failure) => return no_daemon(without_daemon(false), failure),
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
        let agent = without_daemon(true);
        if let (Some(daemon_bundle), Some(own_bundle)) = (
            info.launch
                .as_ref()
                .and_then(|launch| launch.bundle_path.as_deref()),
            agent.bundle_path.as_deref(),
        ) && daemon_bundle != own_bundle
        {
            cx.warn(
                warning::AGENT_BUNDLE_MISMATCH,
                format!(
                    "the daemon runs from {daemon_bundle}, but this stackly is from {own_bundle}"
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
            info: Some(info),
            health: health.as_ref().ok().cloned(),
            client: Some(ClientContract {
                protocol: ProtocolVersion::CURRENT,
                contract_digest: CONTRACT_DIGEST,
                contract_drift: drift,
            }),
            agent,
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

/// The result of a status when no daemon answered: only the agent block.
fn no_daemon(agent: AgentReport, failure: CliFailure) -> CommandOutput<DaemonStatus> {
    CommandOutput::partial(
        DaemonStatus {
            info: None,
            health: None,
            client: None,
            agent,
        },
        failure,
    )
}

impl Render for DaemonStatus {
    fn render(&self, out: &mut HumanWriter) {
        let Some(info) = &self.info else {
            let mut rows = vec![("Status", Cell::toned("not running", Tone::Caution))];
            rows.extend(render_agent_rows(&self.agent, None));
            out.kv(&rows);
            return;
        };
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
        rows.extend(render_agent_rows(&self.agent, info.launch.as_ref()));
        out.kv(&rows);
    }
}

/// The tone of an agent state: `enabled` positive; `requires_approval`,
/// `disabled`, and `not_registered` caution; `not_found` critical;
/// `unavailable` neutral.
fn state_tone(state: &str) -> Tone {
    match state {
        s if s == BackgroundAgentState::Enabled.as_code() => Tone::Positive,
        s if s == BackgroundAgentState::NotFound.as_code() => Tone::Critical,
        s if s == BackgroundAgentState::Unavailable.as_code() => Tone::Neutral,
        _ => Tone::Caution,
    }
}

/// The agent rows every daemon command prints: `Agent`, `Launched` (when
/// the daemon's launch is known), `App`, and `Next` (when something is left
/// to do).
pub fn render_agent_rows(
    agent: &AgentReport,
    launch: Option<&DaemonLaunch>,
) -> Vec<(&'static str, Cell)> {
    let state = agent.reason.map_or_else(
        || agent.state.to_owned(),
        |reason| format!("{} ({reason})", agent.state),
    );
    let mut rows = vec![("Agent", Cell::toned(&state, state_tone(agent.state)))];
    if let Some(launch) = launch {
        let text = match (launch.context, launch.label.as_deref()) {
            (LaunchContextKind::Launchd, Some(label)) => format!("launchd ({label})"),
            (LaunchContextKind::Launchd, None) => "launchd".to_owned(),
            (LaunchContextKind::Manual, _) => "by hand".to_owned(),
            (LaunchContextKind::Unrecognized, _) => "unknown".to_owned(),
        };
        rows.push(("Launched", Cell::fixed(text)));
    }
    rows.push(("App", Cell::optional(agent.bundle_path.as_deref())));
    if let Some(next) = recovery_text(agent.recovery) {
        rows.push(("Next", Cell::fixed(next)));
    }
    rows
}
