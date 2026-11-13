//! The environment methods (spec 0044, *API surface*): `env.validate`,
//! `env.plan`, `env.list`, `env.get`, `env.import`, and `env.remove`.
//!
//! Validate and plan are read only and are not operations (AC-4, AC-6):
//! they read the manifest through `read_file_limited`, validate it against
//! the catalog in memory, and plan it through the same `gather` and
//! `evaluate` as create, whose only touch on the system is the trial bind
//! of the port check. List reads only the registry and the supervisor;
//! get reads the file only with `checkManifest`. Import and remove are the
//! operations `import_environment` and `remove_environment` on the
//! resource `Environment(id)` with `LockKey::EnvironmentName`:
//! `replay_operation`, then `start_operation`, then the wait for the end,
//! and the membership cache loads again after each (AC-15).
//!
//! Spec 0045 adds the lock and the local overrides next to the manifest,
//! which plan and get read through `read_file_limited`, `env.lock`, which
//! writes only the lock file through `write_file_atomic` and is no
//! operation, and `env.export`, which reads the registry and the pinned
//! packages only.

pub mod children;
mod down;
mod dto;
mod status;
mod up;

pub use down::{down, preview_down};
pub use up::up;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_core::catalog::{Catalog, CatalogState};
use stackly_core::environment::local::{LocalFile, LocalInput, LocalState};
use stackly_core::environment::lock::{
    LockFile, LockInput, LockRefresh, LockState, emit, local_file_name, lock_file_name,
    resolve_entries, state as lock_state,
};
use stackly_core::environment::{
    ImportRequest, InstanceView, ManifestRead, PlanDeps, PlanInputs, UnreadableReason,
    environment_view, import_work, manifest_location, manifest_state, plan as plan_of,
    read_manifest, remove_work, validate as validate_bytes,
};
use stackly_core::environment::{manifest_file_name, validate as validate_manifest};
use stackly_core::instance::create::CreateDeps;
use stackly_core::operation::{BeginRequest, BusyPolicy, Origin};
use stackly_core::supervisor::{InstanceStatus, Supervisor};
use stackly_domain::{
    Environment, EnvironmentId, Instance, InstanceFilter, InstanceLifecycle, JournalPayload,
    LockKey, OperationJournal, OperationKind, OperationResource, Registry, ResourceRevision,
    Runtime,
};
use stackly_protocol::codes::{env as env_codes, internal};
use stackly_protocol::env::{
    EnvExport, EnvExportParams, EnvGetParams, EnvLeaseInfo, EnvLocalInfo, EnvLockInfo,
    EnvLockParams, EnvLockResult, EnvPlan, EnvPlanParams, EnvSource, EnvValidateParams,
    EnvValidation, EnvironmentDetail, EnvironmentList, ImportEnvironmentParams, LockedService,
    RemoveEnvironmentParams, RemovedEnvironment,
};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::InstanceSummary;
use stackly_protocol::message::Call;
use stackly_protocol::{Count, ErrorBody, Timestamp};
use stackly_storage::SqliteRegistry;
use uuid::Uuid;

pub use dto::with_env_hint;

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::{StartRequest as OperationStart, replay_operation, start_operation};
use crate::ipc::provider_import::wait_final;
use crate::map;
use crate::state::DaemonCore;

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&internal::UNEXPECTED, message))
}

fn registry_of(core: &DaemonCore) -> Result<SqliteRegistry, Box<ErrorBody>> {
    core.registry().ok_or_else(|| {
        Box::new(ErrorBody::new(
            &stackly_protocol::codes::registry::UNAVAILABLE,
            "the registry is not available",
        ))
    })
}

fn registry_error(error: &stackly_domain::RegistryError) -> Box<ErrorBody> {
    Box::new(map::registry_error(error))
}

/// `env.manifest_unreadable` with its `reason`.
fn unreadable(reason: UnreadableReason) -> Box<ErrorBody> {
    Box::new(with_env_hint(
        ErrorBody::new(
            &env_codes::MANIFEST_UNREADABLE,
            "the manifest file can't be read",
        )
        .with_detail("reason", reason.as_code()),
    ))
}

fn not_found() -> Box<ErrorBody> {
    Box::new(with_env_hint(ErrorBody::new(
        &env_codes::NOT_FOUND,
        "no environment has that id",
    )))
}

/// Runs a blocking registry or file job off the async threads.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Box<ErrorBody>> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| unexpected("a blocking task of an environment method stopped"))
}

/// What reading a manifest gave: its bytes and where it lives.
struct Read {
    bytes: Vec<u8>,
    location: stackly_domain::ManifestLocation,
}

/// Reads the manifest at `path` and its location (AC-11), through the
/// `Filesystem` port only.
async fn read_file(core: &DaemonCore, path: &Path) -> Result<Read, Box<ErrorBody>> {
    let platform = core.platform().clone();
    let path = path.to_path_buf();
    blocking(move || {
        let bytes = read_manifest(platform.fs(), &path)?;
        let location = manifest_location(platform.fs(), &path)?;
        Ok::<_, UnreadableReason>(Read { bytes, location })
    })
    .await?
    .map_err(unreadable)
}

/// Every instance, retained ones included, as the plan and the status see
/// it, with the summary `instance.get` would send for it. A retained
/// member is a blocker in the plan and `unavailable` in the status (AC-9),
/// so it is never left out.
struct Instances {
    views: Vec<InstanceView>,
    summaries: Vec<InstanceSummary>,
}

