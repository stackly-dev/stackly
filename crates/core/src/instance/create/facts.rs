//! What create reads before it judges a draft (spec 0020, *Evaluator
//! create*): [`gather`] is the only IO of preflight and of the create
//! precheck, and [`CreateFacts`] is all [`evaluate`](super::evaluate) sees.
//! Nothing here writes, spawns, or shows a dialog.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use stackly_domain::{
    EndpointName, InstallMode, Instance, InstanceFilter, InstanceId, ProviderName,
    ProviderSourceId, Registry, RegistryError, Runtime, RuntimeState, SecretRef,
};
use stackly_platform::{
    DirectoryLayout, FileKind, Platform, PlatformError, SecretBackend, SecretPresence,
    SecretStoreError, StoreState, VolumeFacts, check_private_dir,
};

use super::draft::{CreateDraft, SecretSourceKind};
use super::port::{PortFacts, gather_ports, slots};
use crate::PreparedLayout;
use crate::catalog::{CatalogEntry, CatalogLocation, CatalogState, UnavailableReason};
use crate::provider::{Category, NameCheck, ValidatedPackage, read_package};
use crate::supervisor::PortProbe;

/// What [`gather`] reads from.
pub struct CreateDeps<'a> {
    /// The registry.
    pub registry: &'a dyn Registry,
    /// The catalog the daemon loaded.
    pub catalog: &'a CatalogState,
    /// The row of the loaded catalog in `provider_sources`, once recorded
    /// (K-2); the pin step needs it.
    pub source: Option<ProviderSourceId>,
    /// The platform ports.
    pub platform: &'a Platform,
    /// The checked layout.
    pub prepared: &'a PreparedLayout,
    /// The trial binds.
    pub probe: &'a dyn PortProbe,
}

/// Where the package create uses comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageSource {
    /// The catalog inside the app bundle; create pins it.
    BundledCatalog,
    /// The catalog of `STACKLY_CATALOG_DIR`; create pins it.
    DevelopmentCatalog,
    /// A package already pinned in the data folder; create does not pin.
    Pinned,
}

/// The package a draft resolved to.
#[derive(Debug, Clone)]
pub struct ProviderFound {
    /// The package, validated.
    pub package: Arc<ValidatedPackage>,
    /// Where it comes from.
    pub source: PackageSource,
    /// The name shown for the provider.
    pub display_name: String,
    /// Its category.
    pub category: Category,
}

/// Why a draft resolved to no package (*Resolusi provider dan revision*).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderProblem {
    /// The catalog is not loaded.
    CatalogUnavailable {
        /// Why: `not_configured`, `missing`, `index_invalid`, or
        /// `unsupported_schema`.
        reason: &'static str,
    },
    /// No entry or pinned package matches.
    NotFound,
    /// More than one pinned package matches.
    Ambiguous,
    /// The entry is planned or invalid.
    Unavailable {
        /// `planned` or `invalid`.
        state: &'static str,
    },
}

/// Whether the runtime has every program the package runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramCheck {
    /// Every program is a regular executable file inside the runtime root.
    Complete,
    /// This program, relative to the runtime root, is not.
    Missing(String),
}

/// How a folder create writes under was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirFact {
    /// A private folder of the user.
    Private,
    /// Anything else, with why.
    Problem(String),
}

/// The data root as create needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFacts {
    /// `instances/`.
    pub instances: DirFact,
    /// `retained/`.
    pub retained: DirFact,
    /// The file system of the data root, or why it could not be read.
    pub volume: Result<VolumeFacts, String>,
}

/// What the store said about one source reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeFact {
    /// The item exists.
    Present,
    /// The item does not exist.
    Missing,
    /// The store is locked.
    Locked,
    /// The store did not answer in time.
    Busy,
    /// Any other failure.
    Failed,
}

/// What the registry and the store said about one source reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFact {
    /// The text is not a secret reference.
    Invalid,
    /// No registry row names it.
    Unregistered,
    /// A row names it; the store said this.
    Registered(ProbeFact),
}

