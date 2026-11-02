//! `stackly instance autostart <target> on|off` (spec 0031, AC-16, AC-18):
//! whether an instance starts at every login. It changes the policy and
//! nothing else: turning it on starts nothing now, and turning it off does
//! not stop a running instance.

use clap::{Args, ValueEnum};
use serde::Serialize;
use stackly_platform::BackgroundAgentState;
use stackly_protocol::instance::{InstanceSummary, SetStartupParams, StartupPolicyKind};
use stackly_protocol::{Call, Reply};

use super::resolve::TargetArg;
use crate::args::IdempotencyKeyArg;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::warning;
use crate::output::human::{HumanWriter, Render};
use crate::session::{CallSpec, RequestTimeout};

/// The two settings of the verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    /// Start the instance at every login
    On,
    /// Start it only when asked
    Off,
}

/// `stackly instance autostart`.
#[derive(Debug, Args)]
pub struct AutostartArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// Whether the instance starts at login
    #[arg(value_enum)]
    pub switch: Switch,
    /// Send this key so a repeated request gets the same answer; by default a new one is made
    #[arg(long, value_name = "KEY")]
    pub idempotency_key: Option<IdempotencyKeyArg>,
}

/// The `result` of `autostart`: the instance as `instance.set_startup`
/// answers.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct AutostartResult {
    instance: InstanceSummary,
}

impl RunCommand for AutostartArgs {
    type Output = AutostartResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<AutostartResult> {
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let startup = match self.switch {
            Switch::On => StartupPolicyKind::AtLogin,
            Switch::Off => StartupPolicyKind::Manual,
        };
        let key = self.idempotency_key.as_ref().map_or_else(
            || uuid::Uuid::now_v7().to_string(),
            |key| key.as_str().to_owned(),
        );
        let call = Call::InstanceSetStartup(SetStartupParams {
            instance_id: id,
            startup,
            expected_revision: None,
        });
        let spec = CallSpec {
            timeout: RequestTimeout::Interactive,
            idempotency_key: Some(key),
        };
        let instance = match cx.call(call, spec).await {
            Ok(Reply::InstanceSetStartup(instance)) => *instance,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        if startup == StartupPolicyKind::AtLogin {
            warn_when_the_agent_is_off(cx, &instance.name).await;
        }
        CommandOutput::ok(AutostartResult { instance })
    }
}

/// Warns that the instance will not start at login while the background
/// agent is not enabled, for the default root; an override root has no
/// agent to ask about.
async fn warn_when_the_agent_is_off(cx: &mut Context, name: &str) {
    let agent = cx.agent.clone();
    if agent.root_override() {
        return;
    }
    let (_, assessment) = agent.status().await;
    if assessment.state != BackgroundAgentState::Enabled {
        cx.warn(
            warning::AGENT_OFF,
            format!(
                "The background agent is off, so “{name}” won’t start at login until you run stackly daemon install."
            ),
        );
    }
}

impl Render for AutostartResult {
    fn render(&self, out: &mut HumanWriter) {
        let state = match self.instance.autostart.policy {
            StartupPolicyKind::AtLogin => "at login",
            _ => "off",
        };
        out.line(&format!("Autostart for “{}”: {state}", self.instance.name));
    }
}
