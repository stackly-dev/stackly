//! The permission summary of a provider package (spec 0037, *Ringkasan
//! izin*): what it may run, read, hand to a process, and download, shown
//! before an approval and compared with the last decision. It is for
//! display and comparison only; nothing enforces it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use stackly_domain::{Arch, Capability, Os, RevisionLabel, Sha256Digest};
use stackly_platform::HttpsUrl;

use crate::provider::permission_facts::{self, UseSite};
use crate::provider::{
    ArtifactStatus, EndpointPurpose, EndpointTransport, InstanceDir, ValidatedPackage,
};

/// The schema name of the stored JSON form, `decided_permissions`.
pub const PERMISSIONS_SCHEMA: &str = "stackly.provider-permissions/v1";
/// Most download hosts listed per artifact.
pub const DOWNLOAD_HOSTS_MAX: usize = 8;

/// The capability order of spec 0017 (*Urutan capability*), not the enum
/// order.
const CAPABILITY_ORDER: [Capability; 8] = [
    Capability::MultiInstance,
    Capability::Persistence,
    Capability::Reload,
    Capability::Metrics,
    Capability::Tls,
    Capability::ColdBackup,
    Capability::OnlineBackup,
    Capability::LogicalBackup,
];

/// What a package may do, computed from the validated package.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Permissions {
    /// The supported capabilities, in the capability order of spec 0017.
    pub capabilities: Vec<Capability>,
    /// `permissions.managedFilesystem`, ordered by code.
    pub managed_filesystem: Vec<InstanceDir>,
    /// `runtime.executable`, then each additional executable by alias.
    pub executables: Vec<PermittedExecutable>,
    /// The secret fields handed to a process, ordered by name.
    pub secrets: Vec<String>,
    /// The endpoints, ordered by name.
    pub endpoints: Vec<PermittedEndpoint>,
    /// The `available` artifacts, in `versions.json` order.
    pub downloads: Vec<PermittedDownload>,
    /// The `openWith` entries, in manifest order.
    pub open_with: Vec<PermittedOpenWith>,
}

/// One executable the package runs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PermittedExecutable {
    /// `runtime` or the alias.
    pub name: String,
    /// The path under the runtime root.
    pub path: String,
    /// Where the manifest runs it, in [`ExecutableUse`] order.
    pub uses: Vec<ExecutableUse>,
}

/// Where a package runs an executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ExecutableUse {
    /// `lifecycle.start`.
    Start,
    /// A `runExecutable` step of `lifecycle.initialize`.
    Initialize,
    /// A `command` stop.
    Stop,
    /// A `run` reload action.
    Reload,
    /// An `exec` readiness or health probe.
    Probe,
    /// `runtime.versionCheck`, always the runtime executable.
    VersionCheck,
    /// A `terminal` `openWith` entry.
    OpenWith,
}

impl ExecutableUse {
    /// Every use, in order.
    pub const ALL: [Self; 7] = [
        Self::Start,
        Self::Initialize,
        Self::Stop,
        Self::Reload,
        Self::Probe,
        Self::VersionCheck,
        Self::OpenWith,
    ];

    /// Returns the stable code, such as `version_check`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Initialize => "initialize",
            Self::Stop => "stop",
            Self::Reload => "reload",
            Self::Probe => "probe",
            Self::VersionCheck => "version_check",
            Self::OpenWith => "open_with",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|use_| use_.as_code() == code)
    }

    const fn of(site: UseSite) -> Self {
        match site {
            UseSite::Start => Self::Start,
            UseSite::Initialize => Self::Initialize,
            UseSite::Stop => Self::Stop,
            UseSite::Reload => Self::Reload,
            UseSite::Probe => Self::Probe,
            UseSite::VersionCheck => Self::VersionCheck,
            UseSite::OpenWith => Self::OpenWith,
        }
    }
}

/// One endpoint the package declares.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PermittedEndpoint {
    /// The endpoint name.
    pub name: String,
    /// `tcp`, `udp`, or `unix`.
    pub transport: String,
    /// `client`, `monitoring`, or `admin`.
    pub purpose: String,
    /// The suggested port of an IP endpoint.
    pub default_port: Option<u16>,
}

