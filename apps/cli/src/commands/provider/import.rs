//! `stackly provider import <dir> [--dry-run] [--approve] [--yes]` (spec
//! 0037, *CLI*, AC-14). The folder is made absolute here and read by the
//! daemon: `provider.preview_import` first, then `provider.import` with
//! the previewed `packageSha256`, so bytes that changed in between fail
//! `provider.import_changed`. Importing runs nothing and asks nothing;
//! `--approve` chains `provider.approve` (K6), and its confirmation rule
//! is checked before `provider.import` is sent.

use std::path::PathBuf;

use clap::Args;
use serde::Serialize;
use stackly_protocol::ErrorBody;
use stackly_protocol::catalog::ProviderDetail;
use stackly_protocol::codes::provider as codes;
use stackly_protocol::provider::{
    ApprovalState, ImportBlockReason, ImportPreview, ImportProviderParams, PreviewImportParams,
};
use stackly_protocol::{Call, Reply};

use super::approve::approve_detail;
use super::copy;
use super::local::{
    PreviewResult, counts, esc, finding_lines, operation_call, preview_text, require_confirmable,
    shows_text, to_stderr,
};
use super::outcome::Outcome;
use super::validate::absolute;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};
use crate::session::CallSpec;

/// `stackly provider import`.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// The provider package folder; a relative path starts at the current
    /// folder
    #[arg(value_name = "DIR")]
    pub dir: PathBuf,
    /// Only preview the package; nothing is imported
    #[arg(long, conflicts_with = "approve")]
    pub dry_run: bool,
    /// Approve the package right after importing it, which asks first
    #[arg(long)]
    pub approve: bool,
    /// Confirm the approval without asking; needed when stackly runs
    /// noninteractive
    #[arg(short = 'y', long, requires = "approve")]
    pub yes: bool,
}

/// The `provider import` result: the preview with `--dry-run`, else the
/// entry after the import (and the approval).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ImportResult {
    /// `--dry-run`.
    Preview(PreviewResult),
    /// The imported entry.
    Imported(Outcome),
}

impl Render for ImportResult {
    fn render(&self, out: &mut HumanWriter) {
        match self {
            Self::Preview(preview) => preview.render(out),
            Self::Imported(outcome) => outcome.render(out),
        }
    }
}

impl RunCommand for ImportArgs {
    type Output = ImportResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<ImportResult> {
        let path = match absolute(&self.dir) {
            Ok(path) => path.display().to_string(),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let preview = match preview(cx, &path).await {
            Ok(preview) => preview,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if self.dry_run {
            return dry_run(cx, preview);
        }
        if let Some(failure) = blocked(&preview) {
            if shows_text(cx, false) {
                to_stderr(cx, |out| {
                    preview_text(out, &preview);
                    finding_lines(out, &preview);
                });
            }
            return CommandOutput::failed(failure);
        }
        if self.approve
            && let Err(failure) = require_confirmable(cx, self.yes)
        {
            return CommandOutput::failed(failure);
        }
        let Some(expected) = preview.package_sha256.clone() else {
            return CommandOutput::failed(CliFailure::unexpected_reply(None));
        };
        let same = preview
            .existing
            .as_ref()
            .is_some_and(|existing| existing.same_package && existing.stored_copy_valid);
        if !same && shows_text(cx, false) {
            to_stderr(cx, |out| preview_text(out, &preview));
        }
        let detail = match import(cx, path, expected).await {
            Ok(detail) => detail,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let headline = if same {
            copy::ALREADY_IMPORTED.to_owned()
        } else {
            imported_line(&detail)
        };
        if !self.approve {
            return CommandOutput::ok(ImportResult::Imported(Outcome::new(headline, detail)));
        }
        if shows_text(cx, false) {
            cx.output.stderr.line(&headline);
        }
        let approved = approve_detail(cx, detail, self.yes, Some(headline)).await;
        CommandOutput {
            result: approved.result.map(ImportResult::Imported),
            failure: approved.failure,
            warnings: approved.warnings,
            hints: approved.hints,
        }
    }
}

/// `provider.preview_import` for `path`.
async fn preview(cx: &mut Context, path: &str) -> Result<ImportPreview, CliFailure> {
    let call = Call::ProviderPreviewImport(PreviewImportParams {
        path: path.to_owned(),
    });
    match cx.call(call, CallSpec::control()).await? {
        Reply::ProviderPreviewImport(preview) => Ok(*preview),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// `provider.import` for `path`, bound to the previewed bytes.
async fn import(
    cx: &mut Context,
    path: String,
    expected: String,
) -> Result<ProviderDetail, CliFailure> {
    let call = Call::ProviderImport(ImportProviderParams {
        path,
        expected_package_sha256: expected,
    });
    match cx.call(call, operation_call()).await? {
        Reply::ProviderImport(detail) => Ok(*detail),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// The `--dry-run` outcome: the preview as result, exit 0 when it can be
/// imported, else the failure of [`blocked`] with the finding lines on
/// stderr and the preview kept.
fn dry_run(cx: &mut Context, preview: ImportPreview) -> CommandOutput<ImportResult> {
    match blocked(&preview) {
        None => CommandOutput::ok(ImportResult::Preview(PreviewResult(preview))),
        Some(failure) => {
            if shows_text(cx, false) {
                to_stderr(cx, |out| finding_lines(out, &preview));
            }
            CommandOutput::partial(ImportResult::Preview(PreviewResult(preview)), failure)
        }
    }
}

/// Why `preview` cannot be imported: `provider.import_limit` for the
/// import limit, which is no fault of the package, else
/// `cli.provider_invalid`; `None` when it can be imported.
fn blocked(preview: &ImportPreview) -> Option<CliFailure> {
    if preview.verdict.importable {
        return None;
    }
    if preview.verdict.reason == Some(ImportBlockReason::ImportLimit) {
        let body = ErrorBody::new(&codes::IMPORT_LIMIT, "too many imported providers");
        let body = match codes::hint(codes::IMPORT_LIMIT.code, body.details()) {
            Some(hint) => body.with_hint(hint),
            None => body,
        };
        return Some(CliFailure::local(body));
    }
    let (errors, warnings) = counts(preview);
    Some(CliFailure::provider_not_importable(errors, warnings))
}

/// The line after an import: what the restored or missing decision means.
fn imported_line(detail: &ProviderDetail) -> String {
    let name = esc(&detail.summary.display_name);
    match detail.summary.approval {
        Some(ApprovalState::Approved) => {
            format!("Imported {name}. Your approval of this exact package still applies.")
        }
        Some(ApprovalState::Revoked) => {
            format!("Imported {name}. You revoked the approval of this exact package.")
        }
        _ => format!("Imported {name}. Approve it before you install anything."),
    }
}
