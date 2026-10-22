//! `stackly runtime install <provider>@<selector>` (spec 0018, AC-16).
//!
//! The command resolves the selector first (`runtime.resolve`), so its
//! result can show what was chosen, then starts the install with the digest
//! it saw as `expectedSha256`: the artifact it names is the one that is
//! installed, even when the catalog moves in between.

use std::str::FromStr;

use clap::{Args, ValueEnum};
use serde::Serialize;
use stackly_protocol::operation::{OperationState, OperationSummary};
use stackly_protocol::runtime::{
    InstallOutcome, InstallRuntimeParams, ResolvedArtifact, RuntimeListPage,
};
use stackly_protocol::{Call, Reply, ResourceKind};
use uuid::Uuid;

use super::{all_runtimes, read_runtime};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render, wire_name};
use crate::session::CallSpec;
use crate::wait::{Accepted, AsyncArgs, Submission};

/// What `<provider>@<selector>` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTarget {
    /// The provider ID, `<namespace>/<name>`.
    pub provider: String,
    /// `stable`, a line, an exact version, or `sha256:<hex>`.
    pub selector: String,
}

impl FromStr for RuntimeTarget {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let usage = || {
            "expected <provider>@<selector>; use <provider>@stable or <provider>@<line>".to_owned()
        };
        let (provider, selector) = text.split_once('@').ok_or_else(usage)?;
        if selector.is_empty() || selector.contains('@') {
            return Err(usage());
        }
        match provider.split_once('/') {
            Some((namespace, name))
                if !namespace.is_empty() && !name.is_empty() && !name.contains('/') => {}
            _ => {
                return Err(
                    "the provider must be written <namespace>/<name>, such as official/postgresql"
                        .to_owned(),
                );
            }
        }
        Ok(Self {
            provider: provider.to_owned(),
            selector: selector.to_owned(),
        })
    }
}

/// What `--output` can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputKind {
    /// Only the ID of the runtime, one line on stdout.
    Id,
}

/// `stackly runtime install`.
#[derive(Debug, Args)]
pub struct InstallArgs {
    /// The provider and what to install, such as official/postgresql@18
    #[arg(value_name = "PROVIDER@SELECTOR")]
    pub target: RuntimeTarget,
    /// Refuse the install when the selector resolves to another artifact than this SHA-256
    #[arg(long, value_name = "SHA256")]
    pub expect_sha256: Option<String>,
    /// Resolve the selector and show the artifact without installing it
    #[arg(long, conflicts_with = "output")]
    pub dry_run: bool,
    /// Write only the runtime ID, after waiting for the install
    #[arg(long, value_enum, value_name = "KIND", conflicts_with = "json")]
    pub output: Option<OutputKind>,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// The `result` of `install`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum InstallOutput {
    /// `--dry-run`: the artifact the selector resolved to.
    Resolved(ResolvedArtifact),
    /// The install.
    Install(Box<InstallResult>),
}

/// The `result` of an install that ran.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    /// The operation: as accepted, or with `--wait` as it stands at the
    /// end. Null when the artifact was installed already.
    pub operation: Option<OperationSummary>,
    /// The key the request carried.
    pub idempotency_key: String,
    /// The artifact the selector resolved to.
    pub resolved: ResolvedArtifact,
    /// The runtime: the one the operation made or found, the one it will
    /// make, or the one that was installed already.
    pub runtime_id: Option<Uuid>,
    /// True when the artifact was installed already, so nothing ran.
    pub already_installed: bool,
    #[serde(skip)]
    display_name: String,
    #[serde(skip)]
    id_only: bool,
}

