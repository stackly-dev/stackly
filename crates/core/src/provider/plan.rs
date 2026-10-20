//! Execution plans: pure data built from a validated package and the
//! context of one action, for the executor of spec 0010 to run as is.
//!
//! Plans are built right before each action and never stored and read
//! back, so their types are `Serialize` only. They never hold a secret
//! value, never a shell, and every path lies under an instance folder the
//! manifest permits or under the runtime root.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU8;
use std::ops::RangeInclusive;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Serialize, Serializer};
use stackly_domain::{
    Capability, EndpointName, ExactVersion, InstanceId, InstanceName, Port, Rule, RuntimeLine,
    SecretRef,
};
use stackly_platform::{InstancePaths, SecretValue};

use super::interpolate::{
    EndpointTransport, ExecRef, InstanceDir, Interp, PathBase, PathExpr, Piece, SECRETS_DIR, Token,
};
use super::manifest::{
    CommandSpec, InitStepSpec, Manifest, ProbeDef, ReloadActionSpec, StdinSpec, StopSignal,
    StopSpec,
};
use super::names::SettingName;
use super::settings::{ScalarValue, SettingValue, SettingsError, SettingsItem, SettingsValues};
use super::template::{self, OUTPUT_MAX_LEN, TemplateFormat};
use super::validate::ValidatedPackage;
use crate::secret::ResolvedSecrets;
use crate::secret::delivery::render_secret_placeholder;

/// The `PATH` every child process gets. A POSIX constant; the Windows
/// phase moves it into the platform.
pub const CHILD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
/// Longest argv entry after rendering, in bytes.
pub const ARGUMENT_MAX_LEN: usize = 4096;

/// Everything a plan needs besides the package, gathered by the caller
/// right before the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanContext {
    /// The instance's ID.
    pub instance_id: InstanceId,
    /// The instance's name.
    pub instance_name: InstanceName,
    /// The instance's folders, `run` included (spec 0004).
    pub paths: InstancePaths,
    /// The runtime the action uses.
    pub runtime: RuntimeContext,
    /// The concrete address of every endpoint.
    pub endpoints: BTreeMap<EndpointName, ResolvedEndpoint>,
    /// The instance's settings, from
    /// [`SettingsSchema::validate`](super::SettingsSchema::validate).
    pub settings: SettingsValues,
}

/// The runtime an action runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContext {
    /// The canonical runtime root.
    pub root: PathBuf,
    /// The exact version.
    pub version: ExactVersion,
    /// The runtime line.
    pub line: RuntimeLine,
}

/// The concrete address of one endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "content",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ResolvedEndpoint {
    /// A TCP or UDP endpoint. `port` is `None` for an Auto port not
    /// allocated yet.
    Ip {
        /// The loopback host.
        host: IpAddr,
        /// The port, once allocated.
        #[serde(serialize_with = "serialize_optional_port")]
        port: Option<Port>,
    },
    /// A Unix socket in the instance's run folder.
    Unix,
}

/// A secret in a plan: the settings field and its reference, never its
/// value. `Debug` and `Serialize` show only `<secret:field>`.
#[derive(Clone, PartialEq, Eq)]
pub struct PlanSecret {
    /// The secret settings field.
    pub field: SettingName,
    /// The reference the executor resolves.
    pub reference: SecretRef,
}

impl fmt::Debug for PlanSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<secret:{}>", self.field)
    }
}

impl Serialize for PlanSecret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("<secret:{}>", self.field))
    }
}

/// The plan of the initialize action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InitPlan {
    /// The steps, in order.
    pub steps: Vec<InitStep>,
    /// The deadline of the whole action (`timeouts.initSeconds`).
    #[serde(serialize_with = "serialize_duration")]
    pub deadline: Duration,
    /// Entries to delete at the end of the action, whether it succeeded or
    /// failed, without duplicates and in step order: every materialized
    /// secret, and the first component under the instance's `secrets`
    /// folder of every sensitive config file that `renderConfig` writes
    /// there (the intermediate folder for a two component destination).
    pub cleanup: Vec<PathBuf>,
}

/// One step of an initialize or reload plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "content",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InitStep {
    /// Make each folder with mode 0700, parent first, accepting an
    /// existing one only when it is a private folder of the user and not a
    /// symlink.
    EnsureDirectories(Vec<PathBuf>),
    /// Write a rendered config file atomically with mode 0600.
    WriteFile(FileWrite),
    /// Write the exact bytes of a secret to a 0600 file in the instance's
    /// `secrets` folder (`<run>/secrets`, spec 0011), deleted at the end of
    /// the action.
    MaterializeSecret {
        /// The secret.
        secret: PlanSecret,
        /// The file name inside the `secrets` folder.
        file_name: String,
    },
    /// Run a command to completion.
    Run(RunSpec),
}

