//! `stackly daemon restart` (spec 0015, AC-10, *Penantian restart*, and
//! spec 0030, AC-15): shows the plan, asks when instances stop for a
//! moment, asks the daemon to restart so launchd starts it again, follows
//! the plan to its end, and reports what became of each instance. When
//! launchd does not start the daemon (it cannot after the app was replaced
//! under a registered agent), the CLI starts it through macOS once the
//! respawn is overdue.

use std::time::Duration;

use serde::Serialize;
use stackly_platform::bundle::LAUNCHD_THROTTLE_INTERVAL_SECS;
use stackly_platform::{AgentOp, BackgroundAgentState};
use stackly_protocol::codes::daemon as daemon_codes;
use stackly_protocol::daemon::{DaemonInfo, LaunchContextKind};
use stackly_protocol::restart::{PlanState, RestartPlanView};
use tokio::time::Instant;
use uuid::Uuid;

use super::Deps;
use super::agent::AgentReport;
use super::plan::{PlanPrompt, RestartDurations, judge, render_plan, stops_instances};
use super::start::start_flow;
use super::status::render_agent_rows;
use super::wait::{
    DAEMON_POLL_INTERVAL, DAEMON_RESTORE_WAIT, DAEMON_START_WAIT, DAEMON_STOP_WAIT, RESPAWN_GRACE,
    RestartRequest, WaitEnd, daemon_absent, not_up_yet, plan_is_final, sleep_or_interrupt,
    wait_for_daemon,
};
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::{Warning, warning};
use crate::output::human::{Cell, HumanWriter, Render};

/// What the person asked of `daemon restart`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartOptions {
    /// `--force`: restart although an operation that cannot resume runs.
    pub force: bool,
    /// False for `--no-wait-instances`: stop after the new daemon answers.
    pub wait_instances: bool,
}

/// The `daemon restart` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartResult {
    /// The run that was asked to stop; null when no daemon ran.
    pub previous_server_instance_id: Option<Uuid>,
    /// The agent, filled only when the CLI had to look at it.
    pub agent: Option<AgentReport>,
    /// The new daemon; null only when the command failed before it
    /// answered.
    pub info: Option<DaemonInfo>,
    /// The restart plan as last seen, or null when no plan was written.
    pub plan: Option<RestartPlanView>,
    /// How long the parts of the restart took; null until the plan has
    /// reached the new daemon.
    pub durations: Option<RestartDurations>,
}

impl RestartResult {
    fn new(previous: Option<Uuid>, agent: Option<AgentReport>) -> Self {
        Self {
            previous_server_instance_id: previous,
            agent,
            info: None,
            plan: None,
            durations: None,
        }
    }
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
        if let Some(plan) = &self.plan {
            render_plan(plan, self.durations.as_ref(), out);
        }
    }
}

fn partial(
    previous: Option<Uuid>,
    agent: Option<AgentReport>,
    failure: CliFailure,
    warnings: Vec<Warning>,
) -> CommandOutput<RestartResult> {
    let mut output = CommandOutput::partial(RestartResult::new(previous, agent), failure);
    output.warnings = warnings;
    output
}

/// What the wait for the new run needs to finish the plan.
struct Follow {
    /// The plan the old daemon wrote, or `None` for a restart without one.
    plan_id: Option<Uuid>,
    /// The last state the old daemon published.
    plan: Option<RestartPlanView>,
    /// When the command began, which bounds the whole plan.
    started: Instant,
    /// Whether to wait for the instances (`--no-wait-instances` is false).
    wait_instances: bool,
}

