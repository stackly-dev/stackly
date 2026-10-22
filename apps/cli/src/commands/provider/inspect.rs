//! `stackly provider inspect` (spec 0017, AC-9): one provider with its
//! identity, capabilities, version matrix, and artifacts. The `result` is
//! the `ProviderDetail` the daemon sent.

use clap::Args;
use serde::Serialize;
use stackly_protocol::catalog::{
    CapabilityStatus, CatalogArtifact, CatalogIssue, CatalogVersion, GetProviderParams,
    ProviderCapability, ProviderDetail, TargetSupport,
};
use stackly_protocol::{Call, Reply};

use super::render::{size_text, state_tone, support_tone};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::session::CallSpec;

/// The capabilities every provider has, in the words of the Catalog screen.
const BASELINE: &str = "lifecycle, readiness probe, logs";

/// `stackly provider inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// The provider ID with its namespace, such as official/nats
    #[arg(value_name = "PROVIDER")]
    pub provider: String,
}

/// The `provider inspect` result: the detail as the daemon sent it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ProviderInspect(pub ProviderDetail);

impl RunCommand for InspectArgs {
    type Output = ProviderInspect;

    async fn run(self, cx: &mut Context) -> CommandOutput<ProviderInspect> {
        let call = Call::CatalogGetProvider(GetProviderParams {
            provider: self.provider,
        });
        match cx.call(call, CallSpec::control()).await {
            Ok(Reply::CatalogGetProvider(detail)) => CommandOutput::ok(ProviderInspect(*detail)),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for ProviderInspect {
    fn render(&self, out: &mut HumanWriter) {
        let detail = &self.0;
        identity(out, detail);
        if !detail.capabilities.is_empty() {
            out.line("");
            let rows: Vec<Vec<Cell>> = detail.capabilities.iter().map(capability).collect();
            out.table(&["capability", "support"], &rows);
        }
        if !detail.versions.is_empty() {
            out.line("");
            let rows: Vec<Vec<Cell>> = detail.versions.iter().map(matrix_row).collect();
            out.table(&["version", "macos", "linux", "windows", "install"], &rows);
            let artifacts: Vec<Vec<Cell>> = detail
                .versions
                .iter()
                .flat_map(|version| version.artifacts.iter().map(artifact))
                .collect();
            if !artifacts.is_empty() {
                out.line("");
                out.table(
                    &[
                        "artifact", "target", "status", "download", "disk", "min os", "license",
                        "source",
                    ],
                    &artifacts,
                );
            }
        }
        if !detail.issues.is_empty() {
            out.line("");
            let rows: Vec<Vec<Cell>> = detail.issues.iter().map(issue).collect();
            out.table(&["file", "location", "code", "message"], &rows);
        }
    }
}

/// The block at the top: who the provider is and what state it is in. A
/// planned entry adds its note.
fn identity(out: &mut HumanWriter, detail: &ProviderDetail) {
    let summary = &detail.summary;
    let mut state = wire_name(&summary.state);
    if let Some(reason) = &summary.state_reason {
        state.push_str(&format!(" ({})", wire_name(reason)));
    }
    if let Some(note) = &summary.state_note {
        state.push_str(&format!(": {note}"));
    }
    let mut rows = vec![
        ("Name", Cell::text(&summary.display_name)),
        ("ID", Cell::text(&summary.provider)),
        (
            "Category",
            Cell::optional(summary.category.as_ref().map(wire_name).as_deref()),
        ),
        (
            "Description",
            Cell::optional(summary.description.as_deref()),
        ),
        ("State", Cell::toned(&state, state_tone(summary.state))),
        ("Publisher", Cell::optional(summary.publisher.as_deref())),
        ("Trust", Cell::text(&wire_name(&summary.trust))),
        (
            "Provider revision",
            Cell::optional(summary.provider_revision.as_deref()),
        ),
        (
            "Manifest",
            Cell::text(
                &detail
                    .manifest_sha256
                    .as_ref()
                    .map_or_else(String::new, |digest| format!("sha256:{digest}")),
            ),
        ),
    ];
    if !detail.capabilities.is_empty() {
        rows.push(("Baseline", Cell::fixed(BASELINE)));
    }
    if let Some(note) = &detail.note {
        rows.push(("Note", Cell::text(note)));
    }
    out.kv(&rows);
}

/// `<capability>  supported` or `not supported`.
fn capability(status: &CapabilityStatus) -> Vec<Cell> {
    vec![
        Cell::fixed(capability_label(status.capability)),
        Cell::fixed(if status.supported {
            "supported"
        } else {
            "not supported"
        }),
    ]
}

/// The label of a capability, as the Catalog screen writes it.
const fn capability_label(capability: ProviderCapability) -> &'static str {
    match capability {
        ProviderCapability::MultiInstance => "Multiple instances",
        ProviderCapability::Persistence => "Persistent data",
        ProviderCapability::Reload => "Reload without restart",
        ProviderCapability::Metrics => "Metrics",
        ProviderCapability::Tls => "TLS",
        ProviderCapability::ColdBackup => "Cold backup",
        ProviderCapability::OnlineBackup => "Online backup",
        ProviderCapability::LogicalBackup => "Logical backup",
        ProviderCapability::Unrecognized => "Unknown capability",
    }
}

/// One row of the version matrix.
fn matrix_row(version: &CatalogVersion) -> Vec<Cell> {
    let mut row = vec![Cell::text(&version.version)];
    for os in stackly_protocol::catalog::TargetOs::MATRIX {
        row.push(
            version
                .support
                .iter()
                .find(|cell| cell.os == os)
                .map_or_else(|| Cell::fixed(""), cell),
        );
    }
    row.push(Cell::fixed(install(version)));
    row
}

/// `<status> (<note or reason>)`: the note when the cell has one, since it
/// says more than the reason code.
fn cell(support: &TargetSupport) -> Cell {
    let mut text = wire_name(&support.status);
    if let Some(why) = support
        .note
        .clone()
        .or_else(|| support.reason.as_ref().map(wire_name))
    {
        text.push_str(&format!(" ({why})"));
    }
    Cell::toned(&text, support_tone(support.status))
}

/// `yes`, or `no (<reason>)` for this Mac.
fn install(version: &CatalogVersion) -> String {
    match (version.install.installable, &version.install.reason) {
        (true, _) => "yes".to_owned(),
        (false, Some(reason)) => format!("no ({})", wire_name(reason)),
        (false, None) => "no".to_owned(),
    }
}

/// One row of the artifact table.
fn artifact(artifact: &CatalogArtifact) -> Vec<Cell> {
    let size = |bytes: Option<stackly_protocol::Count>| {
        Cell::fixed(bytes.map_or_else(String::new, |bytes| size_text(bytes.get())))
    };
    vec![
        Cell::text(&artifact.id),
        Cell::text(&format!(
            "{}/{}",
            wire_name(&artifact.os),
            wire_name(&artifact.arch)
        )),
        Cell::text(&wire_name(&artifact.status)),
        size(artifact.download_size),
        size(artifact.installed_size),
        Cell::optional(artifact.min_os.as_deref()),
        Cell::optional(artifact.license.as_deref()),
        Cell::optional(artifact.source.as_deref()),
    ]
}

/// One row of the issue table of an invalid entry.
fn issue(issue: &CatalogIssue) -> Vec<Cell> {
    let code = issue.reason.as_ref().map_or_else(
        || issue.code.clone(),
        |reason| format!("{} ({reason})", issue.code),
    );
    vec![
        Cell::text(&issue.file),
        Cell::text(&issue.location),
        Cell::text(&code),
        Cell::text(&issue.message),
    ]
}
