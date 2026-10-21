//! The checked manifest model that validation builds and plans read.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::ops::RangeInclusive;

use serde::Serialize;
use stackly_domain::{
    CapabilitySet, EndpointName, LineCompatibility, Port, ProviderName, RevisionLabel,
    SoftwareName, Target,
};

use super::connection::ConnectionSpec;
use super::interpolate::{EndpointTransport, ExecRef, InstanceDir, Interp, PathExpr};
use super::names::SettingName;
use super::settings::Pattern;
use super::template::TemplateFormat;
use super::timeouts::Timeouts;
use super::version_check::VersionCheck;

/// The catalog category of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum Category {
    /// A database.
    Database,
    /// A message broker.
    Messaging,
    /// A cache.
    Cache,
    /// A reverse proxy.
    Proxy,
    /// An analytics store.
    Analytics,
    /// A test provider, never in the app catalog.
    Testing,
}

impl Category {
    pub(crate) fn from_code(code: &str) -> Option<Self> {
        match code {
            "database" => Some(Self::Database),
            "messaging" => Some(Self::Messaging),
            "cache" => Some(Self::Cache),
            "proxy" => Some(Self::Proxy),
            "analytics" => Some(Self::Analytics),
            "testing" => Some(Self::Testing),
            _ => None,
        }
    }
}

/// What an endpoint is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum EndpointPurpose {
    /// Clients connect here.
    Client,
    /// Metrics and health.
    Monitoring,
    /// Administration.
    Admin,
}

impl EndpointPurpose {
    pub(crate) fn from_code(code: &str) -> Option<Self> {
        match code {
            "client" => Some(Self::Client),
            "monitoring" => Some(Self::Monitoring),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

/// One endpoint the manifest declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointSpec {
    pub(crate) name: EndpointName,
    pub(crate) transport: EndpointTransport,
    pub(crate) purpose: EndpointPurpose,
    pub(crate) default_host: Option<IpAddr>,
    pub(crate) default_port: Option<Port>,
    pub(crate) socket_name: Option<Interp>,
}

impl EndpointSpec {
    /// Returns the endpoint's name.
    #[must_use]
    pub const fn name(&self) -> &EndpointName {
        &self.name
    }

    /// Returns the transport.
    #[must_use]
    pub const fn transport(&self) -> EndpointTransport {
        self.transport
    }

    /// Returns what the endpoint is for.
    #[must_use]
    pub const fn purpose(&self) -> EndpointPurpose {
        self.purpose
    }

    /// Returns the suggested loopback host of a TCP or UDP endpoint.
    #[must_use]
    pub const fn default_host(&self) -> Option<IpAddr> {
        self.default_host
    }

    /// Returns the suggested port of a TCP or UDP endpoint, a hint for the
    /// create form.
    #[must_use]
    pub const fn default_port(&self) -> Option<Port> {
        self.default_port
    }

    /// Returns the `socketName` template of a Unix endpoint as written,
    /// with each `${...}` token whole, for the create form and for the
    /// stored socket name (spec 0020).
    #[must_use]
    pub fn socket_name_template(&self) -> Option<String> {
        use super::interpolate::Piece;
        let socket = self.socket_name.as_ref()?;
        Some(
            socket
                .pieces
                .iter()
                .map(|piece| match piece {
                    Piece::Literal(text) => text.clone(),
                    Piece::Token(token) => format!("${{{token}}}"),
                })
                .collect(),
        )
    }
}

/// `compatibility.dataMarker`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DataMarker {
    pub(crate) file: String,
    pub(crate) pattern: Pattern,
}

/// One entry of `configFiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigFileSpec {
    pub(crate) template: String,
    pub(crate) format: TemplateFormat,
    pub(crate) destination: PathExpr,
    pub(crate) sensitive: bool,
}

/// A checked command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandSpec {
    pub(crate) executable: ExecRef,
    pub(crate) args: Vec<Interp>,
    pub(crate) cwd: PathExpr,
    pub(crate) env: Vec<(String, Interp)>,
    /// The manifest path of the command, for plan error positions.
    pub(crate) at: String,
}

/// A checked initialize step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InitStepSpec {
    CreateManagedDirectories,
    RenderConfig(String),
    MaterializeSecret {
        secret: SettingName,
        /// The file name in the instance's `secrets` folder.
        file_name: String,
    },
    RunExecutable {
        command: CommandSpec,
        stdin: Option<StdinSpec>,
        timeout: Option<u64>,
        success: Vec<u8>,
    },
}

/// The stdin of a `runExecutable` step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StdinSpec {
    /// The exact bytes of a secret field.
    Secret(SettingName),
    /// Literal manifest text, written exactly (spec 0019).
    Text(String),
}

