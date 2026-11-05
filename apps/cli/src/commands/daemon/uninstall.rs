//! `stackly daemon uninstall` (spec 0031, AC-20, *Matikan agent*): shows what
//! turning the background agent off does, asks, has the daemon stop the
//! instances it can with `daemon.shutdown { mode: agent_off }`, and
//! unregisters the agent only when the daemon says it exits. A daemon that
//! stays up leaves the agent registered and the command ends with a
//! `conflict` that lists what could not be stopped.

use serde::Serialize;
use stackly_platform::BackgroundAgentState;
use stackly_protocol::stop_all::{StopAllEntry, StopAllResult};
use tokio::time::Instant;
use uuid::Uuid;

use super::Deps;
use super::agent::AgentReport;
use super::status::render_agent_rows;
use super::wait::{
    AgentOffPreview, DAEMON_POLL_INTERVAL, DAEMON_STOP_WAIT, daemon_absent, sleep_or_interrupt,
};
use crate::confirm::{Approval, Confirm, ConfirmRequest, ConfirmRole, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::OutputMode;
use crate::output::escape::escape_display;
use crate::output::human::{Cell, HumanWriter, Render};

/// The `daemon uninstall` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallResult {
    /// Whether the daemon said it exits. False when it stayed up or was not
    /// asked.
    pub exiting: bool,
    /// What the daemon stopped, failed on, and skipped; null when it was not
    /// asked.
    pub stop_all: Option<StopAllResult>,
    /// Whether the agent was registered when the command started.
    pub was_registered: bool,
    /// The agent after the command.
    pub agent: AgentReport,
}

impl Render for UninstallResult {
    fn render(&self, out: &mut HumanWriter) {
        let mut rows = Vec::new();
        if let Some(stop_all) = &self.stop_all {
            rows.push((
                "Stopped",
                Cell::fixed(format!("{} instances", stop_all.stopped.len())),
            ));
        }
        rows.extend(render_agent_rows(&self.agent, None));
        out.kv(&rows);
    }
}

/// The questions of a turn off, asked of the person at the terminal.
pub trait UninstallPrompt: Send {
    /// Tells the person what turning the agent off does.
    fn show(&mut self, lines: &[String]);

    /// Asks to go on.
    ///
    /// # Errors
    ///
    /// `cli.confirmation_required` when nobody can answer and `--yes` was
    /// not given, and `cli.confirmation_declined` for a no.
    fn confirm(&mut self) -> Result<(), CliFailure>;
}

/// The live [`UninstallPrompt`]: stderr and stdin of the terminal.
pub struct TerminalUninstallPrompt<'a> {
    cx: &'a mut Context,
    approval: Approval,
}

impl<'a> TerminalUninstallPrompt<'a> {
    /// A prompt that asks through `cx`, unless `approval` was given.
    pub fn new(cx: &'a mut Context, approval: Approval) -> Self {
        Self { cx, approval }
    }
}

impl UninstallPrompt for TerminalUninstallPrompt<'_> {
    fn show(&mut self, lines: &[String]) {
        if self.cx.output.mode == OutputMode::Json || self.cx.output.quiet {
            return;
        }
        for line in lines {
            self.cx.output.stderr.line(line);
        }
    }

    fn confirm(&mut self) -> Result<(), CliFailure> {
        let request = ConfirmRequest {
            role: ConfirmRole::Destructive,
            question: "turn off the background agent?",
            consequence: "the running instances stop, and instances set to start at login won’t start until you run stackly daemon install",
        };
        Confirm::ask(self.cx, request, self.approval, OnNoninteractive::Fail).map(|_| ())
    }
}

/// The lines that say what a turn off does (spec 0031, *Copy*).
#[must_use]
pub fn preview_lines(preview: &AgentOffPreview) -> Vec<String> {
    let stopping = preview.preview.items.len() + preview.preview.already_stopping.len();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let mut lines = vec![format!(
        "{} will stop. {} set to start at login won’t start until you turn it on again.",
        plural(stopping, "running instance", "running instances"),
        plural(preview.at_login, "instance", "instances"),
    )];
    for item in &preview.preview.items {
        lines.push(format!("  stops: {}", escape_display(&item.name)));
    }
    lines
}

