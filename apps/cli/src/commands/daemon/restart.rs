//! `stackly daemon restart` (spec 0015, AC-10, *Penantian restart*): asks
//! the daemon to stop so launchd starts it again, then waits for the new
//! run. When launchd does not start it (it cannot after the app was
//! replaced under a registered agent), the CLI starts it through macOS once
//! the respawn is overdue.

use std::time::Duration;

use serde::Serialize;
use stackly_platform::bundle::LAUNCHD_THROTTLE_INTERVAL_SECS;
use stackly_platform::{AgentOp, BackgroundAgentState};
use stackly_protocol::daemon::{DaemonInfo, ShutdownMode};
use tokio::time::Instant;
use uuid::Uuid;

use super::Deps;
use super::agent::AgentReport;
use super::start::start_flow;
use super::status::render_agent_rows;
use super::wait::{
    DAEMON_POLL_INTERVAL, DAEMON_START_WAIT, DAEMON_STOP_WAIT, RESPAWN_GRACE, WaitEnd,
    daemon_absent, not_up_yet, sleep_or_interrupt, wait_for_daemon,
};
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::{Warning, warning};
use crate::output::human::{Cell, HumanWriter, Render};

/// The `daemon restart` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartResult {
    /// The run that was asked to stop; null when no daemon ran.
    pub previous_server_instance_id: Option<Uuid>,
    /// The agent, filled only when the CLI had to look at it.
    pub agent: Option<AgentReport>,
    /// The new daemon; null only when the command failed.
    pub info: Option<DaemonInfo>,
}

impl Render for RestartResult {
    fn render(&self, out: &mut HumanWriter) {
        let mut rows = match &self.agent {
            Some(agent) => render_agent_rows(agent, None),
            None => Vec::new(),
        };
        if let Some(info) = &self.info {
            rows.push(("Daemon", Cell::fixed(format!("running (pid {})", info.pid))));
        }
        out.kv(&rows);
    }
}

fn partial(
    previous: Option<Uuid>,
    agent: Option<AgentReport>,
    failure: CliFailure,
    warnings: Vec<Warning>,
) -> CommandOutput<RestartResult> {
    let mut output = CommandOutput::partial(
        RestartResult {
            previous_server_instance_id: previous,
            agent,
            info: None,
        },
        failure,
    );
    output.warnings = warnings;
    output
}

/// Runs `daemon restart`.
pub async fn restart(deps: &mut Deps<'_>) -> CommandOutput<RestartResult> {
    let agent = deps.agent;
    // `L0`: when launchd last started the daemon, before it is asked to stop.
    let l0 = agent.records().last_launched_at();
    let shutdown = match deps
        .probe
        .shutdown(ShutdownMode::Restart, DAEMON_STOP_WAIT)
        .await
    {
        Ok(shutdown) => shutdown,
        Err(failure) if daemon_absent(&failure) => {
            // No daemon runs, so a restart is a start.
            let flow = start_flow(deps, "restart").await;
            let mut output = match flow.failure {
                Some(failure) => CommandOutput::partial(
                    RestartResult {
                        previous_server_instance_id: None,
                        agent: flow.agent,
                        info: None,
                    },
                    failure,
                ),
                None => CommandOutput::ok(RestartResult {
                    previous_server_instance_id: None,
                    agent: flow.agent,
                    info: flow.info,
                }),
            };
            output.warnings = flow.warnings;
            return output;
        }
        Err(failure) => return partial(None, None, failure, Vec::new()),
    };
    let previous = shutdown.server_instance_id;
    if !shutdown.eof {
        let failure = CliFailure::daemon_wait_timeout("restart", DAEMON_STOP_WAIT);
        return partial(Some(previous), None, failure, Vec::new());
    }
    let t_eof = Instant::now();
    let hard_deadline = t_eof + DAEMON_START_WAIT;
    let throttle = Duration::from_secs(LAUNCHD_THROTTLE_INTERVAL_SECS);
    // How long ago launchd started the daemon that just stopped, kept
    // between zero and the throttle interval.
    let elapsed = l0.map_or(Duration::ZERO, |l0| {
        let seconds = agent.clock().now().as_second() - l0.as_second();
        Duration::from_secs(u64::try_from(seconds).unwrap_or(0)).min(throttle)
    });
    let respawn_at = t_eof + (throttle - elapsed) + RESPAWN_GRACE;
    let mut respawn_seen = false;
    loop {
        match deps.probe.attempt().await {
            Ok(reachable) if reachable.server_instance_id != previous => {
                return up(deps, previous, None).await;
            }
            Ok(_) => {}
            Err(failure) if not_up_yet(&failure) => {}
            Err(failure) => return partial(Some(previous), None, failure, Vec::new()),
        }
        respawn_seen = respawn_seen || launched_after(deps, l0);
        let now = Instant::now();
        if now >= hard_deadline {
            let failure = CliFailure::daemon_wait_timeout("restart", DAEMON_START_WAIT);
            return partial(Some(previous), None, failure, Vec::new());
        }
        if !respawn_seen && !agent.root_override() && now >= respawn_at {
            match overdue(deps, previous, l0).await {
                Overdue::Seen => {
                    respawn_seen = true;
                    continue;
                }
                Overdue::Done(output) => return *output,
            }
        }
        let next = if respawn_seen || agent.root_override() {
            hard_deadline
        } else {
            respawn_at.min(hard_deadline)
        };
        let wait = DAEMON_POLL_INTERVAL.min(next.saturating_duration_since(now));
        if let Err(failure) = sleep_or_interrupt(deps.interrupt, wait).await {
            return partial(Some(previous), None, failure, Vec::new());
        }
    }
}

