//! `stackly daemon install` (spec 0015, AC-8): registers the background
//! agent when it is not registered, and only then. An agent that waits for
//! the user's approval, or that the user switched off, is never registered
//! again behind their back.

use serde::Serialize;
use stackly_platform::{AgentOp, BackgroundAgentState};
use stackly_protocol::daemon::DaemonInfo;

use super::Deps;
use super::agent::AgentReport;
use super::status::render_agent_rows;
use super::wait::{DAEMON_START_WAIT, WaitEnd, not_up_yet, wait_for_daemon};
use crate::output::CommandOutput;
use crate::output::envelope::{Warning, warning};
use crate::output::human::{Cell, HumanWriter, Render};
use tokio::time::Instant;

/// The `daemon install` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    /// The agent after the command.
    pub agent: AgentReport,
    /// The daemon, once it answered; null when it did not.
    pub info: Option<DaemonInfo>,
}

impl Render for InstallResult {
    fn render(&self, out: &mut HumanWriter) {
        let mut rows = render_agent_rows(&self.agent, None);
        if let Some(info) = &self.info {
            rows.push(("Daemon", Cell::fixed(format!("running (pid {})", info.pid))));
        }
        out.kv(&rows);
    }
}

/// Runs `daemon install`.
pub async fn install(deps: &mut Deps<'_>) -> CommandOutput<InstallResult> {
    let agent = deps.agent;
    let (probe, assessment) = agent.status().await;
    match assessment.state {
        BackgroundAgentState::Enabled => {
            let (info, warnings) = match deps.probe.info().await {
                Ok(info) => (Some(info), Vec::new()),
                Err(failure) if not_up_yet(&failure) => (None, vec![not_running()]),
                Err(failure) => {
                    let result = InstallResult {
                        agent: agent.report(&assessment, false),
                        info: None,
                    };
                    return CommandOutput::partial(result, failure);
                }
            };
            let up = info.is_some();
            let mut output = CommandOutput::ok(InstallResult {
                agent: agent.report(&assessment, up),
                info,
            });
            output.warnings = warnings;
            output
        }
        BackgroundAgentState::NotRegistered => {
            let acted = agent.act(AgentOp::Register, deps.interrupt).await;
            if let Some(failure) = acted.failure {
                let result = InstallResult {
                    agent: agent.report(&acted.assessment, false),
                    info: None,
                };
                return CommandOutput::partial(result, failure);
            }
            let deadline = Instant::now() + DAEMON_START_WAIT;
            let ended = wait_for_daemon(deps.probe, deps.interrupt, deadline, |_| true).await;
            let info = match ended {
                Ok(WaitEnd::Up(_)) => deps.probe.info().await.ok(),
                Ok(WaitEnd::TimedOut) => None,
                Err(failure) => {
                    let result = InstallResult {
                        agent: agent.report(&acted.assessment, false),
                        info: None,
                    };
                    return CommandOutput::partial(result, failure);
                }
            };
            let up = info.is_some();
            let mut output = CommandOutput::ok(InstallResult {
                agent: agent.report(&acted.assessment, up),
                info,
            });
            if !up {
                output.warnings.push(not_running());
            }
            output
        }
        _ => {
            let failure = agent
                .state_failure(&assessment, &probe)
                .unwrap_or_else(crate::failure::CliFailure::not_connected);
            let result = InstallResult {
                agent: agent.report(&assessment, false),
                info: None,
            };
            CommandOutput::partial(result, failure)
        }
    }
}

fn not_running() -> Warning {
    Warning::new(
        warning::DAEMON_NOT_RUNNING,
        "the background agent is registered, but the daemon did not answer within 30 seconds",
    )
}

#[cfg(test)]
mod tests {
    use stackly_platform::{AgentOp, AgentOsErrorKind, AgentRegistration, BackgroundAgentError};
    use stackly_protocol::codes::cli;

    use super::super::testing::{AfterInterrupt, Rig, ScriptedProbe, ScriptedRecords, run};
    use super::*;

