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
/// How long a data folder initialization may take.
const INIT_TIMEOUT: Duration = Duration::from_secs(300);

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
    /// The data folder, created by the recipe when it needs one.
    pub data_dir: PathBuf,
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

    /// Runs a one shot command and keeps the tail of its output.
    pub fn run_logged(
        &mut self,
        name: &str,
        command: Command,
        timeout: Duration,
    ) -> Result<Finished> {
        self.run_logged_input(name, command, None, timeout)
    }

    /// [`Ctx::run_logged`] with `input` on stdin.
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

/// The recipe of `provider`: a [`PackageRecipe`] when a package in
/// `<repo>/providers/` has that `metadata.software`, otherwise the hand
/// written recipe.
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
    match provider {
        "mysql" => Ok(Box::new(MySql)),
        _ => bail!("the spike has no steps for provider {provider}"),
    }
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

/// The first run of digits and dots in `text` that starts right after
/// `prefix` (or anywhere when `prefix` is empty), trimmed of a final dot.
fn version_after(text: &str, prefix: &str) -> Option<String> {
    let mut rest = text;
    while let Some(at) = rest.find(prefix) {
        let after = &rest[at + prefix.len()..];
        let digits: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let digits = digits.trim_end_matches('.');
        if digits.starts_with(|c: char| c.is_ascii_digit()) && digits.contains('.') {
            return Some(digits.to_owned());
        }
        if prefix.is_empty() {
            rest = &rest[rest.char_indices().nth(1).map_or(rest.len(), |(i, _)| i)..];
        } else {
            rest = after;
        }
        if rest.is_empty() {
            break;
        }
    }
    None
}

