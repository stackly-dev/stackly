//! `stackly env validate [--file <path>]` (spec 0044, *CLI*, AC-16): makes
//! the path absolute, asks the daemon's `env.validate`, and prints the ok
//! line for a valid manifest. The finding lines and, for a manifest that
//! is not valid, the count go to stderr; exit 2 with `cli.env_invalid`
//! keeps the validation as result.

use std::path::PathBuf;

use clap::Args;
use serde::Serialize;
use stackly_protocol::env::{EnvValidateParams, EnvValidation};
use stackly_protocol::{Call, Reply};

use super::DEFAULT_FILE;
use super::render::{counts, findings};
use crate::commands::RunCommand;
use crate::commands::path::absolute;
use crate::commands::provider::local::{esc, shows_text, to_stderr};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};
use crate::session::CallSpec;

/// `stackly env validate`.
#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// The manifest; a relative path starts at the current folder
    #[arg(long, value_name = "PATH", default_value = DEFAULT_FILE)]
    pub file: PathBuf,
}

/// The `env validate` result: the validation as the daemon sent it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Validated(pub EnvValidation);

impl Render for Validated {
    fn render(&self, out: &mut HumanWriter) {
        let validation = &self.0;
        if !validation.valid {
            return;
        }
        out.line(&format!(
            "ok {} {} services sha256:{}",
            esc(validation.name.as_deref().unwrap_or_default()),
            validation.services.len(),
            esc(validation.manifest_sha256.as_deref().unwrap_or_default())
        ));
    }
}

/// `env.validate` for the manifest at `path`.
///
/// # Errors
///
/// The daemon's error, such as `env.manifest_unreadable`.
pub async fn validate(cx: &mut Context, path: String) -> Result<EnvValidation, CliFailure> {
    let call = Call::EnvValidate(EnvValidateParams { path });
    match cx.call(call, CallSpec::control()).await? {
        Reply::EnvValidate(validation) => Ok(*validation),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

impl RunCommand for ValidateArgs {
    type Output = Validated;

    async fn run(self, cx: &mut Context) -> CommandOutput<Validated> {
        let path = match absolute(&self.file) {
            Ok(path) => path.display().to_string(),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let validation = match validate(cx, path).await {
            Ok(validation) => validation,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if shows_text(cx, false) {
            to_stderr(cx, |out| findings(out, &validation));
        }
        if validation.valid {
            CommandOutput::ok(Validated(validation))
        } else {
            let (errors, warnings) = counts(&validation);
            CommandOutput::partial(
                Validated(validation),
                CliFailure::env_invalid(errors, warnings),
            )
        }
    }
}
