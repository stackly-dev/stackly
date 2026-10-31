//! The `runtime` methods (spec 0018, *Alur install*, *Resolusi selector*):
//! `runtime.resolve`, `runtime.install`, `runtime.list`, and
//! `runtime.get`.
//!
//! `runtime.install` goes through
//! [`start_operation`](crate::ipc::operation::start_operation) like every
//! method that starts an operation, but its replay comes first and skips
//! the catalog altogether: a repeated request gets the reply it had from
//! the artifact the journal of the first one froze, never from a catalog
//! that may have changed. The precheck answers `alreadyInstalled` without
//! an operation and refuses a runtime in another state.

use std::collections::BTreeMap;
use std::sync::Arc;

use stackly_core::catalog::{Catalog, CatalogEntry, NotInstallableReason, ProviderEntry};
use stackly_core::operation::{
    BeginRequest, BusyPolicy, NoOpReply, OperationError, Origin, Precheck, PrecheckOutcome,
};
use stackly_core::provider::VersionArtifact;
use stackly_core::runtime::compatibility::support_status;
use stackly_core::runtime::download::DownloadPolicy;
use stackly_core::runtime::install::{InstallDeps, InstallInput, install_runtime_work};
use stackly_core::runtime::resolve::resolve;
use stackly_core::runtime::validate::VersionCheckSpec;
use stackly_domain::{
    ArtifactRecord, DetailKey, ErrorCodeName, InstallMode, InstanceFilter, LockKey, OperationKind,
    OperationResource, ProviderName, Registry, Runtime, RuntimeId, RuntimeSelector, RuntimeState,
    Sha256Digest, SoftwareName,
};
use stackly_platform::HostFacts;
use stackly_protocol::codes::{request, resource};
use stackly_protocol::error::FieldError;
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::json::FieldCode;
use stackly_protocol::message::Call;
use stackly_protocol::runtime::{
    GetRuntimeParams, InstallOutcome, InstallRuntimeParams, InstallRuntimeReply,
    ListRuntimesParams, RUNTIME_PAGE_BYTES_MAX, ResolvedArtifact, RuntimeDetail, RuntimeListPage,
    RuntimeSummary,
};
use stackly_protocol::{ErrorBody, ResourceKind, ResourceRef};
use stackly_storage::SqliteRegistry;

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::{
    StartOutcome, StartRequest as OperationStart, replay_operation, start_operation,
};
use crate::map;
use crate::state::DaemonCore;

/// What a selector resolved to, with everything an install freezes.
struct Selection {
    resolved: ResolvedArtifact,
    input: InstallInput,
    sha256: Sha256Digest,
}

/// The real `runtime.resolve`.
pub fn resolve_runtime(
    core: &DaemonCore,
    params: InstallRuntimeParams,
) -> HandlerFuture<'_, Box<ResolvedArtifact>> {
    Box::pin(async move { select(core, &params).map(|selection| Box::new(selection.resolved)) })
}

/// The real `runtime.install`.
pub fn install<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: InstallRuntimeParams,
) -> HandlerFuture<'a, Box<InstallRuntimeReply>> {
    let context = context.clone();
    Box::pin(async move {
        let fingerprint = request_fingerprint(&Call::RuntimeInstall(params.clone()));
        let key = context.idempotency_key.as_deref();
        let start = OperationStart::new("runtime.install", key, fingerprint);

        // A repeated request gets the reply it had: the lookup of the key
        // comes before the catalog and the registry (K12).
        if let Some(replay) = replay_operation(core, &context, start).await? {
            let input = InstallInput::from_payload(&replay.input)
                .map_err(|_| unexpected("the journal of the operation holds no install input"))?;
            let resolved = map::runtime::resolved_from_input(&input)
                .map_err(|_| unexpected("the journal of the operation holds no install input"))?;
            return Ok(Box::new(InstallRuntimeReply {
                resolved,
                outcome: InstallOutcome::Started(Box::new(replay.accepted)),
            }));
        }

        let registry = registry_of(core)?;
        let selection = select(core, &params)?;
        let runtime_id = RuntimeId::generate();
        let payload = selection.input.to_payload().map_err(|error| {
            tracing::error!(%error, "an install input does not fit the journal");
            unexpected("the install input does not fit the journal")
        })?;
        let work = install_runtime_work(
            InstallDeps {
                platform: Arc::new(core.platform().clone()),
                layout: Arc::new(core.prepared().clone()),
                policy: DownloadPolicy::default(),
            },
            &selection.input,
            runtime_id,
        )
        .map_err(|_| unexpected("the install input does not build a work"))?;
        let sha256 = selection.sha256;
        let precheck: Box<dyn Precheck> = {
            let registry = registry.clone();
            Box::new(move || install_precheck(&registry, &sha256))
        };
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::InstallRuntime,
            resource: OperationResource::Runtime(runtime_id),
            creates_resource: true,
            extra_locks: vec![LockKey::Artifact(sha256)],
            // Two installs of one artifact run one after the other and end
            // in one runtime.
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input: payload,
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "runtime.install",
                key,
                request_fingerprint(&Call::RuntimeInstall(params)),
            ),
            request,
        )
        .await?;
        let outcome = match outcome {
            StartOutcome::Accepted(accepted) => InstallOutcome::Started(accepted),
            StartOutcome::NothingToDo => {
                let runtime = installed_runtime(core, &registry, &sha256).await?;
                InstallOutcome::AlreadyInstalled(Box::new(runtime))
            }
        };
        Ok(Box::new(InstallRuntimeReply {
            resolved: selection.resolved,
            outcome,
        }))
    })
}

