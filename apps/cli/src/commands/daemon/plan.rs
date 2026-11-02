//! The restart plan as `stackly daemon restart` and `daemon status` show it
//! (spec 0030, AC-15): the preview before the restart, the result per
//! instance after it, the three durations, and the questions the person
//! answers in between.

use serde::Serialize;
use stackly_protocol::restart::{
    PlanCloseReason, PlanDecision, PlanOutcome, PlanReason, PlanState, RestartPlanInstance,
    RestartPlanView, RestartPreview,
};

use crate::confirm::{Approval, Confirm, ConfirmRequest, ConfirmRole, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::OutputMode;
use crate::output::envelope::{Warning, warning};
use crate::output::human::{Cell, HumanWriter, wire_name};
use crate::output::style::Tone;

/// The failure code a `keep` instance carries when its identity could not
/// be proven, before and after the restart.
const IDENTITY_UNPROVEN: &str = "identity_unproven";

/// How long each part of a planned restart took, in milliseconds. A part is
/// null while the plan has not reached its end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartDurations {
    /// The old daemon stopping the `stop_restore` instances: `handedOverAt`
    /// minus `createdAt`.
    pub stopping: Option<u64>,
    /// No daemon ran: `reconciledAt` minus `handedOverAt`.
    pub daemon_gap: Option<u64>,
    /// The new daemon adopting and starting instances again: `finishedAt`
    /// minus `reconciledAt`.
    pub restoring: Option<u64>,
}

impl RestartDurations {
    /// The durations of `plan`.
    #[must_use]
    pub fn of(plan: &RestartPlanView) -> Self {
        let between = |from: stackly_protocol::Timestamp,
                       to: Option<stackly_protocol::Timestamp>| {
            let to = to?;
            u64::try_from(to.as_millisecond() - from.as_millisecond()).ok()
        };
        Self {
            stopping: plan
                .handed_over_at
                .and_then(|end| between(plan.created_at, Some(end))),
            daemon_gap: plan
                .handed_over_at
                .and_then(|start| between(start, plan.reconciled_at)),
            restoring: plan
                .reconciled_at
                .and_then(|start| between(start, plan.finished_at)),
        }
    }
}

/// How the end of a plan maps to the command's result.
#[derive(Debug)]
pub struct Verdict {
    /// The failure, when the plan did not end well.
    pub failure: Option<CliFailure>,
    /// A warning per instance that was `unknown` before and still is.
    pub warnings: Vec<Warning>,
}

/// Whether `instance` stayed `unknown`: the preview could not prove its
/// identity and the new daemon could not either.
fn stayed_unknown(instance: &RestartPlanInstance) -> bool {
    instance.reason == PlanReason::IdentityUnproven
        && instance.failure_code.as_deref() == Some(IDENTITY_UNPROVEN)
}

/// Judges a final `plan` (AC-15): `finished` is ok unless an instance
/// failed (exit 7), where an instance that stayed `unknown` is only a
/// warning; `aborted` is `cli.restart_aborted`; `expired` means nothing
/// came back.
#[must_use]
pub fn judge(plan: &RestartPlanView) -> Verdict {
    let mut warnings = Vec::new();
    let mut failed = false;
    for instance in &plan.instances {
        if instance.outcome != Some(PlanOutcome::Failed) {
            continue;
        }
        if stayed_unknown(instance) {
            warnings.push(Warning::new(
                warning::RESTART_INSTANCE_UNKNOWN,
                format!(
                    "{} was unknown before the restart and still is, so stackly never touched its process",
                    instance.name
                ),
            ));
        } else {
            failed = true;
        }
    }
    let failure = match plan.state {
        PlanState::Aborted => Some(CliFailure::restart_aborted(
            plan.id,
            &plan.close_reason.map_or_else(
                || "unknown".to_owned(),
                |reason: PlanCloseReason| wire_name(&reason),
            ),
        )),
        PlanState::Expired => Some(CliFailure::restart_restore_failed(plan.id)),
        _ if failed => Some(CliFailure::restart_restore_failed(plan.id)),
        _ => None,
    };
    Verdict { failure, warnings }
}

