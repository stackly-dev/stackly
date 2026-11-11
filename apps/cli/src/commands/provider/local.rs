//! What the provider verbs of spec 0037 share: the permissions block, the
//! rows of a local entry in `inspect`, the import preview, the
//! confirmation rule, and the calls that start a provider operation. Every
//! value comes from the daemon; the CLI never reads a package.

use serde::Serialize;
use stackly_protocol::catalog::{ProviderDetail, TrustLevel};
use stackly_protocol::codes::provider as codes;
use stackly_protocol::provider::{
    ApprovalState, FindingSeverity, ImportBlockReason, ImportPreview, LocalProviderInfo,
    ProviderPermissions, RevocationInfo, RevocationSource,
};
use stackly_protocol::{Call, ErrorBody, Reply};

use super::copy;
use super::inspect::capability_label;
use super::render::size_text;
use crate::confirm::{Confirm, ConfirmRequest, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::OutputMode;
use crate::output::escape::escape_display;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::Tone;
use crate::session::{CallSpec, RequestTimeout};

/// The spec of a call that starts a provider operation: a fresh
/// idempotency key and the long limit, since the operation may wait in
/// the provider's queue.
pub fn operation_call() -> CallSpec {
    CallSpec {
        timeout: RequestTimeout::Interactive,
        idempotency_key: Some(uuid::Uuid::now_v7().to_string()),
    }
}

/// Reads the detail of `provider` with `catalog.get_provider`.
///
/// # Errors
///
/// The daemon's error, such as `catalog.provider_not_found`.
pub async fn get_detail(cx: &mut Context, provider: &str) -> Result<ProviderDetail, CliFailure> {
    let call = Call::CatalogGetProvider(stackly_protocol::catalog::GetProviderParams {
        provider: provider.to_owned(),
    });
    match cx.call(call, CallSpec::control()).await? {
        Reply::CatalogGetProvider(detail) => Ok(*detail),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// The import facts of `detail`, or `provider.import_not_found` with
/// reason `not_local` for a catalog entry, before any mutation is sent.
///
/// # Errors
///
/// `provider.import_not_found` when `detail` is not an imported entry.
pub fn local_of(detail: &ProviderDetail) -> Result<&LocalProviderInfo, CliFailure> {
    match &detail.local {
        Some(local) if detail.summary.trust == TrustLevel::Local => Ok(local),
        _ => {
            let body = ErrorBody::new(
                &codes::IMPORT_NOT_FOUND,
                format!("no imported provider {}", detail.summary.provider),
            )
            .with_detail("provider", detail.summary.provider.clone())
            .with_detail("reason", "not_local");
            let hint = codes::hint(codes::IMPORT_NOT_FOUND.code, body.details());
            Err(CliFailure::local(match hint {
                Some(hint) => body.with_hint(hint),
                None => body,
            }))
        }
    }
}

/// Whether human text may go to stderr: not with `--json`, and not with
/// `--quiet` unless a question follows that needs it.
pub fn shows_text(cx: &Context, asking: bool) -> bool {
    cx.output.mode != OutputMode::Json && (asking || !cx.output.quiet)
}

/// Writes what `write` renders to stderr, line by line, uncolored.
pub fn to_stderr(cx: &mut Context, write: impl FnOnce(&mut HumanWriter)) {
    let mut writer = HumanWriter::new(false, jiff::Timestamp::now(), cx.output.tz.clone());
    write(&mut writer);
    for line in writer.into_string().lines() {
        cx.output.stderr.line(line);
    }
}

/// Whether `stackly` will ask the person at the terminal: interactive,
/// not `--json`, and without `--yes`.
pub fn will_ask(cx: &Context, yes: bool) -> bool {
    !yes && cx.prompt_allowed() && cx.output.mode != OutputMode::Json
}

/// Fails `cli.confirmation_required` when no confirmation can come:
/// without `--yes`, noninteractive or with `--json` (spec 0037, *CLI*).
///
/// # Errors
///
/// `cli.confirmation_required` with `details.flag` `--yes`.
pub fn require_confirmable(cx: &Context, yes: bool) -> Result<(), CliFailure> {
    if yes || will_ask(cx, yes) {
        Ok(())
    } else {
        Err(CliFailure::confirmation_required("--yes"))
    }
}

/// Confirms `request` with `--yes`, the terminal, or not at all.
///
/// # Errors
///
/// `cli.confirmation_required` and `cli.confirmation_declined`.
pub fn confirm(cx: &mut Context, request: ConfirmRequest<'_>, yes: bool) -> Result<(), CliFailure> {
    require_confirmable(cx, yes)?;
    let approval = crate::confirm::Approval {
        given: yes,
        flag: "--yes",
    };
    Confirm::ask(cx, request, approval, OnNoninteractive::Fail).map(|_| ())
}

/// Confirms a destructive action: `consequence` goes to stderr with the
/// question when asking, and alone (unless `--quiet`) with `--yes`.
///
/// # Errors
///
/// `cli.confirmation_required` and `cli.confirmation_declined`.
pub fn confirm_action(
    cx: &mut Context,
    question: &str,
    consequence: &str,
    yes: bool,
) -> Result<(), CliFailure> {
    if !will_ask(cx, yes) && yes && shows_text(cx, false) {
        cx.output.stderr.line(consequence);
    }
    let request = ConfirmRequest {
        role: crate::confirm::ConfirmRole::Destructive,
        question,
        consequence,
    };
    confirm(cx, request, yes)
}

/// The permissions block: one heading, then the groups in the order of
/// the Catalog screen, empty groups left out.
pub fn permission_lines(out: &mut HumanWriter, permissions: Option<&ProviderPermissions>) {
    let Some(permissions) = permissions else {
        out.line("Permissions: not available, the stored copy does not load");
        return;
    };
    out.line("Permissions:");
    let mut any = false;
    if !permissions.executables.is_empty() {
        any = true;
        out.line("  Runs:");
        for executable in &permissions.executables {
            let uses: Vec<&str> = executable
                .uses
                .iter()
                .map(|used| copy::use_label(*used))
                .collect();
            out.line(&format!(
                "    {} ({}): {}",
                esc(&executable.path),
                esc(&executable.name),
                uses.join(", ")
            ));
        }
    }
    if !permissions.downloads.is_empty() {
        any = true;
        out.line("  Downloads:");
        for download in &permissions.downloads {
            out.line(&format!(
                "    {} {} {}/{}, {} from {}",
                esc(&download.artifact),
                esc(&download.version),
                wire_name(&download.os),
                wire_name(&download.arch),
                size_text(download.size.get()),
                esc(&download.hosts.join(", "))
            ));
            out.line(&format!("      sha256:{}", esc(&download.sha256)));
        }
    }
    let groups = [
        ("Writes to", esc(&permissions.managed_filesystem.join(", "))),
        ("Receives secrets", esc(&permissions.secrets.join(", "))),
        (
            "Listens on",
            permissions
                .endpoints
                .iter()
                .map(|endpoint| {
                    let mut text = format!(
                        "{} ({}, {})",
                        esc(&endpoint.name),
                        esc(&endpoint.transport),
                        esc(&endpoint.purpose)
                    );
                    if let Some(port) = endpoint.default_port {
                        text.push_str(&format!(", default port {port}"));
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        (
            "Opens",
            permissions
                .open_with
                .iter()
                .map(|action| match &action.target {
                    Some(target) => format!(
                        "{} ({}, {})",
                        esc(&action.name),
                        esc(&action.kind),
                        esc(target)
                    ),
                    None => format!("{} ({})", esc(&action.name), esc(&action.kind)),
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        (
            "Features",
            permissions
                .capabilities
                .iter()
                .map(|capability| capability_label(*capability))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    ];
    for (label, text) in groups {
        if !text.is_empty() {
            any = true;
            out.line(&format!("  {label}: {text}"));
        }
    }
    if !any {
        out.line("  nothing");
    }
}

/// The rows `inspect` adds for a local entry (spec 0037, *CLI*).
pub fn local_rows(out: &HumanWriter, detail: &ProviderDetail) -> Vec<(&'static str, Cell)> {
    let mut rows = Vec::new();
    let Some(local) = &detail.local else {
        return rows;
    };
    let approval = detail.summary.approval.unwrap_or(ApprovalState::Pending);
    rows.push(("Approval", approval_cell(out, approval, local)));
    rows.push((
        "Imported",
        Cell::fixed(format!(
            "{} from {}",
            out.time(local.imported_at),
            esc(&local.origin_path)
        )),
    ));
    if local.quarantined == Some(true) {
        rows.push(("", Cell::fixed(copy::QUARANTINE_NOTE)));
    }
    rows.push((
        "Package",
        Cell::fixed(format!(
            "sha256:{}, {}",
            esc(&local.package_sha256),
            size_text(local.package_bytes.get())
        )),
    ));
    rows
}

/// `needs approval`, with the last decision when there is one, or the
/// decision and its time.
fn approval_cell(out: &HumanWriter, approval: ApprovalState, local: &LocalProviderInfo) -> Cell {
    let label = copy::approval_label(approval);
    let text = match (&local.decision, approval) {
        (Some(decision), ApprovalState::Pending) => format!(
            "{label} (you {} revision {} on {})",
            match decision.decision {
                ApprovalState::Revoked => "revoked",
                _ => "approved",
            },
            esc(&decision.provider_revision),
            out.time(decision.decided_at)
        ),
        (Some(decision), ApprovalState::Approved | ApprovalState::Revoked) => {
            format!("{label} on {}", out.time(decision.decided_at))
        }
        _ => label.to_owned(),
    };
    let tone = match approval {
        ApprovalState::Approved => Tone::Neutral,
        _ => Tone::Caution,
    };
    Cell::toned(&text, tone)
}

/// `by you on <time>` or `by the catalog (revision <n>) since <date>:
/// <reason>`, always about the installed catalog.
pub fn revocation_text(out: &HumanWriter, revocation: &RevocationInfo) -> String {
    match revocation.by {
        RevocationSource::Catalog => {
            let mut text = String::from("by the catalog in this version of Stackly");
            if let Some(revision) = revocation.catalog_revision {
                text.push_str(&format!(" (revision {})", revision.get()));
            }
            text.push_str(&format!(" since {}", date(out, revocation.since)));
            if let Some(reason) = &revocation.reason {
                text.push_str(&format!(": {}", esc(reason)));
            }
            text
        }
        RevocationSource::User => format!("by you on {}", out.time(revocation.since)),
        RevocationSource::Unrecognized => format!("since {}", out.time(revocation.since)),
    }
}

/// `YYYY-MM-DD` of `ts` in the output's zone.
pub fn date(out: &HumanWriter, ts: stackly_protocol::Timestamp) -> String {
    let full = out.time(ts);
    full.get(..10).map_or_else(|| full.clone(), str::to_owned)
}

/// Text from the daemon, escaped for the terminal.
pub fn esc(text: &str) -> String {
    escape_display(text).into_owned()
}

/// Errors and warnings of a preview: error findings, the findings left
/// out, and every import issue are errors.
#[must_use]
pub fn counts(preview: &ImportPreview) -> (u64, u64) {
    let warnings = preview
        .findings
        .iter()
        .filter(|finding| finding.severity == FindingSeverity::Warning)
        .count() as u64;
    let errors = preview.findings.len() as u64 - warnings
        + preview.omitted_findings.get()
        + preview.import_issues.len() as u64;
    (errors, warnings)
}

/// The finding and issue lines of a preview, for stderr:
/// `<file> <location>: <code>: <message>`, warnings marked.
pub fn finding_lines(out: &mut HumanWriter, preview: &ImportPreview) {
    for finding in &preview.findings {
        let prefix = if finding.severity == FindingSeverity::Warning {
            "warning"
        } else {
            "error"
        };
        out.line(&format!(
            "{prefix}: {} {}: {}: {}",
            esc(&finding.file),
            esc(&finding.location),
            esc(&finding.code),
            esc(&finding.message)
        ));
    }
    for issue in &preview.import_issues {
        out.line(&format!(
            "error: {} {}: {}: {}",
            esc(&issue.file),
            esc(&issue.location),
            esc(&issue.code),
            esc(&issue.message)
        ));
    }
    let omitted = preview.omitted_findings.get();
    if omitted > 0 {
        out.line(&format!("{omitted} more finding(s) not shown"));
    }
}

/// The `import --dry-run` result: the preview as the daemon sent it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct PreviewResult(pub ImportPreview);

impl Render for PreviewResult {
    fn render(&self, out: &mut HumanWriter) {
        preview_text(out, &self.0);
    }
}

/// The text of a preview: identity, size, folder, permissions, then the
/// notes of *Copy*.
pub fn preview_text(out: &mut HumanWriter, preview: &ImportPreview) {
    let versions = {
        let mut text = esc(&preview.versions.join(", "));
        let more = preview
            .version_count
            .get()
            .saturating_sub(preview.versions.len() as u64);
        if more > 0 {
            text.push_str(&format!(", +{more} more"));
        }
        text
    };
    let mut rows = vec![
        ("Name", Cell::optional(preview.display_name.as_deref())),
        ("ID", Cell::optional(preview.provider.as_deref())),
        (
            "Revision",
            Cell::optional(preview.provider_revision.as_deref()),
        ),
        (
            "Publisher",
            Cell::fixed(
                preview
                    .publisher
                    .as_deref()
                    .map_or_else(String::new, |p| format!("{} (not verified)", esc(p))),
            ),
        ),
        (
            "Category",
            Cell::optional(preview.category.as_ref().map(wire_name).as_deref()),
        ),
        ("Software", Cell::optional(preview.software.as_deref())),
        ("Versions", Cell::fixed(versions)),
        (
            "Package",
            Cell::fixed(
                preview
                    .package_sha256
                    .as_deref()
                    .map_or_else(String::new, |d| format!("sha256:{}", esc(d))),
            ),
        ),
        (
            "Size",
            Cell::fixed(format!(
                "{}, {} files",
                size_text(preview.package_bytes.get()),
                preview.file_count.get()
            )),
        ),
        ("Folder", Cell::text(&preview.origin_path)),
    ];
    if preview.quarantined == Some(true) {
        let agent = preview.quarantine_agent.as_deref().map_or_else(
            || "downloaded".to_owned(),
            |agent| format!("downloaded by {}", esc(agent)),
        );
        rows.push(("Quarantine", Cell::fixed(agent)));
    }
    out.kv(&rows);
    if preview.permissions.is_some() {
        out.line("");
        permission_lines(out, preview.permissions.as_ref());
    }
    let mut notes = Vec::new();
    if preview.quarantined == Some(true) {
        notes.push(copy::QUARANTINE_NOTE.to_owned());
    }
    if let Some(existing) = &preview.existing {
        notes.push(match (existing.same_package, existing.stored_copy_valid) {
            (true, true) => copy::SAME_PACKAGE.to_owned(),
            (true, false) => copy::DAMAGED_COPY.to_owned(),
            (false, _) => copy::replaces(&esc(&existing.provider_revision)),
        });
    }
    if !preview.changes.is_empty() {
        notes.push(copy::differs(&preview.changes));
    }
    if preview.revocation.is_some() {
        notes.push(copy::REVOKED_BY_CATALOG.to_owned());
    }
    if preview.verdict.reason == Some(ImportBlockReason::ImportLimit) {
        notes.push(copy::IMPORT_LIMIT.to_owned());
    }
    if preview.verdict.importable {
        notes.push(copy::IMPORT_FOOTNOTE.to_owned());
    }
    if !notes.is_empty() {
        out.line("");
        for note in notes {
            out.line(&note);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(name: &str) -> ImportPreview {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/provider")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn issues_and_omitted_findings_count_as_errors() {
        let mut invalid = preview("import-preview-invalid.json");
        assert_eq!(counts(&invalid), (1, 1));
        invalid.omitted_findings = stackly_protocol::Count::saturating(3);
        assert_eq!(counts(&invalid), (4, 1));
    }

    #[test]
    fn a_missing_permission_block_says_why() {
        let mut out = HumanWriter::new(false, jiff::Timestamp::UNIX_EPOCH, jiff::tz::TimeZone::UTC);
        permission_lines(&mut out, None);
        assert_eq!(
            out.into_string(),
            "Permissions: not available, the stored copy does not load\n"
        );
    }
}