    async fn install_with(rig: &Rig, probe: &ScriptedProbe) -> CommandOutput<InstallResult> {
        let mut interrupt = AfterInterrupt::never();
        let mut deps = Deps {
            agent: &rig.env,
            probe,
            interrupt: &mut interrupt,
        };
        install(&mut deps).await
    }

    fn failure_code(output: &CommandOutput<InstallResult>) -> (&str, u8) {
        let failure = output.failure.as_ref().expect("the install failed");
        (failure.body.code(), failure.exit_code())
    }

    #[tokio::test(start_paused = true)]
    async fn a_never_registered_bundle_registers_once_and_waits_for_the_daemon() {
        let rig = Rig::plain();
        let probe = ScriptedProbe::new(|n, _| {
            if n < 3 {
                Err(super::super::testing::missing())
            } else {
                Ok(super::super::wait::Reachable {
                    server_instance_id: run(1),
                })
            }
        });

        let output = install_with(&rig, &probe).await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        let result = output.result.expect("a result");
        assert_eq!(result.agent.state, "enabled");
        assert_eq!(
            result.info.map(|info| info.server_instance_id),
            Some(run(1))
        );
        assert_eq!(rig.fake.calls(), [AgentOp::Status, AgentOp::Register]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_enabled_agent_is_not_registered_again_and_exits_0() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let probe = ScriptedProbe::running(run(1));

        let output = install_with(&rig, &probe).await;

        assert!(output.failure.is_none());
        assert_eq!(rig.fake.calls(), [AgentOp::Status]);
        assert!(output.result.expect("a result").info.is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn an_enabled_agent_without_a_daemon_warns_and_exits_0() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);

        let output = install_with(&rig, &ScriptedProbe::no_daemon()).await;

        assert!(output.failure.is_none());
        assert_eq!(output.warnings[0].code, warning::DAEMON_NOT_RUNNING);
        let result = output.result.expect("a result");
        assert!(result.info.is_none());
        assert_eq!(result.agent.recovery, "start");
    }

    #[tokio::test(start_paused = true)]
    async fn approval_and_a_switch_in_login_items_never_register() {
        let waiting = Rig::plain();
        waiting
            .fake
            .set_registration(AgentRegistration::RequiresApproval);
        let output = install_with(&waiting, &ScriptedProbe::no_daemon()).await;
        assert_eq!(failure_code(&output), ("cli.agent_requires_approval", 4));
        assert_eq!(waiting.fake.calls(), [AgentOp::Status]);

        let switched_off = Rig::new(ScriptedRecords::unreadable(), false);
        switched_off
            .fake
            .set_registration(AgentRegistration::RequiresApproval);
        let output = install_with(&switched_off, &ScriptedProbe::no_daemon()).await;
        assert_eq!(failure_code(&output), ("cli.agent_disabled", 4));
        assert_eq!(switched_off.fake.calls(), [AgentOp::Status]);
        assert!(!switched_off.fake.changed_registration());
    }

    #[tokio::test(start_paused = true)]
    async fn a_registration_that_needs_approval_exits_4_without_waiting() {
        let rig = Rig::plain();
        rig.fake.on_register(AgentRegistration::RequiresApproval);

        let output = install_with(&rig, &ScriptedProbe::no_daemon()).await;

        assert_eq!(failure_code(&output), ("cli.agent_requires_approval", 4));
        let result = output.result.expect("a partial result");
        assert_eq!(result.agent.state, "requires_approval");
        assert_eq!(result.agent.recovery, "approve_in_login_items");
        assert!(result.info.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_registration_macos_does_not_find_exits_6() {
        let rig = Rig::plain();
        rig.fake.on_register(AgentRegistration::NotFound);

        let output = install_with(&rig, &ScriptedProbe::no_daemon()).await;

        assert_eq!(failure_code(&output), ("cli.agent_not_found", 6));
        assert_eq!(output.result.expect("a result").agent.state, "not_found");
    }

    #[tokio::test(start_paused = true)]
    async fn a_registration_macos_did_not_keep_exits_7_without_an_os_code() {
        let rig = Rig::plain();
        rig.fake.on_register(AgentRegistration::NotRegistered);

        let output = install_with(&rig, &ScriptedProbe::no_daemon()).await;

        let failure = output.failure.expect("the install failed");
        assert!(failure.body.is(&cli::AGENT_OS_ERROR));
        assert_eq!(failure.exit_code(), 7);
        assert_eq!(failure.body.detail("osCode"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn every_register_error_gets_its_code_and_exit() {
        let os = |kind: AgentOsErrorKind, code: i64| BackgroundAgentError::Os {
            op: AgentOp::Register,
            kind,
            domain: "SMAppServiceErrorDomain".to_owned(),
            code,
            message: "refused".to_owned(),
        };
        for (error, expected) in [
            (
                os(AgentOsErrorKind::InvalidSignature, 3),
                ("cli.agent_invalid_signature", 4),
            ),
            (
                os(AgentOsErrorKind::NotFound, 1),
                ("cli.agent_not_found", 6),
            ),
            (os(AgentOsErrorKind::Other, 2), ("cli.agent_os_error", 7)),
            (
                os(AgentOsErrorKind::LaunchDenied, 1),
                ("cli.agent_requires_approval", 4),
            ),
            (
                os(AgentOsErrorKind::MustBeEnabled, 1),
                ("cli.agent_requires_approval", 4),
            ),
        ] {
            let rig = Rig::plain();
            rig.fake.fail_next(AgentOp::Register, error);

            let output = install_with(&rig, &ScriptedProbe::no_daemon()).await;

            assert_eq!(failure_code(&output), expected);
            assert_eq!(rig.fake.count(AgentOp::Register), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_denied_registration_is_read_again_and_the_launch_record_makes_it_disabled() {
        let rig = Rig::new(ScriptedRecords::unreadable(), false);
        rig.fake.set_registration(AgentRegistration::NotRegistered);
        rig.fake.fail_next(
            AgentOp::Register,
            BackgroundAgentError::Os {
                op: AgentOp::Register,
                kind: AgentOsErrorKind::LaunchDenied,
                domain: "SMAppServiceErrorDomain".to_owned(),
                code: 1,
                message: "denied".to_owned(),
            },
        );
        rig.fake.on_register(AgentRegistration::RequiresApproval);
        // The failed call leaves the state as it was; the reread says
        // `not_registered`, which is still an approval problem to the user.
        let output = install_with(&rig, &ScriptedProbe::no_daemon()).await;
        assert_eq!(failure_code(&output), ("cli.agent_requires_approval", 4));
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_never_answers_after_the_registration_is_a_warning() {
        let rig = Rig::plain();

        let started = tokio::time::Instant::now();
        let output = install_with(&rig, &ScriptedProbe::no_daemon()).await;

        assert!(output.failure.is_none());
        assert_eq!(output.warnings[0].code, warning::DAEMON_NOT_RUNNING);
        assert!(started.elapsed() >= DAEMON_START_WAIT);
        assert!(output.result.expect("a result").info.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn the_unavailable_states_fail_with_their_codes_and_never_call_register() {
        let override_rig = Rig::new(ScriptedRecords::none(), true);
        let output = install_with(&override_rig, &ScriptedProbe::no_daemon()).await;
        assert_eq!(failure_code(&output), ("cli.agent_root_override", 2));
        assert!(
            override_rig.fake.calls().is_empty(),
            "the OS is never asked"
        );

        let outside = Rig::plain();
        outside.fake.set_not_bundled("/work/target/debug/stackly");
        let output = install_with(&outside, &ScriptedProbe::no_daemon()).await;
        assert_eq!(failure_code(&output), ("cli.agent_not_bundled", 5));

        let xcode = Rig::plain();
        xcode.fake.set_helper_missing();
        let output = install_with(&xcode, &ScriptedProbe::no_daemon()).await;
        assert_eq!(failure_code(&output), ("cli.agent_helper_missing", 6));
        for rig in [&outside, &xcode] {
            assert_eq!(rig.fake.calls(), [AgentOp::Status]);
        }
    }
}