/// The secret store, as create reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFacts {
    /// Which store this is.
    pub backend: SecretBackend,
    /// The state it was last seen in.
    pub state: StoreState,
    /// A fact per `copyFrom` and `bind` reference text of the draft.
    pub sources: BTreeMap<String, SourceFact>,
}

/// Everything [`evaluate`](super::evaluate) needs.
#[derive(Debug, Clone)]
pub struct CreateFacts {
    /// The package, or why there is none.
    pub provider: Result<ProviderFound, ProviderProblem>,
    /// The row of the loaded catalog in `provider_sources`.
    pub source_id: Option<ProviderSourceId>,
    /// The runtime of the draft.
    pub runtime: Option<Runtime>,
    /// The canonical folder of that runtime, when it exists.
    pub runtime_root: Option<PathBuf>,
    /// Whether it has the programs of the package, once both are known.
    pub programs: Option<ProgramCheck>,
    /// Every runtime that is `installed`, for the label of the equivalent
    /// command.
    pub installed_runtimes: Vec<Runtime>,
    /// Every instance that is not retained.
    pub instances: Vec<Instance>,
    /// The ID of every instance, retained ones included.
    pub known_ids: Vec<InstanceId>,
    /// The trial binds of the fixed ports that reached them.
    pub ports: BTreeMap<EndpointName, PortFacts>,
    /// The data root.
    pub data: DataFacts,
    /// The bytes free in `instances/`, or why they could not be read.
    pub disk: Result<u64, String>,
    /// The secret store.
    pub secrets: SecretFacts,
    /// The layout that derives the paths of the new instance.
    pub layout: DirectoryLayout,
}

/// Why [`gather`] could not read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GatherError {
    /// The registry failed.
    #[error("the registry could not be read: {0}")]
    Registry(#[from] RegistryError),
    /// A platform port failed in a way no row can express.
    #[error("the platform could not be read: {0}")]
    Platform(#[from] PlatformError),
}

/// Reads everything [`evaluate`](super::evaluate) needs for `draft`.
///
/// It only reads: the registry, the catalog in memory, the pinned package
/// folder, file metadata, trial binds, and the secret store's state and
/// presence checks (never with a dialog). It blocks, so async callers run
/// it in `spawn_blocking`, outside the gate of `begin`.
///
/// # Errors
///
/// Returns [`GatherError`] when the registry cannot be read or a platform
/// port fails where no preflight row can say so.
pub fn gather(deps: &CreateDeps<'_>, draft: &CreateDraft) -> Result<CreateFacts, GatherError> {
    let registry = deps.registry;
    let instances = registry
        .instances(InstanceFilter {
            include_retained: false,
        })?
        .value;
    let known_ids = registry.known_instance_ids()?.value;
    let runtimes = registry.runtimes()?.value;
    let installed_runtimes: Vec<Runtime> = runtimes
        .iter()
        .filter(|runtime| runtime.state() == RuntimeState::Installed)
        .cloned()
        .collect();
    let runtime = runtimes
        .into_iter()
        .find(|runtime| runtime.id() == draft.runtime_id);

    let provider = resolve_provider(deps, draft)?;
    let runtime_root = runtime
        .as_ref()
        .and_then(|runtime| runtime_root(deps, runtime));
    let programs = match (&runtime, &provider, &runtime_root) {
        (Some(runtime), Ok(found), root)
            if runtime.software() == found.package.software()
                && runtime.state() == RuntimeState::Installed =>
        {
            Some(check_programs(
                deps.platform,
                &found.package,
                root.as_deref(),
            ))
        }
        _ => None,
    };
    let ports = match &provider {
        Ok(found) => gather_ports(deps.probe, draft, &slots(&found.package), &instances),
        Err(_) => BTreeMap::new(),
    };
    Ok(CreateFacts {
        provider,
        source_id: deps.source,
        runtime,
        runtime_root,
        programs,
        installed_runtimes,
        instances,
        known_ids,
        ports,
        data: gather_data(deps),
        disk: deps
            .platform
            .fs()
            .available_space(&deps.prepared.layout().instances_dir())
            .map_err(|error| error.to_string()),
        secrets: gather_secrets(deps, draft)?,
        layout: deps.prepared.layout().clone(),
    })
}

/// *Resolusi provider dan revision*.
fn resolve_provider(
    deps: &CreateDeps<'_>,
    draft: &CreateDraft,
) -> Result<Result<ProviderFound, ProviderProblem>, GatherError> {
    let asked = draft.provider.revision.as_deref();
    let entry = match deps.catalog {
        CatalogState::Loaded(catalog) => draft
            .provider
            .name
            .parse::<ProviderName>()
            .ok()
            .and_then(|id| catalog.entry(&id).map(|entry| (catalog, entry))),
        CatalogState::NotConfigured | CatalogState::Unavailable { .. } => None,
    };
    match (entry, asked) {
        (Some((catalog, CatalogEntry::Provider(entry))), asked)
            if asked
                .is_none_or(|revision| revision == entry.package.key().revision().to_string()) =>
        {
            let source = match &catalog.meta().location {
                CatalogLocation::Bundled(_) => PackageSource::BundledCatalog,
                CatalogLocation::Development(_) => PackageSource::DevelopmentCatalog,
            };
            Ok(Ok(ProviderFound {
                package: Arc::clone(&entry.package),
                source,
                display_name: entry.package.manifest().display_name().to_owned(),
                category: entry.package.manifest().category(),
            }))
        }
        (Some((_, CatalogEntry::Planned(_))), None) => {
            Ok(Err(ProviderProblem::Unavailable { state: "planned" }))
        }
        (Some((_, CatalogEntry::Invalid(_))), None) => {
            Ok(Err(ProviderProblem::Unavailable { state: "invalid" }))
        }
        (_, None) => Ok(Err(match deps.catalog {
            CatalogState::NotConfigured => ProviderProblem::CatalogUnavailable {
                reason: "not_configured",
            },
            CatalogState::Unavailable { reason, .. } => ProviderProblem::CatalogUnavailable {
                reason: unavailable_code(*reason),
            },
            CatalogState::Loaded(_) => ProviderProblem::NotFound,
        })),
        (_, Some(revision)) => find_pinned(deps, &draft.provider.name, revision),
    }
}

const fn unavailable_code(reason: UnavailableReason) -> &'static str {
    reason.as_code()
}