/// The signal of a graceful stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum StopSignal {
    /// SIGTERM.
    Term,
    /// SIGINT.
    Int,
    /// SIGQUIT.
    Quit,
}

/// A checked `lifecycle.stop`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StopSpec {
    Signal(StopSignal),
    Command {
        command: CommandSpec,
        success: Vec<u8>,
    },
}

/// A checked reload action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReloadActionSpec {
    Hangup,
    Run {
        command: CommandSpec,
        success: Vec<u8>,
    },
    None,
}

/// A checked `lifecycle.reload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReloadSpec {
    pub(crate) render: Vec<String>,
    pub(crate) action: ReloadActionSpec,
    pub(crate) verify: bool,
}

/// A checked probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeDef {
    Tcp {
        endpoint: EndpointName,
    },
    Http {
        endpoint: EndpointName,
        path: String,
        expect: RangeInclusive<u16>,
    },
    Exec {
        command: CommandSpec,
        success: Vec<u8>,
    },
}

/// A checked readiness block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadinessSpec {
    pub(crate) probe: ProbeDef,
    pub(crate) interval_ms: u64,
    pub(crate) attempt_ms: u64,
}

/// A checked health block, with the readiness probe when it has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HealthSpec {
    pub(crate) probe: ProbeDef,
    pub(crate) interval_seconds: u64,
    pub(crate) attempt_ms: u64,
    pub(crate) failure_threshold: u8,
}

/// A managed settings field and the expression that computes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedField {
    pub(crate) name: SettingName,
    pub(crate) expression: Interp,
    pub(crate) integer: bool,
}

/// The checked `provider.yaml` of a valid package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub(crate) id: ProviderName,
    pub(crate) software: SoftwareName,
    pub(crate) display_name: String,
    pub(crate) revision: RevisionLabel,
    pub(crate) category: Category,
    pub(crate) description: Option<String>,
    pub(crate) platforms: Vec<Target>,
    pub(crate) line_compatibility: LineCompatibility,
    pub(crate) data_marker: Option<DataMarker>,
    pub(crate) executable: Vec<String>,
    pub(crate) capabilities: CapabilitySet,
    pub(crate) endpoints: BTreeMap<EndpointName, EndpointSpec>,
    pub(crate) config_files: BTreeMap<String, ConfigFileSpec>,
    pub(crate) initialize: Vec<InitStepSpec>,
    pub(crate) start: CommandSpec,
    pub(crate) stop: StopSpec,
    pub(crate) reload: Option<ReloadSpec>,
    pub(crate) readiness: ReadinessSpec,
    pub(crate) health: HealthSpec,
    pub(crate) timeouts: Timeouts,
    pub(crate) log_files: Vec<PathExpr>,
    pub(crate) cold_backup_paths: Vec<InstanceDir>,
    pub(crate) managed_fs: BTreeSet<InstanceDir>,
    pub(crate) aliases: BTreeMap<String, Vec<String>>,
    pub(crate) managed_fields: Vec<ManagedField>,
    pub(crate) connections: Vec<ConnectionSpec>,
    pub(crate) version_check: Option<VersionCheck>,
}

impl Manifest {
    /// Returns `metadata.id`.
    #[must_use]
    pub const fn id(&self) -> &ProviderName {
        &self.id
    }

    /// Returns `metadata.software`.
    #[must_use]
    pub const fn software(&self) -> &SoftwareName {
        &self.software
    }

    /// Returns `metadata.displayName`.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns `metadata.revision`.
    #[must_use]
    pub const fn revision(&self) -> &RevisionLabel {
        &self.revision
    }

    /// Returns `metadata.category`.
    #[must_use]
    pub const fn category(&self) -> Category {
        self.category
    }

    /// Returns `metadata.description`.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns `compatibility.platforms`.
    #[must_use]
    pub fn platforms(&self) -> &[Target] {
        &self.platforms
    }

    /// Returns the capabilities the manifest declares and can fulfil.
    #[must_use]
    pub const fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Returns `permissions.managedFilesystem`.
    #[must_use]
    pub const fn managed_filesystem(&self) -> &BTreeSet<InstanceDir> {
        &self.managed_fs
    }

    /// Returns the folders a cold backup copies.
    #[must_use]
    pub fn cold_backup_paths(&self) -> &[InstanceDir] {
        &self.cold_backup_paths
    }

    /// Returns the `connection` entries in manifest order; the first is the
    /// main connection, and a package without the block has none (spec
    /// 0021).
    #[must_use]
    pub fn connections(&self) -> &[ConnectionSpec] {
        &self.connections
    }

    /// Returns `runtime.versionCheck`, or `None` when the manifest leaves
    /// it out (spec 0018).
    #[must_use]
    pub const fn version_check(&self) -> Option<&VersionCheck> {
        self.version_check.as_ref()
    }
}