/// Whether `preview` has an instance that is stopped for a moment.
#[must_use]
pub fn stops_instances(preview: &RestartPreview) -> bool {
    preview
        .instances
        .iter()
        .any(|instance| instance.decision == PlanDecision::StopRestore)
}

/// The lines that tell the person what a restart does, before it runs.
#[must_use]
pub fn preview_lines(preview: &RestartPreview) -> Vec<String> {
    let keep = preview
        .instances
        .iter()
        .filter(|instance| instance.decision == PlanDecision::Keep)
        .count();
    let stop = preview.instances.len() - keep;
    let mut lines = vec![format!(
        "restarting stacklyd keeps {keep} {} running and stops {stop} for a moment",
        if keep == 1 { "instance" } else { "instances" }
    )];
    for instance in &preview.instances {
        let action = match instance.decision {
            PlanDecision::Keep => "keeps running",
            PlanDecision::StopRestore => "stops, then starts again",
            PlanDecision::Unrecognized => "unknown",
        };
        lines.push(format!(
            "  {} ({}): {action}, {}",
            instance.name,
            instance.provider,
            reason_text(instance.reason)
        ));
    }
    for operation in &preview.blocking_operations {
        if !operation.resumable {
            lines.push(format!(
                "  operation {} ({}) must finish first; --force restarts anyway",
                operation.operation_id,
                wire_name(&operation.kind)
            ));
        }
    }
    lines
}

fn reason_text(reason: PlanReason) -> &'static str {
    match reason {
        PlanReason::HandoverProven => "its process survives a restart",
        PlanReason::HandoverUnproven => "its process is not proven to survive a restart",
        PlanReason::RespawnPending => "it waits for a respawn",
        PlanReason::IdentityUnproven => "its identity is unproven, so it is never touched",
        PlanReason::SupervisorUnavailable => "the registry is in recovery mode",
        PlanReason::Unrecognized => "for a reason this stackly does not know",
    }
}

/// The questions of a restart, asked of the person at the terminal.
pub trait PlanPrompt: Send {
    /// Whether a person can answer: a terminal, and not `--json`.
    fn can_ask(&self) -> bool;

    /// Tells the person what the restart does.
    fn show(&mut self, preview: &RestartPreview);

    /// Asks to go on. `again` is true for the plan that replaced the one
    /// that was confirmed, which `--yes` does not cover.
    ///
    /// # Errors
    ///
    /// `cli.confirmation_required` when nobody can answer and `--yes` was
    /// not given, and `cli.confirmation_declined` for a no.
    fn confirm(&mut self, again: bool) -> Result<(), CliFailure>;
}

/// The live [`PlanPrompt`]: stderr and stdin of the terminal.
pub struct TerminalPrompt<'a> {
    cx: &'a mut Context,
    approval: Approval,
}

impl<'a> TerminalPrompt<'a> {
    /// A prompt that asks through `cx`, unless `approval` was given.
    pub fn new(cx: &'a mut Context, approval: Approval) -> Self {
        Self { cx, approval }
    }
}

impl PlanPrompt for TerminalPrompt<'_> {
    fn can_ask(&self) -> bool {
        self.cx.prompt_allowed() && self.cx.output.mode == OutputMode::Human
    }

    fn show(&mut self, preview: &RestartPreview) {
        if self.cx.output.mode == OutputMode::Json || self.cx.output.quiet {
            return;
        }
        for line in preview_lines(preview) {
            self.cx.output.stderr.line(&line);
        }
    }

    fn confirm(&mut self, again: bool) -> Result<(), CliFailure> {
        if !again && self.approval.given {
            return Ok(());
        }
        if !self.can_ask() {
            return Err(CliFailure::confirmation_required(self.approval.flag));
        }
        let request = ConfirmRequest {
            role: ConfirmRole::Standard,
            question: if again {
                "the plan changed. restart stacklyd with this plan?"
            } else {
                "restart stacklyd?"
            },
            consequence: "the instances that stop are started again by the new daemon",
        };
        let approval = Approval {
            given: false,
            flag: self.approval.flag,
        };
        Confirm::ask(self.cx, request, approval, OnNoninteractive::Fail).map(|_| ())
    }
}