/// The real `runtime.list`: ordered by ID, a page that stops before its
/// summaries pass [`RUNTIME_PAGE_BYTES_MAX`] of JSON, and the installs that
/// have not ended on the first page.
pub fn list(
    core: &DaemonCore,
    params: ListRuntimesParams,
) -> HandlerFuture<'_, Box<RuntimeListPage>> {
    Box::pin(async move {
        let registry = registry_of(core)?;
        let limit = usize::try_from(params.limit_or_default()).unwrap_or(usize::MAX);
        let after = params.after;
        let first_page = after.is_none();
        let catalog = core.catalog();
        let host = core.platform().info().host_facts();
        let read = tokio::task::spawn_blocking(move || read_runtimes(&registry, first_page))
            .await
            .map_err(|_| unexpected("the runtime list task failed"))?
            .map_err(|error| Box::new(map::registry_error(&error)))?;
        let names = DisplayNames::new(catalog.as_deref());
        let remaining: Vec<&Runtime> = read
            .runtimes
            .iter()
            .filter(|runtime| after.is_none_or(|after| runtime.id().as_uuid() > after))
            .collect();
        let mut items = Vec::new();
        let mut bytes = 0;
        for runtime in &remaining {
            let summary = summary_of(runtime, &read, &names, catalog.as_deref(), &host);
            let size = serde_json::to_vec(&summary).map_or(0, |encoded| encoded.len());
            if !items.is_empty() && (items.len() >= limit || bytes + size > RUNTIME_PAGE_BYTES_MAX)
            {
                break;
            }
            bytes += size;
            items.push(summary);
        }
        // `next` names the last runtime of the page, and only when more
        // follow it.
        let next = remaining
            .get(items.len())
            .and(items.last())
            .map(|last| last.id);
        let installing = read
            .installing
            .iter()
            .filter_map(|(operation, runtime_id, input)| {
                let display = names.of(&input.software);
                map::runtime::installing_runtime(operation, *runtime_id, input, display)
            })
            .collect();
        Ok(Box::new(RuntimeListPage {
            items,
            next,
            installing,
        }))
    })
}

/// The real `runtime.get`.
pub fn get(core: &DaemonCore, params: GetRuntimeParams) -> HandlerFuture<'_, Box<RuntimeDetail>> {
    Box::pin(async move {
        let registry = registry_of(core)?;
        let id = params.runtime_id;
        let catalog = core.catalog();
        let host = core.platform().info().host_facts();
        let read = tokio::task::spawn_blocking(move || read_runtimes(&registry, false))
            .await
            .map_err(|_| unexpected("the runtime read task failed"))?
            .map_err(|error| Box::new(map::registry_error(&error)))?;
        let Some(runtime) = read
            .runtimes
            .iter()
            .find(|runtime| runtime.id().as_uuid() == id)
        else {
            return Err(Box::new(
                ErrorBody::new(&resource::NOT_FOUND, "the runtime does not exist").with_resource(
                    ResourceRef {
                        kind: ResourceKind::Runtime,
                        id: id.to_string(),
                    },
                ),
            ));
        };
        let names = DisplayNames::new(catalog.as_deref());
        let summary = summary_of(runtime, &read, &names, catalog.as_deref(), &host);
        let (path, artifact) = match runtime.install() {
            InstallMode::Managed { artifact_sha256 } => (
                core.prepared()
                    .layout()
                    .runtime_dir(
                        runtime.software(),
                        runtime.version(),
                        runtime.target(),
                        artifact_sha256,
                    )
                    .display()
                    .to_string(),
                read.artifacts.get(artifact_sha256),
            ),
            InstallMode::External { root, .. } => (root.display().to_string(), None),
        };
        Ok(Box::new(map::runtime::runtime_detail(
            summary, path, artifact,
        )))
    })
}