/// One rendered config file to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FileWrite {
    /// The destination.
    pub path: PathBuf,
    /// The content, with secrets still unresolved.
    pub content: RenderedContent,
    /// Whether the file holds a secret.
    pub sensitive: bool,
}

/// Rendered config content whose secrets the executor resolves through
/// [`RenderedContent::materialize`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RenderedContent {
    /// The config file it renders.
    pub config_file: String,
    /// Text, with a segment for each secret.
    pub segments: Vec<Segment>,
}

/// One piece of rendered content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "content",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Segment {
    /// Final text.
    Text(String),
    /// A secret, written through the escaper of `format`.
    Secret {
        /// The secret.
        secret: PlanSecret,
        /// The format whose escaper writes it.
        format: TemplateFormat,
    },
}

/// The executable a command runs: a path relative to the runtime root. The
/// executor checks it right before exec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ExecutablePath {
    /// The canonical runtime root.
    pub runtime_root: PathBuf,
    /// The path inside it.
    pub relative: PathBuf,
}

impl ExecutablePath {
    /// Returns the absolute path to exec.
    #[must_use]
    pub fn absolute(&self) -> PathBuf {
        self.runtime_root.join(&self.relative)
    }
}

/// One process to start: no shell, every argument one argv element as is,
/// and an env built only from the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Command {
    /// The executable.
    pub program: ExecutablePath,
    /// The arguments, without argv[0].
    pub args: Vec<String>,
    /// The working folder.
    pub cwd: PathBuf,
    /// The whole environment, sorted by key: `PATH`, `HOME`, `TMPDIR`,
    /// and the manifest's env. Nothing is inherited.
    pub env: Vec<(String, EnvValue)>,
}

/// The value of one env variable. `Debug` and `Serialize` show a secret
/// only as `<secret:field>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum EnvValue {
    /// Final text.
    Text(String),
    /// The exact bytes of a secret (only in `lifecycle.start` and
    /// `runExecutable`).
    Secret(PlanSecret),
}

/// A command run to completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RunSpec {
    /// The command.
    pub command: Command,
    /// What stdin gets.
    pub stdin: Stdin,
    /// The time limit.
    #[serde(serialize_with = "serialize_duration")]
    pub timeout: Duration,
    /// Exit codes that count as success.
    pub success_exit_codes: Vec<u8>,
}

/// What a command's stdin gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum Stdin {
    /// Nothing.
    Null,
    /// The exact bytes of a secret, then end of file.
    Secret(PlanSecret),
    /// Literal manifest text, written exactly, then end of file (spec
    /// 0019). It is not a secret, so `Debug` and `Serialize` show it.
    Text(String),
}

/// The plan of the start action and of the running process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RunPlan {
    /// The config files, rendered before every spawn (spec 0010): one
    /// `EnsureDirectories` with the folders their destinations need, then
    /// one `WriteFile` per config file, sorted by name.
    pub render: Vec<InitStep>,
    /// The main process: stdin null, stdout and stderr captured.
    pub spawn: Command,
    /// Readiness after spawn, with the `readinessSeconds` deadline.
    pub readiness: ProbePlan,
    /// The deadline from a respawn after a crash to ready.
    #[serde(serialize_with = "serialize_duration")]
    pub recovery_deadline: Duration,
    /// Health while running.
    pub health: HealthPlan,
    /// How to stop the process.
    pub stop: StopPlan,
    /// Log files the process writes besides stdout and stderr.
    pub log_files: Vec<PathBuf>,
    /// Every endpoint with its concrete address.
    pub endpoints: Vec<(EndpointName, ResolvedEndpoint)>,
}

/// A probe repeated until it succeeds or the deadline passes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ProbePlan {
    /// The probe.
    pub probe: ProbeSpec,
    /// The pause after one attempt ends.
    #[serde(serialize_with = "serialize_duration")]
    pub interval: Duration,
    /// The limit of one attempt.
    #[serde(serialize_with = "serialize_duration")]
    pub attempt_timeout: Duration,
    /// The limit of the whole phase.
    #[serde(serialize_with = "serialize_duration")]
    pub deadline: Duration,
}

/// The health probe of a running instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HealthPlan {
    /// The probe.
    pub probe: ProbeSpec,
    /// The pause between attempts.
    #[serde(serialize_with = "serialize_duration")]
    pub interval: Duration,
    /// The limit of one attempt.
    #[serde(serialize_with = "serialize_duration")]
    pub attempt_timeout: Duration,
    /// Consecutive failures that make the instance unhealthy; fewer make
    /// it degraded.
    pub failure_threshold: NonZeroU8,
}

/// One probe attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "content",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProbeSpec {
    /// A connect that succeeds within the attempt timeout.
    Tcp {
        /// Where to connect.
        target: ProbeTarget,
    },
    /// An HTTP/1.1 GET to a loopback endpoint, no redirects, at most 64 KiB
    /// of body, successful when the status is in range.
    Http {
        /// The loopback host.
        host: IpAddr,
        /// The port.
        #[serde(serialize_with = "serialize_port")]
        port: Port,
        /// The literal path.
        path: String,
        /// The statuses that count as success.
        expect_status: RangeInclusive<u16>,
    },
    /// A runtime executable whose exit code decides; stdin is always null.
    Exec(RunSpec),
}