/// The text rows of a plan, shared by `restart` and `status`.
pub fn render_plan(
    plan: &RestartPlanView,
    durations: Option<&RestartDurations>,
    out: &mut HumanWriter,
) {
    let state = wire_name(&plan.state);
    let tone = match plan.state {
        PlanState::Finished => Tone::Positive,
        PlanState::Aborted | PlanState::Expired => Tone::Critical,
        _ => Tone::Caution,
    };
    let mut head = vec![("Plan", Cell::toned(&state, tone))];
    if let Some(reason) = plan.close_reason {
        head.push(("Closed", Cell::fixed(wire_name(&reason))));
    }
    if let Some(durations) = durations {
        let text = [
            ("stopping", durations.stopping),
            ("daemon gap", durations.daemon_gap),
            ("restoring", durations.restoring),
        ]
        .iter()
        .filter_map(|(label, millis)| millis.map(|millis| format!("{label} {}", seconds(millis))))
        .collect::<Vec<_>>()
        .join(", ");
        if !text.is_empty() {
            head.push(("Took", Cell::fixed(text)));
        }
    }
    out.kv(&head);
    if plan.instances.is_empty() {
        return;
    }
    let rows: Vec<Vec<Cell>> = plan
        .instances
        .iter()
        .map(|instance| {
            let outcome = instance.outcome.map_or_else(
                || Cell::fixed("pending"),
                |outcome| {
                    let tone = match outcome {
                        PlanOutcome::Adopted | PlanOutcome::Restored => Tone::Positive,
                        PlanOutcome::Failed => Tone::Critical,
                        _ => Tone::Neutral,
                    };
                    let mut text = wire_name(&outcome);
                    if let Some(code) = &instance.failure_code {
                        text.push_str(&format!(" ({code})"));
                    }
                    Cell::toned(&text, tone)
                },
            );
            vec![
                Cell::text(&instance.name),
                Cell::fixed(wire_name(&instance.decision)),
                outcome,
            ]
        })
        .collect();
    out.table(&["INSTANCE", "DECISION", "RESULT"], &rows);
}

