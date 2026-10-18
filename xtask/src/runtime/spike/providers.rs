//! How the spike starts, checks, and stops each provider (spec 0003,
//! Langkah spike per provider). PostgreSQL and Valkey run from maintainer
//! artifacts built on this Mac.
// provisional: F9 (the provider manifest takes over this lifecycle)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::process::{self, CleanEnv, Finished, LOG_TAIL_BYTES, Server};
use super::result::{LogTail, Probe, StepStatus};
use crate::runtime::ports;

/// How long one short client command (a ping, a version query) may run.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long one HTTP probe may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a data folder initialization may take.
const INIT_TIMEOUT: Duration = Duration::from_secs(300);

/// What a recipe works with.
#[derive(Debug)]
pub struct Ctx {
    /// The extracted artifact.
    pub artifact_root: PathBuf,
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
        let finished = process::run(command, timeout)?;
        self.logs.push(LogTail {
            process: name.to_owned(),
            stdout: last_bytes(&finished.stdout),
            stderr: last_bytes(&finished.stderr),
        });
        Ok(finished)
    }

    fn port(&self, index: usize) -> Result<u16> {
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
    fn version_args(&self) -> &'static [&'static str];

    /// Log files the server writes itself, as `(name, path)`.
    fn log_files(&self, _ctx: &Ctx) -> Vec<(String, PathBuf)> {
        Vec::new()
    }
}