/// Everything the three read methods need from the registry, read once.
struct Read {
    runtimes: Vec<Runtime>,
    instances: Vec<stackly_domain::Instance>,
    artifacts: BTreeMap<Sha256Digest, ArtifactRecord>,
    installing: Vec<(stackly_domain::Operation, RuntimeId, InstallInput)>,
}

fn read_runtimes(
    registry: &SqliteRegistry,
    with_installing: bool,
) -> Result<Read, stackly_domain::RegistryError> {
    let runtimes = registry.runtimes()?.value;
    let instances = registry
        .instances(InstanceFilter {
            include_retained: true,
        })?
        .value;
    let mut artifacts = BTreeMap::new();
    for runtime in &runtimes {
        if let InstallMode::Managed { artifact_sha256 } = runtime.install()
            && let Some(record) = registry.artifact(artifact_sha256)?.value
        {
            artifacts.insert(*artifact_sha256, record);
        }
    }
    let mut installing = Vec::new();
    if with_installing {
        for (operation, journal) in registry.unfinished_operations()?.value {
            if operation.kind() != OperationKind::InstallRuntime {
                continue;
            }
            let OperationResource::Runtime(runtime_id) = operation.resource() else {
                continue;
            };
            if let Ok(input) = InstallInput::from_payload(journal.journal.input()) {
                installing.push((operation, runtime_id, input));
            }
        }
    }
    Ok(Read {
        runtimes,
        instances,
        artifacts,
        installing,
    })
}

/// The summary of one runtime: its users and dependents from the read, its
/// display name and support from the catalog.
fn summary_of(
    runtime: &Runtime,
    read: &Read,
    names: &DisplayNames,
    catalog: Option<&stackly_core::catalog::CatalogState>,
    host: &HostFacts,
) -> RuntimeSummary {
    let users = read
        .instances
        .iter()
        .filter(|instance| instance.runtime_id() == runtime.id())
        .map(map::runtime::runtime_user)
        .collect();
    let dependents = read
        .runtimes
        .iter()
        .filter(|other| other.depends_on().contains(&runtime.id()))
        .map(|other| other.id().as_uuid())
        .collect();
    let rows = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => read
            .artifacts
            .get(artifact_sha256)
            .map(|artifact| artifact.compatibility.as_slice())
            .unwrap_or_default(),
        _ => &[],
    };
    let support = catalog_support(catalog, runtime, host)
        .unwrap_or_else(|| map::runtime::registry_support(rows, host));
    map::runtime::runtime_summary(
        runtime,
        names.of(runtime.software().as_str()),
        support,
        users,
        dependents,
    )
}

/// The support of a runtime whose artifact is still in the catalog, from
/// the catalog's records (spec 0018, *`support_status`*).
fn catalog_support(
    catalog: Option<&stackly_core::catalog::CatalogState>,
    runtime: &Runtime,
    host: &HostFacts,
) -> Option<(
    stackly_protocol::catalog::SupportStatus,
    stackly_protocol::catalog::SupportReason,
)> {
    let catalog = loaded_catalog(catalog)?;
    let InstallMode::Managed { artifact_sha256 } = runtime.install() else {
        return None;
    };
    for entry in catalog.entries() {
        let CatalogEntry::Provider(provider) = entry else {
            continue;
        };
        if provider.package.software() != runtime.software() {
            continue;
        }
        let found = provider.artifacts().iter().find(|artifact| {
            matches!(
                &artifact.status,
                stackly_core::provider::ArtifactStatus::Available { sha256, .. }
                    if sha256 == artifact_sha256
            )
        });
        if let Some(artifact) = found {
            let verdict = support_status(
                artifact,
                artifact.compatibility(),
                Some(provider.package.key()),
                host,
            );
            return Some(map::runtime::verdict_support(&verdict));
        }
    }
    None
}

fn loaded_catalog(catalog: Option<&stackly_core::catalog::CatalogState>) -> Option<&Catalog> {
    match catalog? {
        stackly_core::catalog::CatalogState::Loaded(catalog) => Some(catalog),
        _ => None,
    }
}

/// The catalog's display name for each software, with the software name as
/// the fallback.
struct DisplayNames(BTreeMap<String, String>);