/// Where a TCP probe connects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "content",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProbeTarget {
    /// A loopback host and port.
    Ip {
        /// The host.
        host: IpAddr,
        /// The port.
        #[serde(serialize_with = "serialize_port")]
        port: Port,
    },
    /// A Unix socket.
    Unix {
        /// The socket path.
        path: PathBuf,
    },
}

/// How to stop the main process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "content",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StopPlan {
    /// Send a signal and wait for the exit.
    Signal {
        /// The signal.
        signal: StopSignal,
        /// The limit from the signal to the exit.
        #[serde(serialize_with = "serialize_duration")]
        timeout: Duration,
    },
    /// Run a command, then wait for the exit; both share one budget.
    Command {
        /// The command.
        run: Box<RunSpec>,
        /// The shared budget.
        #[serde(serialize_with = "serialize_duration")]
        timeout: Duration,
    },
}

/// The plan of the reload action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReloadPlan {
    /// Config files to render again: only `EnsureDirectories` and
    /// `WriteFile`.
    pub steps: Vec<InitStep>,
    /// What tells the process.
    pub action: ReloadAction,
    /// Readiness to check afterwards, with the reload deadline.
    pub verify: Option<ProbePlan>,
    /// The budget of the action through verification.
    #[serde(serialize_with = "serialize_duration")]
    pub deadline: Duration,
}

/// What tells the process to reload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum ReloadAction {
    /// Send SIGHUP.
    Hangup,
    /// Run a command.
    Run(Box<RunSpec>),
    /// Nothing: the process watches its files.
    None,
}

/// Error returned when a plan cannot be built for a context. Messages never
/// hold a settings text value or a secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlanError {
    /// The provider lacks the capability the action needs (BR-016).
    #[error("the provider does not support {capability} (BR-016)")]
    Unsupported {
        /// The missing capability.
        capability: Capability,
    },
    /// An endpoint has no concrete address of the right kind in the
    /// context, or a TCP or UDP endpoint has no port yet.
    #[error("endpoint {endpoint} has no concrete address")]
    EndpointUnresolved {
        /// The endpoint.
        endpoint: EndpointName,
    },
    /// An endpoint's host is not loopback.
    #[error("endpoint {endpoint} is not on a loopback address")]
    EndpointNotLoopback {
        /// The endpoint.
        endpoint: EndpointName,
    },
    /// A value the config format's escaper cannot write.
    #[error("{field} holds a value the config format cannot represent")]
    UnrepresentableValue {
        /// The token or field.
        field: String,
    },
    /// A rendered `yaml` or `json` file that does not parse.
    #[error("rendered config file {config_file} does not parse")]
    RenderedOutputInvalid {
        /// The config file.
        config_file: String,
    },
    /// A rendered file larger than 1 MiB.
    #[error("rendered config file {config_file} is larger than 1 MiB")]
    OutputTooLarge {
        /// The config file.
        config_file: String,
    },
    /// An argv entry longer than 4096 bytes after rendering.
    #[error("{position} is longer than 4096 bytes after rendering")]
    ArgumentTooLong {
        /// The manifest position.
        position: String,
    },
    /// A context path that is not UTF-8.
    #[error("{position} uses a path that is not UTF-8")]
    PathNotUtf8 {
        /// The token or position.
        position: String,
    },
    /// A context path that is not absolute and normal.
    #[error("{position} uses a path that is not absolute or holds . or ..")]
    PathEscapesScope {
        /// The token or position.
        position: String,
    },
    /// The settings no longer match the schema.
    #[error("settings do not match the provider's schema: {0}")]
    Settings(SettingsError),
    /// The resolved secrets lack a field (only from
    /// [`RenderedContent::materialize`]); a caller bug, since the secret
    /// store reports a missing secret before the action starts.
    #[error("secret {field} is unavailable")]
    SecretUnavailable {
        /// The secret field.
        field: SettingName,
    },
}

impl PlanError {
    /// Returns the business rule this error enforces, if any.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        match self {
            Self::Unsupported { .. } => Some(Rule::Br016),
            _ => None,
        }
    }
}

impl RenderedContent {
    /// Fills the secret segments from the secrets of the action and returns
    /// the final bytes, with the same escaper and the same 1 MiB limit as
    /// the renderer. Each value is read through
    /// [`render_secret_placeholder`] (spec 0011).
    ///
    /// # Errors
    ///
    /// Returns [`PlanError::SecretUnavailable`] when `secrets` lacks a
    /// field, [`PlanError::UnrepresentableValue`] when the format cannot
    /// hold a secret, and [`PlanError::OutputTooLarge`] past 1 MiB.
    pub fn materialize(&self, secrets: &ResolvedSecrets) -> Result<Vec<u8>, PlanError> {
        self.materialize_with(|secret| secrets.get(&secret.field))
    }

