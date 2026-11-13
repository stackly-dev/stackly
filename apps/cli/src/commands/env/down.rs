//! `stackly env down [target] [--wait] [--timeout <duration>]
//! [--idempotency-key <key>]` (spec 0047, *CLI*, AC-18): reads
//! `env.preview_down`, writes what stops and what keeps running to stderr,
//! then starts the daemon's `environment_down` with `env.down` and the IDs
//! of that preview. It asks nothing, because Down deletes nothing: no data,
//! runtime, instance, or membership.
//!
//! Without `--wait` it prints the operation it started. With `--wait` it
//! writes one line per instance that ended, then the summary, and exits 0
//! only when the Down `succeeded`; `env.down_incomplete` exits 7. A refusal
//! with `env.down_changed` is retried once with a fresh preview, then
//! exits 3. A file target is first matched against the manifest locations
//! of `env.list`, so a missing or broken manifest still goes down; only a
//! file no environment is registered at is planned (spec 0046, *Target*).

use clap::Args;
use serde::Serialize;
use stackly_protocol::codes::env as env_codes;
use stackly_protocol::env::{
    DownInstance, DownKeep, EnvDownParams, EnvDownPreview, EnvGetParams, EnvPreviewDownParams,
    EnvironmentDetail,
};
use stackly_protocol::operation::{OperationDetail, OperationSummary};
use stackly_protocol::stop_all::DemandSourceKind;
use stackly_protocol::{Call, ErrorBody, Reply, ResourceKind};
use uuid::Uuid;

use super::status::planned_id;
use super::target::{Resolved, TargetArgs, list};
use crate::commands::RunCommand;
use crate::commands::provider::local::{esc, shows_text};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};
use crate::session::{CallSpec, RequestTimeout};
use crate::wait::{AsyncArgs, Submission, WaitArgs, WaitMode, Waiter, settle, settled};

/// `stackly env down`.
#[derive(Debug, Args)]
pub struct DownArgs {
    /// The manifest or the environment.
    #[command(flatten)]
    pub target: TargetArgs,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// The `env down` result: `{ operation, idempotencyKey, preview,
/// environment }`, `environment` filled after `--wait`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BroughtDown {
    /// The `environment_down` operation: as accepted, or with `--wait` as
    /// it stands at the end.
    pub operation: OperationSummary,
    /// The key the request carried.
    pub idempotency_key: String,
    /// The preview whose IDs `env.down` carried.
    pub preview: EnvDownPreview,
    /// The environment read once after the wait; null without `--wait`.
    pub environment: Option<EnvironmentDetail>,
}

impl Render for BroughtDown {
    fn render(&self, out: &mut HumanWriter) {
        if !settled(self.operation.state) {
            out.line(&format!(
                "Started bringing down {} (operation {})",
                esc(&self.preview.environment.name),
                self.operation.id
            ));
            return;
        }
        if self.operation.state == stackly_protocol::operation::OperationState::Succeeded {
            let (stopped, kept) = summary_counts(&self.preview, &self.operation);
            out.line(&format!("Down. {stopped} stopped, {kept} kept running."));
        }
    }
}

/// The reason an instance keeps running (spec 0047, *CLI*): "used by"
/// with the environments, "started by you", "starts at login", joined
/// with "; ".
#[must_use]
pub fn keep_reason(keep: &DownKeep) -> String {
    let mut reasons: Vec<String> = Vec::new();
    let mut push = |reason: String| {
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    };
    for source in &keep.demand {
        match source {
            DemandSourceKind::Environment if !keep.environments.is_empty() => {
                let names: Vec<String> = keep
                    .environments
                    .iter()
                    .map(|environment| esc(&environment.name))
                    .collect();
                push(format!("used by {}", names.join(", ")));
            }
            DemandSourceKind::Environment => push("used by an environment".to_owned()),
            DemandSourceKind::Manual => push("started by you".to_owned()),
            DemandSourceKind::Startup => push("starts at login".to_owned()),
            DemandSourceKind::Unrecognized => push("still needed".to_owned()),
        }
    }
    if reasons.is_empty() && !keep.environments.is_empty() {
        let names: Vec<String> = keep
            .environments
            .iter()
            .map(|environment| esc(&environment.name))
            .collect();
        reasons.push(format!("used by {}", names.join(", ")));
    }
    reasons.join("; ")
}

