//! `stackly env up [target] [--locked] [--wait] [--timeout <duration>]
//! [--idempotency-key <key>]` (spec 0046, *CLI*, AC-17): asks `env.plan`
//! for the manifest, writes the plan to stderr, then starts the daemon's
//! `environment_up` with `env.up` and the plan's `planDigest`. It asks
//! nothing. Without `--wait` it prints the operation it started; with
//! `--wait` it writes one line per phase change of a service to stderr,
//! then the services with the ports that are really bound, and exits 0
//! only when the Up `succeeded`.
//!
//! A manifest that is not valid exits 2 (`cli.env_invalid`) and a blocked
//! plan 3 (`cli.env_plan_blocked`), both before `env.up`. A plan blocked
//! only by references without a value still goes to `env.up`, so the
//! daemon answers `secret.needs_secret` with every reference (exit 6); the
//! CLI adds a `stackly secret set <reference>` hint per reference.

use clap::Args;
use serde::Serialize;
use stackly_protocol::codes::{env as env_codes, secret as secret_codes};
use stackly_protocol::env::{EnvGetParams, EnvPlan, EnvUpParams, EnvironmentDetail, PlanBlocker};
use stackly_protocol::operation::{OperationDetail, OperationSummary};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use super::plan::plan;
use super::render::{counts, findings, plan_sections, plan_table};
use super::services::{PhaseLines, not_ready_lines, ready_line, up_table};
use super::target::TargetArgs;
use crate::commands::RunCommand;
use crate::commands::provider::local::{esc, shows_text, to_stderr};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};
use crate::session::{CallSpec, RequestTimeout};
use crate::wait::{AsyncArgs, Submission, WaitArgs, WaitMode, Waiter, Watch, settled};

/// The digest sent for a plan that has none because only references
/// without a value block it: the daemon checks the digest of applicable
/// plans only and answers such a plan with `secret.needs_secret` (spec
/// 0046, *Precheck Up*).
pub const NO_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// `stackly env up`.
#[derive(Debug, Args)]
pub struct UpArgs {
    /// The manifest or the environment.
    #[command(flatten)]
    pub target: TargetArgs,
    /// Use only the versions in stackly.lock; a missing or stale lock
    /// blocks the Up
    #[arg(long)]
    pub locked: bool,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// The `env up` result: `{ operation, idempotencyKey, environment }`,
/// `environment` filled after `--wait`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Brought {
    /// The `environment_up` operation: as accepted, or with `--wait` as it
    /// stands at the end.
    pub operation: OperationSummary,
    /// The key the request carried.
    pub idempotency_key: String,
    /// The environment read once after the wait; null without `--wait`.
    pub environment: Option<EnvironmentDetail>,
    #[serde(skip)]
    name: String,
}

impl Render for Brought {
    fn render(&self, out: &mut HumanWriter) {
        match &self.environment {
            Some(detail) => {
                up_table(out, detail);
                out.line("");
                out.line(&ready_line(detail));
            }
            None if settled(self.operation.state) => {}
            None => out.line(&format!(
                "Started bringing up {} (operation {})",
                esc(&self.name),
                self.operation.id
            )),
        }
    }
}

/// Whether `blocker` is an existing instance that cannot start because a
/// reference has no value.
fn needs_secret(blocker: &PlanBlocker) -> bool {
    blocker.error.is(&env_codes::INSTANCE_BLOCKED)
        && blocker.error.detail("reason") == Some("needs_secret")
}