/// The recipe of `provider`, when the spike knows it.
pub fn recipe(provider: &str) -> Option<Box<dyn Recipe>> {
    match provider {
        "nats" => Some(Box::new(Nats)),
        "traefik" => Some(Box::new(Traefik)),
        "clickhouse" => Some(Box::new(ClickHouse)),
        "mysql" => Some(Box::new(MySql)),
        "postgresql" => Some(Box::new(Postgres)),
        "valkey" => Some(Box::new(Valkey)),
        _ => None,
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

/// The version at the very start of `text` (after white space), cut to its
/// first `count` parts, as `^[0-9]+(\.[0-9]+){count - 1}` would take it.
fn leading_version(text: &str, count: usize) -> Option<String> {
    let digits: String = text
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    first_parts(&digits, count)
}

/// Fails unless a one shot command exited 0 within `timeout`.
fn require_success(finished: &Finished, what: &str, timeout: Duration) -> Result<()> {
    match finished.status {
        Some(status) if status.success() => Ok(()),
        Some(status) => bail!(
            "{what} failed with {}: {}",
            process::describe(status),
            finished.complaint()
        ),
        None => bail!("{what} did not finish within {} s", timeout.as_secs()),
    }
}

/// A quoted string for a NATS configuration file.
fn nats_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
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

fn http_probe(name: &str, port: u16, path: &str) -> Probe {
    match process::http_get(port, path, HTTP_TIMEOUT) {
        Ok((200, body)) => Probe {
            name: name.to_owned(),
            status: StepStatus::Pass,
            detail: format!("GET {path} gave 200 {}", body.trim()),
        },
        Ok((code, body)) => Probe {
            name: name.to_owned(),
            status: StepStatus::Fail,
            detail: format!("GET {path} gave {code} {}", body.trim()),
        },
        Err(err) => Probe {
            name: name.to_owned(),
            status: StepStatus::Fail,
            detail: format!("{err:#}"),
        },
    }
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

/// NATS: a config file with client and monitoring ports on loopback and
/// JetStream in the run folder.
struct Nats;

impl Recipe for Nats {
    fn start(&self, ctx: &mut Ctx) -> Result<Command> {
        ctx.ports = ports::allocate(2)?;
        let config = ctx.run_dir.join("nats.conf");
        let store = ctx.run_dir.join("jetstream");
        write(
            &config,
            &format!(
                "listen: {}\nhttp: {}\njetstream {{\n  store_dir: {}\n}}\n",
                nats_string(&format!("127.0.0.1:{}", ctx.port(0)?)),
                nats_string(&format!("127.0.0.1:{}", ctx.port(1)?)),
                nats_string(&path_text(&store)),
            ),
        )?;
        let mut command = ctx.command("nats-server")?;
        command.arg("-c").arg(&config);
        Ok(command)
    }

    fn ready(&self, ctx: &Ctx) -> Result<bool> {
        Ok(matches!(
            process::http_get(ctx.port(1)?, "/healthz", HTTP_TIMEOUT),
            Ok((200, _))
        ))
    }

    fn version(&self, ctx: &Ctx) -> Result<String> {
        let mut command = ctx.command("nats-server")?;
        command.arg("--version");
        output_of(
            &process::run(command, CLIENT_TIMEOUT)?,
            "nats-server --version",
        )
    }

    fn normalize(&self, raw: &str) -> Option<String> {
        version_after(raw, "v")
    }

    fn probes(&self, ctx: &Ctx) -> Vec<Probe> {
        match ctx.port(1) {
            Ok(port) => vec![http_probe(
                "jetstream",
                port,
                "/healthz?js-enabled-only=true",
            )],
            Err(err) => vec![Probe {
                name: "jetstream".to_owned(),
                status: StepStatus::Fail,
                detail: format!("{err:#}"),
            }],
        }
    }

    fn stop(&self, _ctx: &Ctx, server: &Server) -> Result<()> {
        server.signal("TERM")
    }

    fn version_args(&self) -> &'static [&'static str] {
        &["--version"]
    }
}

/// Traefik: one loopback entry point with ping, an empty file provider,
/// no API, no version check, and no telemetry.
struct Traefik;

impl Recipe for Traefik {
    fn start(&self, ctx: &mut Ctx) -> Result<Command> {
        ctx.ports = ports::allocate(1)?;
        let dynamic = ctx.run_dir.join("dynamic");
        process::create_dir(&dynamic)?;
        let mut command = ctx.command("traefik")?;
        command
            .arg(format!(
                "--entryPoints.web.address=127.0.0.1:{}",
                ctx.port(0)?
            ))
            .arg("--ping=true")
            .arg("--ping.entryPoint=web")
            .arg(format!(
                "--providers.file.directory={}",
                path_text(&dynamic)
            ))
            .arg("--api=false")
            .arg("--global.checknewversion=false")
            .arg("--global.sendanonymoususage=false");
        Ok(command)
    }

    fn ready(&self, ctx: &Ctx) -> Result<bool> {
        Ok(matches!(
            process::http_get(ctx.port(0)?, "/ping", HTTP_TIMEOUT),
            Ok((200, _))
        ))
    }

    fn version(&self, ctx: &Ctx) -> Result<String> {
        let mut command = ctx.command("traefik")?;
        command.arg("version");
        let text = output_of(&process::run(command, CLIENT_TIMEOUT)?, "traefik version")?;
        text.lines()
            .find_map(|line| line.trim().strip_prefix("Version:"))
            .map(|version| version.trim().to_owned())
            .with_context(|| format!("traefik version printed no Version: line: {}", text.trim()))
    }

    fn normalize(&self, raw: &str) -> Option<String> {
        version_after(raw, "")
    }

    fn stop(&self, _ctx: &Ctx, server: &Server) -> Result<()> {
        server.signal("TERM")
    }

    fn version_args(&self) -> &'static [&'static str] {
        &["version"]
    }
}

/// ClickHouse: a minimal config with only TCP and HTTP on loopback, data
/// and logs in the run folder, and a `default` user reachable from
/// localhost only.
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

    fn version_args(&self) -> &'static [&'static str] {
        &["--version"]
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

/// MySQL: `--initialize-insecure`, then a server with no networking at
/// all, reachable only through a socket in a private folder.
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

    fn version_args(&self) -> &'static [&'static str] {
        &["--version"]
    }

    fn log_files(&self, ctx: &Ctx) -> Vec<(String, PathBuf)> {
        vec![
            ("init.err".to_owned(), ctx.run_dir.join("init.err")),
            ("mysqld.err".to_owned(), ctx.run_dir.join("mysqld.err")),
        ]
    }
}

