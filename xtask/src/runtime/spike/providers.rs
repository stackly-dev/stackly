//! How the spike starts, checks, and stops each provider (spec 0003,
//! Langkah spike per provider). A provider with a package in `providers/`
//! runs from that package ([`PackageRecipe`], spec 0036 AC-16); the hand
//! written recipes below are for the providers that have none yet.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

#[cfg(target_os = "macos")]
use super::package_recipe::{self, PackageRecipe};
use super::process::{self, CleanEnv, Finished, LOG_TAIL_BYTES, Server};
use super::result::{LogTail, Probe};

/// How long one short client command (a ping, a version query) may run.
pub const CLIENT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long one HTTP probe may take.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(2);

/// What a recipe works with.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The extracted artifact.
    pub artifact_root: PathBuf,
    /// The exact version of the inventory entry.
    pub version: String,
    /// The release line of the inventory entry.
    pub line: String,
    /// The executables of the inventory entry, relative to the root.
    pub executables: Vec<String>,
    /// The run folder: working folder, configuration, and logs.
    pub run_dir: PathBuf,
    /// The environment of every provider process.
    pub env: CleanEnv,
    /// `/tmp/sk-<8 hex>`, when the recipe uses a socket.
    pub socket_dir: Option<PathBuf>,
    /// The loopback ports the server may listen on.
    pub ports: Vec<u16>,
    /// Output of one shot commands such as `--initialize`.
    pub logs: Vec<LogTail>,
}

impl Ctx {
    /// The absolute path of the listed executable whose file name is
    /// `name`.
    pub fn executable(&self, name: &str) -> Result<PathBuf> {
        self.executables
            .iter()
            .find(|path| path.rsplit('/').next() == Some(name))
            .map(|path| self.artifact_root.join(path))
            .with_context(|| format!("the inventory entry does not list the {name} executable"))
    }

    /// A clean command for the listed executable `name`.
    pub fn command(&self, name: &str) -> Result<Command> {
        Ok(self.env.command(&self.executable(name)?))
    }

    /// The private socket folder.
    pub fn socket_dir(&self) -> Result<&Path> {
        self.socket_dir
            .as_deref()
            .context("this recipe has no socket folder")
    }

    /// Runs a one shot command with `input` on stdin and keeps the tail of
    /// its output.
    pub fn run_logged_input(
        &mut self,
        name: &str,
        command: Command,
        input: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<Finished> {
        let finished = process::run_with_input(command, input, timeout)?;
        self.logs.push(LogTail {
            process: name.to_owned(),
            stdout: last_bytes(&finished.stdout),
            stderr: last_bytes(&finished.stderr),
        });
        Ok(finished)
    }

    pub(super) fn port(&self, index: usize) -> Result<u16> {
        self.ports
            .get(index)
            .copied()
            .context("the port was not allocated")
    }
}

/// The start, readiness, version, probes, and stop of one provider.
pub trait Recipe {
    /// True when the server listens on a Unix socket in `/tmp/sk-*`.
    fn uses_socket_dir(&self) -> bool {
        false
    }

    /// Prepares data before the first start, such as `--initialize`.
    fn init(&self, _ctx: &mut Ctx) -> Result<()> {
        Ok(())
    }

    /// Allocates the ports, writes the configuration, and returns the
    /// server command. It runs right before the server starts.
    fn start(&self, ctx: &mut Ctx) -> Result<Command>;

    /// One readiness attempt: true once the server answers.
    fn ready(&self, ctx: &Ctx) -> Result<bool>;

    /// The version text the running server or its binary reports.
    fn version(&self, ctx: &Ctx) -> Result<String>;

    /// The version in the inventory's form, from the raw text.
    fn normalize(&self, raw: &str) -> Option<String>;

    /// Extra probes on the running server.
    fn probes(&self, _ctx: &Ctx) -> Vec<Probe> {
        Vec::new()
    }

    /// Asks the server to stop; the worker waits for the exit.
    fn stop(&self, ctx: &Ctx, server: &Server) -> Result<()>;

    /// Arguments that make the main executable print its version, for the
    /// quarantine probe.
    fn version_args(&self) -> Vec<String>;

    /// Log files the server writes itself, as `(name, path)`.
    fn log_files(&self, _ctx: &Ctx) -> Vec<(String, PathBuf)> {
        Vec::new()
    }
}

/// The recipe of `provider`: a [`PackageRecipe`] from the package in
/// `<repo>/providers/` whose `metadata.software` it is.
///
/// # Errors
///
/// Fails for a provider the spike has no steps for, and for a package that
/// is not publishable or has no `runtime.versionCheck`.
pub fn recipe(repo: &Path, provider: &str) -> Result<Box<dyn Recipe>> {
    #[cfg(target_os = "macos")]
    if let Some(dir) = package_recipe::find_package(repo, provider)? {
        return Ok(Box::new(PackageRecipe::load(&dir, provider)?));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = repo;
    bail!("the spike has no steps for provider {provider}")
}

/// The last [`LOG_TAIL_BYTES`] of a text, cut at a character boundary.
pub fn last_bytes(text: &str) -> String {
    let max = usize::try_from(LOG_TAIL_BYTES).unwrap_or(usize::MAX);
    if text.len() <= max {
        return text.to_owned();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    #[test]
    fn a_provider_without_a_package_has_no_recipe() {
        assert!(
            recipe(&repo(), "redis")
                .err()
                .is_some_and(|err| err.to_string() == "the spike has no steps for provider redis")
        );
    }

    #[test]
    fn tails_and_config_strings_are_safe() {
        assert_eq!(last_bytes("abc"), "abc");
        let long = format!("{}Ü", "x".repeat(5000));
        assert!(last_bytes(&long).ends_with('Ü'));
        assert!(last_bytes(&long).len() <= 4096);
    }
}