/// A short reason for the error code an instance is left alone with.
fn reason(code: &str) -> &str {
    match code {
        "supervisor.stop_requires_override" => "an environment uses it",
        "supervisor.identity_unproven" => "it can’t be confirmed as running",
        "operation.recovery_required" => "an operation needs recovery",
        "supervisor.stop_timeout" => "it didn’t stop in time",
        "operation.cancelled" => "the stop was cancelled",
        other => other,
    }
}

fn blockers(entries: &[StopAllEntry], names: &dyn Fn(Uuid) -> String) -> Vec<String> {
    entries
        .iter()
        .map(|entry| format!("{} · {}", names(entry.instance_id), reason(&entry.code)))
        .collect()
}

/// Runs `daemon uninstall` with a fresh idempotency `key`.
pub async fn uninstall(
    deps: &mut Deps<'_>,
    prompt: &mut dyn UninstallPrompt,
    key: &str,
) -> CommandOutput<UninstallResult> {
    let agent = deps.agent;
    let (probe, assessment) = agent.status().await;
    if agent.root_override() {
        let result = UninstallResult {
            exiting: false,
            stop_all: None,
            was_registered: false,
            agent: agent.report(&assessment, false),
        };
        return CommandOutput::partial(result, CliFailure::agent_root_override());
    }
    let was_registered = !matches!(
        assessment.state,
        BackgroundAgentState::NotRegistered | BackgroundAgentState::NotFound
    );
    let finish = |exiting: bool, stop_all: Option<StopAllResult>, assessment: &_, up: bool| {
        UninstallResult {
            exiting,
            stop_all,
            was_registered,
            agent: agent.report(assessment, up),
        }
    };
    if assessment.state == BackgroundAgentState::NotRegistered {
        return CommandOutput::ok(finish(false, None, &assessment, false));
    }

    let preview = match deps.probe.agent_off_preview().await {
        Ok(preview) => preview,
        Err(failure) if daemon_absent(&failure) => {
            // With no daemon, launchd does not start one while the agent
            // waits for approval or was switched off, so unregistering is
            // safe. An enabled agent would start one: the person starts it
            // first, so the instances stop in order.
            return match assessment.state {
                BackgroundAgentState::RequiresApproval | BackgroundAgentState::Disabled => {
                    unregister_only(deps, finish).await
                }
                BackgroundAgentState::Enabled => {
                    CommandOutput::partial(finish(false, None, &assessment, false), failure)
                }
                _ => {
                    let failure = agent
                        .state_failure(&assessment, &probe)
                        .unwrap_or_else(CliFailure::not_connected);
                    CommandOutput::partial(finish(false, None, &assessment, false), failure)
                }
            };
        }
        Err(failure) => {
            return CommandOutput::partial(finish(false, None, &assessment, true), failure);
        }
    };
    let name_of = |id: Uuid| {
        preview
            .preview
            .items
            .iter()
            .find(|item| item.instance_id == id)
            .map(|item| item.name.clone())
            .or_else(|| {
                preview
                    .preview
                    .already_stopping
                    .iter()
                    .find(|item| item.instance_id == id)
                    .map(|item| item.name.clone())
            })
            .or_else(|| {
                preview
                    .preview
                    .skipped
                    .iter()
                    .find(|item| item.instance_id == id)
                    .map(|item| item.name.clone())
            })
            .unwrap_or_else(|| id.to_string())
    };
    // An instance the daemon must skip would keep it up, so nothing is sent.
    if !preview.preview.skipped.is_empty() {
        let list: Vec<String> = preview
            .preview
            .skipped
            .iter()
            .map(|skip| format!("{} · {}", skip.name, reason(&skip.code)))
            .collect();
        return CommandOutput::partial(
            finish(false, None, &assessment, true),
            CliFailure::agent_off_blocked(0, &list),
        );
    }
    prompt.show(&preview_lines(&preview));
    if let Err(failure) = prompt.confirm() {
        return CommandOutput::partial(finish(false, None, &assessment, true), failure);
    }

    let reply = match deps.probe.agent_off(key, DAEMON_STOP_WAIT).await {
        Ok(reply) => reply,
        Err(failure) => {
            return CommandOutput::partial(finish(false, None, &assessment, true), failure);
        }
    };
    let stop_all = reply.accepted.stop_all.clone();
    if !reply.accepted.exiting {
        let stopped = stop_all.as_ref().map_or(0, |result| result.stopped.len());
        let list = stop_all.as_ref().map_or_else(Vec::new, |result| {
            let mut list = blockers(&result.failed, &name_of);
            list.extend(blockers(&result.skipped, &name_of));
            list
        });
        return CommandOutput::partial(
            finish(false, stop_all, &assessment, true),
            CliFailure::agent_off_blocked(stopped, &list),
        );
    }
    if !reply.eof {
        return CommandOutput::partial(
            finish(true, stop_all, &assessment, true),
            CliFailure::daemon_wait_timeout("stop", DAEMON_STOP_WAIT),
        );
    }
    // The connection ended; the daemon is gone once nothing answers.
    let deadline = Instant::now() + DAEMON_STOP_WAIT;
    loop {
        match deps.probe.attempt().await {
            Err(failure) if daemon_absent(&failure) => break,
            Ok(_) | Err(_) => {}
        }
        let now = Instant::now();
        if now >= deadline {
            return CommandOutput::partial(
                finish(true, stop_all, &assessment, true),
                CliFailure::daemon_wait_timeout("stop", DAEMON_STOP_WAIT),
            );
        }
        if let Err(failure) =
            sleep_or_interrupt(deps.interrupt, DAEMON_POLL_INTERVAL.min(deadline - now)).await
        {
            return CommandOutput::partial(finish(true, stop_all, &assessment, true), failure);
        }
    }
    let acted = agent.unregister().await;
    let result = finish(true, stop_all, &acted.assessment, false);
    match acted.failure {
        Some(failure) => CommandOutput::partial(result, failure),
        None => CommandOutput::ok(result),
    }
}

