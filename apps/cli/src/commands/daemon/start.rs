//! `stackly daemon start` (spec 0015, AC-9): does nothing when the daemon
//! already answers, waits out a recent launch, and otherwise starts the
//! daemon through the one way there is, `relaunch`. A state that needs the
//! user (not registered, waiting for approval, switched off) fails without
//! changing the registration.

use serde::Serialize;
use stackly_platform::AgentOp;
use stackly_protocol::daemon::DaemonInfo;
use tokio::time::Instant;

use super::Deps;
use super::agent::AgentReport;
use super::status::render_agent_rows;
use super::wait::{DAEMON_START_WAIT, RECENT_LAUNCH_WINDOW, WaitEnd, not_up_yet, wait_for_daemon};
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::Warning;
use crate::output::human::{Cell, HumanWriter, Render};

/// The `daemon start` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    /// The daemon already answered, so nothing was done.
    pub already_running: bool,
    /// The agent; null exactly when `alreadyRunning`, since that path never
    /// asks the OS.
    pub agent: Option<AgentReport>,
    /// The daemon; null only when the command failed.
    pub info: Option<DaemonInfo>,
}

impl Render for StartResult {
    fn render(&self, out: &mut HumanWriter) {
        let mut rows = match &self.agent {
            Some(agent) => render_agent_rows(agent, None),
            None => Vec::new(),
        };
        if let Some(info) = &self.info {
            let text = if self.already_running {
                format!("already running (pid {})", info.pid)
            } else {
                format!("running (pid {})", info.pid)
            };
            rows.push(("Daemon", Cell::fixed(text)));
        }
        out.kv(&rows);
    }
}

/// What the start steps ended with, for `start` and for `restart` of a
/// daemon that does not run.
#[derive(Debug)]
pub struct Flow {
    /// The daemon already answered.
    pub already_running: bool,
    /// The agent, when the OS was asked.
    pub agent: Option<AgentReport>,
    /// The daemon, once it answered.
    pub info: Option<DaemonInfo>,
    /// The error the command ends with.
    pub failure: Option<CliFailure>,
    /// Warnings collected on the way.
    pub warnings: Vec<Warning>,
}

impl Flow {
    fn failed(agent: Option<AgentReport>, failure: CliFailure) -> Self {
        Self {
            already_running: false,
            agent,
            info: None,
            failure: Some(failure),
            warnings: Vec::new(),
        }
    }
}

/// Runs `daemon start`.
pub async fn start(deps: &mut Deps<'_>) -> CommandOutput<StartResult> {
    let flow = start_flow(deps, "start").await;
    let result = StartResult {
        already_running: flow.already_running,
        agent: flow.agent,
        info: flow.info,
    };
    let mut output = match flow.failure {
        Some(failure) => CommandOutput::partial(result, failure),
        None => CommandOutput::ok(result),
    };
    output.warnings = flow.warnings;
    output
}

/// The steps of `start`; `action` names the command in a wait timeout
/// (`start`, or `restart` for a daemon that did not run).
pub async fn start_flow(deps: &mut Deps<'_>, action: &str) -> Flow {
    let agent = deps.agent;
    match deps.probe.attempt().await {
        Ok(_) => {
            return match deps.probe.info().await {
                Ok(info) => Flow {
                    already_running: true,
                    agent: None,
                    info: Some(info),
                    failure: None,
                    warnings: Vec::new(),
                },
                Err(failure) => Flow::failed(None, failure),
            };
        }
        Err(failure) if not_up_yet(&failure) => {}
        Err(failure) => {
            let (_, assessment) = agent.status().await;
            return Flow::failed(Some(agent.report(&assessment, false)), failure);
        }
    }
    let (probe, assessment) = agent.status().await;
    if let Some(failure) = agent.state_failure(&assessment, &probe) {
        return Flow::failed(Some(agent.report(&assessment, false)), failure);
    }
    // The state is `enabled`. A launch record younger than the window means
    // the daemon may be starting or waiting out launchd's throttle, so the
    // socket gets a chance before anything is changed.
    if launched_recently(deps) {
        let deadline = Instant::now() + RECENT_LAUNCH_WINDOW;
        match wait_for_daemon(deps.probe, deps.interrupt, deadline, |_| true).await {
            Ok(WaitEnd::Up(_)) => return up(deps, agent.report(&assessment, true)).await,
            Ok(WaitEnd::TimedOut) => {}
            Err(failure) => {
                return Flow::failed(Some(agent.report(&assessment, false)), failure);
            }
        }
    }
    let acted = agent.act(AgentOp::Relaunch, deps.interrupt).await;
    if let Some(failure) = acted.failure {
        return Flow::failed(Some(agent.report(&acted.assessment, false)), failure);
    }
    let deadline = Instant::now() + DAEMON_START_WAIT;
    match wait_for_daemon(deps.probe, deps.interrupt, deadline, |_| true).await {
        Ok(WaitEnd::Up(_)) => up(deps, agent.report(&acted.assessment, true)).await,
        Ok(WaitEnd::TimedOut) => Flow::failed(
            Some(agent.report(&acted.assessment, false)),
            CliFailure::daemon_wait_timeout(action, DAEMON_START_WAIT),
        ),
        Err(failure) => Flow::failed(Some(agent.report(&acted.assessment, false)), failure),
    }
}