/// The sentence for the code Down can't stop an instance with.
fn blocked_reason(code: &str) -> String {
    match code {
        "supervisor.identity_unproven" => "Stackly can’t confirm this process".to_owned(),
        "operation.recovery_required" => "an operation for it needs your attention".to_owned(),
        other => esc(other),
    }
}

/// Whether Down has nothing at all to do: nothing stops, no lease is held,
/// and no Up is cancelled.
#[must_use]
pub fn is_empty(preview: &EnvDownPreview) -> bool {
    preview.stop.is_empty() && preview.lease_count.get() == 0 && !preview.cancels_up
}

/// The lines of a preview, written to stderr before `env.down`.
#[must_use]
pub fn preview_lines(preview: &EnvDownPreview) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for instance in &preview.stop {
        lines.push(format!("stop {}", esc(&instance.name)));
    }
    for keep in &preview.keep {
        lines.push(format!(
            "keep {}: {}",
            esc(&keep.instance.name),
            keep_reason(keep)
        ));
    }
    for blocked in &preview.blocked {
        lines.push(format!(
            "skip {}: {}",
            esc(&blocked.instance.name),
            blocked_reason(&blocked.code)
        ));
    }
    if preview.cancels_up {
        lines.push("cancels the running up".to_owned());
    }
    if is_empty(preview) {
        lines.push("Nothing to stop.".to_owned());
    }
    lines
}

/// Every instance the preview names.
fn instances(preview: &EnvDownPreview) -> impl Iterator<Item = &DownInstance> {
    preview
        .stop
        .iter()
        .chain(preview.keep.iter().map(|keep| &keep.instance))
        .chain(preview.idle.iter())
        .chain(preview.blocked.iter().map(|blocked| &blocked.instance))
}

/// The name of instance `id` in the preview, or the ID.
fn name_of(preview: &EnvDownPreview, id: &str) -> String {
    instances(preview)
        .find(|instance| instance.instance_id.to_string() == id)
        .map_or_else(|| esc(id), |instance| esc(&instance.name))
}

/// How many instances a finished Down stopped, and how many keep running:
/// those the preview kept plus those another demand kept since.
fn summary_counts(preview: &EnvDownPreview, operation: &OperationSummary) -> (usize, usize) {
    let Some(result) = &operation.result else {
        return (0, preview.keep.len());
    };
    let stopped = result
        .resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Instance)
        .count();
    let kept = result
        .partial
        .iter()
        .filter(|item| item.code.as_deref() == Some(env_codes::INSTANCE_IN_USE.code))
        .count();
    (stopped, preview.keep.len() + kept)
}

/// One line per instance the finished Down `operation` ended:
/// `stopped <instance>`, `kept <instance>`, `failed <instance>: <code>`,
/// `cancelled <instance>`, or `left running <instance>`.
#[must_use]
pub fn ended_lines(preview: &EnvDownPreview, operation: &OperationSummary) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(result) = &operation.result {
        for resource in &result.resources {
            if resource.kind == ResourceKind::Instance {
                lines.push(format!("stopped {}", name_of(preview, &resource.id)));
            }
        }
        for item in &result.partial {
            if item.code.as_deref() == Some(env_codes::INSTANCE_IN_USE.code) {
                let name = item.resource.as_ref().map_or_else(
                    || esc(&item.item),
                    |resource| name_of(preview, &resource.id),
                );
                lines.push(format!("kept {name}"));
            }
        }
    }
    if let Some(failure) = &operation.failure
        && failure.error.is(&env_codes::DOWN_INCOMPLETE)
    {
        lines.extend(incomplete_lines(preview, &failure.error));
    }
    lines
}