/// The database user of the PostgreSQL spike; it keeps the OS user name
/// out of the evidence.
const POSTGRES_USER: &str = "stackly";

/// PostgreSQL: `initdb` with the ICU locale provider, then `bin/postgres`
/// as a direct child that listens only on a socket in a private folder.
struct Postgres;

impl Postgres {
    /// `psql` against the spike server, without `.psqlrc`, stopping at the
    /// first error, printing bare values, and running each of `sql` in
    /// order.
    fn psql(ctx: &Ctx, sql: &[&str]) -> Result<Command> {
        let mut command = ctx.command("psql")?;
        command
            .arg("-X")
            .arg("-q")
            .args(["-v", "ON_ERROR_STOP=1"])
            .arg("-h")
            .arg(ctx.socket_dir()?)
            .arg("-p")
            .arg(ctx.port(0)?.to_string())
            .args(["-U", POSTGRES_USER, "-d", "postgres", "-At"]);
        for statement in sql {
            command.arg("-c").arg(statement);
        }
        Ok(command)
    }

    /// The bare output of `sql`, trimmed.
    fn query(ctx: &Ctx, sql: &[&str]) -> Result<String> {
        let what = sql.last().copied().unwrap_or("psql");
        Ok(
            output_of(&process::run(Self::psql(ctx, sql)?, CLIENT_TIMEOUT)?, what)?
                .trim()
                .to_owned(),
        )
    }

    /// A probe that passes when the last of `sql` prints `expected`.
    fn query_probe(ctx: &Ctx, name: &str, sql: &[&str], expected: &str) -> Probe {
        match Self::query(ctx, sql) {
            Ok(found) if found == expected => Probe {
                name: name.to_owned(),
                status: StepStatus::Pass,
                detail: format!("{} gave {found}", sql.join("; ")),
            },
            Ok(found) => Probe {
                name: name.to_owned(),
                status: StepStatus::Fail,
                detail: format!("{} gave `{found}`, not `{expected}`", sql.join("; ")),
            },
            Err(err) => Probe {
                name: name.to_owned(),
                status: StepStatus::Fail,
                detail: format!("{err:#}"),
            },
        }
    }

    /// `pg_dump --compress=zstd` of the `postgres` database into a non
    /// empty file in the run folder.
    fn zstd_probe(ctx: &Ctx) -> Probe {
        let outcome = (|| {
            let dump = ctx.run_dir.join("dump.zst");
            let mut command = ctx.command("pg_dump")?;
            command
                .arg("-h")
                .arg(ctx.socket_dir()?)
                .arg("-p")
                .arg(ctx.port(0)?.to_string())
                .args(["-U", POSTGRES_USER, "--compress=zstd", "-f"])
                .arg(&dump)
                .arg("postgres");
            let finished = process::run(command, CLIENT_TIMEOUT)?;
            require_success(&finished, "pg_dump --compress=zstd", CLIENT_TIMEOUT)?;
            let size = fs::metadata(&dump)
                .with_context(|| format!("pg_dump wrote no {}", dump.display()))?
                .len();
            if size == 0 {
                bail!("pg_dump --compress=zstd wrote an empty file");
            }
            Ok(format!("pg_dump --compress=zstd wrote {size} bytes"))
        })();
        match outcome {
            Ok(detail) => Probe {
                name: "zstd".to_owned(),
                status: StepStatus::Pass,
                detail,
            },
            Err(err) => Probe {
                name: "zstd".to_owned(),
                status: StepStatus::Fail,
                detail: format!("{err:#}"),
            },
        }
    }
}

impl Recipe for Postgres {
    fn uses_socket_dir(&self) -> bool {
        true
    }

    fn init(&self, ctx: &mut Ctx) -> Result<()> {
        let mut command = ctx.command("initdb")?;
        command
            .arg("-D")
            .arg(&ctx.data_dir)
            .args(["-U", POSTGRES_USER])
            .arg("--auth-local=trust")
            .arg("--auth-host=scram-sha-256")
            .args(["-E", "UTF8"])
            .arg("--locale=C")
            .arg("--locale-provider=icu")
            .arg("--icu-locale=und");
        let finished = ctx.run_logged("initdb", command, INIT_TIMEOUT)?;
        require_success(&finished, "initdb", INIT_TIMEOUT)
    }

