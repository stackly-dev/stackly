//! `stackly instance stop --all` (spec 0032, AC-9): reads the preview of
//! `instance.preview_stop_all`, prints it, asks, and sends `instance.stop_all`
//! with the `expectedInstanceIds` of that preview, so only what was shown can
//! stop. The copy follows the *Copy Stop All* table, the same words the menu
//! bar popover uses.

use serde::Serialize;
use stackly_protocol::codes::{self, ErrorSpec};
use stackly_protocol::stop_all::{
    DemandSourceKind, StopAllEntry, StopAllParams, StopAllPreview, StopAllResult,
};
use stackly_protocol::{Call, ErrorBody, Reply};
use uuid::Uuid;

use super::escaped;
use crate::args::IdempotencyKeyArg;
use crate::commands::RunCommand;
use crate::confirm::{Approval, Confirm, ConfirmRequest, ConfirmRole, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::human::{HumanWriter, Render};
use crate::output::{CommandOutput, OutputMode};
use crate::session::{CallSpec, RequestTimeout};

/// How many times `instance.stop_all` is sent with the same key when the
/// reply takes longer than the interactive limit; the daemon answers the
/// repeat from the operation that still runs (spec 0031, *Operasi
/// `stop_all_instances`* step 9).
const SENDS: usize = 3;

/// `stackly instance stop --all`.
#[derive(Debug)]
pub struct StopAll {
    /// `--yes`.
    pub yes: bool,
    /// `--idempotency-key`.
    pub idempotency_key: Option<IdempotencyKeyArg>,
}

/// The `result` of `stop --all`: the preview the person saw, and how the stop
/// ended. `outcome` is null when nothing was sent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopAllOutput {
    /// What would stop, as the daemon showed it.
    pub preview: StopAllPreview,
    /// What the daemon did; null when the person declined, the set changed,
    /// or nothing was running.
    pub outcome: Option<StopAllResult>,
}

/// The sentence for the code an instance is left alone with.
fn reason(code: &str) -> &str {
    match code {
        "supervisor.identity_unproven" => "Stackly can’t confirm this process",
        "supervisor.stop_requires_override" => "used by an environment",
        "operation.recovery_required" => "an operation for it needs your attention in Stackly",
        other => other,
    }
}

fn demand_text(source: DemandSourceKind) -> &'static str {
    match source {
        DemandSourceKind::Manual => "Started by you",
        DemandSourceKind::Startup => "Started at login",
        _ => "Used by an environment",
    }
}

fn at_login_sentence(count: u64) -> String {
    match count {
        0 => "None of them start at your next login.".to_owned(),
        1 => "1 of them starts again at your next login.".to_owned(),
        k => format!("{k} of them start again at your next login."),
    }
}

/// The footer of the preview: what happens to data and to the next login.
fn footer(preview: &StopAllPreview) -> String {
    format!(
        "Data is kept. {}",
        at_login_sentence(preview.at_login_count.get())
    )
}

/// The rows of the preview, one per instance.
fn preview_rows(preview: &StopAllPreview) -> Vec<String> {
    let mut rows = Vec::new();
    for item in &preview.items {
        let demand: Vec<&str> = item.demand.iter().copied().map(demand_text).collect();
        let name = escaped(&item.name);
        rows.push(if demand.is_empty() {
            name
        } else {
            format!("{name} · {}", demand.join(", "))
        });
    }
    for item in &preview.already_stopping {
        rows.push(format!("{} · already stopping", escaped(&item.name)));
    }
    for skip in &preview.skipped {
        rows.push(format!(
            "{} · won’t stop: {}",
            escaped(&skip.name),
            reason(&skip.code)
        ));
    }
    rows
}

fn plural(count: usize) -> String {
    if count == 1 {
        "1 service".to_owned()
    } else {
        format!("{count} services")
    }
}