/// The lifecycle the summary shows: a live one whose exit went unrecorded
/// is `unknown` (spec 0010).
pub(crate) const fn shown_lifecycle(
    stored: InstanceLifecycle,
    status: &InstanceStatus,
) -> InstanceLifecycle {
    let live = matches!(
        stored,
        InstanceLifecycle::Starting
            | InstanceLifecycle::Running
            | InstanceLifecycle::Recovering
            | InstanceLifecycle::Stopping
    );
    if live && status.exit_unrecorded {
        InstanceLifecycle::Unknown
    } else {
        stored
    }
}

fn view_of(
    core: &DaemonCore,
    instance: &Instance,
    status: &InstanceStatus,
    runtimes: &[Runtime],
) -> InstanceView {
    // A missing runtime row leaves the line unknown; the instance still
    // gets its view, so a plan never drops its service.
    let runtime = runtimes
        .iter()
        .find(|runtime| runtime.id() == instance.runtime_id());
    let runtime_line = runtime.map(|runtime| runtime.line().clone());
    let runtime_artifact_sha256 = runtime.and_then(|runtime| match runtime.install() {
        stackly_domain::InstallMode::Managed { artifact_sha256 } => Some(*artifact_sha256),
        stackly_domain::InstallMode::External { .. } => None,
    });
    let detail = status
        .readiness
        .detail()
        .map(|detail| detail.as_str().to_owned())
        .or_else(|| instance.last_failure().map(|code| code.as_str().to_owned()))
        .map(|detail| core.redactor().redact(&detail).into_owned());
    InstanceView {
        id: instance.id(),
        name: instance.name().clone(),
        provider: instance.provider_revision().provider().clone(),
        runtime_version: instance.data_version().clone(),
        runtime_line,
        lifecycle: shown_lifecycle(instance.lifecycle(), status),
        initialized: instance.data_initialized_at().is_some(),
        readiness: status.readiness.state(),
        health: status.health.state(),
        blocker: status
            .blocker
            .as_ref()
            .map(|blocker| blocker.as_code().to_owned()),
        detail,
        endpoints: instance
            .endpoints()
            .iter()
            .filter_map(|endpoint| match endpoint {
                stackly_domain::Endpoint::Ip(ip) => {
                    Some((ip.name().clone(), ip.address(), ip.effective_port()))
                }
                _ => None,
            })
            .collect(),
        runtime_artifact_sha256,
    }
}

async fn instances(
    core: &DaemonCore,
    registry: &SqliteRegistry,
    supervisor: &Supervisor,
) -> Result<Instances, Box<ErrorBody>> {
    let read = registry.clone();
    let (instances, runtimes) = blocking(move || {
        let instances = read
            .instances(InstanceFilter {
                include_retained: true,
            })?
            .value;
        let runtimes = read.runtimes()?.value;
        Ok::<_, stackly_domain::RegistryError>((instances, runtimes))
    })
    .await?
    .map_err(|error| registry_error(&error))?;
    let mut views = Vec::with_capacity(instances.len());
    let mut summaries = Vec::with_capacity(instances.len());
    for instance in &instances {
        let status = supervisor.status(instance.id());
        views.push(view_of(core, instance, &status, &runtimes));
        summaries.push(crate::ipc::instance::summary_of(core, supervisor, instance));
    }
    Ok(Instances { views, summaries })
}

async fn environments(registry: &SqliteRegistry) -> Result<Vec<Environment>, Box<ErrorBody>> {
    let read = registry.clone();
    blocking(move || read.environments().map(|environments| environments.value))
        .await?
        .map_err(|error| registry_error(&error))
}

/// `env.validate` (AC-4): the file against the catalog in memory. An
/// invalid manifest is a successful reply with `valid` false; nothing is
/// written, and neither the registry nor the Keychain is read.
pub fn validate(
    core: &DaemonCore,
    params: EnvValidateParams,
) -> HandlerFuture<'_, Box<EnvValidation>> {
    Box::pin(async move {
        let catalog = crate::catalog::loaded(core)?;
        let path = PathBuf::from(&params.path);
        let platform = core.platform().clone();
        let bytes = blocking(move || read_manifest(platform.fs(), &path))
            .await?
            .map_err(unreadable)?;
        let validation = blocking(move || validate_bytes(&bytes, &catalog)).await?;
        Ok(Box::new(dto::validation(&params.path, &validation)))
    })
}

/// The file a plan reads: the path of a `file` source, or the location of
/// a registered environment.
pub(crate) async fn plan_source(
    registry: &SqliteRegistry,
    source: &EnvSource,
) -> Result<(String, Option<EnvironmentId>), Box<ErrorBody>> {
    match source {
        EnvSource::File(path) => Ok((path.clone(), None)),
        EnvSource::Registered(id) => {
            let id = EnvironmentId::from_uuid(*id).map_err(|_| not_found())?;
            let found = environments(registry)
                .await?
                .into_iter()
                .find(|env| env.id() == id)
                .ok_or_else(not_found)?;
            let location = found
                .manifest_location()
                .ok_or_else(|| unreadable(UnreadableReason::NotFound))?;
            Ok((location.as_str().to_owned(), Some(id)))
        }
    }
}

/// `env.plan` (AC-5 to AC-10): validate, then what Up would do on this
/// Mac. It starts no operation and writes nothing (AC-6).
pub fn plan(core: &DaemonCore, params: EnvPlanParams) -> HandlerFuture<'_, Box<EnvPlan>> {
    Box::pin(async move {
        let registry = registry_of(core)?;
        let (file, registered) = plan_source(&registry, &params.source).await?;
        let planned = plan_with(core, &file, registered, params.locked)
            .await
            .map_err(PlanFailure::into_body)?;
        let home = core.home_mask().clone();
        Ok(Box::new(dto::plan(
            dto::validation(&file, &planned.validation),
            &planned.plan,
            &home,
        )))
    })
}

