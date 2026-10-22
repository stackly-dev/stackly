//! `stackly runtime inspect <runtime-id>` (spec 0018, AC-16): one runtime
//! with its artifact, dependencies, signatures, and compatibility records.

use clap::Args;
use stackly_protocol::runtime::{
    ArtifactInfo, CompatibilityRecordView, RuntimeDetail, SignatureInfo,
};
use uuid::Uuid;

use super::list::state_tone;
use super::read_runtime;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::escape::escape_display;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};

/// `stackly runtime inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// The runtime ID, a whole UUID
    #[arg(value_name = "RUNTIME_ID")]
    pub id: RuntimeIdArg,
}

/// A whole runtime ID, in either case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeIdArg(Uuid);

impl RuntimeIdArg {
    /// The ID; its text form is always lower case.
    #[must_use]
    pub const fn get(self) -> Uuid {
        self.0
    }
}

impl std::str::FromStr for RuntimeIdArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if text.len() != 36 {
            return Err(
                "expected a whole runtime id, such as 0192f3a6-0000-7000-8000-0000000000a1"
                    .to_owned(),
            );
        }
        Uuid::try_parse(text)
            .map(Self)
            .map_err(|_| "expected a whole runtime id in hex with hyphens".to_owned())
    }
}

impl RunCommand for InspectArgs {
    type Output = RuntimeDetail;

    async fn run(self, cx: &mut Context) -> CommandOutput<RuntimeDetail> {
        let id: Uuid = self.id.get();
        match read_runtime(cx, id).await {
            Ok(detail) => CommandOutput::ok(detail),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for RuntimeDetail {
    fn render(&self, out: &mut HumanWriter) {
        let summary = &self.summary;
        let time = |out: &HumanWriter, at: Option<stackly_protocol::Timestamp>| {
            at.map_or_else(String::new, |at| out.time_and_age(at))
        };
        let users: Vec<String> = summary
            .used_by
            .iter()
            .map(|user| {
                if user.retained {
                    format!("{} (retained)", user.name)
                } else {
                    user.name.clone()
                }
            })
            .collect();
        let rows = vec![
            ("ID", Cell::fixed(summary.id.to_string())),
            ("Software", Cell::text(&summary.software)),
            ("Name", Cell::text(&summary.display_name)),
            ("Version", Cell::text(&summary.version)),
            ("Line", Cell::text(&summary.line)),
            (
                "Target",
                Cell::fixed(format!(
                    "{}-{}",
                    wire_name(&summary.target.os),
                    wire_name(&summary.target.arch)
                )),
            ),
            ("Source", Cell::fixed(wire_name(&summary.install_mode))),
            (
                "State",
                Cell::toned(&wire_name(&summary.state), state_tone(summary.state)),
            ),
            (
                "Support",
                Cell::fixed(format!(
                    "{} ({})",
                    wire_name(&summary.support),
                    wire_name(&summary.support_reason)
                )),
            ),
            ("Path", Cell::text(&self.path)),
            ("Installed", Cell::fixed(time(out, summary.installed_at))),
            ("Verified", Cell::fixed(time(out, summary.last_verified_at))),
            ("Used by", Cell::text(&users.join(", "))),
            ("Executables", Cell::text(&self.executables.join(", "))),
        ];
        out.kv(&rows);
        if let Some(artifact) = &self.artifact {
            render_artifact(out, artifact);
        }
        if !self.compatibility.is_empty() {
            render_compatibility(out, &self.compatibility);
        }
    }
}

fn render_artifact(out: &mut HumanWriter, artifact: &ArtifactInfo) {
    out.line("");
    let size = |bytes: Option<stackly_protocol::Count>| {
        bytes.map_or_else(String::new, |bytes| format!("{} bytes", bytes.get()))
    };
    out.kv(&[
        (
            "Artifact",
            Cell::text(artifact.catalog_id.as_deref().unwrap_or_default()),
        ),
        ("Digest", Cell::fixed(format!("sha256:{}", artifact.sha256))),
        ("Download", Cell::fixed(size(artifact.size))),
        ("Unpacked", Cell::fixed(size(artifact.unpacked_size))),
        (
            "Format",
            Cell::fixed(artifact.format.as_ref().map(wire_name).unwrap_or_default()),
        ),
        (
            "Minimum macOS",
            Cell::text(artifact.minimum_os.as_deref().unwrap_or_default()),
        ),
        (
            "Source",
            Cell::text(artifact.source.as_deref().unwrap_or_default()),
        ),
    ]);
    if let Some(signature) = &artifact.signature {
        render_signature(out, signature);
    }
    if !artifact.dependencies.is_empty() {
        out.line("");
        out.line("Dependencies");
        let rows: Vec<Vec<Cell>> = artifact
            .dependencies
            .iter()
            .map(|dependency| {
                vec![
                    Cell::text(&dependency.name),
                    Cell::fixed(wire_name(&dependency.kind)),
                    Cell::text(dependency.version.as_deref().unwrap_or_default()),
                    Cell::text(dependency.license.as_deref().unwrap_or_default()),
                ]
            })
            .collect();
        out.indented_table(2, &["name", "kind", "version", "license"], &rows);
    }
    for warning in &artifact.warnings {
        out.line(&format!("warning: {}", escape_display(warning)));
    }
}

fn render_signature(out: &mut HumanWriter, signature: &SignatureInfo) {
    out.line("");
    out.line(&format!(
        "Signature  policy {}, {} Mach-O files{}",
        wire_name(&signature.policy),
        signature.mach_o_files.get(),
        if signature.adhoc_signed_at_install {
            ", signed ad hoc at install"
        } else {
            ""
        }
    ));
    if !signature.executables.is_empty() {
        let rows: Vec<Vec<Cell>> = signature
            .executables
            .iter()
            .map(|executable| {
                vec![
                    Cell::text(&executable.path),
                    Cell::fixed(wire_name(&executable.kind)),
                    Cell::text(executable.team_id.as_deref().unwrap_or_default()),
                ]
            })
            .collect();
        out.indented_table(2, &["executable", "signature", "team"], &rows);
    }
}

fn render_compatibility(out: &mut HumanWriter, records: &[CompatibilityRecordView]) {
    out.line("");
    out.line("Compatibility records");
    let rows: Vec<Vec<Cell>> = records
        .iter()
        .map(|record| {
            vec![
                Cell::fixed(wire_name(&record.scope)),
                Cell::fixed(wire_name(&record.result)),
                Cell::fixed(format!(
                    "macOS {} {}",
                    record.os_version,
                    wire_name(&record.arch)
                )),
                Cell::fixed(out.time(record.tested_at)),
                Cell::text(&record.checks.join(", ")),
            ]
        })
        .collect();
    out.indented_table(2, &["scope", "result", "host", "tested", "checks"], &rows);
}
