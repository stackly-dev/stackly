//! `stackly diagnostics preview` and `export` (spec 0034, *CLI*, AC-14,
//! AC-16): the daemon builds a diagnostic bundle and the CLI lists what it
//! holds; `export` then asks before it has the daemon copy exactly those
//! bytes to a path on this Mac. The CLI never reads a secret, never writes
//! the file itself, and nothing is uploaded.

use std::path::{Component, Path, PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;
use stackly_protocol::diagnostics::{BundlePreview, ExportBundleParams, ExportedBundle};
use stackly_protocol::{Call, Reply};

use super::{RunCommand, execute};
use crate::commands::provider::render::size_text;
use crate::confirm::{Confirm, ConfirmArgs, ConfirmRequest, ConfirmRole, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::escape::escape_display;
use crate::output::human::{Cell, HumanWriter, Render};
use crate::output::{CommandOutput, OutputMode};
use crate::progress::ProgressMode;
use crate::session::{CallSpec, RequestTimeout};

/// The progress line while the daemon builds the bundle.
pub const PREPARING: &str = "Preparing the bundle…";

/// What every listing of a bundle ends with.
pub const REDACTED: &str = "Secrets and connection strings are redacted. Paths in your home folder are shown as ~. Nothing is uploaded.";

/// The file name when the daemon's suggestion is not a plain name.
const FALLBACK_FILE_NAME: &str = "stackly-diagnostics.zip";

/// The fewest columns DESCRIPTION keeps on a narrow terminal.
const DESCRIPTION_MIN_COLUMNS: usize = 24;

/// The verbs of `stackly diagnostics`.
#[derive(Debug, Subcommand)]
pub enum DiagnosticsCommand {
    /// Build a diagnostic bundle and list what it holds; nothing is saved or uploaded
    #[command(
        after_help = "Examples:\n  stackly diagnostics preview\n  stackly diagnostics preview --json"
    )]
    Preview(PreviewArgs),
    /// Build a diagnostic bundle, list what it holds, and save it as a zip on this Mac after you confirm; nothing is uploaded
    #[command(
        after_help = "Examples:\n  stackly diagnostics export\n  stackly diagnostics export --output ~/Downloads/stackly.zip\n  stackly diagnostics export --output bundle.zip --force --yes --json"
    )]
    Export(ExportArgs),
}

impl DiagnosticsCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Preview(args) => execute(args, cx).await,
            Self::Export(args) => execute(args, cx).await,
        }
    }
}

/// `stackly diagnostics preview` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct PreviewArgs {}

impl RunCommand for PreviewArgs {
    type Output = BundlePreview;