/// A manifest read and planned, with the instance views the plan saw.
pub(crate) struct Planned {
    /// Where the manifest lives.
    pub location: stackly_domain::ManifestLocation,
    /// Its validation.
    pub validation: stackly_core::environment::Validation,
    /// The plan.
    pub plan: stackly_core::environment::Plan,
}

/// Why [`plan_with`] read no plan.
pub(crate) enum PlanFailure {
    /// The manifest can't be read.
    Unreadable(UnreadableReason),
    /// Anything else, as its error body.
    Body(Box<ErrorBody>),
}

impl PlanFailure {
    /// The error body a reply sends.
    pub(crate) fn into_body(self) -> Box<ErrorBody> {
        match self {
            Self::Unreadable(reason) => unreadable(reason),
            Self::Body(body) => body,
        }
    }
}

impl From<Box<ErrorBody>> for PlanFailure {
    fn from(body: Box<ErrorBody>) -> Self {
        Self::Body(body)
    }
}

/// Reads the manifest at `file` and plans it as `env.plan` does, with the
/// lock and the local overrides next to it (spec 0045) and `locked`: the
/// one plan `env.plan`, the `env.up` precheck, and the `accept` step of
/// Up all read (spec 0046 AC-1).
pub(crate) async fn plan_with(
    core: &DaemonCore,
    file: &str,
    registered: Option<EnvironmentId>,
    locked: bool,
) -> Result<Planned, PlanFailure> {
    let supervisor = core.supervision().supervisor()?;
    let registry = registry_of(core)?;
    let state = crate::ipc::instance::catalog_state(core)?;
    let catalog = crate::catalog::loaded(core)?;
    let read = {
        let platform = core.platform().clone();
        let path = PathBuf::from(file);
        blocking(move || {
            let bytes = read_manifest(platform.fs(), &path)?;
            let location = manifest_location(platform.fs(), &path)?;
            Ok::<_, UnreadableReason>(Read { bytes, location })
        })
        .await?
        .map_err(PlanFailure::Unreadable)?
    };
    let Instances { views, .. } = instances(core, &registry, &supervisor).await?;
    let platform = core.platform().clone();
    let prepared = core.prepared().clone();
    let source = core.catalog_source().id();
    let probe = crate::ipc::instance::serialized_probe();
    let planned = blocking(move || {
        let validation = validate_bytes(&read.bytes, &catalog);
        let (lock, local) = side_files(platform.fs(), &read.location);
        let host = platform.info().host_facts();
        let deps = PlanDeps {
            create: CreateDeps {
                registry: &registry,
                catalog: &state,
                source,
                platform: &platform,
                prepared: &prepared,
                probe: &*probe,
            },
            catalog: &catalog,
            host: &host,
            instances: &views,
        };
        let planned = plan_of(
            &deps,
            &PlanInputs {
                manifest: &validation,
                location: &read.location,
                registered,
                lock: Some(&lock),
                overrides: Some(&local),
                locked,
            },
        )?;
        Ok::<_, stackly_core::environment::PlanError>(Planned {
            location: read.location,
            validation,
            plan: planned,
        })
    })
    .await?
    .map_err(|error| {
        tracing::error!(%error, "cannot read the facts of an environment plan");
        unexpected("the facts of the plan could not be read")
    })?;
    Ok(planned)
}

/// The lock and the local override file next to the manifest at
/// `location`, read through the `Filesystem` port.
fn side_files(
    fs: &dyn stackly_platform::Filesystem,
    location: &stackly_domain::ManifestLocation,
) -> (LockInput, LocalInput) {
    let file = manifest_file_name(location);
    let folder = location
        .as_path()
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf);
    let lock = lock_file_name(&file);
    let local = local_file_name(&file);
    (
        LockInput {
            content: LockFile::read(fs, &folder.join(&lock)),
            file: lock,
        },
        LocalInput {
            content: LocalFile::read(fs, &folder.join(&local)),
            file: local,
        },
    )
}

/// When the row of `environment` last changed: the registry's
/// `updated_at` (spec 0044 AC-14), so an import that changes nothing never
/// moves it. An environment that was never written has no stamp; `now`
/// stands in, which no read from the registry reaches.
fn updated_at(core: &DaemonCore, environment: &Environment) -> Timestamp {
    environment.updated_at().map_or_else(
        || Timestamp::from_jiff(core.platform().clock().now()),
        Timestamp::from_jiff,
    )
}

/// `env.list` (AC-14): at most 64 summaries from the registry and the
/// supervisor, never from a file.
pub fn list(core: &DaemonCore) -> HandlerFuture<'_, EnvironmentList> {
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let mut environments = environments(&registry).await?;
        environments.sort_by(|a, b| a.name().cmp(b.name()));
        environments.truncate(stackly_protocol::env::ENV_LIST_MAX);
        let Instances { views, .. } = instances(core, &registry, &supervisor).await?;
        let engine = core.operations().writer().ok();
        // One engine read for the whole list (spec 0046 AC-16).
        let active = status::active_operations(engine.as_ref(), None).await;
        let items = environments
            .iter()
            .map(|environment| {
                let mut summary = dto::summary(
                    environment,
                    &environment_view(environment, &views),
                    updated_at(core, environment),
                );
                summary.active_operation = status::up_now(core, environment, &active).active;
                summary
            })
            .collect();
        Ok(EnvironmentList { items })
    })
}

