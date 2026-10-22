//! The facts of the instance detail page (spec 0021, AC-4): everything
//! `instance.get_detail` answers with, built from a snapshot of the
//! registry, the pinned provider package, and the directory layout, without
//! IO, a spawn, or a secret value. `stacklyd` reads the snapshot and maps
//! the result to the `InstanceDetail` DTO; a client never sees a domain
//! type.

use std::path::PathBuf;

use stackly_domain::{
    Endpoint, Environment, EnvironmentId, EnvironmentName, InstallMode, Instance, InstanceId,
    InstancePathEntry, InstancePathKind, Lease, LeaseHolder, LeaseState, Ownership,
    ProviderRevisionKey, ResourceRevision, Runtime, RuntimeId, SecretFieldName, SecretRef,
    SecretStatus, SecretUse, SecretUser, Sha256Digest, StartupPolicy, Target,
};
use stackly_platform::DirectoryLayout;

use crate::instance::connection::{RenderedConnection, render};
use crate::provider::{
    PlanContext, ResolvedEndpoint, RuntimeContext, SettingsValues, ValidatedPackage,
};

/// What [`build`] reads. Every list comes from one read of the registry;
/// none of them holds a secret value.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DetailSnapshot<'a> {
    /// The instance.
    pub instance: &'a Instance,
    /// The runtime it runs on.
    pub runtime: &'a Runtime,
    /// The stored paths of the instance (`Registry::instance_paths`).
    pub paths: &'a [InstancePathEntry],
    /// The uses of secrets by this instance (`Registry::secret_uses_by`).
    pub secret_uses: &'a [SecretUse],
    /// The cached status of secret references, as the registry knows them
    /// (spec 0011). A reference without an entry has status `unknown`.
    pub secret_statuses: &'a [(SecretRef, SecretStatus)],
    /// Every environment (`Registry::environments`).
    pub environments: &'a [Environment],
    /// The leases of this instance that are not released
    /// (`Registry::leases` with `Unreleased`).
    pub leases: &'a [Lease],
    /// The pinned package, or `None` when it cannot be read (spec 0021,
    /// *Value sourcing*): a package that is not the pinned one counts as
    /// unreadable.
    pub package: Option<&'a ValidatedPackage>,
    /// The layout, for the run folder of the instance and its roots.
    pub layout: &'a DirectoryLayout,
}

impl<'a> DetailSnapshot<'a> {
    /// Bundles one snapshot.
    #[must_use]
    #[expect(clippy::too_many_arguments, reason = "one argument per registry read")]
    pub const fn new(
        instance: &'a Instance,
        runtime: &'a Runtime,
        paths: &'a [InstancePathEntry],
        secret_uses: &'a [SecretUse],
        secret_statuses: &'a [(SecretRef, SecretStatus)],
        environments: &'a [Environment],
        leases: &'a [Lease],
        package: Option<&'a ValidatedPackage>,
        layout: &'a DirectoryLayout,
    ) -> Self {
        Self {
            instance,
            runtime,
            paths,
            secret_uses,
            secret_statuses,
            environments,
            leases,
            package,
            layout,
        }
    }
}

/// The detail of one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InstanceDetailData {
    /// The instance.
    pub instance_id: InstanceId,
    /// The revision of the instance this detail was built from.
    pub revision: ResourceRevision,
    /// The pinned provider revision.
    pub provider: DetailProvider,
    /// The runtime.
    pub runtime: DetailRuntime,
    /// The absolute path of the data folder.
    pub data_path: PathBuf,
    /// When the instance starts on its own.
    pub startup: StartupPolicy,
    /// One row per secret the instance uses, by field.
    pub secrets: Vec<DetailSecret>,
    /// The environments that hold the instance, by name and then ID.
    pub dependents: Vec<DetailDependent>,
    /// The connection strings, in manifest order; none when the pinned
    /// package cannot be read.
    pub connections: Vec<RenderedConnection>,
}

/// The pinned provider revision of an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DetailProvider {
    /// The pinned key: provider, revision, and manifest digest.
    pub key: ProviderRevisionKey,
    /// `metadata.displayName` of the pinned package, or `None` when the
    /// package cannot be read. Never taken from the catalog.
    pub display_name: Option<String>,
    /// Whether the pinned package was read: false when the snapshot has no
    /// package or holds one that is not the pinned one, and then
    /// `display_name` is `None` and `connections` is empty. True for a
    /// readable package without a `connection` block too.
    pub package_readable: bool,
}

/// The runtime of an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DetailRuntime {
    /// The runtime ID.
    pub id: RuntimeId,
    /// The exact version.
    pub version: String,
    /// The OS and architecture it was built for.
    pub target: Target,
    /// The digest of the installed artifact; `None` for a runtime in a
    /// folder the user owns.
    pub artifact_sha256: Option<Sha256Digest>,
    /// Whether Stackly installed it.
    pub managed: bool,
}

/// One secret an instance uses.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DetailSecret {
    /// The settings field.
    pub field: SecretFieldName,
    /// The title from the pinned package's schema, else the field name.
    pub title: String,
    /// The reference.
    pub reference: SecretRef,
    /// The cached status; `unknown` before the first probe.
    pub status: SecretStatus,
}