/// The first `count` dot separated parts of a version, when it has them.
fn first_parts(version: &str, count: usize) -> Option<String> {
    let parts: Vec<&str> = version.split('.').collect();
    (parts.len() >= count && parts[..count].iter().all(|p| !p.is_empty()))
        .then(|| parts[..count].join("."))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The text a short command printed, or an error naming its failure.
fn output_of(finished: &Finished, what: &str) -> Result<String> {
    match finished.status {
        None => bail!(
            "{what} did not finish within {} s",
            CLIENT_TIMEOUT.as_secs()
        ),
        Some(status) if status.success() => Ok(finished.stdout.clone()),
        Some(status) => bail!(
            "{what} failed with {}: {}",
            process::describe(status),
            finished.complaint()
        ),
    }
}

/// MySQL: `--initialize-insecure`, then a server with no networking at
/// all, reachable only through a socket in a private folder.
// provisional: F42 (the MySQL package replaces this recipe)
struct MySql;

impl MySql {
    fn socket(ctx: &Ctx) -> Result<String> {
        Ok(path_text(&ctx.socket_dir()?.join("m.sock")))
    }

    fn client(ctx: &Ctx, name: &str) -> Result<Command> {
        let mut command = ctx.command(name)?;
        command
            .arg("--no-defaults")
            .arg("-u")
            .arg("root")
            .arg(format!("--socket={}", Self::socket(ctx)?));
        Ok(command)
    }
}

impl Recipe for MySql {
    fn uses_socket_dir(&self) -> bool {
        true
    }

    fn init(&self, ctx: &mut Ctx) -> Result<()> {
        let mut command = ctx.command("mysqld")?;
        command
            .arg("--no-defaults")
            .arg("--initialize-insecure")
            .arg(format!("--basedir={}", path_text(&ctx.artifact_root)))
            .arg(format!("--datadir={}", path_text(&ctx.data_dir)))
            .arg(format!(
                "--log-error={}",
                path_text(&ctx.run_dir.join("init.err"))
            ));
        let finished = ctx.run_logged("init", command, INIT_TIMEOUT)?;
        match finished.status {
            Some(status) if status.success() => Ok(()),
            Some(status) => bail!(
                "mysqld --initialize-insecure failed with {}",
                process::describe(status)
            ),
            None => bail!(
                "mysqld --initialize-insecure did not finish within {} s",
                INIT_TIMEOUT.as_secs()
            ),
        }
    }

    fn start(&self, ctx: &mut Ctx) -> Result<Command> {
        let mut command = ctx.command("mysqld")?;
        command
            .arg("--no-defaults")
            .arg(format!("--basedir={}", path_text(&ctx.artifact_root)))
            .arg(format!("--datadir={}", path_text(&ctx.data_dir)))
            .arg(format!("--socket={}", Self::socket(ctx)?))
            .arg("--skip-networking")
            .arg("--mysqlx=OFF")
            .arg(format!(
                "--pid-file={}",
                path_text(&ctx.run_dir.join("mysqld.pid"))
            ))
            .arg(format!(
                "--log-error={}",
                path_text(&ctx.run_dir.join("mysqld.err"))
            ));
        Ok(command)
    }

    fn ready(&self, ctx: &Ctx) -> Result<bool> {
        let mut command = Self::client(ctx, "mysqladmin")?;
        command.arg("ping");
        Ok(process::run(command, CLIENT_TIMEOUT)?.success())
    }

    fn version(&self, ctx: &Ctx) -> Result<String> {
        let mut command = Self::client(ctx, "mysql")?;
        command.args(["-N", "-B", "-e", "select version()"]);
        Ok(
            output_of(&process::run(command, CLIENT_TIMEOUT)?, "select version()")?
                .trim()
                .to_owned(),
        )
    }

    fn normalize(&self, raw: &str) -> Option<String> {
        version_after(raw, "").and_then(|version| first_parts(&version, 3))
    }

    fn stop(&self, ctx: &Ctx, _server: &Server) -> Result<()> {
        let mut command = Self::client(ctx, "mysqladmin")?;
        command.arg("shutdown");
        output_of(
            &process::run(command, CLIENT_TIMEOUT)?,
            "mysqladmin shutdown",
        )
        .map(drop)
    }

    fn version_args(&self) -> Vec<String> {
        vec!["--version".to_owned()]
    }

    fn log_files(&self, ctx: &Ctx) -> Vec<(String, PathBuf)> {
        vec![
            ("init.err".to_owned(), ctx.run_dir.join("init.err")),
            ("mysqld.err".to_owned(), ctx.run_dir.join("mysqld.err")),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn hand_written(provider: &str) -> Box<dyn Recipe> {
        recipe(&repo(), provider).unwrap()
    }

    #[test]
    fn versions_of_the_hand_written_recipes_are_normalized_as_before() {
        let mysql = hand_written("mysql");
        assert_eq!(mysql.normalize("8.4.11\n").as_deref(), Some("8.4.11"));
        assert_eq!(
            mysql.normalize("9.7.2-commercial").as_deref(),
            Some("9.7.2")
        );
        assert!(
            recipe(&repo(), "redis")
                .err()
                .is_some_and(|err| err.to_string() == "the spike has no steps for provider redis")
        );
    }

    #[test]
    fn the_hand_written_recipes_keep_their_shape() {
        let mysql = hand_written("mysql");
        assert!(mysql.uses_socket_dir());
        assert_eq!(mysql.version_args(), ["--version"]);
    }

    #[test]
    fn version_after_skips_text_that_is_not_a_version() {
        assert_eq!(
            version_after("v-dev then v2.15.1.", "v").as_deref(),
            Some("2.15.1")
        );
        assert_eq!(
            version_after("Version: 3.7.15", "").as_deref(),
            Some("3.7.15")
        );
        assert_eq!(version_after("no version", ""), None);
    }

    #[test]
    fn tails_and_config_strings_are_safe() {
        assert_eq!(last_bytes("abc"), "abc");
        let long = format!("{}Ü", "x".repeat(5000));
        assert!(last_bytes(&long).ends_with('Ü'));
        assert!(last_bytes(&long).len() <= 4096);
    }
}