/// The active leases of `environment` (spec 0047 AC-12): their count and
/// the oldest `acquired_at`, read from the registry.
async fn lease_info(
    registry: &SqliteRegistry,
    environment: EnvironmentId,
) -> Result<EnvLeaseInfo, Box<ErrorBody>> {
    let read = registry.clone();
    let leases = blocking(move || {
        read.leases_of_environment(environment, stackly_domain::LeaseFilter::Unreleased)
    })
    .await?
    .map_err(|error| registry_error(&error))?
    .value;
    let active: Vec<_> = leases.iter().filter(|lease| lease.is_active()).collect();
    Ok(EnvLeaseInfo {
        active_count: Count::saturating(active.len() as u64),
        held_since: active
            .iter()
            .map(|lease| lease.acquired_at())
            .min()
            .map(Timestamp::from_jiff),
    })
}

/// The detail of the environment `id`, reading its file only when
/// `check_manifest` is true.
async fn detail_of(
    core: &DaemonCore,
    id: EnvironmentId,
    check_manifest: bool,
) -> Result<Box<EnvironmentDetail>, Box<ErrorBody>> {
    let supervisor = core.supervision().supervisor()?;
    let registry = registry_of(core)?;
    let environment = environments(&registry)
        .await?
        .into_iter()
        .find(|env| env.id() == id)
        .ok_or_else(not_found)?;
    let Instances { views, summaries } = instances(core, &registry, &supervisor).await?;
    let view = environment_view(&environment, &views);
    let updated = updated_at(core, &environment);
    let (manifest_state, problems, side) = if check_manifest {
        let (state, problems) = check_file(core, &environment).await?;
        let side = match state {
            stackly_core::environment::ManifestState::InSync
            | stackly_core::environment::ManifestState::Changed => {
                side_info(core, &environment).await?
            }
            _ => None,
        };
        (
            Some(map::catalog::wire(state.as_code())),
            Some(Count::saturating(problems as u64)),
            side,
        )
    } else {
        (None, None, None)
    };
    let (lock, local) = side.unzip();
    let engine = core.operations().writer().ok();
    let active = status::active_operations(
        engine.as_ref(),
        Some(stackly_domain::OperationResource::Environment(id)),
    )
    .await;
    let mut up = status::up_now(core, &environment, &active);
    if let Some(report) = up.last_up.as_mut() {
        status::refine_references(core, &environment, report).await;
    }
    let mut summary = dto::summary(&environment, &view, updated);
    summary.active_operation = up.active.clone();
    let services = view
        .services
        .iter()
        .map(|service| {
            let mut wire = dto::service(service, &summaries);
            if let Some(base) = wire.not_ready.take() {
                let instance = service
                    .instance
                    .as_ref()
                    .map(|instance| (instance.id, instance.lifecycle));
                let blocker = instance.and_then(|(id, _)| supervisor.status(id).blocker);
                wire.not_ready = Some(status::not_ready(
                    base,
                    service.alias.as_str(),
                    instance,
                    blocker.as_ref(),
                    up.last_up.as_ref(),
                ));
            }
            wire
        })
        .collect();
    Ok(Box::new(EnvironmentDetail {
        summary,
        services,
        detached: dto::detached_members(&view),
        manifest_state,
        problems,
        lock,
        local,
        last_up: up.last_up,
        lease: Some(lease_info(&registry, id).await?),
    }))
}

/// The lock and the local override file of a registered environment
/// whose manifest is `in_sync` or `changed` (spec 0045 AC-13): no
/// evaluation of create, and an override's state only by whether its
/// service has a member.
async fn side_info(
    core: &DaemonCore,
    environment: &Environment,
) -> Result<Option<(EnvLockInfo, EnvLocalInfo)>, Box<ErrorBody>> {
    let Some(location) = environment.manifest_location().cloned() else {
        return Ok(None);
    };
    let catalog: Option<Arc<Catalog>> = match core.catalog().as_deref() {
        Some(CatalogState::Loaded(catalog)) => Some(Arc::clone(catalog)),
        _ => None,
    };
    let platform = core.platform().clone();
    let home = core.home_mask().clone();
    let members: Vec<stackly_domain::MemberAlias> = environment
        .members()
        .iter()
        .map(|member| member.alias().clone())
        .collect();
    blocking(move || {
        let fs = platform.fs();
        let manifest = read_manifest(fs, location.as_path())
            .ok()
            .zip(catalog.as_deref())
            .and_then(|(bytes, catalog)| validate_manifest(&bytes, catalog).manifest);
        let (lock, local) = side_files(fs, &location);
        let state = match (&manifest, &lock.content) {
            (Some(manifest), content) => lock_state(manifest, content).0,
            (None, LockFile::Absent) => LockState::Absent,
            (None, LockFile::Unreadable(_)) => LockState::Unreadable,
            (None, LockFile::Invalid(_)) => LockState::Invalid,
            (None, LockFile::Valid { .. }) => LockState::InSync,
        };
        let locked = lock.content.lock();
        let lock_info = EnvLockInfo {
            file: lock.file.clone(),
            state: map::catalog::wire(state.as_code()),
            targets: locked.map(|lock| lock.targets()).unwrap_or_default(),
            services: Count::saturating(locked.map_or(0, |lock| lock.services().len()) as u64),
        };
        let mut local_info = EnvLocalInfo {
            file: local.file.clone(),
            state: None,
            overrides: Vec::new(),
            problems: Count::saturating(0),
        };
        match &local.content {
            LocalFile::Absent => {}
            LocalFile::Unreadable(_) => {
                local_info.state = Some(map::catalog::wire(LocalState::Unreadable.as_code()));
            }
            LocalFile::Invalid { findings, .. } => {
                local_info.state = Some(map::catalog::wire(LocalState::Invalid.as_code()));
                local_info.problems = Count::saturating(
                    findings.count_of(stackly_core::environment::Severity::Error) as u64,
                );
            }
            LocalFile::Loaded { document, .. } => {
                let checked = match (&manifest, catalog.as_deref()) {
                    (Some(manifest), Some(catalog)) => stackly_core::environment::local::validate(
                        document,
                        manifest,
                        catalog,
                        &|alias| members.contains(alias),
                    ),
                    _ => stackly_core::environment::local::LocalOverrides::default(),
                };
                let errors = checked
                    .findings
                    .count_of(stackly_core::environment::Severity::Error);
                let state = if errors > 0 {
                    LocalState::Invalid
                } else {
                    LocalState::Present
                };
                local_info.state = Some(map::catalog::wire(state.as_code()));
                local_info.problems = Count::saturating(errors as u64);
                local_info.overrides = checked
                    .overrides
                    .iter()
                    .map(|found| {
                        let has_member = found
                            .alias
                            .as_ref()
                            .is_some_and(|alias| members.contains(alias));
                        dto::plan_override(
                            &stackly_core::environment::PlanOverride {
                                service: found.service.clone(),
                                field: found.field(),
                                value: found.value_text(),
                                state: checked.state_of(found, has_member),
                            },
                            &home,
                        )
                    })
                    .collect();
            }
        }
        Some((lock_info, local_info))
    })
    .await
}