/// An environment that holds the instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DetailDependent {
    /// The environment.
    pub environment_id: EnvironmentId,
    /// Its name.
    pub name: EnvironmentName,
    /// Whether the environment owns the instance.
    pub ownership: Ownership,
    /// Whether the environment holds an active lease on the instance.
    pub lease_active: bool,
}

/// Why a detail could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DetailError {
    /// The registry has no `data` path for the instance.
    #[error("the registry holds no data path for the instance")]
    MissingDataPath,
}

/// Builds the detail of the instance in `snapshot`.
///
/// # Errors
///
/// Returns [`DetailError::MissingDataPath`] when `paths` has no `data` row.
pub fn build(snapshot: &DetailSnapshot<'_>) -> Result<InstanceDetailData, DetailError> {
    let instance = snapshot.instance;
    let runtime = snapshot.runtime;
    let data = snapshot
        .paths
        .iter()
        .find(|entry| entry.kind() == InstancePathKind::Data)
        .ok_or(DetailError::MissingDataPath)?;
    let data_path = snapshot.layout.roots().resolve(data.root(), data.path());
    let package = snapshot
        .package
        .filter(|package| package.key() == instance.provider_revision());
    let secrets = secrets(snapshot, package);
    let connections = package.map_or_else(Vec::new, |package| {
        let context = plan_context(snapshot);
        let refs: Vec<(SecretFieldName, SecretRef)> = secrets
            .iter()
            .map(|secret| (secret.field.clone(), secret.reference.clone()))
            .collect();
        render(package, &context, instance, &refs)
    });
    Ok(InstanceDetailData {
        instance_id: instance.id(),
        revision: instance.revision(),
        provider: DetailProvider {
            key: instance.provider_revision().clone(),
            display_name: package.map(|package| package.manifest().display_name().to_owned()),
            package_readable: package.is_some(),
        },
        runtime: DetailRuntime {
            id: runtime.id(),
            version: runtime.version().to_string(),
            target: runtime.target(),
            artifact_sha256: match runtime.install() {
                InstallMode::Managed { artifact_sha256 } => Some(*artifact_sha256),
                InstallMode::External { .. } => None,
            },
            managed: matches!(runtime.install(), InstallMode::Managed { .. }),
        },
        data_path,
        startup: instance.startup(),
        secrets,
        dependents: dependents(snapshot),
        connections,
    })
}

/// One row per secret use of the instance, ordered by field.
fn secrets(snapshot: &DetailSnapshot<'_>, package: Option<&ValidatedPackage>) -> Vec<DetailSecret> {
    let id = snapshot.instance.id();
    let mut rows: Vec<DetailSecret> = snapshot
        .secret_uses
        .iter()
        .filter(|used| used.user == SecretUser::Instance(id))
        .map(|used| DetailSecret {
            field: used.field.clone(),
            title: package
                .and_then(|package| package.settings_schema().field(used.field.as_str()))
                .map_or_else(|| used.field.to_string(), |field| field.title().to_owned()),
            reference: used.reference.clone(),
            status: snapshot
                .secret_statuses
                .iter()
                .find(|(reference, _)| *reference == used.reference)
                .map_or(SecretStatus::Unknown, |(_, status)| *status),
        })
        .collect();
    rows.sort_by(|a, b| a.field.cmp(&b.field));
    rows
}

/// The environments with the instance as a member, by name and then ID.
fn dependents(snapshot: &DetailSnapshot<'_>) -> Vec<DetailDependent> {
    let id = snapshot.instance.id();
    let mut rows: Vec<DetailDependent> = snapshot
        .environments
        .iter()
        .filter_map(|environment| {
            let member = environment
                .members()
                .iter()
                .find(|member| member.instance_id() == id)?;
            let lease_active = snapshot.leases.iter().any(|lease| {
                lease.instance_id() == id
                    && lease.holder() == LeaseHolder::Environment(environment.id())
                    && lease.state() == LeaseState::Active
            });
            Some(DetailDependent {
                environment_id: environment.id(),
                name: environment.name().clone(),
                ownership: member.ownership(),
                lease_active,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.environment_id.cmp(&b.environment_id))
    });
    rows
}

/// The plan context of a start without settings (spec 0021, *Render di
/// daemon*): the run folder from the layout, the runtime, and the effective
/// port of every endpoint.
fn plan_context(snapshot: &DetailSnapshot<'_>) -> PlanContext {
    let instance = snapshot.instance;
    let runtime = snapshot.runtime;
    let endpoints = instance
        .endpoints()
        .iter()
        .map(|endpoint| match endpoint {
            Endpoint::Ip(ip) => (
                ip.name().clone(),
                ResolvedEndpoint::Ip {
                    host: ip.address(),
                    port: ip.effective_port(),
                },
            ),
            Endpoint::Unix(unix) => (unix.name().clone(), ResolvedEndpoint::Unix),
        })
        .collect();
    let root = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => snapshot.layout.runtime_dir(
            runtime.software(),
            runtime.version(),
            runtime.target(),
            artifact_sha256,
        ),
        InstallMode::External { root, .. } => root.clone(),
    };
    PlanContext {
        instance_id: instance.id(),
        instance_name: instance.name().clone(),
        paths: snapshot.layout.instance_paths(instance.id()),
        runtime: RuntimeContext {
            root,
            version: runtime.version().clone(),
            line: runtime.line().clone(),
        },
        endpoints,
        settings: SettingsValues::default(),
    }
}