    async fn run(self, cx: &mut Context) -> CommandOutput<BundlePreview> {
        match preview(cx).await {
            Ok(preview) => CommandOutput::ok(preview),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

/// `stackly diagnostics export`.
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Where to save the zip; a relative path starts in the current folder (default: the suggested name there)
    #[arg(long, value_name = "PATH")]
    pub output: Option<String>,
    /// Replace a file that is already at that path
    #[arg(long)]
    pub force: bool,
    /// `--yes`.
    #[command(flatten)]
    pub confirm: ConfirmArgs,
}

/// The `export` result: the preview that was shown, and the file written,
/// or null when the export did not happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// What the bundle holds.
    pub preview: BundlePreview,
    /// The file the daemon wrote.
    pub exported: Option<ExportedBundle>,
}

impl RunCommand for ExportArgs {
    type Output = ExportResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<ExportResult> {
        let preview = match preview(cx).await {
            Ok(preview) => preview,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let mut result = ExportResult {
            preview,
            exported: None,
        };
        let destination = match destination(self.output.as_deref(), &result.preview) {
            Ok(destination) => destination,
            Err(failure) => return CommandOutput::partial(result, failure),
        };
        show_listing(cx, &result.preview);
        let question = format!("Save this bundle to {}?", escape_display(&destination));
        let request = ConfirmRequest {
            role: if self.force {
                ConfirmRole::Destructive
            } else {
                ConfirmRole::Standard
            },
            question: &question,
            consequence: if self.force {
                "a file already at that path is replaced; nothing is uploaded"
            } else {
                "the zip is written only to this Mac; nothing is uploaded"
            },
        };
        if let Err(failure) =
            Confirm::ask(cx, request, self.confirm.approval(), OnNoninteractive::Fail)
        {
            return CommandOutput::partial(result, failure);
        }
        let call = Call::DiagnosticsExportBundle(ExportBundleParams {
            preview_id: result.preview.preview_id,
            destination,
            replace: self.force,
        });
        match cx.call(call, long_call()).await {
            Ok(Reply::DiagnosticsExportBundle(exported)) => {
                result.exported = Some(exported);
                CommandOutput::ok(result)
            }
            Ok(_) => CommandOutput::partial(result, CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::partial(result, failure),
        }
    }
}

/// Has the daemon build a bundle, with the progress line on stderr.
///
/// # Errors
///
/// The daemon's error, such as `diagnostics.bundle_failed`, or the
/// connection's.
async fn preview(cx: &mut Context) -> Result<BundlePreview, CliFailure> {
    match cx.progress {
        ProgressMode::Live => cx.output.stderr.live(PREPARING),
        ProgressMode::Lines => cx.output.stderr.line(PREPARING),
        ProgressMode::Off => {}
    }
    match cx.call(Call::DiagnosticsPreviewBundle, long_call()).await {
        Ok(Reply::DiagnosticsPreviewBundle(preview)) => Ok(*preview),
        Ok(_) => Err(CliFailure::unexpected_reply(None)),
        Err(failure) => Err(failure),
    }
}

/// The time limit of both calls: a preview runs the whole doctor first,
/// which may wait 15 seconds for a slot and run 15 more (spec 0034 AC-1),
/// and an export copies up to 64 MiB and syncs it, both past the 10
/// seconds of a control call.
const fn long_call() -> CallSpec {
    CallSpec {
        timeout: RequestTimeout::Interactive,
        idempotency_key: None,
    }
}

/// Writes the listing of `preview` to stderr before the question, for
/// people only: not with `--json` and not with `--quiet`.
fn show_listing(cx: &mut Context, preview: &BundlePreview) {
    if cx.output.mode == OutputMode::Json || cx.output.quiet {
        return;
    }
    let mut writer = HumanWriter::new(false, jiff::Timestamp::now(), cx.output.tz.clone());
    preview.render(&mut writer);
    for line in writer.into_string().lines() {
        cx.output.stderr.line(line);
    }
}

/// The absolute destination the daemon gets: `--output` as it is when it
/// is absolute, else under the current folder; without `--output`, the
/// suggested file name in the current folder.
///
/// # Errors
///
/// `cli.usage` when the current folder cannot be read or the path is not
/// UTF-8.
fn destination(output: Option<&str>, preview: &BundlePreview) -> Result<String, CliFailure> {
    let wanted = match output {
        Some(output) => PathBuf::from(output),
        None => PathBuf::from(file_name(&preview.suggested_file_name)),
    };
    let path = if wanted.is_absolute() {
        wanted
    } else {
        let cwd = std::env::current_dir().map_err(|error| {
            CliFailure::usage(
                "invalid_value",
                &format!("cannot read the current folder: {error}"),
            )
        })?;
        cwd.join(wanted)
    };
    clean(&path)
        .into_os_string()
        .into_string()
        .map_err(|_| CliFailure::usage("invalid_value", "the destination is not valid utf-8"))
}

/// The last component of the daemon's suggested name, so a suggestion with
/// a folder in it never moves the file elsewhere.
fn file_name(suggested: &str) -> &str {
    match Path::new(suggested).components().next_back() {
        Some(Component::Normal(name)) => name.to_str().unwrap_or(FALLBACK_FILE_NAME),
        _ => FALLBACK_FILE_NAME,
    }
}

/// `path` without `.` components, repeated slashes, or a trailing slash,
/// which never change where it points. A path with `..` gets its parent
/// folder made canonical when that folder exists, as the shell would
/// resolve it; otherwise it goes as it is and the daemon names the problem.
fn clean(path: &Path) -> PathBuf {
    let cleaned: PathBuf = path.components().collect();
    if !cleaned
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return cleaned;
    }
    match (cleaned.parent(), cleaned.file_name()) {
        (Some(parent), Some(name)) => std::fs::canonicalize(parent)
            .map(|parent| parent.join(name))
            .unwrap_or(cleaned),
        _ => cleaned,
    }
}

impl Render for BundlePreview {
    fn render(&self, out: &mut HumanWriter) {
        let rows: Vec<Vec<Cell>> = self
            .entries
            .iter()
            .map(|entry| {
                let mut size = size_text(entry.bytes.get());
                if entry.truncated {
                    size.push_str(", truncated");
                }
                vec![
                    Cell::text(&entry.path),
                    Cell::fixed(size),
                    Cell::text(&entry.description),
                ]
            })
            .collect();
        out.fitted_table(
            &["path", "size", "description"],
            &rows,
            DESCRIPTION_MIN_COLUMNS,
        );
        out.line("");
        let entries = self.entries.len();
        out.line(&format!(
            "{entries} {} · {} · {} as a zip",
            if entries == 1 { "entry" } else { "entries" },
            size_text(self.total_bytes.get()),
            size_text(self.zip_bytes.get())
        ));
        if self.recovery {
            out.line("The registry is in recovery mode, so the entries that need it are left out.");
        }
        out.line(REDACTED);
    }
}

impl Render for ExportResult {
    /// `Saved <path> (<size>).` once the daemon wrote the file; the listing
    /// went to stderr before the question.
    fn render(&self, out: &mut HumanWriter) {
        if let Some(exported) = &self.exported {
            out.line(&format!(
                "Saved {} ({}).",
                escape_display(&exported.path),
                size_text(exported.bytes.get())
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_and_extra_slashes_are_dropped_without_touching_the_disk() {
        for (path, expected) in [
            ("/Users/me/./Downloads//b.zip", "/Users/me/Downloads/b.zip"),
            ("/Users/me/Downloads/b.zip/", "/Users/me/Downloads/b.zip"),
            ("/a/b.zip", "/a/b.zip"),
        ] {
            assert_eq!(clean(Path::new(path)), PathBuf::from(expected), "{path}");
        }
    }

    #[test]
    fn a_parent_component_resolves_through_the_real_parent_folder() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("inner");
        std::fs::create_dir(&inner).unwrap();
        let canonical = std::fs::canonicalize(dir.path()).unwrap();

        assert_eq!(clean(&inner.join("../b.zip")), canonical.join("b.zip"));
        let missing = dir.path().join("gone/../../b.zip");
        assert_eq!(clean(&missing), missing, "a missing folder goes as it is");
        let ends_in_parent = dir.path().join("inner/..");
        assert_eq!(clean(&ends_in_parent), ends_in_parent);
    }

    #[test]
    fn only_the_last_plain_component_of_the_suggestion_is_used() {
        for (suggested, expected) in [
            (
                "stackly-diagnostics-20261110T080000Z.zip",
                "stackly-diagnostics-20261110T080000Z.zip",
            ),
            ("../../etc/b.zip", "b.zip"),
            ("/tmp/b.zip", "b.zip"),
            ("..", FALLBACK_FILE_NAME),
            ("", FALLBACK_FILE_NAME),
        ] {
            assert_eq!(file_name(suggested), expected, "{suggested:?}");
        }
    }
}