/// *State manifest*: reads the file of `environment` and compares it.
async fn check_file(
    core: &DaemonCore,
    environment: &Environment,
) -> Result<(stackly_core::environment::ManifestState, usize), Box<ErrorBody>> {
    let catalog: Option<Arc<Catalog>> = match core.catalog().as_deref() {
        Some(CatalogState::Loaded(catalog)) => Some(Arc::clone(catalog)),
        _ => None,
    };
    let platform = core.platform().clone();
    let environment = environment.clone();
    blocking(move || {
        let read = match environment.manifest_location() {
            None => ManifestRead::Missing,
            Some(location) => match read_manifest(platform.fs(), location.as_path()) {
                Ok(bytes) => ManifestRead::Bytes(bytes),
                Err(UnreadableReason::NotFound) => ManifestRead::Missing,
                Err(_) => ManifestRead::Unreadable,
            },
        };
        manifest_state(&environment, &read, catalog.as_deref())
    })
    .await
}

/// `env.get` (AC-14).
pub fn get(core: &DaemonCore, params: EnvGetParams) -> HandlerFuture<'_, Box<EnvironmentDetail>> {
    Box::pin(async move {
        let id = EnvironmentId::from_uuid(params.id).map_err(|_| not_found())?;
        detail_of(core, id, params.check_manifest).await
    })
}

fn input_of(value: &serde_json::Value) -> JournalPayload {
    JournalPayload::new(value.to_string(), OperationJournal::INPUT_MAX)
        .unwrap_or_else(|_| JournalPayload::empty())
}

/// The environment ID the journal input of a replayed operation names.
fn journal_environment(input: &JournalPayload) -> Option<(EnvironmentId, Option<String>)> {
    let input = serde_json::from_str::<serde_json::Value>(input.as_str()).ok()?;
    let id = input
        .get("environmentId")?
        .as_str()?
        .parse::<Uuid>()
        .ok()
        .and_then(|id| EnvironmentId::from_uuid(id).ok())?;
    let name = input
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Some((id, name))
}

/// `env.import` (AC-12): the `import_environment` operation, which reads
/// and validates the file again under the locks and registers it through
/// `commit_guarded`. It installs, creates, starts, leases, and trusts
/// nothing.
pub fn import<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ImportEnvironmentParams,
) -> HandlerFuture<'a, Box<EnvironmentDetail>> {
    let context = context.clone();
    Box::pin(async move {
        import_inner(core, &context, params)
            .await
            .map_err(|error| Box::new(with_env_hint(*error)))
    })
}