    /// Fills every secret segment with the placeholder `x`, as validation
    /// renders a template to check that its output parses.
    pub(crate) fn materialize_sample(&self) -> Result<Vec<u8>, PlanError> {
        let sample = SecretValue::new("x".to_owned())
            .expect("a one byte text without NUL is a valid secret value");
        self.materialize_with(|_| Some(&sample))
    }

    /// Fills the secret segments with the values `value` gives, as
    /// [`materialize`](Self::materialize) does.
    fn materialize_with<'s>(
        &self,
        value: impl Fn(&PlanSecret) -> Option<&'s SecretValue>,
    ) -> Result<Vec<u8>, PlanError> {
        let mut out = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Text(text) => out.extend_from_slice(text.as_bytes()),
                Segment::Secret { secret, format } => {
                    let value = value(secret).ok_or_else(|| PlanError::SecretUnavailable {
                        field: secret.field.clone(),
                    })?;
                    let escaped = render_secret_placeholder(value, *format).ok_or_else(|| {
                        PlanError::UnrepresentableValue {
                            field: format!("secret.{}", secret.field),
                        }
                    })?;
                    out.extend_from_slice(escaped.as_bytes());
                }
            }
            if out.len() > OUTPUT_MAX_LEN {
                return Err(PlanError::OutputTooLarge {
                    config_file: self.config_file.clone(),
                });
            }
        }
        Ok(out)
    }
}

/// A token's value while a plan renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Text(String),
    Integer(i64),
    Boolean(bool),
    Secret(PlanSecret),
}

/// Resolves tokens against one context.
pub(crate) struct Resolver<'a> {
    manifest: &'a Manifest,
    context: &'a PlanContext,
    managed: BTreeMap<SettingName, Value>,
}

impl<'a> Resolver<'a> {
    /// Checks the context's endpoints and computes managed fields.
    pub(crate) fn new(manifest: &'a Manifest, context: &'a PlanContext) -> Result<Self, PlanError> {
        for (name, spec) in &manifest.endpoints {
            match (spec.transport, context.endpoints.get(name)) {
                (EndpointTransport::Unix, Some(ResolvedEndpoint::Unix)) => {}
                (
                    EndpointTransport::Tcp | EndpointTransport::Udp,
                    Some(ResolvedEndpoint::Ip { host, .. }),
                ) => {
                    if !host.is_loopback() {
                        return Err(PlanError::EndpointNotLoopback {
                            endpoint: name.clone(),
                        });
                    }
                }
                _ => {
                    return Err(PlanError::EndpointUnresolved {
                        endpoint: name.clone(),
                    });
                }
            }
        }
        let mut resolver = Self {
            manifest,
            context,
            managed: BTreeMap::new(),
        };
        let mut managed = BTreeMap::new();
        for field in &manifest.managed_fields {
            let position = format!("settings.{}", field.name);
            let value = match (field.integer, field.expression.pieces.as_slice()) {
                (true, [Piece::Token(token)]) => resolver.value(token)?,
                _ => Value::Text(resolver.render(&field.expression, &position)?),
            };
            managed.insert(field.name.clone(), value);
        }
        resolver.managed = managed;
        Ok(resolver)
    }