/// Whether the launch record is newer than `l0`, or appeared when there was
/// none: launchd started a new run.
fn launched_after(deps: &Deps<'_>, l0: Option<jiff::Timestamp>) -> bool {
    match (deps.agent.records().last_launched_at(), l0) {
        (Some(now), Some(before)) => now > before,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// Reads the new daemon and builds the success result.
async fn up(
    deps: &mut Deps<'_>,
    previous: Uuid,
    agent: Option<AgentReport>,
) -> CommandOutput<RestartResult> {
    match deps.probe.info().await {
        Ok(info) => CommandOutput::ok(RestartResult {
            previous_server_instance_id: Some(previous),
            agent,
            info: Some(info),
        }),
        Err(failure) => partial(Some(previous), agent, failure, Vec::new()),
    }
}

/// How step 5 and 6 of the wait ended.
enum Overdue {
    /// A new run showed up while the status was read; keep waiting.
    Seen,
    /// The command is over.
    Done(Box<CommandOutput<RestartResult>>),
}

/// The respawn is overdue: read the agent, and when it is `enabled`, start
/// the daemon through `relaunch` and wait for the new run.
async fn overdue(deps: &mut Deps<'_>, previous: Uuid, l0: Option<jiff::Timestamp>) -> Overdue {
    let agent = deps.agent;
    let (probe, assessment) = agent.status().await;
    if let Some(failure) = agent.state_failure(&assessment, &probe) {
        let report = agent.report(&assessment, false);
        return Overdue::Done(Box::new(partial(
            Some(previous),
            Some(report),
            failure,
            Vec::new(),
        )));
    }
    // The respawn may have happened while the status was read.
    if let Ok(reachable) = deps.probe.attempt().await
        && reachable.server_instance_id != previous
    {
        let report = agent.report(&assessment, true);
        return Overdue::Done(Box::new(up(deps, previous, Some(report)).await));
    }
    if launched_after(deps, l0) {
        return Overdue::Seen;
    }
    let acted = agent.act(AgentOp::Relaunch, deps.interrupt).await;
    let report = agent.report(&acted.assessment, false);
    if let Some(failure) = acted.failure {
        return Overdue::Done(Box::new(partial(
            Some(previous),
            Some(report),
            failure,
            Vec::new(),
        )));
    }
    debug_assert_eq!(acted.assessment.state, BackgroundAgentState::Enabled);
    let warnings = vec![Warning::new(
        warning::AGENT_RELAUNCHED,
        "launchd didn't start stacklyd again, so stackly started it through macOS (this happens after the app is replaced)",
    )];
    let deadline = Instant::now() + DAEMON_START_WAIT;
    let ended = wait_for_daemon(deps.probe, deps.interrupt, deadline, |reachable| {
        reachable.server_instance_id != previous
    })
    .await;
    let mut output = match ended {
        Ok(WaitEnd::Up(_)) => {
            let report = agent.report(&acted.assessment, true);
            up(deps, previous, Some(report)).await
        }
        Ok(WaitEnd::TimedOut) => partial(
            Some(previous),
            Some(report),
            CliFailure::daemon_wait_timeout("restart", DAEMON_START_WAIT),
            Vec::new(),
        ),
        Err(failure) => partial(Some(previous), Some(report), failure, Vec::new()),
    };
    output.warnings = warnings;
    Overdue::Done(Box::new(output))
}

#[cfg(test)]
mod tests {
    use stackly_platform::{AgentOp, AgentRegistration};
    use stackly_protocol::ErrorBody;
    use stackly_protocol::codes::{cli, daemon};
    use stackly_testkit::FakeBackgroundAgent;

    use super::super::testing::{
        AfterInterrupt, Rig, ScriptedProbe, ScriptedRecords, missing, run,
    };
    use super::super::wait::{Reachable, Shutdown};
    use super::*;

    const L0: &str = "2026-01-01T00:00:00Z";

    fn at(seconds: i64) -> jiff::Timestamp {
        L0.parse::<jiff::Timestamp>()
            .expect("a fixed time")
            .checked_add(jiff::SignedDuration::from_secs(seconds))
            .expect("in range")
    }

    fn eof() -> Result<Shutdown, CliFailure> {
        Ok(Shutdown {
            server_instance_id: run(1),
            eof: true,
        })
    }

    async fn restart_with(
        rig: &Rig,
        probe: &ScriptedProbe,
        interrupt: &mut AfterInterrupt,
    ) -> CommandOutput<RestartResult> {
        let mut deps = Deps {
            agent: &rig.env,
            probe,
            interrupt,
        };
        restart(&mut deps).await
    }

    /// A rig whose launch record keeps `L0`, and whose wall clock stands
    /// `since_l0` seconds after it (negative: `L0` is in the future).
    fn rig_with_l0(since_l0: i64) -> Rig {
        let rig = Rig::new(ScriptedRecords::new(|_| Some(at(0))), false);
        rig.clock.set(at(since_l0));
        rig.fake.set_registration(AgentRegistration::Enabled);
        rig
    }

    /// A probe that has no new daemon until the fake agent was relaunched.
    fn up_after_relaunch(rig: &Rig) -> ScriptedProbe {
        let fake = rig.fake.clone();
        ScriptedProbe::new(move |_, _| {
            if fake.count(AgentOp::Relaunch) > 0 {
                Ok(Reachable {
                    server_instance_id: run(2),
                })
            } else {
                Err(missing())
            }
        })
        .on_shutdown(eof())
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_run_seen_by_the_handshake_ends_the_wait_and_asks_the_os_nothing() {
        let rig = rig_with_l0(100);
        let probe = ScriptedProbe::new(|_, elapsed| {
            if elapsed >= Duration::from_secs(3) {
                Ok(Reachable {
                    server_instance_id: run(2),
                })
            } else {
                Err(missing())
            }
        })
        .on_shutdown(eof());

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        let result = output.result.expect("a result");
        assert_eq!(result.previous_server_instance_id, Some(run(1)));
        assert_eq!(
            result.info.map(|info| info.server_instance_id),
            Some(run(2))
        );
        assert!(result.agent.is_none());
        assert!(rig.fake.calls().is_empty());
        assert_eq!(probe.shutdown_modes(), [ShutdownMode::Restart]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_newer_launch_record_means_launchd_started_it_and_nothing_is_relaunched() {
        let rig = Rig::new(
            ScriptedRecords::new(|elapsed| {
                Some(if elapsed >= Duration::from_secs(2) {
                    at(50)
                } else {
                    at(0)
                })
            }),
            false,
        );
        rig.clock.set(at(100));
        rig.fake.set_registration(AgentRegistration::Enabled);
        // The new daemon needs 25 seconds: past the respawn bound, inside
        // the 30 seconds.
        let probe = ScriptedProbe::new(|_, elapsed| {
            if elapsed >= Duration::from_secs(25) {
                Ok(Reachable {
                    server_instance_id: run(2),
                })
            } else {
                Err(missing())
            }
        })
        .on_shutdown(eof());

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        assert!(rig.fake.calls().is_empty(), "{:?}", rig.fake.calls());
        assert!(output.warnings.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn no_new_run_by_the_respawn_bound_relaunches_once_with_a_warning() {
        // The daemon ran 2 seconds before the stop: 8 seconds of throttle
        // and 5 of grace make the bound 13 seconds after the EOF.
        let rig = rig_with_l0(2);
        let probe = up_after_relaunch(&rig);
        let started = tokio::time::Instant::now();

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_secs(13) && elapsed < Duration::from_secs(14),
            "{elapsed:?}"
        );
        assert_eq!(rig.fake.calls(), [AgentOp::Status, AgentOp::Relaunch]);
        assert_eq!(output.warnings[0].code, warning::AGENT_RELAUNCHED);
        let result = output.result.expect("a result");
        assert_eq!(result.agent.expect("an agent").state, "enabled");
        assert_eq!(
            result.info.map(|info| info.server_instance_id),
            Some(run(2))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_respawn_bound_follows_how_long_ago_the_daemon_started() {
        for (since_l0, low, high) in [
            // older than the throttle: clamped to 10, so 0 + 5
            (100, 5, 6),
            // exactly the throttle
            (10, 5, 6),
            // L0 in the future counts as 0: 10 + 5
            (-60, 15, 16),
            // 9 seconds ago: 1 + 5
            (9, 6, 7),
        ] {
            let rig = rig_with_l0(since_l0);
            let probe = up_after_relaunch(&rig);
            let started = tokio::time::Instant::now();

            let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

            assert!(output.failure.is_none(), "{since_l0}");
            let elapsed = started.elapsed();
            assert!(
                elapsed >= Duration::from_secs(low) && elapsed < Duration::from_secs(high),
                "{since_l0}: {elapsed:?}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_launch_record_the_bound_is_15_seconds() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let probe = up_after_relaunch(&rig);
        let started = tokio::time::Instant::now();

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_secs(15) && elapsed < Duration::from_secs(16),
            "{elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_respawn_that_shows_up_while_the_status_is_read_cancels_the_relaunch() {
        // The launch record turns newer as soon as the status has been read.
        let fake = FakeBackgroundAgent::new();
        let rig = Rig::with_fake(
            fake.clone(),
            ScriptedRecords::new(move |_| {
                Some(if fake.count(AgentOp::Status) > 0 {
                    at(50)
                } else {
                    at(0)
                })
            }),
            false,
        );
        rig.clock.set(at(2));
        rig.fake.set_registration(AgentRegistration::Enabled);
        let fake = rig.fake.clone();
        let probe = ScriptedProbe::new(move |_, elapsed| {
            if fake.count(AgentOp::Status) > 0 && elapsed >= Duration::from_secs(20) {
                Ok(Reachable {
                    server_instance_id: run(2),
                })
            } else {
                Err(missing())
            }
        })
        .on_shutdown(eof());

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        assert_eq!(rig.fake.calls(), [AgentOp::Status], "no relaunch");
    }

    #[tokio::test(start_paused = true)]
    async fn a_state_that_needs_the_user_at_the_bound_fails_without_a_relaunch() {
        let rig = rig_with_l0(2);
        rig.fake
            .set_registration(AgentRegistration::RequiresApproval);
        let probe = ScriptedProbe::no_daemon().on_shutdown(eof());

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        let failure = output.failure.expect("refused");
        assert!(
            failure.body.is(&cli::AGENT_DISABLED),
            "launch record exists"
        );
        assert_eq!(failure.exit_code(), 4);
        let result = output.result.expect("a partial result");
        assert!(result.info.is_none());
        assert!(result.agent.is_some());
        assert_eq!(rig.fake.calls(), [AgentOp::Status]);
        assert!(output.warnings.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_relaunch_that_ends_in_approval_fails_at_once_without_a_warning() {
        let rig = rig_with_l0(2);
        rig.fake.on_relaunch(AgentRegistration::RequiresApproval);
        let probe = ScriptedProbe::no_daemon().on_shutdown(eof());

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert_eq!(output.failure.expect("refused").exit_code(), 4);
        assert!(output.warnings.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_relaunched_daemon_that_never_answers_times_out_30_seconds_after_the_relaunch() {
        let rig = rig_with_l0(2);
        let probe = ScriptedProbe::no_daemon().on_shutdown(eof());
        let started = tokio::time::Instant::now();

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        let failure = output.failure.expect("timeout");
        assert!(failure.body.is(&cli::DAEMON_WAIT_TIMEOUT));
        assert_eq!(failure.body.detail("action"), Some("restart"));
        assert!(started.elapsed() >= Duration::from_secs(13 + 30));
        assert_eq!(output.warnings[0].code, warning::AGENT_RELAUNCHED);
    }

    #[tokio::test(start_paused = true)]
    async fn the_root_override_waits_30_seconds_and_never_relaunches() {
        let rig = Rig::new(ScriptedRecords::none(), true);
        let probe = ScriptedProbe::no_daemon().on_shutdown(eof());
        let started = tokio::time::Instant::now();

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert_eq!(output.failure.expect("timeout").exit_code(), 8);
        assert!(started.elapsed() >= DAEMON_START_WAIT);
        assert!(rig.fake.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_manual_daemon_refuses_and_nothing_else_happens() {
        let rig = rig_with_l0(2);
        let refusal = CliFailure::daemon(
            ErrorBody::new(&daemon::RESTART_UNAVAILABLE, "not started by launchd")
                .with_detail("launchContext", "manual"),
            None,
        );
        let probe = ScriptedProbe::running(run(1)).on_shutdown(Err(refusal));

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        let failure = output.failure.expect("refused");
        assert!(failure.body.is(&daemon::RESTART_UNAVAILABLE));
        assert_eq!(failure.exit_code(), 5);
        assert!(rig.fake.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_that_stays_open_is_a_stop_timeout_of_20_seconds() {
        let rig = rig_with_l0(2);
        let probe = ScriptedProbe::running(run(1)).on_shutdown(Ok(Shutdown {
            server_instance_id: run(1),
            eof: false,
        }));

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        let failure = output.failure.expect("timeout");
        assert!(failure.body.is(&cli::DAEMON_WAIT_TIMEOUT));
        assert_eq!(failure.body.detail("seconds"), Some("20"));
        assert_eq!(
            output.result.expect("partial").previous_server_instance_id,
            Some(run(1))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_does_not_run_is_started_like_start_does() {
        let rig = rig_with_l0(100);
        let fake = rig.fake.clone();
        let probe = ScriptedProbe::new(move |_, _| {
            if fake.count(AgentOp::Relaunch) > 0 {
                Ok(Reachable {
                    server_instance_id: run(3),
                })
            } else {
                Err(missing())
            }
        });

        let output = restart_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        let result = output.result.expect("a result");
        assert_eq!(result.previous_server_instance_id, None);
        assert_eq!(result.agent.expect("an agent").state, "enabled");
        assert_eq!(
            result.info.map(|info| info.server_instance_id),
            Some(run(3))
        );
        assert_eq!(rig.fake.calls(), [AgentOp::Status, AgentOp::Relaunch]);
    }

    #[tokio::test(start_paused = true)]
    async fn ctrl_c_while_waiting_for_the_new_run_exits_130_without_a_relaunch() {
        let rig = rig_with_l0(2);
        let probe = ScriptedProbe::no_daemon().on_shutdown(eof());
        let mut interrupt = AfterInterrupt::new(Some(Duration::from_secs(4)));

        let output = restart_with(&rig, &probe, &mut interrupt).await;

        let failure = output.failure.expect("interrupted");
        assert_eq!(failure.exit_code(), 130);
        assert!(rig.fake.calls().is_empty());
    }
}