/// The lines of the `instance` items of `env.down_incomplete`.
fn incomplete_lines(preview: &EnvDownPreview, error: &ErrorBody) -> Vec<String> {
    error
        .items()
        .iter()
        .filter(|item| item.kind() == "instance")
        .map(|item| {
            let name = item.field("name").map_or_else(
                || {
                    item.field("instanceId")
                        .map_or_else(String::new, |id| name_of(preview, id))
                },
                esc,
            );
            match item.field("outcome") {
                Some("left_running") => format!("left running {name}"),
                Some("cancelled") => format!("cancelled {name}"),
                _ => match item.field("code") {
                    Some(code) => format!("failed {name}: {}", esc(code)),
                    None => format!("failed {name}"),
                },
            }
        })
        .collect()
}

/// The ID of the environment a target names. A file is matched against the
/// manifest locations of `env.list` first, so Down works on a manifest
/// that is missing or not valid (spec 0047 AC-2).
async fn environment_id(
    cx: &mut Context,
    target: &TargetArgs,
) -> Result<Uuid, Box<CommandOutput<BroughtDown>>> {
    match target.resolve(cx).await {
        Ok(Resolved::Registered(summary)) => Ok(summary.id),
        Ok(Resolved::File(file)) => {
            let registered = list(cx)
                .await
                .map_err(|failure| Box::new(CommandOutput::failed(failure)))?;
            if let Some(summary) = registered
                .items
                .into_iter()
                .find(|item| item.manifest_location.as_deref() == Some(file.as_str()))
            {
                return Ok(summary.id);
            }
            planned_id(cx, file, "down").await
        }
        Err(failure) => Err(Box::new(CommandOutput::failed(failure))),
    }
}