/// The flow of a daemon that came up: reads its info.
async fn up(deps: &mut Deps<'_>, report: AgentReport) -> Flow {
    match deps.probe.info().await {
        Ok(info) => Flow {
            already_running: false,
            agent: Some(report),
            info: Some(info),
            failure: None,
            warnings: Vec::new(),
        },
        Err(failure) => Flow::failed(Some(report), failure),
    }
}

/// Whether the launch record says launchd started the daemon less than
/// [`RECENT_LAUNCH_WINDOW`] ago.
fn launched_recently(deps: &Deps<'_>) -> bool {
    let Some(launched) = deps.agent.records().last_launched_at() else {
        return false;
    };
    let now = deps.agent.clock().now();
    let age = now.as_second() - launched.as_second();
    (0..RECENT_LAUNCH_WINDOW.as_secs() as i64).contains(&age)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use stackly_platform::{AgentOp, AgentRegistration};
    use stackly_protocol::ErrorBody;
    use stackly_protocol::codes::ipc;

    use super::super::testing::{
        AfterInterrupt, Rig, ScriptedProbe, ScriptedRecords, missing, run,
    };
    use super::super::wait::Reachable;
    use super::*;

    async fn start_with(
        rig: &Rig,
        probe: &ScriptedProbe,
        interrupt: &mut AfterInterrupt,
    ) -> CommandOutput<StartResult> {
        let mut deps = Deps {
            agent: &rig.env,
            probe,
            interrupt,
        };
        start(&mut deps).await
    }

    fn up_after(seconds: u64) -> ScriptedProbe {
        ScriptedProbe::new(move |_, elapsed| {
            if elapsed >= Duration::from_secs(seconds) {
                Ok(Reachable {
                    server_instance_id: run(7),
                })
            } else {
                Err(missing())
            }
        })
    }

    fn failure_code(output: &CommandOutput<StartResult>) -> (&str, u8) {
        let failure = output.failure.as_ref().expect("the start failed");
        (failure.body.code(), failure.exit_code())
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_answers_is_left_alone_and_the_os_is_never_asked() {
        let rig = Rig::plain();

        let output = start_with(
            &rig,
            &ScriptedProbe::running(run(1)),
            &mut AfterInterrupt::never(),
        )
        .await;

        let result = output.result.expect("a result");
        assert!(output.failure.is_none());
        assert!(result.already_running);
        assert!(result.agent.is_none());
        assert!(result.info.is_some());
        assert!(rig.fake.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn an_enabled_agent_without_a_daemon_is_relaunched_once_and_waited_for() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);

        let output = start_with(&rig, &up_after(2), &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        let result = output.result.expect("a result");
        assert!(!result.already_running);
        assert_eq!(result.agent.expect("an agent").state, "enabled");
        assert_eq!(rig.fake.calls(), [AgentOp::Status, AgentOp::Relaunch]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_launch_under_15_seconds_old_gets_the_socket_15_seconds_before_a_relaunch() {
        let rig = Rig::new(
            ScriptedRecords::new(|_| Some("2026-01-01T00:00:00Z".parse().unwrap())),
            false,
        );
        rig.fake.set_registration(AgentRegistration::Enabled);
        // 5 seconds after the launch, and the daemon comes up 8 seconds in.
        rig.clock.set("2026-01-01T00:00:05Z".parse().unwrap());

        let output = start_with(&rig, &up_after(8), &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        assert_eq!(
            rig.fake.calls(),
            [AgentOp::Status],
            "nothing was relaunched"
        );

        // The same launch with no daemon: 15 seconds of waiting, then a relaunch.
        let rig = Rig::new(
            ScriptedRecords::new(|_| Some("2026-01-01T00:00:00Z".parse().unwrap())),
            false,
        );
        rig.fake.set_registration(AgentRegistration::Enabled);
        rig.clock.set("2026-01-01T00:00:05Z".parse().unwrap());
        let started = tokio::time::Instant::now();
        let output = start_with(&rig, &up_after(20), &mut AfterInterrupt::never()).await;
        assert!(output.failure.is_none());
        assert_eq!(rig.fake.calls(), [AgentOp::Status, AgentOp::Relaunch]);
        assert!(started.elapsed() >= Duration::from_secs(20));
    }

    #[tokio::test(start_paused = true)]
    async fn an_old_launch_record_does_not_delay_the_relaunch() {
        let rig = Rig::new(
            ScriptedRecords::new(|_| Some("2026-01-01T00:00:00Z".parse().unwrap())),
            false,
        );
        rig.fake.set_registration(AgentRegistration::Enabled);
        rig.clock.set("2026-01-01T00:10:00Z".parse().unwrap());
        let started = tokio::time::Instant::now();

        let output = start_with(&rig, &up_after(1), &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(rig.fake.calls(), [AgentOp::Status, AgentOp::Relaunch]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_state_that_needs_the_user_fails_without_changing_the_registration() {
        for (registration, record, expected) in [
            (
                AgentRegistration::NotFound,
                false,
                ("cli.agent_not_registered", 6),
            ),
            (
                AgentRegistration::NotRegistered,
                false,
                ("cli.agent_not_registered", 6),
            ),
            (
                AgentRegistration::RequiresApproval,
                false,
                ("cli.agent_requires_approval", 4),
            ),
            (
                AgentRegistration::RequiresApproval,
                true,
                ("cli.agent_disabled", 4),
            ),
        ] {
            let records = if record {
                ScriptedRecords::unreadable()
            } else {
                ScriptedRecords::none()
            };
            let rig = Rig::new(records, false);
            rig.fake.set_registration(registration);

            let output = start_with(
                &rig,
                &ScriptedProbe::no_daemon(),
                &mut AfterInterrupt::never(),
            )
            .await;

            assert_eq!(failure_code(&output), expected, "{registration:?} {record}");
            let result = output.result.expect("a partial result");
            assert!(!result.already_running);
            assert!(result.agent.is_some());
            assert!(result.info.is_none());
            assert_eq!(rig.fake.calls(), [AgentOp::Status]);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_error_that_is_not_a_missing_daemon_is_reported_as_it_is() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let untrusted = ScriptedProbe::new(|_, _| {
            Err(CliFailure::daemon(
                ErrorBody::new(&ipc::ENDPOINT_UNTRUSTED, "not a socket"),
                None,
            ))
        });

        let output = start_with(&rig, &untrusted, &mut AfterInterrupt::never()).await;

        assert_eq!(failure_code(&output), ("ipc.endpoint_untrusted", 4));
        assert_eq!(rig.fake.calls(), [AgentOp::Status], "no relaunch");
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_never_comes_up_after_the_relaunch_times_out_after_30_seconds() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let started = tokio::time::Instant::now();

        let output = start_with(
            &rig,
            &ScriptedProbe::no_daemon(),
            &mut AfterInterrupt::never(),
        )
        .await;

        assert_eq!(failure_code(&output), ("cli.daemon_wait_timeout", 8));
        assert!(started.elapsed() >= DAEMON_START_WAIT);
        assert_eq!(rig.fake.count(AgentOp::Relaunch), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_relaunch_that_ends_in_approval_fails_at_once_without_waiting() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        rig.fake.on_relaunch(AgentRegistration::RequiresApproval);
        let started = tokio::time::Instant::now();

        let output = start_with(
            &rig,
            &ScriptedProbe::no_daemon(),
            &mut AfterInterrupt::never(),
        )
        .await;

        assert_eq!(failure_code(&output), ("cli.agent_requires_approval", 4));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn the_root_override_fails_before_the_os_and_before_any_change() {
        let rig = Rig::new(ScriptedRecords::none(), true);

        let output = start_with(
            &rig,
            &ScriptedProbe::no_daemon(),
            &mut AfterInterrupt::never(),
        )
        .await;

        assert_eq!(failure_code(&output), ("cli.agent_root_override", 2));
        assert!(rig.fake.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn ctrl_c_while_waiting_for_the_daemon_exits_130_and_changes_nothing_more() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let mut interrupt = AfterInterrupt::new(Some(Duration::from_secs(3)));

        let output = start_with(&rig, &ScriptedProbe::no_daemon(), &mut interrupt).await;

        let failure = output.failure.expect("interrupted");
        assert_eq!(failure.exit_code(), 130);
        assert_eq!(failure.body.detail("state"), Some("waiting_for_daemon"));
        assert_eq!(
            rig.fake.count(AgentOp::Relaunch),
            1,
            "the relaunch ran to its end"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn ctrl_c_during_a_relaunch_is_held_until_the_call_ended_and_then_exits_130() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        // Ctrl+C is already there when the call returns.
        let mut interrupt = AfterInterrupt::new(Some(Duration::ZERO));

        let output = start_with(&rig, &ScriptedProbe::no_daemon(), &mut interrupt).await;

        let failure = output.failure.expect("interrupted");
        assert_eq!(failure.exit_code(), 130);
        assert_eq!(failure.body.detail("state"), Some("agent_call"));
        assert_eq!(rig.fake.calls(), [AgentOp::Status, AgentOp::Relaunch]);
    }
}
