//! How the spike starts, checks, and stops each provider (spec 0003,
//! Langkah spike per provider). A provider with a package in `providers/`
//! runs from that package ([`PackageRecipe`], spec 0036 AC-16); the hand
//! written recipes below are for the providers that have none yet.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

#[cfg(target_os = "macos")]
use super::package_recipe::{self, PackageRecipe};
use super::process::{self, CleanEnv, Finished, LOG_TAIL_BYTES, Server};
use super::result::{LogTail, Probe};
use crate::runtime::ports;

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
        "clickhouse" => Ok(Box::new(ClickHouse)),
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

/// Text escaped for an XML element.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn write(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// ClickHouse: a minimal config with only TCP and HTTP on loopback, data
/// and logs in the run folder, and a `default` user reachable from
/// localhost only.
// provisional: F43 (the ClickHouse package replaces this recipe)
struct ClickHouse;

impl ClickHouse {
    fn http(ctx: &Ctx, path: &str) -> Result<(u16, String)> {
        process::http_get(ctx.port(1)?, path, HTTP_TIMEOUT)
    }
}

impl Recipe for ClickHouse {
    fn start(&self, ctx: &mut Ctx) -> Result<Command> {
        ctx.ports = ports::allocate(2)?;
        let run = &ctx.run_dir;
        let folder = |name: &str| format!("{}/", xml(&path_text(&run.join(name))));
        let config = run.join("config.xml");
        let users = run.join("users.xml");
        write(
            &config,
            &format!(
                r#"<clickhouse>
    <logger>
        <level>information</level>
        <log>{log}</log>
        <errorlog>{errorlog}</errorlog>
        <console>0</console>
    </logger>
    <listen_host>127.0.0.1</listen_host>
    <tcp_port>{tcp}</tcp_port>
    <http_port>{http}</http_port>
    <path>{data}</path>
    <tmp_path>{tmp}</tmp_path>
    <user_files_path>{user_files}</user_files_path>
    <format_schema_path>{format_schemas}</format_schema_path>
    <user_scripts_path>{user_scripts}</user_scripts_path>
    <user_directories>
        <users_xml>
            <path>{users}</path>
        </users_xml>
    </user_directories>
</clickhouse>
"#,
                log = xml(&path_text(&run.join("clickhouse-server.log"))),
                errorlog = xml(&path_text(&run.join("clickhouse-server.err.log"))),
                tcp = ctx.port(0)?,
                http = ctx.port(1)?,
                data = folder("data"),
                tmp = folder("data-tmp"),
                user_files = folder("user_files"),
                format_schemas = folder("format_schemas"),
                user_scripts = folder("user_scripts"),
                users = xml(&path_text(&users)),
            ),
        )?;
        write(
            &users,
            r"<clickhouse>
    <profiles>
        <default/>
    </profiles>
    <quotas>
        <default/>
    </quotas>
    <users>
        <default>
            <password></password>
            <networks>
                <ip>127.0.0.1</ip>
                <ip>::1</ip>
            </networks>
            <profile>default</profile>
            <quota>default</quota>
        </default>
    </users>
</clickhouse>
",
        )?;
        let mut command = ctx.command("clickhouse")?;
        command
            .arg("server")
            .arg(format!("--config-file={}", path_text(&config)));
        Ok(command)
    }

    fn ready(&self, ctx: &Ctx) -> Result<bool> {
        Ok(matches!(Self::http(ctx, "/ping"), Ok((200, body)) if body.starts_with("Ok.")))
    }

    fn version(&self, ctx: &Ctx) -> Result<String> {
        match Self::http(ctx, "/?query=SELECT%20version()")? {
            (200, body) => Ok(body.trim().to_owned()),
            (code, body) => bail!("SELECT version() gave HTTP {code}: {}", body.trim()),
        }
    }

    fn normalize(&self, raw: &str) -> Option<String> {
        version_after(raw, "").and_then(|version| first_parts(&version, 4))
    }

    fn stop(&self, _ctx: &Ctx, server: &Server) -> Result<()> {
        server.signal("TERM")
    }

    fn version_args(&self) -> Vec<String> {
        vec!["--version".to_owned()]
    }

    fn log_files(&self, ctx: &Ctx) -> Vec<(String, PathBuf)> {
        vec![
            (
                "clickhouse-server.log".to_owned(),
                ctx.run_dir.join("clickhouse-server.log"),
            ),
            (
                "clickhouse-server.err.log".to_owned(),
                ctx.run_dir.join("clickhouse-server.err.log"),
            ),
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
        let clickhouse = hand_written("clickhouse");
        assert_eq!(
            clickhouse.normalize("26.8.29.10\n").as_deref(),
            Some("26.8.29.10")
        );
        assert_eq!(clickhouse.normalize("26.8"), None);
        assert!(
            recipe(&repo(), "redis")
                .err()
                .is_some_and(|err| err.to_string() == "the spike has no steps for provider redis")
        );
    }

    #[test]
    fn the_hand_written_recipes_keep_their_shape() {
        let clickhouse = hand_written("clickhouse");
        assert!(!clickhouse.uses_socket_dir());
        assert_eq!(clickhouse.version_args(), ["--version"]);
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
        assert_eq!(xml("a&b<c>"), "a&amp;b&lt;c&gt;");
    }
}