    fn dir(&self, dir: InstanceDir) -> Result<&'a Path, PlanError> {
        let path = dir.path(&self.context.paths);
        checked_path(path, &format!("instance.{}", dir.as_code()))?;
        Ok(path)
    }

    /// The instance's `secrets` folder, `<run>/secrets` (spec 0011).
    fn secrets_dir(&self) -> Result<PathBuf, PlanError> {
        Ok(self.dir(InstanceDir::Run)?.join(SECRETS_DIR))
    }

    fn runtime_root(&self) -> Result<&'a Path, PlanError> {
        let root = self.context.runtime.root.as_path();
        checked_path(root, "runtime.root")?;
        Ok(root)
    }

    fn endpoint(&self, name: &EndpointName) -> Result<(IpAddr, Port), PlanError> {
        match self.context.endpoints.get(name) {
            Some(ResolvedEndpoint::Ip {
                host,
                port: Some(port),
            }) => Ok((*host, *port)),
            _ => Err(PlanError::EndpointUnresolved {
                endpoint: name.clone(),
            }),
        }
    }

    fn socket(&self, name: &EndpointName) -> Result<PathBuf, PlanError> {
        let spec = self
            .manifest
            .endpoints
            .get(name)
            .and_then(|spec| spec.socket_name.as_ref());
        let Some(socket_name) = spec else {
            return Err(PlanError::EndpointUnresolved {
                endpoint: name.clone(),
            });
        };
        let file = self.render(socket_name, &format!("endpoint.{name}.socket"))?;
        Ok(self.dir(InstanceDir::Run)?.join(file))
    }

    fn executable(&self, executable: &ExecRef) -> Result<ExecutablePath, PlanError> {
        let components = match executable {
            ExecRef::Runtime => Some(&self.manifest.executable),
            ExecRef::Alias(alias) => self.manifest.aliases.get(alias),
        };
        let relative: PathBuf = components.into_iter().flatten().collect();
        Ok(ExecutablePath {
            runtime_root: self.runtime_root()?.to_path_buf(),
            relative,
        })
    }

    fn secret(&self, field: &SettingName) -> Result<PlanSecret, PlanError> {
        match self.context.settings.get(field.as_str()) {
            Some(SettingValue::Secret(reference)) => Ok(PlanSecret {
                field: field.clone(),
                reference: reference.clone(),
            }),
            _ => Err(missing_setting(field)),
        }
    }

    /// Returns a token's value.
    pub(crate) fn value(&self, token: &Token) -> Result<Value, PlanError> {
        let position = token.to_string();
        let text = |path: &Path| path_text(path, &position).map(Value::Text);
        match token {
            Token::InstanceId => Ok(Value::Text(self.context.instance_id.to_string())),
            Token::InstanceName => Ok(Value::Text(self.context.instance_name.to_string())),
            Token::InstanceShortKey => Ok(Value::Text(
                self.context.instance_id.short_key().to_string(),
            )),
            Token::InstanceDir(dir) => text(self.dir(*dir)?),
            Token::InstanceSecrets => text(&self.secrets_dir()?),
            Token::RuntimeRoot => text(self.runtime_root()?),
            Token::RuntimeExecutable => text(&self.executable(&ExecRef::Runtime)?.absolute()),
            Token::RuntimeAlias(alias) => {
                text(&self.executable(&ExecRef::Alias(alias.clone()))?.absolute())
            }
            Token::RuntimeVersion => Ok(Value::Text(self.context.runtime.version.to_string())),
            Token::RuntimeLine => Ok(Value::Text(self.context.runtime.line.to_string())),
            Token::EndpointHost(name) => Ok(Value::Text(self.endpoint(name)?.0.to_string())),
            Token::EndpointPort(name) => {
                Ok(Value::Integer(i64::from(self.endpoint(name)?.1.get())))
            }
            Token::EndpointAddress(name) => {
                let (host, port) = self.endpoint(name)?;
                Ok(Value::Text(SocketAddr::new(host, port.get()).to_string()))
            }
            Token::EndpointSocket(name) => text(&self.socket(name)?),
            Token::EndpointSocketDir(_) => text(self.dir(InstanceDir::Run)?),
            Token::Setting(name) => {
                if let Some(value) = self.managed.get(name) {
                    return Ok(value.clone());
                }
                match self.context.settings.get(name.as_str()) {
                    Some(SettingValue::Scalar(ScalarValue::Text(value))) => {
                        Ok(Value::Text(value.clone()))
                    }
                    Some(SettingValue::Scalar(ScalarValue::Integer(value))) => {
                        Ok(Value::Integer(*value))
                    }
                    Some(SettingValue::Scalar(ScalarValue::Boolean(value))) => {
                        Ok(Value::Boolean(*value))
                    }
                    _ => Err(missing_setting(name)),
                }
            }
            Token::Secret(name) => self.secret(name).map(Value::Secret),
            Token::ProviderId => Ok(Value::Text(self.manifest.id.to_string())),
            Token::ProviderRevision => Ok(Value::Text(self.manifest.revision.to_string())),
        }
    }

    /// Returns the items of an array setting.
    pub(crate) fn items(&self, name: &SettingName) -> &'a [SettingsItem] {
        match self.context.settings.get(name.as_str()) {
            Some(SettingValue::Array(items)) => items,
            _ => &[],
        }
    }

    /// Returns whether a setting holds a list.
    pub(crate) fn is_array(&self, name: &SettingName) -> bool {
        matches!(
            self.context.settings.get(name.as_str()),
            Some(SettingValue::Array(_))
        )
    }

    /// Renders an interpolated string as one text.
    pub(crate) fn render(&self, interp: &Interp, position: &str) -> Result<String, PlanError> {
        let mut out = String::new();
        for piece in &interp.pieces {
            match piece {
                Piece::Literal(text) => out.push_str(text),
                Piece::Token(token) => match self.value(token)? {
                    Value::Text(text) => out.push_str(&text),
                    Value::Integer(number) => out.push_str(&number.to_string()),
                    Value::Boolean(flag) => out.push_str(if flag { "true" } else { "false" }),
                    Value::Secret(_) => {
                        return Err(PlanError::UnrepresentableValue {
                            field: format!("{position}: {token}"),
                        });
                    }
                },
            }
        }
        Ok(out)
    }

    /// Returns the folder a path expression starts from.
    fn base(&self, base: PathBase) -> Result<PathBuf, PlanError> {
        match base {
            PathBase::Dir(dir) => self.dir(dir).map(Path::to_path_buf),
            PathBase::Secrets => self.secrets_dir(),
        }
    }

    /// Builds the path of a path expression.
    fn path(&self, expr: &PathExpr) -> Result<PathBuf, PlanError> {
        let mut path = self.base(expr.base)?;
        path.extend(&expr.components);
        Ok(path)
    }

    fn command(&self, spec: &CommandSpec) -> Result<Command, PlanError> {
        let program = self.executable(&spec.executable)?;
        let mut args = Vec::with_capacity(spec.args.len());
        for (index, arg) in spec.args.iter().enumerate() {
            let position = format!("{}.args[{index}]", spec.at);
            let text = self.render(arg, &position)?;
            if text.len() > ARGUMENT_MAX_LEN {
                return Err(PlanError::ArgumentTooLong { position });
            }
            args.push(text);
        }
        let cwd = self.path(&spec.cwd)?;
        let temp = path_text(self.dir(InstanceDir::Temp)?, "instance.temp")?;
        let mut env = BTreeMap::from([
            ("PATH".to_owned(), EnvValue::Text(CHILD_PATH.to_owned())),
            ("HOME".to_owned(), EnvValue::Text(temp.clone())),
            ("TMPDIR".to_owned(), EnvValue::Text(temp)),
        ]);
        for (key, value) in &spec.env {
            // Validation allows a secret only as the whole value, and only
            // in the commands that resolve secrets.
            let value = match value.pieces.as_slice() {
                [Piece::Token(Token::Secret(field))] => EnvValue::Secret(self.secret(field)?),
                _ => EnvValue::Text(self.render(value, &format!("{}.env.{key}", spec.at))?),
            };
            env.insert(key.clone(), value);
        }
        Ok(Command {
            program,
            args,
            cwd,
            env: env.into_iter().collect(),
        })
    }

    fn run(
        &self,
        spec: &CommandSpec,
        stdin: Stdin,
        timeout: Duration,
        success: &[u8],
    ) -> Result<RunSpec, PlanError> {
        Ok(RunSpec {
            command: self.command(spec)?,
            stdin,
            timeout,
            success_exit_codes: success.to_vec(),
        })
    }

    fn probe(&self, probe: &ProbeDef, attempt: Duration) -> Result<ProbeSpec, PlanError> {
        Ok(match probe {
            ProbeDef::Tcp { endpoint } => {
                let target = match self
                    .manifest
                    .endpoints
                    .get(endpoint)
                    .map(|spec| spec.transport)
                {
                    Some(EndpointTransport::Unix) => ProbeTarget::Unix {
                        path: self.socket(endpoint)?,
                    },
                    _ => {
                        let (host, port) = self.endpoint(endpoint)?;
                        ProbeTarget::Ip { host, port }
                    }
                };
                ProbeSpec::Tcp { target }
            }
            ProbeDef::Http {
                endpoint,
                path,
                expect,
            } => {
                let (host, port) = self.endpoint(endpoint)?;
                ProbeSpec::Http {
                    host,
                    port,
                    path: path.clone(),
                    expect_status: expect.clone(),
                }
            }
            ProbeDef::Exec { command, success } => {
                ProbeSpec::Exec(self.run(command, Stdin::Null, attempt, success)?)
            }
        })
    }

    /// The folders a config file's destination needs, parent first: the
    /// managed folder its token names when `with_managed` (the initialize
    /// action creates those itself), the `secrets` folder, which no step
    /// creates, and the intermediate folder of a two component destination.
    fn destination_dirs(
        &self,
        destination: &PathExpr,
        with_managed: bool,
    ) -> Result<Vec<PathBuf>, PlanError> {
        let base = self.base(destination.base)?;
        let mut dirs = Vec::new();
        if with_managed || destination.base == PathBase::Secrets {
            dirs.push(base.clone());
        }
        if let [folder, _] = destination.components.as_slice() {
            dirs.push(base.join(folder));
        }
        Ok(dirs)
    }

    /// Renders one config file, checking that `yaml` and `json` output
    /// parses with a placeholder for each secret.
    fn write_file(&self, package: &ValidatedPackage, name: &str) -> Result<FileWrite, PlanError> {
        let (spec, checked) = self
            .manifest
            .config_files
            .get(name)
            .and_then(|spec| Some((spec, package.templates.get(&spec.template)?)))
            .expect("a validated package resolves every config file it names to a template");
        let content = template::render(checked, spec.format, self, name)?;
        let sample = content.materialize_sample()?;
        if !spec.format.output_parses(&String::from_utf8_lossy(&sample)) {
            return Err(PlanError::RenderedOutputInvalid {
                config_file: name.to_owned(),
            });
        }
        Ok(FileWrite {
            path: self.path(&spec.destination)?,
            content,
            sensitive: spec.sensitive,
        })
    }

    /// The steps that render one config file during initialize or reload:
    /// the folders the managed folders do not cover, if any, then the write.
    fn write_steps(
        &self,
        package: &ValidatedPackage,
        name: &str,
        steps: &mut Vec<InitStep>,
    ) -> Result<(), PlanError> {
        let Some(spec) = self.manifest.config_files.get(name) else {
            return Ok(());
        };
        let dirs = self.destination_dirs(&spec.destination, false)?;
        if !dirs.is_empty() {
            steps.push(InitStep::EnsureDirectories(dirs));
        }
        steps.push(InitStep::WriteFile(self.write_file(package, name)?));
        Ok(())
    }

    /// The steps that render every config file before a spawn: one
    /// `EnsureDirectories` with every folder the destinations need, then
    /// one write per config file, sorted by name.
    fn render_steps(&self, package: &ValidatedPackage) -> Result<Vec<InitStep>, PlanError> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut writes = Vec::new();
        for (name, spec) in &self.manifest.config_files {
            for dir in self.destination_dirs(&spec.destination, true)? {
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
            writes.push(InitStep::WriteFile(self.write_file(package, name)?));
        }
        if writes.is_empty() {
            return Ok(writes);
        }
        let mut steps = vec![InitStep::EnsureDirectories(dirs)];
        steps.append(&mut writes);
        Ok(steps)
    }
}