/// Unregisters an agent that has no daemon behind it.
async fn unregister_only(
    deps: &mut Deps<'_>,
    finish: impl Fn(
        bool,
        Option<StopAllResult>,
        &stackly_platform::AgentAssessment,
        bool,
    ) -> UninstallResult,
) -> CommandOutput<UninstallResult> {
    let acted = deps.agent.unregister().await;
    let result = finish(false, None, &acted.assessment, false);
    match acted.failure {
        Some(failure) => CommandOutput::partial(result, failure),
        None => CommandOutput::ok(result),
    }
}

#[cfg(test)]
mod tests {
    use stackly_platform::{AgentOp, AgentRegistration};
    use stackly_protocol::Count;
    use stackly_protocol::daemon::{ShutdownAccepted, ShutdownMode};
    use stackly_protocol::instance::{AutostartSession, StartupPolicyKind};
    use stackly_protocol::instance::{InstanceAutostart, InstanceLifecycle};
    use stackly_protocol::stop_all::{InstanceRef, StopAllItem, StopAllPreview, StopAllSkip};

    use super::super::testing::{AfterInterrupt, Rig, ScriptedProbe, missing, run};
    use super::super::wait::{AgentOffReply, Reachable};
    use super::*;

    struct Prompt {
        answer: Result<(), ()>,
        shown: Vec<String>,
        asked: usize,
    }

    impl Prompt {
        fn yes() -> Self {
            Self {
                answer: Ok(()),
                shown: Vec::new(),
                asked: 0,
            }
        }

        fn no() -> Self {
            Self {
                answer: Err(()),
                ..Self::yes()
            }
        }
    }