/// The one pinned package of `provider` at `revision`: the folder under the
/// provider root, read and checked against its digest and the registry.
fn find_pinned(
    deps: &CreateDeps<'_>,
    provider: &str,
    revision: &str,
) -> Result<Result<ProviderFound, ProviderProblem>, GatherError> {
    let not_found = Ok(Err(ProviderProblem::NotFound));
    let Some((namespace, name)) = provider.split_once('/') else {
        return not_found;
    };
    if [namespace, name, revision]
        .iter()
        .any(|part| part.is_empty() || part.contains(['/', '\\']) || *part == "..")
    {
        return not_found;
    }
    let base = deps
        .prepared
        .layout()
        .providers_dir()
        .join(namespace)
        .join(name)
        .join(revision);
    let Ok(entries) = deps.platform.fs().read_dir(&base) else {
        return not_found;
    };
    let digests: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.info.kind == FileKind::Dir)
        .collect();
    let [one] = digests.as_slice() else {
        return Ok(Err(if digests.is_empty() {
            ProviderProblem::NotFound
        } else {
            ProviderProblem::Ambiguous
        }));
    };
    let dir = base.join(&one.name);
    let Some(package) = read_package(deps.platform.fs(), &dir)
        .ok()
        .and_then(|files| ValidatedPackage::from_files(files, NameCheck::Skip).ok())
    else {
        return not_found;
    };
    let key = package.key();
    let digest_matches = one.name.to_string_lossy() == key.manifest_sha256().to_string();
    let named = key.provider().to_string() == provider && key.revision().to_string() == revision;
    let known = deps.registry.provider_revision(key)?.value.is_some();
    if !(digest_matches && named && known) {
        return not_found;
    }
    Ok(Ok(ProviderFound {
        display_name: package.manifest().display_name().to_owned(),
        category: package.manifest().category(),
        package: Arc::new(package),
        source: PackageSource::Pinned,
    }))
}