async fn import_inner(
    core: &DaemonCore,
    context: &RequestContext,
    params: ImportEnvironmentParams,
) -> Result<Box<EnvironmentDetail>, Box<ErrorBody>> {
    let fingerprint = request_fingerprint(&Call::EnvImport(params.clone()));
    let key = context.idempotency_key.as_deref();
    // A repeated key answers the environment as it is now, before any
    // read, so a file that changed since does not change the answer.
    if let Some(replay) = replay_operation(
        core,
        context,
        OperationStart::new("env.import", key, fingerprint),
    )
    .await?
    {
        if let Some(failure) = replay.failure {
            return Err(Box::new(failure));
        }
        wait_final(core, replay.accepted.operation.id).await?;
        let (id, _) = journal_environment(&replay.input)
            .ok_or_else(|| unexpected("the journal of the import names no environment"))?;
        return detail_of(core, id, false).await;
    }
    let catalog = crate::catalog::loaded(core)?;
    let registry = registry_of(core)?;
    let path = PathBuf::from(&params.path);
    let read = read_file(core, &path).await?;
    let check = Arc::clone(&catalog);
    let bytes = read.bytes;
    let validation = blocking(move || validate_bytes(&bytes, &check)).await?;
    if validation.sha256.to_string() != params.expected_manifest_sha256 {
        return Err(Box::new(ErrorBody::new(
            &env_codes::MANIFEST_CHANGED,
            "the manifest changed since it was planned",
        )));
    }
    let errors = validation
        .findings
        .count_of(stackly_core::environment::Severity::Error);
    let (Some(name), 0) = (validation.name.clone(), errors) else {
        return Err(Box::new(
            ErrorBody::new(&env_codes::MANIFEST_INVALID, "the manifest has errors")
                .with_detail("errors", errors.to_string()),
        ));
    };
    let relink = match params.relink_environment_id {
        Some(id) => Some(EnvironmentId::from_uuid(id).map_err(|_| not_found())?),
        None => None,
    };
    // The resource: the environment the import would update, or a new ID.
    let registered = environments(&registry).await?;
    let existing = match relink {
        Some(id) => Some(
            registered
                .iter()
                .find(|env| env.id() == id)
                .map(Environment::id)
                .ok_or_else(not_found)?,
        ),
        None => registered
            .iter()
            .find(|env| env.name() == &name)
            .map(Environment::id),
    };
    let id = existing.unwrap_or_else(EnvironmentId::generate);
    let home = core.home_mask().clone();
    let input = input_of(&serde_json::json!({
        "environmentId": id.to_string(),
        "name": name.as_str(),
        "location": home.mask(read.location.as_str()),
        "manifestSha256": validation.sha256.to_string(),
    }));
    let work = import_work(ImportRequest {
        id,
        path,
        location: read.location,
        expected: validation.sha256,
        relink,
        catalog,
        home,
    });
    let request = move |origin: Origin| BeginRequest {
        kind: OperationKind::ImportEnvironment,
        resource: OperationResource::Environment(id),
        creates_resource: existing.is_none(),
        extra_locks: vec![LockKey::EnvironmentName(name)],
        busy: BusyPolicy::Queue,
        expected_revision: None,
        origin,
        input,
        work,
        precheck: None,
    };
    let outcome = start_operation(
        core,
        context,
        OperationStart::new("env.import", key, fingerprint),
        request,
    )
    .await?;
    let finished = match outcome.operation_id() {
        Some(operation) => wait_final(core, operation).await,
        None => Ok(()),
    };
    crate::membership::reload(core).await;
    finished?;
    detail_of(core, id, false).await
}

/// `env.remove` (AC-13): the `remove_environment` operation. Instances,
/// data, runtimes, Keychain values, and the manifest file stay; a
/// dedicated instance becomes manual.
pub fn remove<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: RemoveEnvironmentParams,
) -> HandlerFuture<'a, RemovedEnvironment> {
    let context = context.clone();
    Box::pin(async move {
        remove_inner(core, &context, params)
            .await
            .map_err(|error| Box::new(with_env_hint(*error)))
    })
}

async fn remove_inner(
    core: &DaemonCore,
    context: &RequestContext,
    params: RemoveEnvironmentParams,
) -> Result<RemovedEnvironment, Box<ErrorBody>> {
    let fingerprint = request_fingerprint(&Call::EnvRemove(params.clone()));
    let key = context.idempotency_key.as_deref();
    if let Some(replay) = replay_operation(
        core,
        context,
        OperationStart::new("env.remove", key, fingerprint),
    )
    .await?
    {
        if let Some(failure) = replay.failure {
            return Err(Box::new(failure));
        }
        wait_final(core, replay.accepted.operation.id).await?;
        let (id, name) = journal_environment(&replay.input)
            .ok_or_else(|| unexpected("the journal of the remove names no environment"))?;
        return Ok(RemovedEnvironment {
            id: id.as_uuid(),
            name: name.unwrap_or_default(),
        });
    }
    let registry = registry_of(core)?;
    let id = EnvironmentId::from_uuid(params.id).map_err(|_| not_found())?;
    let environment = environments(&registry)
        .await?
        .into_iter()
        .find(|env| env.id() == id)
        .ok_or_else(not_found)?;
    let expected = ResourceRevision::new(params.expected_revision.get())
        .map_err(|_| unexpected("a revision on the wire is at least 1"))?;
    let name = environment.name().clone();
    let input = input_of(&serde_json::json!({
        "environmentId": id.to_string(),
        "name": name.as_str(),
    }));
    let work = remove_work(id, expected, name.clone());
    let lock = LockKey::EnvironmentName(name.clone());
    let request = move |origin: Origin| BeginRequest {
        kind: OperationKind::RemoveEnvironment,
        resource: OperationResource::Environment(id),
        creates_resource: false,
        extra_locks: vec![lock],
        busy: BusyPolicy::Queue,
        expected_revision: None,
        origin,
        input,
        work,
        precheck: None,
    };
    let outcome = start_operation(
        core,
        context,
        OperationStart::new("env.remove", key, fingerprint),
        request,
    )
    .await?;
    let finished = match outcome.operation_id() {
        Some(operation) => wait_final(core, operation).await,
        None => Ok(()),
    };
    crate::membership::reload(core).await;
    finished?;
    Ok(RemovedEnvironment {
        id: id.as_uuid(),
        name: name.to_string(),
    })
}

fn lock_error(
    code: &stackly_protocol::codes::ErrorSpec,
    message: &str,
    detail: (&str, &str),
) -> Box<ErrorBody> {
    Box::new(with_env_hint(
        ErrorBody::new(code, message).with_detail(detail.0, detail.1),
    ))
}