/// One artifact the package may download.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PermittedDownload {
    /// The artifact ID.
    pub artifact: String,
    /// The exact version, as text.
    pub version: String,
    /// The OS it is built for.
    pub os: Os,
    /// The architecture it is built for.
    pub arch: Arch,
    /// The SHA-256 of the download, written by the package author.
    pub sha256: Sha256Digest,
    /// The download size in bytes.
    pub size: u64,
    /// The unique hosts of its URLs, ordered, at most
    /// [`DOWNLOAD_HOSTS_MAX`].
    pub hosts: Vec<String>,
    /// The signing policy code, such as `keep_or_adhoc`.
    pub signing_policy: String,
}

/// One `openWith` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PermittedOpenWith {
    /// The entry name.
    pub name: String,
    /// `app`, `terminal`, `browser`, or `reveal`.
    pub kind: String,
    /// The first bundle ID of an `app` entry, the executable of a
    /// `terminal` entry, and empty otherwise.
    pub target: Option<String>,
}

/// What differs between the permissions of the last decision and the
/// stored package (spec 0037, *Kode perubahan*), in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum PermissionChange {
    /// The `metadata.revision` label.
    Revision,
    /// The executables: a name, path, or use.
    Executables,
    /// The set of (artifact, SHA-256) downloads.
    Downloads,
    /// The set of download hosts.
    DownloadHosts,
    /// The capabilities.
    Capabilities,
    /// The managed folders.
    Filesystem,
    /// The secret fields.
    Secrets,
    /// The endpoints.
    Endpoints,
    /// The `openWith` entries.
    OpenWith,
    /// Other bytes, with none of the changes above.
    Content,
}

impl PermissionChange {
    /// Every change, in order.
    pub const ALL: [Self; 10] = [
        Self::Revision,
        Self::Executables,
        Self::Downloads,
        Self::DownloadHosts,
        Self::Capabilities,
        Self::Filesystem,
        Self::Secrets,
        Self::Endpoints,
        Self::OpenWith,
        Self::Content,
    ];

    /// Returns the stable code, such as `download_hosts`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Revision => "revision",
            Self::Executables => "executables",
            Self::Downloads => "downloads",
            Self::DownloadHosts => "download_hosts",
            Self::Capabilities => "capabilities",
            Self::Filesystem => "filesystem",
            Self::Secrets => "secrets",
            Self::Endpoints => "endpoints",
            Self::OpenWith => "open_with",
            Self::Content => "content",
        }
    }
}

const fn transport_code(transport: EndpointTransport) -> &'static str {
    match transport {
        EndpointTransport::Tcp => "tcp",
        EndpointTransport::Udp => "udp",
        EndpointTransport::Unix => "unix",
    }
}

const fn purpose_code(purpose: EndpointPurpose) -> &'static str {
    match purpose {
        EndpointPurpose::Client => "client",
        EndpointPurpose::Monitoring => "monitoring",
        EndpointPurpose::Admin => "admin",
    }
}

/// Computes the permissions of `package`. Pure and deterministic.
#[must_use]
pub fn permissions(package: &ValidatedPackage) -> Permissions {
    let manifest = package.manifest();
    let declared = package.provider_revision().capabilities();
    let capabilities = CAPABILITY_ORDER
        .into_iter()
        .filter(|capability| declared.contains(*capability))
        .collect();
    let mut managed_filesystem: Vec<InstanceDir> =
        manifest.managed_filesystem().iter().copied().collect();
    managed_filesystem.sort_by_key(|dir| dir.as_code());
    let executables = permission_facts::executables(package)
        .into_iter()
        .map(|fact| PermittedExecutable {
            name: fact.name,
            path: fact.path,
            uses: fact.uses.into_iter().map(ExecutableUse::of).collect(),
        })
        .collect();
    let secrets = permission_facts::secret_fields(package)
        .into_iter()
        .collect();
    let endpoints = package
        .endpoints()
        .map(|endpoint| PermittedEndpoint {
            name: endpoint.name().to_string(),
            transport: transport_code(endpoint.transport()).to_owned(),
            purpose: purpose_code(endpoint.purpose()).to_owned(),
            default_port: endpoint.default_port().map(|port| port.get()),
        })
        .collect();
    let downloads = package
        .versions()
        .artifacts()
        .iter()
        .filter_map(|artifact| match &artifact.status {
            ArtifactStatus::Available {
                urls,
                sha256,
                size,
                signing_policy,
                ..
            } => Some(PermittedDownload {
                artifact: artifact.id.clone(),
                version: artifact.version.to_string(),
                os: artifact.target.os(),
                arch: artifact.target.arch(),
                sha256: *sha256,
                size: *size,
                hosts: hosts(urls),
                signing_policy: signing_policy.as_code().to_owned(),
            }),
            _ => None,
        })
        .collect();
    let open_with = manifest
        .open_with()
        .iter()
        .zip(permission_facts::open_with_targets(package))
        .map(|(entry, target)| PermittedOpenWith {
            name: entry.name().to_owned(),
            kind: entry.kind().as_code().to_owned(),
            target,
        })
        .collect();
    Permissions {
        capabilities,
        managed_filesystem,
        executables,
        secrets,
        endpoints,
        downloads,
        open_with,
    }
}