    fn start(&self, ctx: &mut Ctx) -> Result<Command> {
        // The port names the socket `.s.PGSQL.<port>` even with no TCP
        // listener at all.
        ctx.ports = ports::allocate(1)?;
        let mut command = ctx.command("postgres")?;
        command
            .arg("-D")
            .arg(&ctx.data_dir)
            .arg("-k")
            .arg(ctx.socket_dir()?)
            .arg("-p")
            .arg(ctx.port(0)?.to_string())
            .args(["-c", "listen_addresses="]);
        Ok(command)
    }

    fn ready(&self, ctx: &Ctx) -> Result<bool> {
        Ok(process::run(Self::psql(ctx, &["select 1"])?, CLIENT_TIMEOUT)?.success())
    }

    fn version(&self, ctx: &Ctx) -> Result<String> {
        Self::query(ctx, &["show server_version"])
    }

    fn normalize(&self, raw: &str) -> Option<String> {
        leading_version(raw, 2)
    }

    /// The AC-7 closure probes: ICU, OpenSSL through pgcrypto, lz4, and
    /// zstd.
    fn probes(&self, ctx: &Ctx) -> Vec<Probe> {
        vec![
            Self::query_probe(
                ctx,
                "icu",
                &["select datlocprovider from pg_database where datname = 'postgres'"],
                "i",
            ),
            Self::query_probe(
                ctx,
                "pgcrypto",
                &[
                    "create extension pgcrypto",
                    "select length(gen_random_bytes(16))",
                ],
                "16",
            ),
            Self::query_probe(
                ctx,
                "lz4",
                &[
                    "create table stackly_lz4 (value text compression lz4)",
                    "insert into stackly_lz4 values (repeat('a', 10000))",
                    "select pg_column_compression(value) from stackly_lz4",
                ],
                "lz4",
            ),
            Self::zstd_probe(ctx),
        ]
    }

    /// SIGINT is the fast shutdown of the postmaster.
    fn stop(&self, _ctx: &Ctx, server: &Server) -> Result<()> {
        server.signal("INT")
    }

    fn version_args(&self) -> &'static [&'static str] {
        &["--version"]
    }
}

/// Valkey: one loopback port, data in the run folder, no persistence, and
/// no daemonizing.
struct Valkey;

impl Valkey {
    fn cli(ctx: &Ctx, args: &[&str]) -> Result<Command> {
        let mut command = ctx.command("valkey-cli")?;
        command
            .args(["-h", "127.0.0.1", "-p"])
            .arg(ctx.port(0)?.to_string())
            .args(args);
        Ok(command)
    }
}

impl Recipe for Valkey {
    fn init(&self, ctx: &mut Ctx) -> Result<()> {
        process::create_dir(&ctx.data_dir)
    }

    fn start(&self, ctx: &mut Ctx) -> Result<Command> {
        ctx.ports = ports::allocate(1)?;
        let mut command = ctx.command("valkey-server")?;
        command
            .args(["--bind", "127.0.0.1", "--port"])
            .arg(ctx.port(0)?.to_string())
            .arg("--dir")
            .arg(&ctx.data_dir)
            .args(["--save", "", "--appendonly", "no", "--daemonize", "no"]);
        Ok(command)
    }

    fn ready(&self, ctx: &Ctx) -> Result<bool> {
        let finished = process::run(Self::cli(ctx, &["PING"])?, CLIENT_TIMEOUT)?;
        Ok(finished.success() && finished.stdout.trim() == "PONG")
    }

    /// The `valkey_version` line of `INFO server`; Valkey also reports a
    /// `redis_version` for compatibility, which is not its own version.
    fn version(&self, ctx: &Ctx) -> Result<String> {
        let text = output_of(
            &process::run(Self::cli(ctx, &["INFO", "server"])?, CLIENT_TIMEOUT)?,
            "INFO server",
        )?;
        text.lines()
            .map(str::trim)
            .find(|line| line.starts_with("valkey_version:"))
            .map(str::to_owned)
            .with_context(|| format!("INFO server printed no valkey_version: {}", text.trim()))
    }