/// `env.lock` (AC-2, AC-3): every dedicated service picked or kept from
/// the catalog in memory, then the lock written next to the manifest when
/// its bytes changed. No operation, no audit, no registry write, no
/// download, no process, and no Keychain; the one file it writes is the
/// lock, through `write_file_atomic`.
pub fn lock(core: &DaemonCore, params: EnvLockParams) -> HandlerFuture<'_, Box<EnvLockResult>> {
    Box::pin(async move {
        let registry = registry_of(core)?;
        let catalog = crate::catalog::loaded(core)?;
        let (file, _) = plan_source(&registry, &params.source).await?;
        let read = read_file(core, Path::new(&file)).await?;
        let refresh = match &params.refresh {
            stackly_protocol::env::LockRefresh::None => LockRefresh::None,
            stackly_protocol::env::LockRefresh::All => LockRefresh::All,
            stackly_protocol::env::LockRefresh::Services(aliases) => LockRefresh::Services(
                aliases
                    .iter()
                    .map(|alias| {
                        alias.parse().map_err(|_| {
                            lock_error(
                                &env_codes::SERVICE_NOT_FOUND,
                                "the manifest has no dedicated service of that alias",
                                ("service", "?"),
                            )
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
        };
        let platform = core.platform().clone();
        let home = core.home_mask().clone();
        blocking(move || {
            lock_blocking(
                &platform, &registry, &catalog, &file, &read, &refresh, &home,
            )
        })
        .await?
    })
}

#[allow(clippy::too_many_lines)]
fn lock_blocking(
    platform: &stackly_platform::Platform,
    registry: &SqliteRegistry,
    catalog: &Catalog,
    file: &str,
    read: &Read,
    refresh: &LockRefresh,
    home: &stackly_core::diagnostics::HomeMask,
) -> Result<Box<EnvLockResult>, Box<ErrorBody>> {
    let fs = platform.fs();
    let validation = validate_bytes(&read.bytes, catalog);
    let wire_validation = dto::validation(file, &validation);
    let folder = read
        .location
        .as_path()
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf);
    let path = folder.join(lock_file_name(&manifest_file_name(&read.location)));
    let lock_file = path.to_string_lossy().into_owned();
    let mut reply = EnvLockResult {
        validation: wire_validation,
        lock_file,
        written: false,
        changed: false,
        sha256: None,
        targets: Vec::new(),
        services: Vec::new(),
        blockers: Vec::new(),
    };
    let Some(manifest) = &validation.manifest else {
        return Ok(Box::new(reply));
    };
    let unwritable = |reason: &str| {
        lock_error(
            &env_codes::LOCK_UNWRITABLE,
            "the lock file can't be written",
            ("reason", reason),
        )
    };
    match fs.symlink_metadata(&path) {
        Ok(Some(info)) if info.kind != stackly_platform::FileKind::File => {
            return Err(unwritable("not_a_file"));
        }
        _ => {}
    }
    let existing = LockFile::read(fs, &path);
    let host = platform.info().host_facts();
    let resolution = resolve_entries(manifest, &existing, refresh, catalog, registry, &host)
        .map_err(|error| match error {
            stackly_core::environment::lock::LockError::ServiceNotFound(alias) => lock_error(
                &env_codes::SERVICE_NOT_FOUND,
                "the manifest has no dedicated service of that alias",
                ("service", alias.as_str()),
            ),
            other => {
                tracing::error!(error = %other, "cannot read the trust of a provider for env.lock");
                unexpected("the trust of a provider could not be read")
            }
        })?;
    reply.services = resolution
        .services
        .iter()
        .map(|service| LockedService {
            alias: service.alias.to_string(),
            provider: service.provider.to_string(),
            selector: service.selector.clone(),
            version: service.version.as_ref().map(ToString::to_string),
            artifact_id: service.artifact_id.clone(),
            change: map::catalog::wire(service.change.as_code()),
            previous_version: service.previous_version.as_ref().map(ToString::to_string),
        })
        .collect();
    let Some(lock) = resolution.lock else {
        reply.blockers = resolution
            .blockers
            .iter()
            .map(|blocker| stackly_protocol::env::PlanBlocker {
                service: blocker.service.as_ref().map(ToString::to_string),
                error: dto::blocker_error(blocker, home),
            })
            .collect();
        return Ok(Box::new(reply));
    };
    let bytes = emit(&lock);
    let before = fs
        .read_file_limited(&path, stackly_core::environment::lock::LOCK_MAX_LEN)
        .ok();
    reply.changed = before.as_deref() != Some(bytes.as_slice());
    if reply.changed {
        fs.write_file_atomic(&path, &bytes).map_err(|error| {
            unwritable(match error {
                stackly_platform::PlatformError::PermissionDenied { .. } => "permission_denied",
                _ => "io",
            })
        })?;
        reply.written = true;
    }
    reply.sha256 = Some(stackly_core::environment::manifest_digest(&bytes).to_string());
    reply.targets = lock.targets();
    Ok(Box::new(reply))
}

/// `env.export` (AC-14, AC-15): the host and port of every endpoint of
/// every service with an instance, read from the registry and the pinned
/// packages only. No operation, no audit, no file, no Keychain, and no
/// process; every value goes through the `Redactor`.
pub fn export(core: &DaemonCore, params: EnvExportParams) -> HandlerFuture<'_, Box<EnvExport>> {
    Box::pin(async move {
        let registry = registry_of(core)?;
        let format = match params.format {
            stackly_protocol::env::ExportFormat::Shell => {
                stackly_core::environment::export::ExportFormat::Shell
            }
            _ => stackly_core::environment::export::ExportFormat::Dotenv,
        };
        let registered = environments(&registry).await?;
        let environment = match &params.source {
            EnvSource::Registered(id) => {
                let id = EnvironmentId::from_uuid(*id).map_err(|_| not_found())?;
                registered
                    .into_iter()
                    .find(|env| env.id() == id)
                    .ok_or_else(not_found)?
            }
            EnvSource::File(path) => {
                let read = read_file(core, Path::new(path)).await?;
                let name = stackly_core::environment::declared_name(&read.bytes)
                    .map_err(|findings| {
                        Box::new(with_env_hint(
                            ErrorBody::new(&env_codes::MANIFEST_INVALID, "the manifest has errors")
                                .with_detail("errors", findings.count().to_string()),
                        ))
                    })?
                    .ok_or_else(|| {
                        Box::new(with_env_hint(
                            ErrorBody::new(&env_codes::MANIFEST_INVALID, "the manifest has errors")
                                .with_detail("errors", "1"),
                        ))
                    })?;
                registered
                    .into_iter()
                    .find(|env| {
                        env.name() == &name && env.manifest_location() == Some(&read.location)
                    })
                    .ok_or_else(|| {
                        lock_error(
                            &env_codes::NOT_REGISTERED,
                            "the environment of this file is not registered at this location",
                            ("name", name.as_str()),
                        )
                    })?
            }
        };
        let platform = core.platform().clone();
        let layout = core.prepared().layout().clone();
        let packages = core.pinned_packages().clone();
        let read = registry.clone();
        let target = environment.clone();
        let services = blocking(move || {
            let instances = read
                .instances(InstanceFilter {
                    include_retained: true,
                })?
                .value;
            Ok::<_, stackly_domain::RegistryError>(export_services(
                &target, &instances, &platform, &layout, &packages,
            ))
        })
        .await?
        .map_err(|error| registry_error(&error))?;
        let collected = stackly_core::environment::export::collect(&services, core.redactor())
            .map_err(|error| {
                tracing::error!(%error, "an export value was refused");
                unexpected("an export value holds an unexpected character")
            })?;
        let text = stackly_core::environment::export::render(&collected.variables, format);
        Ok(Box::new(EnvExport {
            environment: stackly_protocol::env::EnvironmentRef {
                id: environment.id().as_uuid(),
                name: environment.name().to_string(),
                revision: crate::ipc::operation::dto::revision(environment.revision()),
            },
            format: map::catalog::wire(format.as_code()),
            text,
            variables: collected
                .variables
                .iter()
                .map(|variable| stackly_protocol::env::ExportVariable {
                    name: variable.name.clone(),
                    value: variable.value.clone(),
                    service: variable.service.to_string(),
                    endpoint: variable.endpoint.to_string(),
                    kind: map::catalog::wire(variable.kind.as_code()),
                    may_change: variable.may_change,
                })
                .collect(),
            skipped: collected
                .skipped
                .iter()
                .map(|skipped| stackly_protocol::env::ExportSkipped {
                    service: skipped.service.to_string(),
                    endpoint: skipped.endpoint.as_ref().map(ToString::to_string),
                    reason: map::catalog::wire(skipped.reason.as_code()),
                })
                .collect(),
        }))
    })
}

/// The services of `environment` in snapshot order, each with its member
/// or, for a shared service without one, the instance its `instanceRef`
/// names, and its endpoints in the order of its pinned package.
fn export_services(
    environment: &Environment,
    instances: &[Instance],
    platform: &stackly_platform::Platform,
    layout: &stackly_platform::DirectoryLayout,
    packages: &crate::detail::PinnedPackages,
) -> Vec<stackly_core::environment::export::ExportService> {
    use stackly_core::environment::export::{ExportEndpoint, ExportService, ExportSource};
    environment
        .declared()
        .iter()
        .map(|service| {
            let instance = environment
                .member(service.alias())
                .and_then(|member| instances.iter().find(|i| i.id() == member.instance_id()))
                .or_else(|| {
                    service.instance_ref().and_then(|name| {
                        instances.iter().find(|i| {
                            i.name() == name && i.lifecycle() != InstanceLifecycle::Retained
                        })
                    })
                });
            let source = match instance {
                None => ExportSource::NotCreated,
                Some(instance) if instance.lifecycle() == InstanceLifecycle::Retained => {
                    ExportSource::Retained
                }
                Some(instance) => {
                    match packages.get(platform, layout, instance.provider_revision()) {
                        None => ExportSource::PackageUnavailable,
                        Some(package) => ExportSource::Instance {
                            running: instance.lifecycle() == InstanceLifecycle::Running,
                            endpoints: package
                                .endpoints()
                                .filter_map(|spec| {
                                    let own = instance
                                        .endpoints()
                                        .iter()
                                        .find(|endpoint| endpoint.name() == spec.name())?;
                                    Some(match own {
                                        stackly_domain::Endpoint::Ip(ip) => ExportEndpoint::Ip {
                                            name: ip.name().clone(),
                                            host: ip.address(),
                                            port: ip.effective_port(),
                                            auto: ip.request() == stackly_domain::PortRequest::Auto,
                                        },
                                        _ => ExportEndpoint::Unix {
                                            name: spec.name().clone(),
                                        },
                                    })
                                })
                                .collect(),
                        },
                    }
                }
            };
            ExportService {
                alias: service.alias().clone(),
                source,
            }
        })
        .collect()
}