/// The sentence that says how the stop ended (*Copy Stop All*, results).
fn outcome_text(preview: &StopAllPreview, outcome: &StopAllResult) -> String {
    let name_of = |id: Uuid| {
        preview
            .items
            .iter()
            .find(|item| item.instance_id == id)
            .map_or_else(|| id.to_string(), |item| escaped(&item.name))
    };
    let total = preview.items.len();
    let stopped = outcome.stopped.len();
    if outcome
        .skipped
        .iter()
        .any(|entry| entry.code == "operation.cancelled")
    {
        return format!("Stop All was cancelled. Stopped {stopped} of {total}.");
    }
    let mut text = match outcome.failed.as_slice() {
        [] => format!("Stopped {}. Data is kept.", plural(stopped)),
        [one] => format!(
            "Stopped {stopped} of {total}. {} didn’t stop.",
            name_of(one.instance_id)
        ),
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|entry| name_of(entry.instance_id))
                .collect();
            format!(
                "Stopped {stopped} of {total}. {} didn’t stop: {}.",
                many.len(),
                names.join(", ")
            )
        }
    };
    if !outcome.skipped.is_empty() {
        text.push_str(&format!(" {} skipped.", outcome.skipped.len()));
    }
    text
}

impl Render for StopAllOutput {
    fn render(&self, out: &mut HumanWriter) {
        match &self.outcome {
            Some(outcome) => out.line(&outcome_text(&self.preview, outcome)),
            None if self.preview.items.is_empty() => out.line("Nothing is running."),
            None => {}
        }
    }
}

/// A failure of the catalog code `code`, or an unexpected one when this build
/// does not know the code.
fn coded(code: &str, message: &str) -> CliFailure {
    let spec: &ErrorSpec = codes::lookup(code).unwrap_or(&codes::internal::UNEXPECTED);
    CliFailure::local(ErrorBody::new(spec, message))
}

/// The failure a finished stop leaves: the first instance that did not stop,
/// or the cancel, or none.
fn failure_of(preview: &StopAllPreview, outcome: &StopAllResult) -> Option<CliFailure> {
    if let Some(first) = outcome.failed.first() {
        return Some(coded(&first.code, &outcome_text(preview, outcome)));
    }
    outcome
        .skipped
        .iter()
        .find(|entry: &&StopAllEntry| entry.code == "operation.cancelled")
        .map(|entry| coded(&entry.code, &outcome_text(preview, outcome)))
}