/// The canonical folder of the runtime, if it exists.
fn runtime_root(deps: &CreateDeps<'_>, runtime: &Runtime) -> Option<PathBuf> {
    let folder = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => deps.prepared.layout().runtime_dir(
            runtime.software(),
            runtime.version(),
            runtime.target(),
            artifact_sha256,
        ),
        InstallMode::External { root, .. } => root.clone(),
    };
    deps.platform.fs().canonicalize(&folder).ok()
}

/// Whether every program the package runs is a regular, executable file
/// inside the runtime root (symlinks are followed once and must stay in).
fn check_programs(
    platform: &Platform,
    package: &ValidatedPackage,
    root: Option<&std::path::Path>,
) -> ProgramCheck {
    let manifest = package.manifest();
    let programs = std::iter::once(&manifest.executable).chain(manifest.aliases.values());
    for components in programs {
        let relative = components.join("/");
        let Some(root) = root else {
            return ProgramCheck::Missing(relative);
        };
        let path: PathBuf = components
            .iter()
            .fold(root.to_path_buf(), |mut path, part| {
                path.push(part);
                path
            });
        let fs = platform.fs();
        let regular = fs
            .canonicalize(&path)
            .ok()
            .filter(|resolved| resolved.starts_with(root))
            .and_then(|resolved| fs.symlink_metadata(&resolved).ok().flatten())
            .is_some_and(|info| {
                info.kind == FileKind::File && info.mode.is_some_and(|mode| mode & 0o111 != 0)
            });
        if !regular {
            return ProgramCheck::Missing(relative);
        }
    }
    ProgramCheck::Complete
}

fn gather_data(deps: &CreateDeps<'_>) -> DataFacts {
    let layout = deps.prepared.layout();
    let fs = deps.platform.fs();
    let user = deps.platform.identity().current_user();
    let check = |path: PathBuf| match (&user, fs.symlink_metadata(&path)) {
        (Ok(user), Ok(Some(info))) => match check_private_dir(&info, user) {
            Ok(()) => DirFact::Private,
            Err(violation) => DirFact::Problem(violation.to_string()),
        },
        (_, Ok(None)) => DirFact::Problem("the folder is missing".to_owned()),
        (Err(error), _) => DirFact::Problem(error.to_string()),
        (_, Err(error)) => DirFact::Problem(error.to_string()),
    };
    DataFacts {
        instances: check(layout.instances_dir()),
        retained: check(layout.retained_dir()),
        volume: fs
            .volume_facts(layout.roots().data())
            .map_err(|error| error.to_string()),
    }
}

fn gather_secrets(deps: &CreateDeps<'_>, draft: &CreateDraft) -> Result<SecretFacts, GatherError> {
    let store = deps.platform.secrets();
    let mut sources = BTreeMap::new();
    for (_, source) in &draft.secrets {
        let (SecretSourceKind::CopyFrom(text) | SecretSourceKind::Bind(text)) = source else {
            continue;
        };
        if sources.contains_key(text) {
            continue;
        }
        let fact = match SecretRef::parse(text) {
            Err(_) => SourceFact::Invalid,
            Ok(reference) => match deps.registry.secret_reference(&reference)?.value {
                None => SourceFact::Unregistered,
                Some(_) => SourceFact::Registered(match store.probe(&reference) {
                    Ok(SecretPresence::Present) => ProbeFact::Present,
                    Ok(_) => ProbeFact::Missing,
                    Err(SecretStoreError::Locked) => ProbeFact::Locked,
                    Err(SecretStoreError::Busy) => ProbeFact::Busy,
                    Err(_) => ProbeFact::Failed,
                }),
            },
        };
        sources.insert(text.clone(), fact);
    }
    Ok(SecretFacts {
        backend: store.backend(),
        state: store.state(),
        sources,
    })
}
