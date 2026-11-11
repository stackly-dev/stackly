//! `stackly provider validate <dir>` (spec 0036, *CLI*, AC-5): makes the
//! folder absolute, asks the daemon's `provider.validate`, and prints the
//! lines of `cargo xtask provider check` and a count. The daemon reads the
//! folder; the CLI never reads a package or runs anything from it.
//!
//! Exit 0 when the package is publishable; otherwise
//! `cli.provider_invalid` (exit 2) with the result kept.

use std::path::{Component, Path, PathBuf};

use clap::Args;
use serde::Serialize;
use stackly_protocol::provider::{
    FindingSeverity, ProviderValidateParams, ProviderValidation, ValidationFinding,
};
use stackly_protocol::{Call, Reply};

use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};
use crate::session::CallSpec;

/// `stackly provider validate`.
#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// The provider package folder; a relative path starts at the current
    /// folder
    #[arg(value_name = "DIR")]
    pub dir: PathBuf,
}

/// The `provider validate` result: the validation as the daemon sent it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ProviderValidate(pub ProviderValidation);

impl RunCommand for ValidateArgs {
    type Output = ProviderValidate;

    async fn run(self, cx: &mut Context) -> CommandOutput<ProviderValidate> {
        let path = match absolute(&self.dir) {
            Ok(path) => path,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::ProviderValidate(ProviderValidateParams {
            path: path.display().to_string(),
        });
        match cx.call(call, CallSpec::control()).await {
            Ok(Reply::ProviderValidate(validation)) => {
                let (errors, warnings) = counts(&validation);
                let result = ProviderValidate(*validation);
                if result.0.publishable {
                    CommandOutput::ok(result)
                } else {
                    CommandOutput::partial(result, CliFailure::provider_invalid(errors, warnings))
                }
            }
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for ProviderValidate {
    fn render(&self, out: &mut HumanWriter) {
        let validation = &self.0;
        let mut warnings = Vec::new();
        for finding in &validation.findings {
            if finding.severity == FindingSeverity::Warning {
                warnings.push(finding);
            } else {
                out.line(&line(&validation.folder, finding));
            }
        }
        if validation.publishable
            && let Some(identity) = &validation.identity
        {
            out.line(&format!(
                "ok {} {} {}",
                identity.provider, identity.revision, identity.manifest_sha256
            ));
        }
        for finding in warnings {
            out.line(&format!("warning: {}", line(&validation.folder, finding)));
        }
        let omitted = u64::from(validation.omitted);
        if omitted > 0 {
            out.line(&format!("{omitted} more finding(s) not shown"));
        }
        let (errors, warnings) = counts(validation);
        out.line(&format!("{errors} error(s), {warnings} warning(s)"));
    }
}

/// `<folder>: <file> <location>: <code>: <message>`, the line of `cargo
/// xtask provider check` (spec 0036 AC-4).
fn line(folder: &str, finding: &ValidationFinding) -> String {
    format!(
        "{folder}: {} {}: {}: {}",
        finding.file, finding.location, finding.code, finding.message
    )
}

/// Errors and warnings, counting the findings left out as errors, since
/// only a package that is not publishable has that many.
fn counts(validation: &ProviderValidation) -> (u64, u64) {
    let warnings = validation
        .findings
        .iter()
        .filter(|finding| finding.severity == FindingSeverity::Warning)
        .count() as u64;
    let total = u64::from(validation.finding_count);
    (total.saturating_sub(warnings), warnings)
}

/// `dir` joined to the current folder when relative, then normalized
/// without touching the filesystem: `.` is dropped and `..` removes the
/// component before it. `import` uses it too.
///
/// # Errors
///
/// `cli.usage` when the current folder cannot be read.
pub(super) fn absolute(dir: &Path) -> Result<PathBuf, CliFailure> {
    let joined = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        let cwd = std::env::current_dir().map_err(|error| {
            CliFailure::usage(
                "invalid_value",
                &format!("cannot read the current folder: {error}"),
            )
        })?;
        cwd.join(dir)
    };
    Ok(normalize(&joined))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizing_drops_dots_and_folds_parents_without_the_filesystem() {
        assert_eq!(
            normalize(Path::new("/a/./b/../c/providers/./mytool")),
            Path::new("/a/c/providers/mytool")
        );
        assert_eq!(normalize(Path::new("/../x")), Path::new("/x"));
        assert_eq!(
            absolute(Path::new("/srv/p/../q")).unwrap(),
            Path::new("/srv/q")
        );
        assert!(absolute(Path::new("rel")).unwrap().is_absolute());
    }
}