impl DisplayNames {
    fn new(catalog: Option<&stackly_core::catalog::CatalogState>) -> Self {
        let mut names = BTreeMap::new();
        if let Some(catalog) = loaded_catalog(catalog) {
            for entry in catalog.entries() {
                if let CatalogEntry::Provider(provider) = entry {
                    names
                        .entry(provider.package.software().to_string())
                        .or_insert_with(|| entry.display_name());
                }
            }
        }
        Self(names)
    }

    fn of(&self, software: &str) -> String {
        self.0
            .get(software)
            .cloned()
            .unwrap_or_else(|| software.to_owned())
    }
}

/// Resolves the request against the catalog and the host: the entry, the
/// selector, the gate, and `expectedSha256`, and freezes what an install
/// needs into its input.
fn select(core: &DaemonCore, params: &InstallRuntimeParams) -> Result<Selection, Box<ErrorBody>> {
    let selector: RuntimeSelector = params.selector.parse().map_err(|_| {
        Box::new(
            ErrorBody::new(
                &request::INVALID_PARAMS,
                "the selector is not stable, a digest, a version, or a line",
            )
            .with_detail("method", "runtime.install")
            .with_field(FieldError::new("selector", FieldCode::InvalidFormat)),
        )
    })?;
    let catalog = crate::catalog::loaded(core)?;
    let host = core.platform().info().host_facts();
    let asked = params.provider.as_str();
    let entry = asked
        .parse::<ProviderName>()
        .ok()
        .and_then(|id| catalog.entry(&id));
    let Some(entry) = entry else {
        let matches = if asked.contains('/') {
            Vec::new()
        } else {
            catalog.find_by_name(asked)
        };
        let suggestion = match matches.as_slice() {
            [only] => Some(*only),
            _ => None,
        };
        return Err(Box::new(map::catalog::provider_not_found(
            asked, suggestion,
        )));
    };
    let provider: &ProviderEntry = match entry {
        CatalogEntry::Provider(provider) => provider,
        CatalogEntry::Planned(_) => {
            return Err(refuse(
                asked,
                &params.selector,
                &host,
                NotInstallableReason::ProviderPlanned,
            ));
        }
        _ => {
            return Err(refuse(
                asked,
                &params.selector,
                &host,
                NotInstallableReason::PackageInvalid,
            ));
        }
    };
    let resolution = resolve(provider, &selector, &host)
        .map_err(|error| Box::new(map::runtime::resolve_error(&error, asked, &params.selector)))?;
    let artifact: &VersionArtifact = resolution.artifact;
    let stackly_core::provider::ArtifactStatus::Available { sha256, .. } = &artifact.status else {
        return Err(unexpected(
            "the gate let an artifact through that is not available",
        ));
    };
    if let Some(expected) = &params.expected_sha256
        && *expected != sha256.to_string()
    {
        return Err(Box::new(map::runtime::selector_changed(
            expected,
            &sha256.to_string(),
        )));
    }
    let key = provider.package.key();
    let software: &SoftwareName = provider.package.software();
    let verdict = support_status(artifact, artifact.compatibility(), Some(key), &host);
    let resolved = map::runtime::resolved_artifact(
        &entry.id().to_string(),
        software.as_str(),
        artifact,
        &verdict,
    )
    .ok_or_else(|| unexpected("the gate let an artifact through that is not available"))?;
    let version_check = provider
        .package
        .version_check()
        .map(VersionCheckSpec::from_check);
    let input = InstallInput::new(
        software,
        &entry.id().to_string(),
        &params.selector,
        &host,
        artifact,
        key,
        version_check.as_ref(),
    )
    .map_err(|error| {
        tracing::error!(%error, "an artifact cannot be frozen into an install input");
        unexpected("the artifact cannot be frozen into an install input")
    })?;
    Ok(Selection {
        resolved,
        input,
        sha256: *sha256,
    })
}

fn refuse(
    provider: &str,
    selector: &str,
    host: &HostFacts,
    reason: NotInstallableReason,
) -> Box<ErrorBody> {
    Box::new(map::runtime::entry_not_installable(
        provider,
        selector,
        host.target,
        reason,
    ))
}