    fn normalize(&self, raw: &str) -> Option<String> {
        version_after(raw, "valkey_version:").and_then(|version| first_parts(&version, 3))
    }

    fn stop(&self, ctx: &Ctx, _server: &Server) -> Result<()> {
        output_of(
            &process::run(Self::cli(ctx, &["shutdown", "nosave"])?, CLIENT_TIMEOUT)?,
            "valkey-cli shutdown nosave",
        )
        .map(drop)
    }

    fn version_args(&self) -> &'static [&'static str] {
        &["--version"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_normalized_per_provider() {
        let nats = recipe("nats").unwrap();
        assert_eq!(
            nats.normalize("nats-server: v2.15.1\n").as_deref(),
            Some("2.15.1")
        );
        let traefik = recipe("traefik").unwrap();
        assert_eq!(traefik.normalize("3.7.15").as_deref(), Some("3.7.15"));
        let clickhouse = recipe("clickhouse").unwrap();
        assert_eq!(
            clickhouse.normalize("26.8.29.10\n").as_deref(),
            Some("26.8.29.10")
        );
        assert_eq!(clickhouse.normalize("26.8"), None);
        let mysql = recipe("mysql").unwrap();
        assert_eq!(mysql.normalize("8.4.11\n").as_deref(), Some("8.4.11"));
        assert_eq!(
            mysql.normalize("9.7.2-commercial").as_deref(),
            Some("9.7.2")
        );
        assert!(recipe("redis").is_none());
    }

    #[test]
    fn postgresql_and_valkey_have_recipes() {
        let postgresql = recipe("postgresql").unwrap();
        assert!(postgresql.uses_socket_dir());
        assert_eq!(postgresql.version_args(), ["--version"]);
        let valkey = recipe("valkey").unwrap();
        assert!(!valkey.uses_socket_dir());
        assert_eq!(valkey.version_args(), ["--version"]);
    }

    #[test]
    fn a_postgresql_server_version_keeps_major_and_minor() {
        let postgresql = recipe("postgresql").unwrap();
        assert_eq!(postgresql.normalize("18.6\n").as_deref(), Some("18.6"));
        assert_eq!(postgresql.normalize("17.11").as_deref(), Some("17.11"));
        assert_eq!(
            postgresql.normalize("18.6 (Stackly r1)").as_deref(),
            Some("18.6")
        );
        assert_eq!(postgresql.normalize("PostgreSQL 18.6"), None);
        assert_eq!(postgresql.normalize("18devel"), None);
    }

    #[test]
    fn a_valkey_version_comes_from_the_valkey_version_line_of_info_server() {
        let valkey = recipe("valkey").unwrap();
        let info = "# Server\r\nredis_version:7.2.4\r\nserver_name:valkey\r\nvalkey_version:9.1.2\r\nvalkey_release_stage:ga\r\nredis_git_sha1:00000000\r\n";
        assert_eq!(valkey.normalize(info).as_deref(), Some("9.1.2"));
        assert_eq!(
            valkey.normalize("valkey_version:9.1.2").as_deref(),
            Some("9.1.2")
        );
        assert_eq!(valkey.normalize("redis_version:7.2.4\r\n"), None);
    }

    #[test]
    fn leading_version_reads_only_from_the_start() {
        assert_eq!(leading_version("  17.11\n", 2).as_deref(), Some("17.11"));
        assert_eq!(leading_version("18.6.1", 2).as_deref(), Some("18.6"));
        assert_eq!(leading_version("18", 2), None);
        assert_eq!(leading_version("v18.6", 2), None);
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
        assert_eq!(nats_string("/a b/\"c\""), "\"/a b/\\\"c\\\"\"");
        assert_eq!(xml("a&b<c>"), "a&amp;b&lt;c&gt;");
    }
}