    impl UninstallPrompt for Prompt {
        fn show(&mut self, lines: &[String]) {
            self.shown.extend_from_slice(lines);
        }

        fn confirm(&mut self) -> Result<(), CliFailure> {
            self.asked += 1;
            self.answer
                .map_err(|()| CliFailure::confirmation_declined())
        }
    }

    fn item(name: &str) -> StopAllItem {
        StopAllItem {
            instance_id: Uuid::from_u128(1),
            name: name.to_owned(),
            lifecycle: InstanceLifecycle::Running,
            demand: Vec::new(),
            autostart: InstanceAutostart {
                policy: StartupPolicyKind::Manual,
                session: AutostartSession::None,
            },
        }
    }

    fn preview(skipped: Vec<StopAllSkip>) -> AgentOffPreview {
        AgentOffPreview {
            preview: StopAllPreview {
                items: vec![item("orders-db")],
                already_stopping: vec![InstanceRef {
                    instance_id: Uuid::from_u128(2),
                    name: "cache".to_owned(),
                }],
                skipped,
                at_login_count: Count::saturating(0),
            },
            at_login: 2,
        }
    }

    fn reply(exiting: bool, result: StopAllResult) -> AgentOffReply {
        AgentOffReply {
            accepted: ShutdownAccepted {
                server_instance_id: run(1),
                mode: ShutdownMode::AgentOff,
                plan_id: None,
                exiting,
                stop_all: Some(result),
            },
            eof: exiting,
        }
    }

    fn stopped_one() -> StopAllResult {
        StopAllResult {
            stopped: vec![Uuid::from_u128(1)],
            failed: Vec::new(),
            skipped: Vec::new(),
        }
    }

    async fn uninstall_with(
        rig: &Rig,
        probe: &ScriptedProbe,
        prompt: &mut Prompt,
    ) -> CommandOutput<UninstallResult> {
        let mut interrupt = AfterInterrupt::never();
        let mut deps = Deps {
            agent: &rig.env,
            probe,
            interrupt: &mut interrupt,
        };
        uninstall(&mut deps, prompt, "key-1").await
    }