/// Answers `alreadyInstalled` without an operation when a managed runtime
/// for the artifact is installed, and refuses one in another state.
fn install_precheck(registry: &SqliteRegistry, sha256: &Sha256Digest) -> PrecheckOutcome {
    let runtimes = match registry.runtimes() {
        Ok(read) => read.value,
        Err(error) => return PrecheckOutcome::Reject(Box::new(RegistryRead(error.to_string()))),
    };
    let found = runtimes.iter().find(|runtime| {
        matches!(
            runtime.install(),
            InstallMode::Managed { artifact_sha256 } if artifact_sha256 == sha256
        )
    });
    match found {
        None => PrecheckOutcome::Proceed,
        Some(runtime) if runtime.state() == RuntimeState::Installed => {
            PrecheckOutcome::NoOp(NoOpReply::nothing())
        }
        Some(runtime) => PrecheckOutcome::Reject(Box::new(AlreadyRegistered {
            runtime: runtime.id(),
            state: runtime.state(),
        })),
    }
}

/// `runtime.already_registered`.
#[derive(Debug)]
struct AlreadyRegistered {
    runtime: RuntimeId,
    state: RuntimeState,
}

impl std::fmt::Display for AlreadyRegistered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a runtime for this artifact exists and is {}",
            self.state.as_code()
        )
    }
}

impl std::error::Error for AlreadyRegistered {}

impl OperationError for AlreadyRegistered {
    fn code(&self) -> ErrorCodeName {
        stackly_core::runtime::codes::ALREADY_REGISTERED
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        vec![
            (key("runtimeId"), self.runtime.to_string()),
            (key("state"), self.state.as_code().to_owned()),
        ]
    }
}

fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("a detail key of this file is a valid key")
}

/// A registry read that failed in the precheck: the request is refused
/// with the code of the registry.
#[derive(Debug)]
struct RegistryRead(String);

impl std::fmt::Display for RegistryRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the registry could not be read: {}", self.0)
    }
}

impl std::error::Error for RegistryRead {}

impl OperationError for RegistryRead {
    fn code(&self) -> ErrorCodeName {
        ErrorCodeName::from_static("registry.unavailable")
    }

    fn retryable(&self) -> bool {
        true
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        Vec::new()
    }
}

/// The installed runtime of `sha256`, after a precheck answered that it is
/// there.
async fn installed_runtime(
    core: &DaemonCore,
    registry: &SqliteRegistry,
    sha256: &Sha256Digest,
) -> Result<RuntimeSummary, Box<ErrorBody>> {
    let registry = registry.clone();
    let catalog = core.catalog();
    let host = core.platform().info().host_facts();
    let digest = *sha256;
    let read = tokio::task::spawn_blocking(move || read_runtimes(&registry, false))
        .await
        .map_err(|_| unexpected("the runtime read task failed"))?
        .map_err(|error| Box::new(map::registry_error(&error)))?;
    let runtime = read
        .runtimes
        .iter()
        .find(|runtime| {
            matches!(
                runtime.install(),
                InstallMode::Managed { artifact_sha256 } if *artifact_sha256 == digest
            )
        })
        .ok_or_else(|| unexpected("the installed runtime vanished while the reply was built"))?;
    let names = DisplayNames::new(catalog.as_deref());
    Ok(summary_of(
        runtime,
        &read,
        &names,
        catalog.as_deref(),
        &host,
    ))
}

/// The summary of `runtime` as `runtime.get` shows it, for the create form
/// (spec 0020, AC-1): the same facts, read now.
///
/// # Errors
///
/// Returns the error body of a failed registry read, and
/// `internal.unexpected` when the runtime is gone from the registry.
pub(crate) async fn summary_of_runtime(
    core: &DaemonCore,
    registry: &SqliteRegistry,
    runtime: &Runtime,
) -> Result<RuntimeSummary, Box<ErrorBody>> {
    let registry = registry.clone();
    let catalog = core.catalog();
    let host = core.platform().info().host_facts();
    let id = runtime.id();
    let read = tokio::task::spawn_blocking(move || read_runtimes(&registry, false))
        .await
        .map_err(|_| unexpected("the runtime read task failed"))?
        .map_err(|error| Box::new(map::registry_error(&error)))?;
    let found = read
        .runtimes
        .iter()
        .find(|candidate| candidate.id() == id)
        .ok_or_else(|| unexpected("the runtime vanished while the reply was built"))?;
    let names = DisplayNames::new(catalog.as_deref());
    Ok(summary_of(found, &read, &names, catalog.as_deref(), &host))
}

fn registry_of(core: &DaemonCore) -> Result<SqliteRegistry, Box<ErrorBody>> {
    core.registry().ok_or_else(|| {
        Box::new(ErrorBody::new(
            &stackly_protocol::codes::registry::UNAVAILABLE,
            "the registry is not available",
        ))
    })
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(
        &stackly_protocol::codes::internal::UNEXPECTED,
        message,
    ))
}