/// Runs `daemon restart`.
pub async fn restart(
    deps: &mut Deps<'_>,
    options: RestartOptions,
    prompt: &mut dyn PlanPrompt,
) -> CommandOutput<RestartResult> {
    let started = Instant::now();
    let agent = deps.agent;
    // `L0`: when launchd last started the daemon, before it is asked to stop.
    let l0 = agent.records().last_launched_at();
    let mut preview = match deps.probe.restart_preview().await {
        Ok(preview) => preview,
        Err(failure) if daemon_absent(&failure) => return start_instead(deps).await,
        Err(failure) => return partial(None, None, failure, Vec::new()),
    };
    let mut again = false;
    let handover = loop {
        prompt.show(&preview);
        // A daemon launchd did not start refuses on its own; asking about a
        // restart that cannot happen would only mislead.
        if preview.launch_context == LaunchContextKind::Launchd
            && stops_instances(&preview)
            && let Err(failure) = prompt.confirm(again)
        {
            return partial(None, None, failure, Vec::new());
        }
        let request = RestartRequest {
            expected_plan_digest: Some(preview.digest.clone()),
            force: options.force,
            idempotency_key: Uuid::now_v7().to_string(),
        };
        let eof_limit = if stops_instances(&preview) {
            DAEMON_RESTORE_WAIT
        } else {
            DAEMON_STOP_WAIT
        };
        match deps.probe.restart(&request, eof_limit).await {
            Ok(handover) => break (handover, eof_limit),
            Err(failure)
                if failure.body.is(&daemon_codes::RESTART_PLAN_CHANGED)
                    && !again
                    && prompt.can_ask() =>
            {
                again = true;
                preview = match deps.probe.restart_preview().await {
                    Ok(preview) => preview,
                    Err(failure) => return partial(None, None, failure, Vec::new()),
                };
            }
            Err(failure) => return partial(None, None, failure, Vec::new()),
        }
    };
    let (handover, eof_limit) = handover;
    let previous = handover.server_instance_id;
    let follow = Follow {
        plan_id: handover.plan_id,
        plan: handover.plan,
        started,
        wait_instances: options.wait_instances,
    };
    if let Some(plan) = follow
        .plan
        .as_ref()
        .filter(|plan| plan.state == PlanState::Aborted)
    {
        return plan_ended(Some(previous), None, None, plan.clone());
    }
    if !handover.eof {
        let failure = CliFailure::daemon_wait_timeout("restart", eof_limit);
        let mut output = partial(Some(previous), None, failure, Vec::new());
        if let Some(result) = output.result.as_mut() {
            result.plan = follow.plan;
        }
        return output;
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
                return up(deps, previous, None, &follow).await;
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
            match overdue(deps, previous, l0, &follow).await {
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

/// No daemon runs, so a restart is a start.
async fn start_instead(deps: &mut Deps<'_>) -> CommandOutput<RestartResult> {
    let flow = start_flow(deps, "restart").await;
    let mut output = match flow.failure {
        Some(failure) => partial(None, flow.agent, failure, Vec::new()),
        None => {
            let mut result = RestartResult::new(None, flow.agent);
            result.info = flow.info;
            CommandOutput::ok(result)
        }
    };
    output.warnings = flow.warnings;
    output
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

/// The result of a plan that is final: the plan, its durations, and the
/// verdict of [`judge`].
fn plan_ended(
    previous: Option<Uuid>,
    agent: Option<AgentReport>,
    info: Option<DaemonInfo>,
    plan: RestartPlanView,
) -> CommandOutput<RestartResult> {
    let verdict = judge(&plan);
    let mut result = RestartResult::new(previous, agent);
    result.info = info;
    result.durations = Some(RestartDurations::of(&plan));
    result.plan = Some(plan);
    let mut output = match verdict.failure {
        Some(failure) => CommandOutput::partial(result, failure),
        None => CommandOutput::ok(result),
    };
    output.warnings = verdict.warnings;
    output
}

/// Reads the new daemon and, when a plan was written, follows it to its
/// end.
async fn up(
    deps: &mut Deps<'_>,
    previous: Uuid,
    agent: Option<AgentReport>,
    follow: &Follow,
) -> CommandOutput<RestartResult> {
    let info = match deps.probe.info().await {
        Ok(info) => info,
        Err(failure) => return partial(Some(previous), agent, failure, Vec::new()),
    };
    let Some(plan_id) = follow.plan_id else {
        let mut result = RestartResult::new(Some(previous), agent);
        result.info = Some(info);
        return CommandOutput::ok(result);
    };
    if !follow.wait_instances {
        let mut result = RestartResult::new(Some(previous), agent);
        result.plan = info
            .restart
            .clone()
            .map(|plan| *plan)
            .filter(|plan| plan.id == plan_id)
            .or_else(|| follow.plan.clone());
        result.info = Some(info);
        return CommandOutput::ok(result);
    }
    let remaining = DAEMON_RESTORE_WAIT.saturating_sub(follow.started.elapsed());
    let watched = tokio::select! {
        watched = deps.probe.watch_plan(plan_id, remaining) => watched,
        () = deps.interrupt.wait() => Err(CliFailure::interrupted_at("waiting_for_instances")),
    };
    match watched {
        Ok(watch) => match watch.plan {
            Some(plan) if watch.settled && plan_is_final(plan.state) => {
                plan_ended(Some(previous), agent, Some(info), plan)
            }
            last => {
                let failure = CliFailure::daemon_wait_timeout("restart", DAEMON_RESTORE_WAIT);
                let mut output = partial(Some(previous), agent, failure, Vec::new());
                if let Some(result) = output.result.as_mut() {
                    result.info = Some(info);
                    result.plan = last;
                }
                output
            }
        },
        Err(failure) => {
            let mut output = partial(Some(previous), agent, failure, Vec::new());
            if let Some(result) = output.result.as_mut() {
                result.info = Some(info);
            }
            output
        }
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
async fn overdue(
    deps: &mut Deps<'_>,
    previous: Uuid,
    l0: Option<jiff::Timestamp>,
    follow: &Follow,
) -> Overdue {
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
        return Overdue::Done(Box::new(up(deps, previous, Some(report), follow).await));
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
            up(deps, previous, Some(report), follow).await
        }
        Ok(WaitEnd::TimedOut) => partial(
            Some(previous),
            Some(report),
            CliFailure::daemon_wait_timeout("restart", DAEMON_START_WAIT),
            Vec::new(),
        ),
        Err(failure) => partial(Some(previous), Some(report), failure, Vec::new()),
    };
    output.warnings.extend(warnings);
    Overdue::Done(Box::new(output))
}

#[cfg(test)]
mod tests {
    use stackly_platform::{AgentOp, AgentRegistration};
    use stackly_protocol::ErrorBody;
    use stackly_protocol::codes::{cli, daemon};
    use stackly_protocol::daemon::ShutdownMode;
    use stackly_testkit::FakeBackgroundAgent;

    use stackly_protocol::Timestamp;
    use stackly_protocol::restart::{
        BlockingOperation, PlanCloseReason, PlanDecision, PlanOutcome, PlanReason,
        RestartPlanInstance, RestartPreview, RestartPreviewInstance,
    };

    use super::super::testing::{
        AfterInterrupt, Rig, ScriptedProbe, ScriptedPrompt, ScriptedRecords, empty_preview,
        missing, run,
    };
    use super::super::wait::{PlanWatch, Reachable, RestartHandover, Shutdown};
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
        restart_asking(rig, probe, interrupt, &mut ScriptedPrompt::nobody(), true).await
    }

    async fn restart_asking(
        rig: &Rig,
        probe: &ScriptedProbe,
        interrupt: &mut AfterInterrupt,
        prompt: &mut ScriptedPrompt,
        wait_instances: bool,
    ) -> CommandOutput<RestartResult> {
        let mut deps = Deps {
            agent: &rig.env,
            probe,
            interrupt,
        };
        let options = RestartOptions {
            force: false,
            wait_instances,
        };
        restart(&mut deps, options, prompt).await
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

    const PLAN: Uuid = Uuid::from_u128(30);

    fn at_ms(millis: i64) -> Timestamp {
        Timestamp::from_jiff(
            at(0)
                .checked_add(jiff::SignedDuration::from_millis(millis))
                .expect("in range"),
        )
    }

    fn preview(digest: char, decisions: &[PlanDecision]) -> RestartPreview {
        RestartPreview {
            digest: digest.to_string().repeat(64),
            instances: decisions
                .iter()
                .enumerate()
                .map(|(n, decision)| RestartPreviewInstance {
                    instance_id: Uuid::from_u128(n as u128 + 1),
                    name: format!("pg{n}"),
                    provider: "postgresql".to_owned(),
                    lifecycle: stackly_protocol::instance::InstanceLifecycle::Running,
                    decision: *decision,
                    reason: PlanReason::HandoverUnproven,
                })
                .collect(),
            blocking_operations: vec![BlockingOperation {
                operation_id: Uuid::from_u128(99),
                kind: stackly_protocol::operation::OperationKind::DaemonRestart,
                resumable: true,
            }],
            ..empty_preview()
        }
    }

    fn stopping() -> RestartPreview {
        preview('a', &[PlanDecision::Keep, PlanDecision::StopRestore])
    }

    fn plan_instance(
        name: &str,
        decision: PlanDecision,
        reason: PlanReason,
        outcome: PlanOutcome,
        failure_code: Option<&str>,
    ) -> RestartPlanInstance {
        RestartPlanInstance {
            instance_id: Uuid::now_v7(),
            name: name.to_owned(),
            decision,
            reason,
            stopped_at: None,
            ready_at: None,
            outcome: Some(outcome),
            failure_code: failure_code.map(str::to_owned),
        }
    }

    fn plan(state: PlanState, instances: Vec<RestartPlanInstance>) -> RestartPlanView {
        RestartPlanView {
            id: PLAN,
            state,
            created_at: at_ms(0),
            handed_over_at: Some(at_ms(2_000)),
            reconciled_at: Some(at_ms(3_000)),
            finished_at: Some(at_ms(5_500)),
            close_reason: None,
            instances,
        }
    }

    fn finished() -> RestartPlanView {
        plan(
            PlanState::Finished,
            vec![
                plan_instance(
                    "pg0",
                    PlanDecision::Keep,
                    PlanReason::HandoverProven,
                    PlanOutcome::Adopted,
                    None,
                ),
                plan_instance(
                    "pg1",
                    PlanDecision::StopRestore,
                    PlanReason::HandoverUnproven,
                    PlanOutcome::Restored,
                    None,
                ),
            ],
        )
    }

    fn handed_over(plan: Option<RestartPlanView>) -> Result<RestartHandover, CliFailure> {
        Ok(RestartHandover {
            server_instance_id: run(1),
            plan_id: Some(PLAN),
            plan,
            eof: true,
        })
    }

    fn watched(plan: RestartPlanView) -> Result<PlanWatch, CliFailure> {
        Ok(PlanWatch {
            plan: Some(plan),
            settled: true,
        })
    }

    /// A probe for a planned restart: the new run answers once the old one
    /// was asked to stop.
    fn planned(preview: RestartPreview) -> ScriptedProbe {
        ScriptedProbe::new(|n, _| {
            if n >= 1 {
                Ok(Reachable {
                    server_instance_id: run(2),
                })
            } else {
                Err(missing())
            }
        })
        .on_preview(Ok(preview))
        .on_restart(handed_over(None))
    }

    #[tokio::test(start_paused = true)]
    async fn a_plan_that_stops_instances_without_a_terminal_or_yes_exits_2_before_any_request() {
        let rig = rig_with_l0(100);
        let probe = planned(stopping());

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::nobody(),
            true,
        )
        .await;

        let failure = output.failure.expect("refused");
        assert!(failure.body.is(&cli::CONFIRMATION_REQUIRED));
        assert_eq!(failure.exit_code(), 2);
        assert!(probe.requests().is_empty(), "nothing reached the daemon");
    }

    #[tokio::test(start_paused = true)]
    async fn a_no_at_the_prompt_exits_9_and_sends_nothing() {
        let rig = rig_with_l0(100);
        let probe = planned(stopping());
        let mut prompt = ScriptedPrompt::answering(&[false]);

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut prompt,
            true,
        )
        .await;

        assert_eq!(output.failure.expect("declined").exit_code(), 9);
        assert!(probe.requests().is_empty());
        assert_eq!(prompt.asked, [false]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_plan_that_keeps_everything_asks_nothing() {
        let rig = rig_with_l0(100);
        let probe = planned(preview('b', &[PlanDecision::Keep])).on_watch(watched(finished()));
        let mut prompt = ScriptedPrompt::nobody();

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut prompt,
            true,
        )
        .await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        assert!(prompt.asked.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_request_carries_the_digest_of_the_confirmed_plan_and_a_key() {
        let rig = rig_with_l0(100);
        let probe = planned(stopping()).on_watch(watched(finished()));
        let mut prompt = ScriptedPrompt::answering(&[true]);

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut prompt,
            true,
        )
        .await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        let requests = probe.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].expected_plan_digest, Some("a".repeat(64)));
        assert!(!requests[0].force);
        assert!(!requests[0].idempotency_key.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_finished_plan_reports_every_instance_and_the_three_durations() {
        let rig = rig_with_l0(100);
        let probe = planned(stopping()).on_watch(watched(finished()));

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::yes(false),
            true,
        )
        .await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        let result = output.result.expect("a result");
        assert_eq!(result.previous_server_instance_id, Some(run(1)));
        assert_eq!(result.plan.expect("a plan").instances.len(), 2);
        let durations = result.durations.expect("durations");
        assert_eq!(durations.stopping, Some(2_000));
        assert_eq!(durations.daemon_gap, Some(1_000));
        assert_eq!(durations.restoring, Some(2_500));
        assert_eq!(
            result.info.map(|info| info.server_instance_id),
            Some(run(2))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_changed_plan_on_a_terminal_shows_the_new_plan_and_asks_once_more() {
        let rig = rig_with_l0(100);
        let changed = CliFailure::daemon(
            ErrorBody::new(&daemon::RESTART_PLAN_CHANGED, "the plan changed"),
            None,
        );
        let probe = ScriptedProbe::new(|n, _| {
            if n >= 1 {
                Ok(Reachable {
                    server_instance_id: run(2),
                })
            } else {
                Err(missing())
            }
        })
        .on_preview(Ok(stopping()))
        .on_preview(Ok(preview('c', &[PlanDecision::StopRestore])))
        .on_restart(Err(changed))
        .on_restart(handed_over(None))
        .on_watch(watched(finished()));
        let mut prompt = ScriptedPrompt::answering(&[true, true]);

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut prompt,
            true,
        )
        .await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        assert_eq!(prompt.asked, [false, true]);
        let requests = probe.requests();
        assert_eq!(requests[1].expected_plan_digest, Some("c".repeat(64)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_changed_plan_without_a_terminal_exits_3_and_does_not_retry() {
        let rig = rig_with_l0(100);
        let changed = CliFailure::daemon(
            ErrorBody::new(&daemon::RESTART_PLAN_CHANGED, "the plan changed"),
            None,
        );
        let probe = ScriptedProbe::running(run(1))
            .on_preview(Ok(stopping()))
            .on_restart(Err(changed));

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::yes(false),
            true,
        )
        .await;

        let failure = output.failure.expect("refused");
        assert!(failure.body.is(&daemon::RESTART_PLAN_CHANGED));
        assert_eq!(failure.exit_code(), 3);
        assert_eq!(probe.requests().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_plan_that_changes_twice_exits_3() {
        let rig = rig_with_l0(100);
        let changed = || {
            CliFailure::daemon(
                ErrorBody::new(&daemon::RESTART_PLAN_CHANGED, "the plan changed"),
                None,
            )
        };
        let probe = ScriptedProbe::running(run(1))
            .on_preview(Ok(stopping()))
            .on_preview(Ok(preview('c', &[PlanDecision::StopRestore])))
            .on_restart(Err(changed()))
            .on_restart(Err(changed()));
        let mut prompt = ScriptedPrompt::answering(&[true, true]);

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut prompt,
            true,
        )
        .await;

        assert_eq!(output.failure.expect("refused").exit_code(), 3);
        assert_eq!(probe.requests().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn an_aborted_plan_exits_7_and_the_old_daemon_keeps_running() {
        let rig = rig_with_l0(100);
        let mut aborted = plan(PlanState::Aborted, Vec::new());
        aborted.close_reason = Some(PlanCloseReason::StopTimeout);
        let probe = ScriptedProbe::running(run(1))
            .on_preview(Ok(stopping()))
            .on_restart(Ok(RestartHandover {
                server_instance_id: run(1),
                plan_id: Some(PLAN),
                plan: Some(aborted),
                eof: false,
            }));

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::yes(false),
            true,
        )
        .await;

        let failure = output.failure.expect("aborted");
        assert!(failure.body.is(&cli::RESTART_ABORTED));
        assert_eq!(failure.exit_code(), 7);
        assert_eq!(failure.body.detail("closeReason"), Some("stop_timeout"));
        let result = output.result.expect("a partial result");
        assert_eq!(result.plan.map(|plan| plan.state), Some(PlanState::Aborted));
        assert!(result.info.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn an_instance_that_failed_to_come_back_exits_7_with_the_full_plan() {
        let rig = rig_with_l0(100);
        let failed = plan(
            PlanState::Finished,
            vec![
                plan_instance(
                    "pg0",
                    PlanDecision::Keep,
                    PlanReason::HandoverProven,
                    PlanOutcome::Adopted,
                    None,
                ),
                plan_instance(
                    "pg1",
                    PlanDecision::StopRestore,
                    PlanReason::HandoverUnproven,
                    PlanOutcome::Failed,
                    Some("endpoint_in_use"),
                ),
            ],
        );
        let probe = planned(stopping()).on_watch(watched(failed));

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::yes(false),
            true,
        )
        .await;

        let failure = output.failure.expect("failed");
        assert!(failure.body.is(&cli::RESTART_RESTORE_FAILED));
        assert_eq!(failure.exit_code(), 7);
        let result = output.result.expect("a partial result");
        assert_eq!(result.plan.expect("a plan").instances.len(), 2);
        assert!(result.durations.is_some());
        assert!(result.info.is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn an_instance_that_was_and_stays_unknown_is_a_warning_and_exit_0() {
        let rig = rig_with_l0(100);
        let unknown = plan(
            PlanState::Finished,
            vec![plan_instance(
                "odd",
                PlanDecision::Keep,
                PlanReason::IdentityUnproven,
                PlanOutcome::Failed,
                Some("identity_unproven"),
            )],
        );
        let probe = planned(preview('d', &[PlanDecision::Keep])).on_watch(watched(unknown));

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::nobody(),
            true,
        )
        .await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].code, warning::RESTART_INSTANCE_UNKNOWN);
    }

    #[tokio::test(start_paused = true)]
    async fn a_plan_that_is_still_open_at_the_limit_exits_8() {
        let rig = rig_with_l0(100);
        let open = plan(PlanState::Restoring, Vec::new());
        let probe = planned(stopping()).on_watch(Ok(PlanWatch {
            plan: Some(open),
            settled: false,
        }));

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::yes(false),
            true,
        )
        .await;

        let failure = output.failure.expect("timeout");
        assert!(failure.body.is(&cli::DAEMON_WAIT_TIMEOUT));
        assert_eq!(failure.exit_code(), 8);
        assert_eq!(failure.body.detail("seconds"), Some("600"));
        let result = output.result.expect("a partial result");
        assert_eq!(
            result.plan.map(|plan| plan.state),
            Some(PlanState::Restoring)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn no_wait_instances_stops_after_the_new_daemon_answers() {
        let rig = rig_with_l0(100);
        // Following the plan would hang; the flag must not start it.
        let probe = planned(stopping())
            .with_restart(plan(PlanState::Restoring, Vec::new()))
            .hang_on_watch();

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::yes(false),
            false,
        )
        .await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        let result = output.result.expect("a result");
        assert_eq!(
            result.plan.map(|plan| plan.state),
            Some(PlanState::Restoring)
        );
        assert!(result.durations.is_none());
        assert!(result.info.is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn ctrl_c_while_following_the_plan_exits_130() {
        let rig = rig_with_l0(100);
        let probe = planned(stopping()).hang_on_watch();
        let mut interrupt = AfterInterrupt::new(Some(Duration::from_secs(5)));

        let output = restart_asking(
            &rig,
            &probe,
            &mut interrupt,
            &mut ScriptedPrompt::yes(false),
            true,
        )
        .await;

        let failure = output.failure.expect("interrupted");
        assert_eq!(failure.exit_code(), 130);
        assert_eq!(failure.body.detail("state"), Some("waiting_for_instances"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_launchd_did_not_start_refuses_without_a_question() {
        let rig = rig_with_l0(2);
        let mut manual = stopping();
        manual.launch_context = LaunchContextKind::Manual;
        let refusal = CliFailure::daemon(
            ErrorBody::new(&daemon::RESTART_UNAVAILABLE, "not started by launchd")
                .with_detail("launchContext", "manual"),
            None,
        );
        let probe = ScriptedProbe::running(run(1))
            .on_preview(Ok(manual))
            .on_restart(Err(refusal));

        let output = restart_asking(
            &rig,
            &probe,
            &mut AfterInterrupt::never(),
            &mut ScriptedPrompt::nobody(),
            true,
        )
        .await;

        assert_eq!(output.failure.expect("refused").exit_code(), 5);
    }

    #[tokio::test(start_paused = true)]
    async fn force_reaches_the_request() {
        let rig = rig_with_l0(100);
        let probe = planned(preview('e', &[PlanDecision::Keep])).on_watch(watched(finished()));
        let mut deps = Deps {
            agent: &rig.env,
            probe: &probe,
            interrupt: &mut AfterInterrupt::never(),
        };
        let options = RestartOptions {
            force: true,
            wait_instances: true,
        };

        let output = restart(&mut deps, options, &mut ScriptedPrompt::nobody()).await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        assert!(probe.requests()[0].force);
    }
}