/// `env.preview_down` for `id`.
async fn preview(cx: &mut Context, id: Uuid) -> Result<EnvDownPreview, CliFailure> {
    match cx
        .call(
            Call::EnvPreviewDown(EnvPreviewDownParams { id }),
            CallSpec::control(),
        )
        .await?
    {
        Reply::EnvPreviewDown(preview) => Ok(*preview),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// `env.get` for `id`, without reading the manifest.
async fn read(cx: &mut Context, id: Uuid) -> Result<EnvironmentDetail, CliFailure> {
    let call = Call::EnvGet(EnvGetParams {
        id,
        check_manifest: false,
    });
    match cx.call(call, CallSpec::control()).await? {
        Reply::EnvGet(detail) => Ok(*detail),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// Writes the preview to stderr, unless the output is JSON or quiet.
fn show(cx: &mut Context, preview: &EnvDownPreview) {
    if shows_text(cx, false) {
        for line in preview_lines(preview) {
            cx.output.stderr.line(&line);
        }
    }
}

/// Sends `env.down` with the IDs of `preview`.
async fn send(
    cx: &mut Context,
    preview: &EnvDownPreview,
    key: &str,
) -> Result<stackly_protocol::env::EnvDownReply, CliFailure> {
    let call = Call::EnvDown(EnvDownParams {
        id: preview.environment.id,
        expected_stop_instance_ids: preview
            .stop
            .iter()
            .map(|instance| instance.instance_id)
            .collect(),
    });
    let spec = CallSpec {
        timeout: RequestTimeout::Control,
        idempotency_key: Some(key.to_owned()),
    };
    match cx.call(call, spec).await? {
        Reply::EnvDown(reply) => Ok(*reply),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

impl RunCommand for DownArgs {
    type Output = BroughtDown;

    async fn run(self, cx: &mut Context) -> CommandOutput<BroughtDown> {
        let id = match environment_id(cx, &self.target).await {
            Ok(id) => id,
            Err(output) => return *output,
        };
        let mut shown = match preview(cx, id).await {
            Ok(preview) => preview,
            Err(failure) => return CommandOutput::failed(failure),
        };
        show(cx, &shown);
        let key = self
            .run
            .idempotency_key
            .as_ref()
            .map_or_else(|| Uuid::now_v7().to_string(), |key| key.as_str().to_owned());
        let reply = match send(cx, &shown, &key).await {
            Ok(reply) => reply,
            Err(failure) if failure.body.is(&env_codes::DOWN_CHANGED) => {
                // What stops changed since the preview: show it again and
                // send once more with the new IDs (AC-18).
                shown = match preview(cx, id).await {
                    Ok(preview) => preview,
                    Err(failure) => return CommandOutput::failed(failure),
                };
                if shows_text(cx, false) {
                    cx.output
                        .stderr
                        .line("What Down will stop changed. Bringing it down with the new list:");
                }
                show(cx, &shown);
                match send(cx, &shown, &key).await {
                    Ok(reply) => reply,
                    Err(failure) => return CommandOutput::failed(failure),
                }
            }
            Err(failure) => return CommandOutput::failed(failure),
        };
        let submission = Submission {
            operation: Some(reply.operation.operation),
            idempotency_key: key,
            replayed: reply.operation.replayed,
            already_in_state: false,
            waited: false,
            failure: None,
        };
        if !self.run.wait.wait {
            return conclude(submission, shown, None);
        }
        waited(cx, submission, &self.run.wait, shown).await
    }
}

/// Waits for the Down, writes the line of each instance that ended, then
/// reads the environment once.
async fn waited(
    cx: &mut Context,
    mut submission: Submission,
    args: &WaitArgs,
    shown: EnvDownPreview,
) -> CommandOutput<BroughtDown> {
    let Some(operation) = submission.operation.clone() else {
        return CommandOutput::failed(CliFailure::unexpected_reply(None));
    };
    submission.waited = true;
    if settled(operation.state) {
        let detail = OperationDetail {
            operation,
            steps: Vec::new(),
            created: Vec::new(),
        };
        submission.failure = settle(WaitMode::Observe, &detail);
    } else {
        let outcome = Waiter::wait(cx, operation.id, args, WaitMode::Observe).await;
        if let Some(detail) = outcome.last {
            submission.operation = Some(detail.operation);
        }
        submission.failure = outcome.failure;
    }
    let environment = if submission.finished() {
        if let Some(operation) = &submission.operation
            && shows_text(cx, false)
        {
            for line in ended_lines(&shown, operation) {
                cx.output.stderr.line(&line);
            }
        }
        read(cx, shown.environment.id).await.ok()
    } else {
        None
    };
    conclude(submission, shown, environment)
}

/// The result, the failure, and the hints of a submitted Down.
fn conclude(
    submission: Submission,
    preview: EnvDownPreview,
    environment: Option<EnvironmentDetail>,
) -> CommandOutput<BroughtDown> {
    let Some(operation) = submission.operation.clone() else {
        return CommandOutput::failed(CliFailure::unexpected_reply(None));
    };
    let hint = submission
        .failure
        .is_none()
        .then(|| submission.wait_hint())
        .flatten();
    let result = BroughtDown {
        operation,
        idempotency_key: submission.idempotency_key.clone(),
        preview,
        environment,
    };
    let mut output = match submission.failure {
        Some(failure) => CommandOutput::partial(result, failure),
        None => CommandOutput::ok(result),
    };
    if submission.replayed {
        output = output
            .with_hint("the idempotency key matched an earlier request, so this is its operation");
    }
    match hint {
        Some(hint) => output.with_hint(hint),
        None => output,
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::Count;
    use stackly_protocol::error::ErrorItem;
    use stackly_protocol::operation::{OperationResult, PartialItem, PartialOutcome};

    use super::*;

    fn fixture() -> EnvDownPreview {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/env/down-preview.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn operation(state: &str) -> OperationSummary {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../protocols/fixtures/ipc/frame-response-operation-get-env-up-incomplete.json",
        );
        let frame: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut operation: OperationSummary =
            serde_json::from_value(frame["content"]["reply"]["content"]["operation"].clone())
                .unwrap();
        operation.state = serde_json::from_value(serde_json::Value::from(state)).unwrap();
        operation.failure = None;
        operation.result = None;
        operation
    }

    fn instance(n: u128) -> stackly_protocol::ResourceRef {
        stackly_protocol::ResourceRef {
            kind: ResourceKind::Instance,
            id: Uuid::parse_str(&format!("0192f3a5-0000-7000-8000-{n:012}"))
                .unwrap()
                .to_string(),
        }
    }

    #[test]
    fn the_preview_says_what_stops_keeps_and_is_skipped() {
        assert_eq!(
            preview_lines(&fixture()),
            [
                "stop erp-development-nats",
                "stop erp-development-postgres",
                "keep shared-valkey: started by you; used by reporting",
                "skip leftover: Stackly can’t confirm this process",
            ]
        );
    }

    #[test]
    fn an_empty_preview_says_nothing_to_stop_and_a_cancelled_up_is_named() {
        let mut empty = fixture();
        empty.stop.clear();
        empty.keep.clear();
        empty.blocked.clear();
        empty.lease_count = Count::saturating(0);
        assert!(is_empty(&empty));
        assert_eq!(preview_lines(&empty), ["Nothing to stop."]);
        empty.cancels_up = true;
        assert!(!is_empty(&empty));
        assert_eq!(preview_lines(&empty), ["cancels the running up"]);
        empty.cancels_up = false;
        empty.lease_count = Count::saturating(1);
        assert!(!is_empty(&empty), "a lease to release is work");
    }

    #[test]
    fn the_keep_reasons_follow_the_copy() {
        let mut keep = fixture().keep.remove(0);
        keep.demand = vec![DemandSourceKind::Startup];
        keep.environments.clear();
        assert_eq!(keep_reason(&keep), "starts at login");
        keep.demand = vec![DemandSourceKind::Environment];
        assert_eq!(keep_reason(&keep), "used by an environment");
    }

    #[test]
    fn a_finished_down_names_each_instance_and_counts_the_kept_ones() {
        let shown = fixture();
        let mut done = operation("succeeded");
        done.result = Some(OperationResult {
            resources: vec![instance(2), instance(1)],
            partial: vec![PartialItem {
                item: "erp-development-postgres".to_owned(),
                resource: Some(instance(1)),
                outcome: PartialOutcome::Skipped,
                code: Some(env_codes::INSTANCE_IN_USE.code.to_owned()),
            }],
        });
        assert_eq!(
            ended_lines(&shown, &done),
            [
                "stopped erp-development-nats",
                "stopped erp-development-postgres",
                "kept erp-development-postgres",
            ]
        );
        assert_eq!(summary_counts(&shown, &done), (2, 2));
    }

    #[test]
    fn an_incomplete_down_names_what_failed_or_was_left_running() {
        let shown = fixture();
        let mut failed = operation("failed");
        let item = |id: &str, outcome: &str, code: &str| {
            ErrorItem::new("instance")
                .with_field("instanceId", id)
                .with_field("outcome", outcome)
                .with_field("code", code)
        };
        let error = ErrorBody::new(&env_codes::DOWN_INCOMPLETE, "not fully down").with_items(vec![
            item(&instance(1).id, "failed", "supervisor.stop_timeout"),
            item(&instance(2).id, "left_running", "env.down_changed"),
            item(&instance(2).id, "cancelled", "operation.cancelled"),
        ]);
        failed.failure = Some(stackly_protocol::operation::OperationFailureInfo {
            reason: "step_failed".to_owned(),
            retryable: false,
            error,
        });
        assert_eq!(
            ended_lines(&shown, &failed),
            [
                "failed erp-development-postgres: supervisor.stop_timeout",
                "left running erp-development-nats",
                "cancelled erp-development-nats",
            ]
        );
    }
}
