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

mod dto;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_core::catalog::{Catalog, CatalogState};
use stackly_core::environment::{
    ImportRequest, InstanceView, ManifestRead, PlanDeps, PlanInputs, UnreadableReason,
    environment_view, import_work, manifest_location, manifest_state, plan as plan_of,
    read_manifest, remove_work, validate as validate_bytes,
};
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
    EnvGetParams, EnvPlan, EnvPlanParams, EnvSource, EnvValidateParams, EnvValidation,
    EnvironmentDetail, EnvironmentList, ImportEnvironmentParams, RemoveEnvironmentParams,
    RemovedEnvironment,
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
const fn shown_lifecycle(stored: InstanceLifecycle, status: &InstanceStatus) -> InstanceLifecycle {
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
async fn plan_source(
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
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let state = crate::ipc::instance::catalog_state(core)?;
        let catalog = crate::catalog::loaded(core)?;
        let (file, registered) = plan_source(&registry, &params.source).await?;
        let read = read_file(core, Path::new(&file)).await?;
        let Instances { views, .. } = instances(core, &registry, &supervisor).await?;
        let platform = core.platform().clone();
        let prepared = core.prepared().clone();
        let source = core.catalog_source().id();
        let probe = crate::ipc::instance::serialized_probe();
        let home = core.home_mask().clone();
        let reply = blocking(move || {
            let validation = validate_bytes(&read.bytes, &catalog);
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
                    lock: None,
                    overrides: None,
                    locked: false,
                },
            )?;
            Ok::<_, stackly_core::environment::PlanError>(dto::plan(
                dto::validation(&file, &validation),
                &planned,
                &home,
            ))
        })
        .await?
        .map_err(|error| {
            tracing::error!(%error, "cannot read the facts of an environment plan");
            unexpected("the facts of the plan could not be read")
        })?;
        Ok(Box::new(reply))
    })
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
        let items = environments
            .iter()
            .map(|environment| {
                dto::summary(
                    environment,
                    &environment_view(environment, &views),
                    updated_at(core, environment),
                )
            })
            .collect();
        Ok(EnvironmentList { items })
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
    let (manifest_state, problems) = if check_manifest {
        let (state, problems) = check_file(core, &environment).await?;
        (
            Some(map::catalog::wire(state.as_code())),
            Some(Count::saturating(problems as u64)),
        )
    } else {
        (None, None)
    };
    Ok(Box::new(EnvironmentDetail {
        summary: dto::summary(&environment, &view, updated),
        services: view
            .services
            .iter()
            .map(|service| dto::service(service, &summaries))
            .collect(),
        detached: dto::detached_members(&view),
        manifest_state,
        problems,
    }))
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