/// Reads the preview.
async fn read_preview(cx: &mut Context) -> Result<StopAllPreview, CliFailure> {
    match cx
        .call(Call::InstancePreviewStopAll, CallSpec::control())
        .await?
    {
        Reply::InstancePreviewStopAll(preview) => Ok(*preview),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// Sends `instance.stop_all`, again with the same key when the reply is
/// late.
async fn send(
    cx: &mut Context,
    expected: Vec<Uuid>,
    key: &str,
) -> Result<StopAllResult, CliFailure> {
    let mut last = None;
    for _ in 0..SENDS {
        let call = Call::InstanceStopAll(StopAllParams {
            expected_instance_ids: Some(expected.clone()),
        });
        let spec = CallSpec {
            timeout: RequestTimeout::Interactive,
            idempotency_key: Some(key.to_owned()),
        };
        match cx.call(call, spec).await {
            Ok(Reply::InstanceStopAll(result)) => return Ok(*result),
            Ok(_) => return Err(CliFailure::unexpected_reply(None)),
            Err(failure) if failure.body.is(&codes::ipc::CALL_TIMEOUT) => last = Some(failure),
            Err(failure) => return Err(failure),
        }
    }
    Err(last.unwrap_or_else(|| CliFailure::unexpected_reply(None)))
}

impl RunCommand for StopAll {
    type Output = StopAllOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<StopAllOutput> {
        let preview = match read_preview(cx).await {
            Ok(preview) => preview,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let nothing_sent = |preview: StopAllPreview| StopAllOutput {
            preview,
            outcome: None,
        };
        if preview.items.is_empty() {
            return CommandOutput::ok(nothing_sent(preview));
        }
        if cx.output.mode != OutputMode::Json && !cx.output.quiet {
            for row in preview_rows(&preview) {
                cx.output.stderr.line(&format!("  {row}"));
            }
        }
        // With --json the noninteractive path holds even at a terminal, so
        // --yes is needed, as for `stop --force`.
        let approval = Approval {
            given: self.yes,
            flag: "--yes",
        };
        if !self.yes && cx.output.mode == OutputMode::Json {
            return CommandOutput::partial(
                nothing_sent(preview),
                CliFailure::confirmation_required(approval.flag),
            );
        }
        let consequence = footer(&preview);
        let request = ConfirmRequest {
            role: ConfirmRole::Standard,
            question: "Stop All Services?",
            consequence: &consequence,
        };
        if let Err(failure) = Confirm::ask(cx, request, approval, OnNoninteractive::Fail) {
            return CommandOutput::partial(nothing_sent(preview), failure);
        }
        let expected: Vec<Uuid> = preview.items.iter().map(|item| item.instance_id).collect();
        let key = self
            .idempotency_key
            .as_ref()
            .map_or_else(|| Uuid::now_v7().to_string(), |key| key.as_str().to_owned());
        match send(cx, expected, &key).await {
            Ok(outcome) => {
                let failure = failure_of(&preview, &outcome);
                let output = StopAllOutput {
                    preview,
                    outcome: Some(outcome),
                };
                match failure {
                    Some(failure) => CommandOutput::partial(output, failure),
                    None => CommandOutput::ok(output),
                }
            }
            Err(failure) if failure.body.is(&codes::instance::STOP_ALL_CHANGED) => {
                CommandOutput::partial(nothing_sent(preview), failure).with_hint(
                    "the running instances changed since the preview; run the command again to see the new list",
                )
            }
            Err(failure) => CommandOutput::partial(nothing_sent(preview), failure),
        }
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::Count;
    use stackly_protocol::instance::{
        AutostartSession, InstanceAutostart, InstanceLifecycle, StartupPolicyKind,
    };
    use stackly_protocol::stop_all::{InstanceRef, StopAllItem, StopAllSkip};

    use super::*;

    fn item(n: u128, name: &str, demand: Vec<DemandSourceKind>) -> StopAllItem {
        StopAllItem {
            instance_id: Uuid::from_u128(n),
            name: name.to_owned(),
            lifecycle: InstanceLifecycle::Running,
            demand,
            autostart: InstanceAutostart {
                policy: StartupPolicyKind::Manual,
                session: AutostartSession::None,
            },
        }
    }

    fn preview() -> StopAllPreview {
        StopAllPreview {
            items: vec![
                item(1, "alpha", vec![DemandSourceKind::Manual]),
                item(
                    2,
                    "bravo",
                    vec![DemandSourceKind::Manual, DemandSourceKind::Startup],
                ),
                item(3, "plain", Vec::new()),
            ],
            already_stopping: vec![InstanceRef {
                instance_id: Uuid::from_u128(4),
                name: "cache".to_owned(),
            }],
            skipped: vec![StopAllSkip {
                instance_id: Uuid::from_u128(5),
                name: "held".to_owned(),
                code: "supervisor.stop_requires_override".to_owned(),
            }],
            at_login_count: Count::saturating(2),
        }
    }

    fn entry(n: u128, code: &str) -> StopAllEntry {
        StopAllEntry {
            instance_id: Uuid::from_u128(n),
            code: code.to_owned(),
        }
    }

    fn result(
        stopped: &[u128],
        failed: Vec<StopAllEntry>,
        skipped: Vec<StopAllEntry>,
    ) -> StopAllResult {
        StopAllResult {
            stopped: stopped.iter().map(|n| Uuid::from_u128(*n)).collect(),
            failed,
            skipped,
        }
    }

    #[test]
    fn the_preview_rows_follow_the_copy_table() {
        assert_eq!(
            preview_rows(&preview()),
            [
                "alpha · Started by you",
                "bravo · Started by you, Started at login",
                "plain",
                "cache · already stopping",
                "held · won’t stop: used by an environment",
            ]
        );
    }

    #[test]
    fn the_footer_names_data_and_the_next_login_by_count() {
        let mut shown = preview();
        for (count, tail) in [
            (0, "None of them start at your next login."),
            (1, "1 of them starts again at your next login."),
            (3, "3 of them start again at your next login."),
        ] {
            shown.at_login_count = Count::saturating(count);
            assert_eq!(footer(&shown), format!("Data is kept. {tail}"));
        }
    }

    #[test]
    fn a_stop_that_finished_says_how_many_stopped() {
        let shown = preview();
        assert_eq!(
            outcome_text(&shown, &result(&[1, 2, 3], Vec::new(), Vec::new())),
            "Stopped 3 services. Data is kept."
        );
        assert_eq!(
            outcome_text(&shown, &result(&[1], Vec::new(), Vec::new())),
            "Stopped 1 service. Data is kept."
        );
    }

    #[test]
    fn a_failed_stop_names_the_instances_that_did_not_stop() {
        let shown = preview();
        let one = result(
            &[1, 3],
            vec![entry(2, "supervisor.stop_timeout")],
            Vec::new(),
        );
        assert_eq!(
            outcome_text(&shown, &one),
            "Stopped 2 of 3. bravo didn’t stop."
        );
        let two = result(
            &[1],
            vec![
                entry(2, "supervisor.stop_timeout"),
                entry(3, "supervisor.stop_timeout"),
            ],
            vec![entry(5, "supervisor.stop_requires_override")],
        );
        assert_eq!(
            outcome_text(&shown, &two),
            "Stopped 1 of 3. 2 didn’t stop: bravo, plain. 1 skipped."
        );
    }

    #[test]
    fn a_cancelled_stop_says_so() {
        let shown = preview();
        let cancelled = result(&[1], Vec::new(), vec![entry(2, "operation.cancelled")]);
        assert_eq!(
            outcome_text(&shown, &cancelled),
            "Stop All was cancelled. Stopped 1 of 3."
        );
        let failure = failure_of(&shown, &cancelled).expect("a cancel is a failure");
        assert_eq!(failure.body.code(), "operation.cancelled");
    }

    #[test]
    fn only_a_failed_stop_or_a_cancel_fails_the_command() {
        let shown = preview();
        assert!(failure_of(&shown, &result(&[1, 2, 3], Vec::new(), Vec::new())).is_none());
        assert!(
            failure_of(
                &shown,
                &result(
                    &[1],
                    Vec::new(),
                    vec![entry(5, "supervisor.stop_requires_override")]
                )
            )
            .is_none()
        );
        let failure = failure_of(
            &shown,
            &result(&[1], vec![entry(2, "supervisor.stop_timeout")], Vec::new()),
        )
        .expect("a failed stop fails the command");
        assert_eq!(failure.body.code(), "supervisor.stop_timeout");
    }

    #[test]
    fn nothing_running_is_said_and_a_declined_stop_says_nothing() {
        let render = |output: &StopAllOutput| {
            let mut out =
                HumanWriter::new(false, jiff::Timestamp::UNIX_EPOCH, jiff::tz::TimeZone::UTC);
            output.render(&mut out);
            out.into_string()
        };
        let empty = StopAllPreview {
            items: Vec::new(),
            already_stopping: Vec::new(),
            skipped: Vec::new(),
            at_login_count: Count::saturating(0),
        };
        let none = StopAllOutput {
            preview: empty,
            outcome: None,
        };
        assert_eq!(render(&none), "Nothing is running.\n");
        let declined = StopAllOutput {
            preview: preview(),
            outcome: None,
        };
        assert_eq!(render(&declined), "");
    }
}