/// The unique hosts of `urls`, ordered, at most [`DOWNLOAD_HOSTS_MAX`]; a
/// URL that is not an `https` URL has none.
fn hosts(urls: &[String]) -> Vec<String> {
    urls.iter()
        .filter_map(|url| HttpsUrl::new(url).ok().map(|url| url.host().to_owned()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(DOWNLOAD_HOSTS_MAX)
        .collect()
}

/// Lists what differs from the permissions of the last decision, made on
/// `decided_revision`, to the stored package's, on `current_revision`, in
/// [`PermissionChange`] order.
///
/// Callers ask only when the decision covers other bytes than the stored
/// ones, so a list without any other change is [`PermissionChange::Content`].
#[must_use]
pub fn permission_changes(
    decided: &Permissions,
    decided_revision: &RevisionLabel,
    current: &Permissions,
    current_revision: &RevisionLabel,
) -> Vec<PermissionChange> {
    let downloads = |permissions: &Permissions| -> BTreeSet<(String, Sha256Digest)> {
        permissions
            .downloads
            .iter()
            .map(|download| (download.artifact.clone(), download.sha256))
            .collect()
    };
    let hosts = |permissions: &Permissions| -> BTreeSet<String> {
        permissions
            .downloads
            .iter()
            .flat_map(|download| download.hosts.iter().cloned())
            .collect()
    };
    let checks = [
        (
            PermissionChange::Revision,
            decided_revision != current_revision,
        ),
        (
            PermissionChange::Executables,
            decided.executables != current.executables,
        ),
        (
            PermissionChange::Downloads,
            downloads(decided) != downloads(current),
        ),
        (
            PermissionChange::DownloadHosts,
            hosts(decided) != hosts(current),
        ),
        (
            PermissionChange::Capabilities,
            decided.capabilities != current.capabilities,
        ),
        (
            PermissionChange::Filesystem,
            decided.managed_filesystem != current.managed_filesystem,
        ),
        (
            PermissionChange::Secrets,
            decided.secrets != current.secrets,
        ),
        (
            PermissionChange::Endpoints,
            decided.endpoints != current.endpoints,
        ),
        (
            PermissionChange::OpenWith,
            decided.open_with != current.open_with,
        ),
    ];
    let changes: Vec<PermissionChange> = checks
        .into_iter()
        .filter_map(|(change, differs)| differs.then_some(change))
        .collect();
    if changes.is_empty() {
        vec![PermissionChange::Content]
    } else {
        changes
    }
}

/// Why stored permissions JSON could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the stored permissions are not stackly.provider-permissions/v1: {detail}")]
#[non_exhaustive]
pub struct PermissionsJsonError {
    /// What was wrong.
    pub detail: String,
}

impl Permissions {
    /// Returns the stored form, `stackly.provider-permissions/v1`: compact
    /// camelCase JSON in the field order of the struct.
    #[must_use]
    pub fn to_json(&self) -> String {
        let json = Json {
            capabilities: self
                .capabilities
                .iter()
                .map(|capability| capability.as_code().to_owned())
                .collect(),
            managed_filesystem: self
                .managed_filesystem
                .iter()
                .map(|dir| dir.as_code().to_owned())
                .collect(),
            executables: self
                .executables
                .iter()
                .map(|executable| JsonExecutable {
                    name: executable.name.clone(),
                    path: executable.path.clone(),
                    uses: executable
                        .uses
                        .iter()
                        .map(|use_| use_.as_code().to_owned())
                        .collect(),
                })
                .collect(),
            secrets: self.secrets.clone(),
            endpoints: self
                .endpoints
                .iter()
                .map(|endpoint| JsonEndpoint {
                    name: endpoint.name.clone(),
                    transport: endpoint.transport.clone(),
                    purpose: endpoint.purpose.clone(),
                    default_port: endpoint.default_port,
                })
                .collect(),
            downloads: self
                .downloads
                .iter()
                .map(|download| JsonDownload {
                    artifact: download.artifact.clone(),
                    version: download.version.clone(),
                    os: download.os.as_code().to_owned(),
                    arch: download.arch.as_code().to_owned(),
                    sha256: download.sha256.to_string(),
                    size: download.size,
                    hosts: download.hosts.clone(),
                    signing_policy: download.signing_policy.clone(),
                })
                .collect(),
            open_with: self
                .open_with
                .iter()
                .map(|entry| JsonOpenWith {
                    name: entry.name.clone(),
                    kind: entry.kind.clone(),
                    target: entry.target.clone(),
                })
                .collect(),
        };
        serde_json::to_string(&json).expect("a permissions summary always encodes as JSON")
    }

    /// Reads the stored form back.
    ///
    /// # Errors
    ///
    /// Returns a [`PermissionsJsonError`] when the text is not the stored
    /// form or holds a code this build does not know.
    pub fn from_json(text: &str) -> Result<Self, PermissionsJsonError> {
        let error = |detail: String| PermissionsJsonError { detail };
        let json: Json = serde_json::from_str(text).map_err(|err| error(err.to_string()))?;
        let code = |what: &str, value: &str| error(format!("unknown {what} {value:?}"));
        Ok(Self {
            capabilities: json
                .capabilities
                .iter()
                .map(|value| value.parse().map_err(|_| code("capability", value)))
                .collect::<Result<_, _>>()?,
            managed_filesystem: json
                .managed_filesystem
                .iter()
                .map(|value| InstanceDir::from_code(value).ok_or_else(|| code("folder", value)))
                .collect::<Result<_, _>>()?,
            executables: json
                .executables
                .into_iter()
                .map(|executable| {
                    Ok(PermittedExecutable {
                        uses: executable
                            .uses
                            .iter()
                            .map(|value| {
                                ExecutableUse::from_code(value)
                                    .ok_or_else(|| code("executable use", value))
                            })
                            .collect::<Result<_, _>>()?,
                        name: executable.name,
                        path: executable.path,
                    })
                })
                .collect::<Result<_, PermissionsJsonError>>()?,
            secrets: json.secrets,
            endpoints: json
                .endpoints
                .into_iter()
                .map(|endpoint| PermittedEndpoint {
                    name: endpoint.name,
                    transport: endpoint.transport,
                    purpose: endpoint.purpose,
                    default_port: endpoint.default_port,
                })
                .collect(),
            downloads: json
                .downloads
                .into_iter()
                .map(|download| {
                    Ok(PermittedDownload {
                        os: download.os.parse().map_err(|_| code("os", &download.os))?,
                        arch: download
                            .arch
                            .parse()
                            .map_err(|_| code("arch", &download.arch))?,
                        sha256: download
                            .sha256
                            .parse()
                            .map_err(|_| code("sha256", &download.sha256))?,
                        artifact: download.artifact,
                        version: download.version,
                        size: download.size,
                        hosts: download.hosts,
                        signing_policy: download.signing_policy,
                    })
                })
                .collect::<Result<_, PermissionsJsonError>>()?,
            open_with: json
                .open_with
                .into_iter()
                .map(|entry| PermittedOpenWith {
                    name: entry.name,
                    kind: entry.kind,
                    target: entry.target,
                })
                .collect(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Json {
    capabilities: Vec<String>,
    managed_filesystem: Vec<String>,
    executables: Vec<JsonExecutable>,
    secrets: Vec<String>,
    endpoints: Vec<JsonEndpoint>,
    downloads: Vec<JsonDownload>,
    open_with: Vec<JsonOpenWith>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonExecutable {
    name: String,
    path: String,
    uses: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonEndpoint {
    name: String,
    transport: String,
    purpose: String,
    default_port: Option<u16>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonDownload {
    artifact: String,
    version: String,
    os: String,
    arch: String,
    sha256: String,
    size: u64,
    hosts: Vec<String>,
    signing_policy: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonOpenWith {
    name: String,
    kind: String,
    target: Option<String>,
}