/// One line about `plan` for `daemon status`, such as
/// `finished 3m ago: 3 adopted, 1 restored`.
#[must_use]
pub fn summary(plan: &RestartPlanView, out: &HumanWriter) -> String {
    let count = |wanted: PlanOutcome| {
        plan.instances
            .iter()
            .filter(|instance| instance.outcome == Some(wanted))
            .count()
    };
    let mut parts = Vec::new();
    for (outcome, label) in [
        (PlanOutcome::Adopted, "adopted"),
        (PlanOutcome::Restored, "restored"),
        (PlanOutcome::Failed, "failed"),
        (PlanOutcome::Skipped, "skipped"),
    ] {
        let n = count(outcome);
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    let when = plan.finished_at.unwrap_or(plan.created_at);
    let mut text = format!("{} {}", wire_name(&plan.state), out.age(when));
    if let Some(reason) = plan.close_reason {
        text.push_str(&format!(" ({})", wire_name(&reason)));
    }
    if !parts.is_empty() {
        text.push_str(&format!(": {}", parts.join(", ")));
    }
    text
}

/// `millis` as seconds with one decimal, such as `2.4s`.
fn seconds(millis: u64) -> String {
    format!("{}.{}s", millis / 1000, millis % 1000 / 100)
}

#[cfg(test)]
mod tests {
    use stackly_protocol::Timestamp;
    use uuid::Uuid;

    use super::*;

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_jiff(
            jiff::Timestamp::from_millisecond(1_700_000_000_000 + millis).expect("in range"),
        )
    }

    pub(super) fn instance(
        name: &str,
        decision: PlanDecision,
        reason: PlanReason,
        outcome: Option<PlanOutcome>,
        failure_code: Option<&str>,
    ) -> RestartPlanInstance {
        RestartPlanInstance {
            instance_id: Uuid::now_v7(),
            name: name.to_owned(),
            decision,
            reason,
            stopped_at: None,
            ready_at: None,
            outcome,
            failure_code: failure_code.map(str::to_owned),
        }
    }

    fn plan(state: PlanState, instances: Vec<RestartPlanInstance>) -> RestartPlanView {
        RestartPlanView {
            id: Uuid::from_u128(30),
            state,
            created_at: at(0),
            handed_over_at: Some(at(2_100)),
            reconciled_at: Some(at(3_500)),
            finished_at: Some(at(6_500)),
            close_reason: None,
            instances,
        }
    }

    #[test]
    fn the_durations_are_the_gaps_between_the_plan_times() {
        let durations = RestartDurations::of(&plan(PlanState::Finished, Vec::new()));

        assert_eq!(durations.stopping, Some(2_100));
        assert_eq!(durations.daemon_gap, Some(1_400));
        assert_eq!(durations.restoring, Some(3_000));
    }

    #[test]
    fn a_plan_that_did_not_get_far_has_null_durations_after_its_last_time() {
        let mut open = plan(PlanState::HandedOver, Vec::new());
        open.reconciled_at = None;
        open.finished_at = None;

        let durations = RestartDurations::of(&open);

        assert_eq!(durations.stopping, Some(2_100));
        assert_eq!((durations.daemon_gap, durations.restoring), (None, None));
    }

    #[test]
    fn a_finished_plan_without_a_failure_is_ok() {
        let verdict = judge(&plan(
            PlanState::Finished,
            vec![
                instance(
                    "a",
                    PlanDecision::Keep,
                    PlanReason::HandoverProven,
                    Some(PlanOutcome::Adopted),
                    None,
                ),
                instance(
                    "b",
                    PlanDecision::StopRestore,
                    PlanReason::HandoverUnproven,
                    Some(PlanOutcome::Restored),
                    None,
                ),
            ],
        ));

        assert!(verdict.failure.is_none());
        assert!(verdict.warnings.is_empty());
    }

    #[test]
    fn one_failed_restore_is_exit_7() {
        let verdict = judge(&plan(
            PlanState::Finished,
            vec![instance(
                "b",
                PlanDecision::StopRestore,
                PlanReason::HandoverUnproven,
                Some(PlanOutcome::Failed),
                Some("endpoint_in_use"),
            )],
        ));

        let failure = verdict.failure.expect("a failure");
        assert_eq!(failure.exit_code(), 7);
    }

    #[test]
    fn an_instance_that_stays_unknown_is_only_a_warning() {
        let verdict = judge(&plan(
            PlanState::Finished,
            vec![instance(
                "odd",
                PlanDecision::Keep,
                PlanReason::IdentityUnproven,
                Some(PlanOutcome::Failed),
                Some("identity_unproven"),
            )],
        ));

        assert!(verdict.failure.is_none());
        assert_eq!(verdict.warnings.len(), 1);
        assert_eq!(verdict.warnings[0].code, warning::RESTART_INSTANCE_UNKNOWN);
    }

    #[test]
    fn a_keep_instance_that_failed_for_another_reason_is_exit_7() {
        let verdict = judge(&plan(
            PlanState::Finished,
            vec![instance(
                "a",
                PlanDecision::Keep,
                PlanReason::HandoverProven,
                Some(PlanOutcome::Failed),
                Some("identity_unproven"),
            )],
        ));

        assert_eq!(verdict.failure.map(|failure| failure.exit_code()), Some(7));
    }

    #[test]
    fn an_aborted_plan_is_cli_restart_aborted_with_its_close_reason() {
        let mut aborted = plan(PlanState::Aborted, Vec::new());
        aborted.close_reason = Some(PlanCloseReason::StopTimeout);

        let failure = judge(&aborted).failure.expect("a failure");

        assert_eq!(failure.exit_code(), 7);
        assert_eq!(failure.body.detail("closeReason"), Some("stop_timeout"));
    }
}