impl RunCommand for InstallArgs {
    type Output = InstallOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstallOutput> {
        let target = &self.target;
        let resolve = Call::RuntimeResolve(InstallRuntimeParams {
            provider: target.provider.clone(),
            selector: target.selector.clone(),
            expected_sha256: self.expect_sha256.clone(),
        });
        let resolved = match cx.call(resolve, CallSpec::control()).await {
            Ok(Reply::RuntimeResolve(resolved)) => *resolved,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        if self.dry_run {
            return CommandOutput::ok(InstallOutput::Resolved(resolved));
        }
        let id_only = self.output == Some(OutputKind::Id);
        let mut run = self.run.clone();
        if id_only {
            run.wait.wait = true;
        }
        let call = Call::RuntimeInstall(InstallRuntimeParams {
            provider: target.provider.clone(),
            selector: target.selector.clone(),
            expected_sha256: Some(resolved.sha256.clone()),
        });
        match cx.submit(call, &run, extract).await {
            Ok(submission) => conclude(cx, submission, resolved, id_only).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

fn extract(reply: Reply) -> Option<Accepted> {
    let Reply::RuntimeInstall(reply) = reply else {
        return None;
    };
    Some(match reply.outcome {
        InstallOutcome::Started(accepted) => Accepted {
            operation: Some(accepted.operation),
            replayed: accepted.replayed,
            already_in_state: false,
        },
        InstallOutcome::AlreadyInstalled(_) => Accepted {
            operation: None,
            replayed: false,
            already_in_state: true,
        },
    })
}

/// The runtime an operation made or found: the first runtime of its result,
/// else the one it was started for.
fn runtime_of(operation: &OperationSummary) -> Option<Uuid> {
    let from_result = operation
        .result
        .iter()
        .flat_map(|result| &result.resources)
        .find(|resource| resource.kind == ResourceKind::Runtime);
    from_result
        .or(operation
            .resource
            .as_ref()
            .filter(|resource| resource.kind == ResourceKind::Runtime))
        .and_then(|resource| Uuid::try_parse(&resource.id).ok())
}

/// The name of the software for people: the catalog's display name from
/// the runtime, or from the install that has not ended, or the software
/// name.
async fn display_name(
    cx: &mut Context,
    resolved: &ResolvedArtifact,
    runtime: Option<Uuid>,
    installed: bool,
    installing: Option<&RuntimeListPage>,
) -> String {
    if installed
        && let Some(id) = runtime
        && let Ok(detail) = read_runtime(cx, id).await
    {
        return detail.summary.display_name;
    }
    let page = match installing {
        Some(page) => Some(page.clone()),
        None => all_runtimes(cx).await.ok(),
    };
    page.and_then(|page| {
        page.installing
            .into_iter()
            .find(|install| Some(install.runtime_id) == runtime)
            .map(|install| install.display_name)
    })
    .unwrap_or_else(|| resolved.software.clone())
}

/// Shapes the result of a submitted install: the runtime, its name, and the
/// failure the wait ended with next to the last snapshot.
async fn conclude(
    cx: &mut Context,
    submission: Submission,
    resolved: ResolvedArtifact,
    id_only: bool,
) -> CommandOutput<InstallOutput> {
    let already = submission.already_in_state;
    let operation = submission.operation.clone();
    let succeeded = operation
        .as_ref()
        .is_some_and(|operation| operation.state == OperationState::Succeeded);
    let (runtime_id, installed_name) = if already {
        // The artifact is installed: its runtime is the one with its digest.
        let page = all_runtimes(cx).await.ok();
        let found = page.as_ref().and_then(|page| {
            page.items.iter().find(|runtime| {
                runtime.artifact_sha256.as_deref() == Some(resolved.sha256.as_str())
            })
        });
        (
            found.map(|runtime| runtime.id),
            found.map(|runtime| runtime.display_name.clone()),
        )
    } else {
        (operation.as_ref().and_then(runtime_of), None)
    };
    let display = match installed_name {
        Some(name) => name,
        None => display_name(cx, &resolved, runtime_id, succeeded, None).await,
    };
    let hint = submission
        .failure
        .is_none()
        .then(|| submission.wait_hint())
        .flatten();
    let replayed = submission.replayed;
    let result = InstallOutput::Install(Box::new(InstallResult {
        operation,
        idempotency_key: submission.idempotency_key,
        resolved,
        runtime_id,
        already_installed: already,
        display_name: display,
        id_only,
    }));
    let output = match submission.failure {
        Some(failure) => CommandOutput::partial(result, failure),
        None => CommandOutput::ok(result),
    };
    let output = if replayed {
        output.with_hint("the idempotency key matched an earlier request, so this is its operation")
    } else {
        output
    };
    match hint {
        Some(hint) => output.with_hint(hint),
        None => output,
    }
}

impl Render for InstallOutput {
    fn render(&self, out: &mut HumanWriter) {
        match self {
            Self::Resolved(resolved) => render_resolved(out, resolved),
            Self::Install(result) => result.render(out),
        }
    }
}

impl Render for InstallResult {
    fn render(&self, out: &mut HumanWriter) {
        let name = format!(
            "{} {}",
            crate::output::escape::escape_display(&self.display_name),
            self.resolved.version
        );
        if self.id_only {
            if let Some(id) = self.runtime_id {
                out.line(&id.to_string());
            }
            return;
        }
        let Some(operation) = self.operation.as_ref().filter(|_| !self.already_installed) else {
            match self.runtime_id {
                Some(id) => out.line(&format!("Already installed: {name} (runtime {id})")),
                None => out.line(&format!("Already installed: {name}")),
            }
            return;
        };
        match (operation.state, self.runtime_id) {
            (OperationState::Succeeded, Some(id)) => {
                out.line(&format!("Installed {name} (runtime {id})"));
            }
            (OperationState::Queued | OperationState::Running, _) => {
                out.line(&format!(
                    "Started installing {name} (operation {})",
                    operation.id
                ));
            }
            (state, _) => out.line(&format!("operation {} {}", operation.id, wire_name(&state))),
        }
    }
}

/// The artifact `--dry-run` resolved to.
fn render_resolved(out: &mut HumanWriter, resolved: &ResolvedArtifact) {
    use crate::output::human::Cell;
    let unpacked = resolved
        .unpacked_size
        .map_or_else(String::new, |size| format!("{} bytes", size.get()));
    out.kv(&[
        ("Provider", Cell::text(&resolved.provider)),
        ("Artifact", Cell::text(&resolved.artifact_id)),
        (
            "Version",
            Cell::text(&format!("{} (line {})", resolved.version, resolved.line)),
        ),
        (
            "Target",
            Cell::fixed(format!(
                "{}-{}",
                wire_name(&resolved.target.os),
                wire_name(&resolved.target.arch)
            )),
        ),
        ("Digest", Cell::fixed(format!("sha256:{}", resolved.sha256))),
        (
            "Download",
            Cell::fixed(format!("{} bytes", resolved.size.get())),
        ),
        ("Unpacked", Cell::fixed(unpacked)),
        ("Format", Cell::fixed(wire_name(&resolved.format))),
        ("Minimum macOS", Cell::text(&resolved.min_os)),
        (
            "Stable",
            Cell::fixed(if resolved.stable { "yes" } else { "no" }),
        ),
        (
            "Support",
            Cell::fixed(format!(
                "{} ({})",
                wire_name(&resolved.support),
                wire_name(&resolved.support_reason)
            )),
        ),
    ]);
}