    /// A daemon that answers until the Turn Off reply, then is gone.
    fn goes_away(reply: AgentOffReply) -> ScriptedProbe {
        ScriptedProbe::new(|n, _| {
            if n >= 1 {
                Err(missing())
            } else {
                Ok(Reachable {
                    server_instance_id: run(1),
                })
            }
        })
        .on_agent_off_preview(Ok(preview(Vec::new())))
        .on_agent_off(Ok(reply))
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_exits_is_followed_by_the_unregister() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let probe = goes_away(reply(true, stopped_one()));
        let mut prompt = Prompt::yes();

        let output = uninstall_with(&rig, &probe, &mut prompt).await;

        assert!(output.failure.is_none(), "{:?}", output.failure);
        let result = output.result.expect("a result");
        assert!(result.exiting && result.was_registered);
        assert_eq!(result.agent.state, "not_registered");
        assert_eq!(
            rig.fake.calls(),
            [AgentOp::Status, AgentOp::Unregister, AgentOp::Status]
        );
        assert_eq!(probe.agent_off_keys(), ["key-1"]);
        assert_eq!(prompt.asked, 1);
        assert!(
            prompt.shown[0]
                .starts_with("2 running instances will stop. 2 instances set to start at login")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_stays_up_leaves_the_agent_registered_and_exits_with_a_conflict() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let held = StopAllEntry {
            instance_id: Uuid::from_u128(1),
            code: "supervisor.stop_timeout".to_owned(),
        };
        let probe = ScriptedProbe::running(run(1))
            .on_agent_off_preview(Ok(preview(Vec::new())))
            .on_agent_off(Ok(reply(
                false,
                StopAllResult {
                    stopped: Vec::new(),
                    failed: vec![held],
                    skipped: Vec::new(),
                },
            )));

        let output = uninstall_with(&rig, &probe, &mut Prompt::yes()).await;

        let failure = output.failure.expect("the turn off was blocked");
        assert_eq!(failure.body.code(), "cli.agent_off_blocked");
        assert!(
            failure
                .body
                .message()
                .contains("orders-db · it didn’t stop in time")
        );
        assert_eq!(rig.fake.count(AgentOp::Unregister), 0);
        let result = output.result.expect("a result");
        assert!(!result.exiting);
    }

    #[tokio::test(start_paused = true)]
    async fn an_instance_the_daemon_must_skip_stops_the_command_before_it_sends_anything() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let skip = StopAllSkip {
            instance_id: Uuid::from_u128(3),
            name: "held".to_owned(),
            code: "supervisor.stop_requires_override".to_owned(),
        };
        let probe = ScriptedProbe::running(run(1)).on_agent_off_preview(Ok(preview(vec![skip])));
        let mut prompt = Prompt::yes();

        let output = uninstall_with(&rig, &probe, &mut prompt).await;

        let failure = output.failure.expect("blocked");
        assert_eq!(failure.body.code(), "cli.agent_off_blocked");
        assert!(
            failure
                .body
                .message()
                .contains("held · an environment uses it")
        );
        assert!(probe.agent_off_keys().is_empty(), "nothing was sent");
        assert_eq!(prompt.asked, 0);
        assert_eq!(rig.fake.count(AgentOp::Unregister), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn declining_sends_nothing_and_unregisters_nothing() {
        let rig = Rig::plain();
        rig.fake.set_registration(AgentRegistration::Enabled);
        let probe = ScriptedProbe::running(run(1)).on_agent_off_preview(Ok(preview(Vec::new())));

        let output = uninstall_with(&rig, &probe, &mut Prompt::no()).await;

        assert_eq!(
            output.failure.expect("declined").body.code(),
            "cli.confirmation_declined"
        );
        assert!(probe.agent_off_keys().is_empty());
        assert_eq!(rig.fake.count(AgentOp::Unregister), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_daemon_an_enabled_agent_is_not_unregistered_and_a_waiting_one_is() {
        let enabled = Rig::plain();
        enabled.fake.set_registration(AgentRegistration::Enabled);
        let output =
            uninstall_with(&enabled, &ScriptedProbe::no_daemon(), &mut Prompt::yes()).await;
        assert!(
            output.failure.is_some(),
            "the person starts the daemon first"
        );
        assert_eq!(enabled.fake.count(AgentOp::Unregister), 0);

        let waiting = Rig::plain();
        waiting
            .fake
            .set_registration(AgentRegistration::RequiresApproval);
        let output =
            uninstall_with(&waiting, &ScriptedProbe::no_daemon(), &mut Prompt::yes()).await;
        assert!(output.failure.is_none(), "{:?}", output.failure);
        assert_eq!(waiting.fake.count(AgentOp::Unregister), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn an_agent_that_is_not_registered_has_nothing_to_turn_off() {
        let rig = Rig::plain();
        let probe = ScriptedProbe::running(run(1));

        let output = uninstall_with(&rig, &probe, &mut Prompt::yes()).await;

        assert!(output.failure.is_none());
        let result = output.result.expect("a result");
        assert!(!result.was_registered && !result.exiting);
        assert_eq!(rig.fake.calls(), [AgentOp::Status]);
    }

    #[test]
    fn the_preview_escapes_the_names_of_the_instances_it_stops() {
        let item: StopAllItem = serde_json::from_value(serde_json::json!({
            "instanceId": uuid::Uuid::nil(),
            "name": "db\u{1b}[2J",
            "lifecycle": "running",
            "demand": [],
            "autostart": {"policy": "manual", "session": "none"},
        }))
        .unwrap();
        let preview = AgentOffPreview {
            preview: StopAllPreview {
                items: vec![item],
                already_stopping: Vec::new(),
                skipped: Vec::new(),
                at_login_count: Count::saturating(0),
            },
            at_login: 0,
        };
        assert_eq!(preview_lines(&preview)[1], "  stops: db\\u{1b}[2J");
    }
}