fn missing_setting(field: &SettingName) -> PlanError {
    PlanError::Settings(SettingsError {
        issues: vec![super::issue::Issue::new(
            super::settings::SETTINGS_FILE,
            super::issue::Location::Field(field.to_string()),
            super::issue::IssueCode::SettingsValue,
            "the value is missing or has another type",
        )],
    })
}

/// Checks that a context path is absolute with no `.` or `..`, and UTF-8,
/// since plans carry paths as text.
fn checked_path(path: &Path, position: &str) -> Result<(), PlanError> {
    let normal = path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir));
    if !normal {
        return Err(PlanError::PathEscapesScope {
            position: position.to_owned(),
        });
    }
    if path.to_str().is_none() {
        return Err(PlanError::PathNotUtf8 {
            position: position.to_owned(),
        });
    }
    Ok(())
}

fn path_text(path: &Path, position: &str) -> Result<String, PlanError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| PlanError::PathNotUtf8 {
            position: position.to_owned(),
        })
}

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn millis(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

impl ValidatedPackage {
    fn resolver<'a>(&'a self, context: &'a PlanContext) -> Result<Resolver<'a>, PlanError> {
        self.settings
            .validate(&context.settings.to_json())
            .map_err(PlanError::Settings)?;
        Resolver::new(&self.manifest, context)
    }

    /// Builds the plan of the initialize action.
    ///
    /// # Errors
    ///
    /// Returns a [`PlanError`] that depends on the context: an unresolved
    /// or non loopback endpoint, settings that no longer match, a path that
    /// is not UTF-8 or not normal, a value the format cannot hold, an
    /// argument above 4096 bytes, or rendered output that is invalid or too
    /// large.
    pub fn init_plan(&self, context: &PlanContext) -> Result<InitPlan, PlanError> {
        let resolver = self.resolver(context)?;
        let manifest = &self.manifest;
        let mut steps = Vec::new();
        let mut cleanup = Vec::new();
        for step in &manifest.initialize {
            match step {
                InitStepSpec::CreateManagedDirectories => {
                    let dirs = InstanceDir::ALL
                        .into_iter()
                        .filter(|dir| manifest.managed_fs.contains(dir))
                        .map(|dir| resolver.dir(dir).map(Path::to_path_buf))
                        .collect::<Result<Vec<_>, _>>()?;
                    steps.push(InitStep::EnsureDirectories(dirs));
                }
                InitStepSpec::RenderConfig(name) => {
                    resolver.write_steps(self, name, &mut steps)?;
                    let secrets_entry = manifest
                        .config_files
                        .get(name)
                        .filter(|spec| spec.destination.base == PathBase::Secrets)
                        .and_then(|spec| spec.destination.components.first());
                    if let Some(first) = secrets_entry {
                        let entry = resolver.secrets_dir()?.join(first);
                        if !cleanup.contains(&entry) {
                            cleanup.push(entry);
                        }
                    }
                }
                InitStepSpec::MaterializeSecret { secret, file_name } => {
                    steps.push(InitStep::MaterializeSecret {
                        secret: resolver.secret(secret)?,
                        file_name: file_name.clone(),
                    });
                    let entry = resolver.secrets_dir()?.join(file_name);
                    if !cleanup.contains(&entry) {
                        cleanup.push(entry);
                    }
                }
                InitStepSpec::RunExecutable {
                    command,
                    stdin,
                    timeout,
                    success,
                } => {
                    let stdin = match stdin {
                        Some(StdinSpec::Secret(field)) => Stdin::Secret(resolver.secret(field)?),
                        Some(StdinSpec::Text(text)) => Stdin::Text(text.clone()),
                        None => Stdin::Null,
                    };
                    let timeout = secs(timeout.unwrap_or(manifest.timeouts.init));
                    steps.push(InitStep::Run(
                        resolver.run(command, stdin, timeout, success)?,
                    ));
                }
            }
        }
        Ok(InitPlan {
            steps,
            deadline: secs(manifest.timeouts.init),
            cleanup,
        })
    }

    /// Builds the plan of the start action and the running process, with
    /// every config file rendered for the spawn. Every TCP and UDP endpoint
    /// must have its port.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`init_plan`](Self::init_plan), and
    /// [`PlanError::EndpointUnresolved`] for an endpoint without a port.
    pub fn run_plan(&self, context: &PlanContext) -> Result<RunPlan, PlanError> {
        let resolver = self.resolver(context)?;
        let manifest = &self.manifest;
        let mut endpoints = Vec::new();
        for (name, spec) in &manifest.endpoints {
            let resolved = context.endpoints.get(name).copied().ok_or_else(|| {
                PlanError::EndpointUnresolved {
                    endpoint: name.clone(),
                }
            })?;
            if spec.transport.is_ip() {
                resolver.endpoint(name)?;
            }
            endpoints.push((name.clone(), resolved));
        }
        let timeouts = &manifest.timeouts;
        let readiness = &manifest.readiness;
        let health = &manifest.health;
        let stop = match &manifest.stop {
            StopSpec::Signal(signal) => StopPlan::Signal {
                signal: *signal,
                timeout: secs(timeouts.stop),
            },
            StopSpec::Command { command, success } => StopPlan::Command {
                run: Box::new(resolver.run(command, Stdin::Null, secs(timeouts.stop), success)?),
                timeout: secs(timeouts.stop),
            },
        };
        let log_files = manifest
            .log_files
            .iter()
            .map(|path| resolver.path(path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RunPlan {
            render: resolver.render_steps(self)?,
            spawn: resolver.command(&manifest.start)?,
            readiness: ProbePlan {
                probe: resolver.probe(&readiness.probe, millis(readiness.attempt_ms))?,
                interval: millis(readiness.interval_ms),
                attempt_timeout: millis(readiness.attempt_ms),
                deadline: secs(timeouts.readiness),
            },
            recovery_deadline: secs(timeouts.recovery),
            health: HealthPlan {
                probe: resolver.probe(&health.probe, millis(health.attempt_ms))?,
                interval: secs(health.interval_seconds),
                attempt_timeout: millis(health.attempt_ms),
                failure_threshold: NonZeroU8::new(health.failure_threshold)
                    .unwrap_or(NonZeroU8::MIN),
            },
            stop,
            log_files,
            endpoints,
        })
    }

    /// Builds the plan of the reload action.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError::Unsupported`] when the provider has no reload
    /// capability (BR-016), and the errors of
    /// [`init_plan`](Self::init_plan).
    pub fn reload_plan(&self, context: &PlanContext) -> Result<ReloadPlan, PlanError> {
        let manifest = &self.manifest;
        let reload = match &manifest.reload {
            Some(reload) if manifest.capabilities.contains(Capability::Reload) => reload,
            _ => {
                return Err(PlanError::Unsupported {
                    capability: Capability::Reload,
                });
            }
        };
        let resolver = self.resolver(context)?;
        let timeouts = &manifest.timeouts;
        let mut steps = Vec::new();
        for name in &reload.render {
            resolver.write_steps(self, name, &mut steps)?;
        }
        let action = match &reload.action {
            ReloadActionSpec::Hangup => ReloadAction::Hangup,
            ReloadActionSpec::Run { command, success } => ReloadAction::Run(Box::new(
                resolver.run(command, Stdin::Null, secs(timeouts.reload), success)?,
            )),
            ReloadActionSpec::None => ReloadAction::None,
        };
        let readiness = &manifest.readiness;
        let verify = if reload.verify {
            Some(ProbePlan {
                probe: resolver.probe(&readiness.probe, millis(readiness.attempt_ms))?,
                interval: millis(readiness.interval_ms),
                attempt_timeout: millis(readiness.attempt_ms),
                deadline: secs(timeouts.reload),
            })
        } else {
            None
        };
        Ok(ReloadPlan {
            steps,
            action,
            verify,
            deadline: secs(timeouts.reload),
        })
    }
}

/// Serializes a duration as text: whole seconds as `120s`, otherwise
/// milliseconds as `500ms`.
fn serialize_duration<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    let text = if value.subsec_nanos() == 0 {
        format!("{}s", value.as_secs())
    } else {
        format!("{}ms", value.as_millis())
    };
    serializer.serialize_str(&text)
}

fn serialize_port<S: Serializer>(value: &Port, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u16(value.get())
}

fn serialize_optional_port<S: Serializer>(
    value: &Option<Port>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(port) => serializer.serialize_some(&port.get()),
        None => serializer.serialize_none(),
    }
}