/// The digest `env.up` gets for `plan`, or `None` when something other
/// than a reference without a value blocks it.
fn digest_of(plan: &EnvPlan) -> Option<String> {
    if plan.applicable {
        return plan.plan_digest.clone();
    }
    let only_secrets = plan.blockers.iter().all(needs_secret)
        && (!plan.blockers.is_empty() || u64::from(plan.needs_secrets) > 0);
    only_secrets.then(|| NO_DIGEST.to_owned())
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

/// The watch of `--wait`: after each read of the Up it reads the
/// environment and writes the phase changes of its services.
struct UpWatch {
    environment: Uuid,
    lines: PhaseLines,
    quiet: bool,
}

impl Watch for UpWatch {
    fn wide(&self) -> bool {
        true
    }

    async fn seen(&mut self, cx: &mut Context, detail: &OperationDetail) {
        if self.quiet {
            return;
        }
        let call = Call::EnvGet(EnvGetParams {
            id: self.environment,
            check_manifest: false,
        });
        let Ok(Reply::EnvGet(environment)) = cx.call_once(call, CallSpec::control()).await else {
            return;
        };
        for line in self.lines.changes(&environment, detail.operation.id) {
            cx.output.stderr.line(&line);
        }
    }
}

/// The `secret.needs_secret` of `env.up` with a `stackly secret set` hint
/// per reference.
fn with_secret_hints(mut failure: CliFailure) -> CliFailure {
    if failure.body.is(&secret_codes::NEEDS_SECRET) {
        let hints: Vec<String> = failure
            .body
            .items()
            .iter()
            .filter_map(|item| item.field("reference"))
            .map(|reference| format!("stackly secret set {}", esc(reference)))
            .collect();
        failure.hints.extend(hints);
    }
    failure
}

impl RunCommand for UpArgs {
    type Output = Brought;

    async fn run(self, cx: &mut Context) -> CommandOutput<Brought> {
        let resolved = match self.target.resolve(cx).await {
            Ok(resolved) => resolved,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let source = resolved.source();
        let plan = match plan(cx, source.clone(), self.locked).await {
            Ok(plan) => plan,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if self.locked && !plan.lock.locked {
            return CommandOutput::failed(CliFailure::env_locked_unsupported());
        }
        if shows_text(cx, false) {
            to_stderr(cx, |out| {
                if plan.validation.valid {
                    plan_table(out, &plan);
                }
                findings(out, &plan.validation);
                plan_sections(out, &plan);
            });
        }
        if !plan.validation.valid {
            let (errors, warnings) = counts(&plan.validation);
            return CommandOutput::failed(CliFailure::env_invalid(errors, warnings));
        }
        let Some(digest) = digest_of(&plan) else {
            return CommandOutput::failed(CliFailure::env_plan_blocked(plan.blockers.len() as u64));
        };
        let call = Call::EnvUp(EnvUpParams {
            source,
            expected_plan_digest: digest,
            locked: self.locked,
        });
        let key = self
            .run
            .idempotency_key
            .as_ref()
            .map_or_else(|| Uuid::now_v7().to_string(), |key| key.as_str().to_owned());
        let spec = CallSpec {
            timeout: RequestTimeout::Control,
            idempotency_key: Some(key.clone()),
        };
        let reply = match cx.call(call, spec).await {
            Ok(Reply::EnvUp(reply)) => *reply,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(with_secret_hints(failure)),
        };
        let environment = reply.environment_id;
        let name = reply.name;
        let submission = Submission {
            operation: Some(reply.operation.operation),
            idempotency_key: key,
            replayed: reply.operation.replayed,
            already_in_state: false,
            waited: false,
            failure: None,
        };
        if !self.run.wait.wait {
            return conclude(cx, submission, None, name).await;
        }
        waited(cx, submission, &self.run.wait, environment, name, &plan).await
    }
}

/// Waits for the Up, then reads the environment once.
async fn waited(
    cx: &mut Context,
    mut submission: Submission,
    args: &WaitArgs,
    environment: Uuid,
    name: String,
    plan: &EnvPlan,
) -> CommandOutput<Brought> {
    let Some(operation) = submission.operation.clone() else {
        return CommandOutput::failed(CliFailure::unexpected_reply(None));
    };
    submission.waited = true;
    let mut watch = UpWatch {
        environment,
        lines: PhaseLines::new(plan),
        quiet: cx.output.quiet || !shows_text(cx, false),
    };
    if settled(operation.state) {
        let detail = OperationDetail {
            operation,
            steps: Vec::new(),
            created: Vec::new(),
        };
        submission.failure = crate::wait::settle(WaitMode::Observe, &detail);
    } else {
        let outcome = Waiter::watch(cx, operation.id, args, WaitMode::Observe, &mut watch).await;
        if let Some(detail) = outcome.last {
            submission.operation = Some(detail.operation);
        }
        submission.failure = outcome.failure;
    }
    let detail = if submission.finished() {
        read(cx, environment).await.ok()
    } else {
        None
    };
    conclude(cx, submission, detail, name).await
}

/// The result, the failure, and the hints of a submitted Up.
async fn conclude(
    cx: &mut Context,
    submission: Submission,
    environment: Option<EnvironmentDetail>,
    name: String,
) -> CommandOutput<Brought> {
    let Some(operation) = submission.operation.clone() else {
        return CommandOutput::failed(CliFailure::unexpected_reply(None));
    };
    if let Some(detail) = environment
        .as_ref()
        .filter(|_| submission.failure.is_some())
        && shows_text(cx, false)
    {
        to_stderr(cx, |out| not_ready_lines(out, detail));
    }
    let hint = submission
        .failure
        .is_none()
        .then(|| submission.wait_hint())
        .flatten();
    let brought = Brought {
        operation,
        idempotency_key: submission.idempotency_key.clone(),
        environment,
        name,
    };
    let mut output = match submission.failure {
        Some(failure) => CommandOutput::partial(brought, failure),
        None => CommandOutput::ok(brought),
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
    use stackly_protocol::ErrorBody;

    use super::*;

    fn plan() -> EnvPlan {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/env/plan-applicable.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn blocker(reason: &str) -> PlanBlocker {
        PlanBlocker {
            service: Some("db".to_owned()),
            error: ErrorBody::new(&env_codes::INSTANCE_BLOCKED, "blocked")
                .with_detail("reason", reason),
        }
    }

    #[test]
    fn only_a_plan_blocked_by_secrets_alone_still_goes_to_env_up() {
        let applicable = plan();
        assert_eq!(digest_of(&applicable), applicable.plan_digest);
        let mut secrets = plan();
        secrets.applicable = false;
        secrets.plan_digest = None;
        secrets.blockers = vec![blocker("needs_secret")];
        assert_eq!(digest_of(&secrets).as_deref(), Some(NO_DIGEST));
        let mut other = secrets.clone();
        other.blockers.push(blocker("failed"));
        assert_eq!(digest_of(&other), None);
        assert_eq!(NO_DIGEST.len(), 64);
    }
}
