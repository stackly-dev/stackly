//! The `create_instance` operation (spec 0020, *Operasi `create_instance`*):
//! `validate`, `pin`, `secrets`, `layout`, `register`, and `init`, all
//! reversible, so a failure, a cancel, or a crash at any step is
//! compensated and nothing but the pinned package is left.

use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use stackly_domain::{
    CommitGuard, ConfigRevisionId, ConfigRevisionNumber, ConfigRevisionRecord, ConfigValuesJson,
    EndpointRequests, GuardDecision, GuardView, Guarded, InstallMode, Instance, InstanceId,
    InstanceManifest, JournalPayload, ManifestRuntime, NewInstance, NewInstanceManifest, Operation,
    OperationId, OperationResource, ProviderSourceId, ReasonCode, RegistryError, ResourceRef,
    ResourceRevision, SecretFieldName, SecretOwner, SecretRef, SecretUser, StageName,
    StartupPolicy, WriteOp,
};
use stackly_platform::{InstancePaths, Platform, SecretValue, WriteMode, check_private_dir};

use super::binding::{BindingFault, EnvironmentBinding};
use super::check::CreateError;
use super::compensator::registration_key;
use super::draft::{CreateDraft, ProviderChoice, SecretSourceKind};
use super::error::PinFailure;
use super::evaluate::{ResolvedCreate, SecretPlan, evaluate_for};
use super::facts::{CreateDeps, PackageSource, gather};
use super::port;
use crate::catalog::{CatalogEntry, CatalogState, PinError};
use crate::instance::create::compensator::INSTANCE_REGISTRATION;
use crate::operation::{
    AuditNote, BoxFuture, OperationContext, OperationError, OperationOutcome, OperationWork,
    RebuildError, StepSpec, WorkError, WorkFactory, WorkOutcome, WorkResult,
};
use crate::secret::{
    FieldContext, PutRequest, PutTarget, SecretCaller, SecretError, SecretInput, SecretService,
    classify_create_failure, value_shape,
};
use crate::supervisor::{InitRequest, PortProbe, Supervisor, SupervisorError, SupervisorEvents};
use crate::{
    CoreError, PreparedLayout, check_data_root_overlap, check_owned_path, create_instance_layout,
    daemon_info,
};

/// A callback a test hangs on a point of the `layout` and `init` steps.
pub type Hook = Arc<dyn Fn(&InstancePaths) + Send + Sync>;

/// Test hooks around the content check of the data folder (AC-8, D9): they
/// are never set outside tests.
#[derive(Clone, Default)]
pub struct CreateHooks {
    /// Runs after `create_instance_layout` and before the data folder is
    /// checked for content: a test plants a file here.
    pub after_layout: Option<Hook>,
    /// Runs after the content check and before `init`: a test makes the
    /// init fail here.
    pub before_init: Option<Hook>,
}

impl std::fmt::Debug for CreateHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateHooks")
            .field("after_layout", &self.after_layout.is_some())
            .field("before_init", &self.before_init.is_some())
            .finish()
    }
}

/// Makes sure the row of the loaded catalog exists in `provider_sources`,
/// right before the first pin (K-2): `stacklyd` holds the ID it recorded at
/// start and records it again here when that failed.
pub trait SourceRecorder: Send + Sync {
    /// Returns the ID of the row, writing it first when it is not there.
    ///
    /// # Errors
    ///
    /// Returns the registry error when the row cannot be written.
    fn ensure(&self) -> Result<ProviderSourceId, RegistryError>;
}

/// What the work needs besides the context of the engine.
pub struct CreateWorkDeps {
    /// The catalog the daemon loaded.
    pub catalog: CatalogState,
    /// The trial binds.
    pub probe: Arc<dyn PortProbe>,
    /// The secret service.
    pub secrets: Arc<SecretService>,
    /// The supervisor, for `init`.
    pub supervisor: Supervisor,
    /// What the clients see change.
    pub events: Arc<dyn SupervisorEvents>,
    /// The source row of the catalog.
    pub sources: Arc<dyn SourceRecorder>,
    /// The test hooks.
    pub hooks: CreateHooks,
}

/// The work of one `create_instance` operation.
pub struct CreateInstanceWork {
    id: InstanceId,
    draft: CreateDraft,
    values: Vec<(String, SecretValue)>,
    caller: SecretCaller,
    binding: Option<EnvironmentBinding>,
    deps: Arc<CreateWorkDeps>,
}

impl CreateInstanceWork {
    /// The work that creates the instance `id` from `draft`. `values` hold
    /// the value of each `value` secret; their shape is read here, so the
    /// draft only carries the two facts the check needs.
    #[must_use]
    pub fn new(
        id: InstanceId,
        mut draft: CreateDraft,
        values: Vec<(String, SecretValue)>,
        caller: SecretCaller,
        deps: Arc<CreateWorkDeps>,
    ) -> Self {
        draft.secret_values = values
            .iter()
            .map(|(field, value)| (field.clone(), value_shape(value)))
            .collect();
        Self {
            id,
            draft,
            values,
            caller,
            binding: None,
            deps,
        }
    }

    /// Returns the work with the environment its `register` batch binds
    /// the new instance to (spec 0046 AC-7). Only an Up starts such a
    /// create; a client's `instance.create` never does.
    #[must_use]
    pub fn with_binding(mut self, binding: EnvironmentBinding) -> Self {
        self.binding = Some(binding);
        self
    }
}

impl OperationWork for CreateInstanceWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_create(*self, ctx).await })
    }
}

fn failed(error: impl OperationError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

fn internal(message: &str) -> WorkError {
    failed(SupervisorError::internal(message))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, WorkError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| internal("a create task stopped"))
}

/// Begins a step and shows its stage.
async fn enter(
    ctx: &mut OperationContext,
    stage: StageName,
    repeatable: bool,
) -> Result<(), WorkError> {
    let spec = StepSpec::new(stage.clone());
    ctx.begin_step(if repeatable { spec.repeatable() } else { spec })
        .await?;
    ctx.set_stage(stage).await
}

async fn run_create(work: CreateInstanceWork, mut ctx: OperationContext) -> WorkResult {
    let CreateInstanceWork {
        id,
        draft,
        values,
        caller,
        binding,
        deps,
    } = work;
    let caller = caller.in_operation(ctx.id());
    // validate: the same checks as the precheck, now under the lock.
    enter(&mut ctx, StageName::VALIDATE, true).await?;
    let resolved = validate(&ctx, &deps, &draft, id).await?;
    // Spec 0035: the note is set as soon as the draft passed, so a failure
    // after it carries the provider and runtime; revision 1 is added once
    // `register` wrote it.
    let note = audit_note(&ctx, &draft).await;
    ctx.set_audit_note(note.clone());

    // pin: copy the catalog package, unless an instance already pinned it.
    enter(&mut ctx, StageName::PIN, true).await?;
    let pinned = pin(&ctx, &deps, &resolved).await?;

    // secrets: each reference is recorded before its value is stored.
    enter(&mut ctx, StageName::SECRETS, false).await?;
    let bound = secrets(&mut ctx, &deps, &caller, id, &resolved, values).await?;

    // layout: the folders, claimed before the first write.
    enter(&mut ctx, StageName::LAYOUT, false).await?;
    let (now, paths) = layout(&mut ctx, &deps, id, &resolved).await?;

    // register: re-check and write in one registry transaction.
    enter(&mut ctx, StageName::REGISTER, false).await?;
    let registration = Registration {
        pinned,
        bound,
        binding,
        now,
    };
    let instance = register(&mut ctx, &deps, id, &resolved, registration).await?;
    deps.events.instance_changed(&instance);
    ctx.set_audit_note(AuditNote {
        config_revision: Some(ConfigRevisionNumber::FIRST),
        ..note
    });

    // init: the provider's init plan, under the instance lock.
    enter(&mut ctx, StageName::INIT, false).await?;
    if let Some(hook) = deps.hooks.before_init.clone() {
        let paths = paths.clone();
        blocking(move || hook(&paths)).await?;
    }
    deps.supervisor
        .execute_init(InitRequest::new(id, caller))
        .await
        .map_err(failed)?;

    Ok(WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Instance(id)],
            partial: Vec::new(),
        },
        finalize: None,
    })
}

/// The audit note of a create (spec 0035, *Catatan audit pekerjaan*): the
/// provider by name, the runtime, and, when the draft names the instance it
/// duplicates and that instance still exists, its ID and its name now
/// (AC-18). An instance that is gone, or a read that fails, leaves both out.
async fn audit_note(ctx: &OperationContext, draft: &CreateDraft) -> AuditNote {
    let mut note = AuditNote::new(None)
        .with("provider", &draft.provider.name)
        .with("runtimeId", draft.runtime_id);
    if let Some(source) = draft.duplicate_of {
        let registry = Arc::clone(ctx.registry());
        let found = blocking(move || registry.instance(source))
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(|read| read.value);
        if let Some(instance) = found {
            note = note
                .with("duplicateOf", source)
                .with("duplicateOfName", instance.name());
        }
    }
    note
}

async fn validate(
    ctx: &OperationContext,
    deps: &Arc<CreateWorkDeps>,
    draft: &CreateDraft,
    id: InstanceId,
) -> Result<ResolvedCreate, WorkError> {
    let registry = Arc::clone(ctx.registry());
    let platform = Arc::clone(ctx.platform());
    let layout = Arc::clone(ctx.layout());
    let deps = Arc::clone(deps);
    let draft = draft.clone();
    let evaluation = blocking(move || {
        let create = CreateDeps {
            registry: &*registry,
            catalog: &deps.catalog,
            source: None,
            platform: &platform,
            prepared: &layout,
            probe: &*deps.probe,
        };
        gather(&create, &draft).map(|facts| evaluate_for(&draft, &facts, id))
    })
    .await?
    .map_err(|error| failed(SupervisorError::internal(error.to_string())))?;
    if let Some(error) = evaluation.first_error() {
        return Err(failed(error.clone()));
    }
    evaluation
        .resolved
        .ok_or_else(|| internal("a draft without an error did not resolve"))
}

/// What `pin` produced: the record to commit, when the registry lacks the
/// revision.
async fn pin(
    ctx: &OperationContext,
    deps: &Arc<CreateWorkDeps>,
    resolved: &ResolvedCreate,
) -> Result<Option<stackly_domain::ProviderRevisionRecord>, WorkError> {
    if resolved.source == PackageSource::Pinned {
        return Ok(None);
    }
    let local = match resolved.source {
        PackageSource::LocalImport {
            source_id,
            approved_at,
        } => Some((source_id, approved_at)),
        _ => None,
    };
    let key = resolved.package.key().clone();
    let failure = |error: PinError| {
        failed(PinFailure {
            provider: key.provider().to_string(),
            revision: key.revision().to_string(),
            error,
        })
    };
    let entry = match &deps.catalog {
        CatalogState::Loaded(catalog) => match catalog.entry(key.provider()) {
            Some(CatalogEntry::Provider(entry)) => Some(entry.clone()),
            _ => None,
        },
        _ => None,
    }
    .ok_or_else(|| {
        failure(PinError::Io {
            op: "find the catalog entry",
            path: PathBuf::new(),
            message: "the catalog no longer holds the provider".to_owned(),
        })
    })?;
    // A local entry pins with its own source row and approval; the
    // catalog's source recorder is not used for it (spec 0037).
    let (source, meta) = match local {
        Some((source_id, approved_at)) => (
            source_id,
            crate::catalog::PinMeta::local(key.provider().namespace(), approved_at),
        ),
        None => {
            let sources = Arc::clone(&deps.sources);
            let source = blocking(move || sources.ensure())
                .await?
                .map_err(|_| failure(PinError::SourceUnrecorded))?;
            (source, crate::catalog::PinMeta::catalog())
        }
    };
    let platform = Arc::clone(ctx.platform());
    let layout = Arc::clone(ctx.layout());
    let pinned = blocking(move || {
        crate::catalog::pin_revision_with(&platform, &layout, &entry, source, meta)
    })
    .await?
    .map_err(failure)?;
    let registry = Arc::clone(ctx.registry());
    let known = blocking(move || registry.provider_revision(&key))
        .await?
        .map_err(|error| failed(SupervisorError::Registry(error)))?
        .value
        .is_some();
    Ok((!known).then_some(pinned.record))
}

/// The `secrets` step: stores or binds every secret of the draft. Returns
/// each bound reference with the revision its value was checked at, which
/// the `register` step checks again (spec 0011, AC-23).
async fn secrets(
    ctx: &mut OperationContext,
    deps: &Arc<CreateWorkDeps>,
    caller: &SecretCaller,
    id: InstanceId,
    resolved: &ResolvedCreate,
    values: Vec<(String, SecretValue)>,
) -> Result<Vec<(SecretRef, ResourceRevision)>, WorkError> {
    let mut values = values;
    let mut bound = Vec::new();
    for plan in &resolved.secrets {
        let is_bind = matches!(plan.source, SecretSourceKind::Bind(_));
        let handle = if is_bind {
            None
        } else {
            Some(ctx.record_secret_intent(plan.reference.clone()).await?)
        };
        let input = secret_input(plan, &mut values)?;
        let target = PutTarget::New {
            reference: plan.reference.clone(),
            owner: SecretOwner::Instance(id),
            purpose: plan.purpose.clone(),
            field: Some(FieldContext::new(plan.field.clone(), plan.rule)),
        };
        let request = PutRequest::new(target, input, WriteMode::Create);
        let service = Arc::clone(&deps.secrets);
        let who = caller.clone();
        let put = blocking(move || service.put(request, &who)).await?;
        let outcome = match put {
            Ok(outcome) => outcome,
            Err(error) => {
                // A `bind` writes nothing, so a failure on its reference is
                // always one on the source: the reference this create would have
                // written stands as the target (AC-7).
                let target = if is_bind {
                    SecretRef::for_instance(id, &plan.purpose)
                } else {
                    plan.reference.clone()
                };
                return Err(failed(SupervisorError::Secret(classify_create_failure(
                    error, &target,
                ))));
            }
        };
        if is_bind {
            let record = &outcome.reference.record;
            bound.push((record.reference.clone(), record.revision));
        }
        if let Some(handle) = handle {
            ctx.confirm_created(handle).await?;
        }
    }
    Ok(bound)
}

fn secret_input(
    plan: &SecretPlan,
    values: &mut Vec<(String, SecretValue)>,
) -> Result<SecretInput, WorkError> {
    let reference = |text: &str| {
        SecretRef::parse(text).map_err(|_| internal("a source reference no longer parses"))
    };
    Ok(match &plan.source {
        SecretSourceKind::Generate => SecretInput::Generate,
        SecretSourceKind::Value => {
            let at = values
                .iter()
                .position(|(field, _)| field.as_str() == plan.field.as_str())
                .ok_or_else(|| internal("a value secret has no value"))?;
            SecretInput::Value(values.remove(at).1)
        }
        SecretSourceKind::CopyFrom(text) => SecretInput::CopyFrom(reference(text)?),
        SecretSourceKind::Bind(text) => SecretInput::Bind(reference(text)?),
    })
}

/// The manifest the instance folder starts with.
fn manifest_of(
    resolved: &ResolvedCreate,
    id: InstanceId,
    operation: OperationId,
    now: jiff::Timestamp,
) -> InstanceManifest {
    let runtime = &resolved.runtime;
    let manifest_runtime = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => ManifestRuntime::managed(
            runtime.id(),
            runtime.version().clone(),
            runtime.line().clone(),
            runtime.target(),
            *artifact_sha256,
        ),
        InstallMode::External { .. } => ManifestRuntime::external(
            runtime.id(),
            runtime.version().clone(),
            runtime.line().clone(),
            runtime.target(),
        ),
    };
    InstanceManifest::new(NewInstanceManifest {
        instance_id: id,
        name: resolved.name.clone(),
        display_name: resolved.display_name.clone(),
        software: runtime.software().clone(),
        provider: resolved.package.key().clone(),
        runtime: manifest_runtime,
        created_at: now,
        created_by_operation: operation,
        stackly_version: daemon_info().version.to_owned(),
    })
}

fn relative(layout: &PreparedLayout, path: &std::path::Path) -> String {
    path.strip_prefix(layout.layout().roots().data())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// The `layout` step: the folders of the instance, which are new, do not
/// overlap another data folder, and hold nothing (BR-002, BR-008).
async fn layout(
    ctx: &mut OperationContext,
    deps: &Arc<CreateWorkDeps>,
    id: InstanceId,
    resolved: &ResolvedCreate,
) -> Result<(jiff::Timestamp, InstancePaths), WorkError> {
    let now = ctx.platform().clock().now();
    let handle = ctx.claim_resource().await?;
    // From here on the paths of the instance resolve in the locations it is
    // created in, also when the default changes before the commit.
    let (data, logs) = resolved.locations;
    ctx.layout().locations().note_instance(id, data, logs);
    let manifest = manifest_of(resolved, id, ctx.id(), now);
    let platform = Arc::clone(ctx.platform());
    let prepared = Arc::clone(ctx.layout());
    let registry = Arc::clone(ctx.registry());
    let hook = deps.hooks.after_layout.clone();
    let outcome = blocking(
        move || -> Result<Result<InstancePaths, LayoutFault>, WorkError> {
            let known = registry
                .known_instance_ids()
                .map(|known| known.value)
                .map_err(|error| failed(SupervisorError::Registry(error)))?;
            let planned = prepared.instance_paths(id);
            check_owned_path(&platform, &prepared, &planned.root)
                .map_err(|error| failed(unavailable(&prepared, &planned.root, &error)))?;
            let paths = match create_instance_layout(&platform, &prepared, &manifest, &known) {
                Ok(paths) => paths,
                Err(CoreError::AlreadyExists { .. }) => {
                    return Ok(Err(LayoutFault::NotEmpty {
                        why: "already_exists",
                        path: relative(&prepared, &planned.data),
                    }));
                }
                Err(CoreError::ShortKeyTaken { key }) => {
                    return Ok(Err(LayoutFault::ShortKeyTaken(key.to_string())));
                }
                Err(error) => {
                    return Err(failed(unavailable(&prepared, &planned.root, &error)));
                }
            };
            if let Some(fault) = overlap(&platform, &prepared, &known, id, &paths) {
                return Ok(Err(fault));
            }
            if let Some(hook) = &hook {
                hook(&paths);
            }
            if let Some(fault) = check_data(&platform, &prepared, &paths) {
                return Ok(Err(fault));
            }
            Ok(Ok(paths))
        },
    )
    .await??;
    match outcome {
        Ok(paths) => {
            ctx.confirm_created(handle).await?;
            Ok((now, paths))
        }
        Err(LayoutFault::NotEmpty { why, path }) => {
            // The item is declined, not removed: what is in the folder is
            // not provably this operation's (BR-008, BR-023).
            ctx.not_created(handle, ReasonCode::from_static(why))
                .await?;
            Err(failed(CreateFault::DataNotEmpty { path }))
        }
        Err(LayoutFault::ShortKeyTaken(key)) => {
            ctx.not_created(handle, ReasonCode::from_static("short_key_taken"))
                .await?;
            Err(failed(CreateFault::ShortKeyTaken { key }))
        }
        Err(LayoutFault::Overlap { path, other }) => {
            Err(failed(CreateFault::DataOverlap { path, other }))
        }
    }
}

fn unavailable(
    prepared: &PreparedLayout,
    path: &std::path::Path,
    error: &CoreError,
) -> CreateError {
    CreateError::DataUnavailable {
        path: relative(prepared, path),
        reason: error.to_string(),
    }
}

/// Why the `layout` step stopped before the folder was confirmed.
enum LayoutFault {
    NotEmpty { why: &'static str, path: String },
    ShortKeyTaken(String),
    Overlap { path: String, other: InstanceId },
}

/// The data folder of an instance that is known must not hold, or sit
/// inside, the new one (BR-002).
fn overlap(
    platform: &Platform,
    prepared: &PreparedLayout,
    known: &[InstanceId],
    id: InstanceId,
    paths: &InstancePaths,
) -> Option<LayoutFault> {
    for other in known.iter().copied().filter(|other| *other != id) {
        for root in [
            prepared.instance_paths(other).data,
            prepared.retained_instance_paths(other).data,
        ] {
            if platform
                .fs()
                .symlink_metadata(&root)
                .ok()
                .flatten()
                .is_none()
            {
                continue;
            }
            if check_data_root_overlap(platform, &paths.data, [&root]).is_err() {
                return Some(LayoutFault::Overlap {
                    path: relative(prepared, &paths.data),
                    other,
                });
            }
        }
    }
    None
}

/// The data folder must be a private folder of the user with nothing in it.
fn check_data(
    platform: &Platform,
    prepared: &PreparedLayout,
    paths: &InstancePaths,
) -> Option<LayoutFault> {
    let not_empty = || LayoutFault::NotEmpty {
        why: "data_not_empty",
        path: relative(prepared, &paths.data),
    };
    let fs = platform.fs();
    let Ok(Some(info)) = fs.symlink_metadata(&paths.data) else {
        return Some(not_empty());
    };
    let owned = platform
        .identity()
        .current_user()
        .is_ok_and(|user| check_private_dir(&info, &user).is_ok());
    if !owned {
        return Some(not_empty());
    }
    match fs.read_dir(&paths.data) {
        Ok(entries) if entries.is_empty() => None,
        _ => Some(not_empty()),
    }
}

/// Re-checks every rule that comes from the registry and writes the
/// instance, its first config revision, and its secret uses in one registry
/// transaction (spec 0020, "Commit terjaga di langkah `register`", AC-10):
/// no other commit lands between the check and the write, so the step never
/// meets a generation conflict and never retries.
/// What the `register` step writes besides the resolved create.
struct Registration {
    /// The provider revision a `pin` copied, if any.
    pinned: Option<stackly_domain::ProviderRevisionRecord>,
    /// The bound references with the revision `secrets` checked.
    bound: Vec<(SecretRef, ResourceRevision)>,
    /// The environment Up binds the instance to, if any.
    binding: Option<EnvironmentBinding>,
    /// The commit time of the layout.
    now: jiff::Timestamp,
}

async fn register(
    ctx: &mut OperationContext,
    deps: &Arc<CreateWorkDeps>,
    id: InstanceId,
    resolved: &ResolvedCreate,
    registration: Registration,
) -> Result<Instance, WorkError> {
    let Registration {
        pinned,
        bound,
        binding,
        now,
    } = registration;
    let handle = ctx
        .record_custom_intent(INSTANCE_REGISTRATION, registration_key(id), None)
        .await?;
    // The startup policy is the default setting unless the draft names one,
    // and the setting is read inside the guarded commit, so the instance is
    // built there (spec 0032, AC-18).
    let operation = ctx.id();
    let build = {
        let resolved = resolved.clone();
        move |startup| {
            Instance::new(NewInstance {
                id,
                name: resolved.name.clone(),
                display_name: resolved.display_name.clone(),
                provider_revision: resolved.package.key().clone(),
                software: resolved.runtime.software().clone(),
                runtime_id: resolved.runtime.id(),
                data_line: resolved.runtime.line().clone(),
                data_version: resolved.runtime.version().clone(),
                endpoints: resolved.endpoints.clone(),
                startup,
                created_at: now,
                created_by: operation,
            })
        }
    };
    let instance = build(resolved.startup.unwrap_or(StartupPolicy::Manual))
        .map_err(|error| internal(&error.to_string()))?;
    let values = ConfigValuesJson::new(resolved.settings.to_json().to_string())
        .map_err(|error| internal(&error.to_string()))?;
    let config = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        id,
        values,
        EndpointRequests::of(&instance),
        Some(ctx.id()),
        now,
    );
    let uses = resolved
        .secrets
        .iter()
        .map(|plan| (plan.field.clone(), plan.reference.clone()))
        .collect::<std::collections::BTreeMap<SecretFieldName, SecretRef>>();

    let created: Arc<std::sync::Mutex<Option<Instance>>> = Arc::default();
    let guard: CommitGuard = {
        let resolved = resolved.clone();
        let created = Arc::clone(&created);
        Box::new(move |view| {
            if let Some(error) = recheck_trust(view, &resolved)? {
                return Ok(GuardDecision::Refuse(Box::new(error)));
            }
            if let Some(error) = recheck_bound(view, &bound)? {
                return Ok(GuardDecision::Refuse(Box::new(error)));
            }
            let bind_ops = match &binding {
                Some(binding) => match binding.guard_ops(view, id, now)? {
                    Ok(ops) => ops,
                    Err(fault) => return Ok(GuardDecision::Refuse(Box::new(fault))),
                },
                None => Vec::new(),
            };
            Ok(match recheck(view, &resolved, id, pinned) {
                Err(error) => GuardDecision::Refuse(Box::new(error)),
                Ok(record) => {
                    let startup = match resolved.startup {
                        Some(startup) => startup,
                        None => view.daemon_settings()?.default_startup,
                    };
                    let Ok(instance) = build(startup) else {
                        return Ok(GuardDecision::Refuse(Box::new(
                            CreateError::SettingsInvalid,
                        )));
                    };
                    *created
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(instance.clone());
                    let (data, logs) = resolved.locations;
                    let mut ops = Vec::new();
                    ops.extend(record.map(WriteOp::InsertProviderRevision));
                    ops.push(WriteOp::InsertInstanceAt {
                        instance,
                        data,
                        logs,
                    });
                    ops.push(WriteOp::InsertConfigRevision(config));
                    ops.push(WriteOp::ReplaceSecretUses {
                        user: SecretUser::Instance(id),
                        uses,
                    });
                    // The member and its lease follow the instance they
                    // refer to, in the same transaction (spec 0046 AC-7).
                    ops.extend(bind_ops);
                    GuardDecision::Write(ops)
                }
            })
        })
    };
    match ctx.commit_guarded(guard).await {
        Ok(Guarded::Committed(_)) => {}
        Ok(Guarded::Refused(reason)) => {
            let reason = match reason.downcast::<CreateError>() {
                Ok(error) => return Err(failed(*error)),
                Err(reason) => reason,
            };
            let reason = match reason.downcast::<BindingFault>() {
                Ok(fault) => return Err(failed(*fault)),
                Err(reason) => reason,
            };
            return Err(match reason.downcast::<SecretError>() {
                Ok(error) => failed(SupervisorError::Secret(*error)),
                Err(_) => internal("the register guard refused with an unknown reason"),
            });
        }
        Err(WorkError::Engine(crate::operation::EngineError::Registry(error))) => {
            return Err(failed(SupervisorError::Registry(error)));
        }
        Err(other) => return Err(other),
    }
    let instance = created
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .unwrap_or(instance);
    deps.supervisor.note_created(&instance);
    ctx.confirm_created(handle).await?;
    Ok(instance)
}

/// Checks the trust of the package again inside the guarded commit (spec
/// 0037, *Gate trust*): a local entry's import row must still approve
/// exactly its bytes, and a pinned revision must not be revoked, so a
/// revoke that committed after the gate still stops the create.
fn recheck_trust(
    view: &dyn stackly_domain::GuardView,
    resolved: &ResolvedCreate,
) -> Result<Option<CreateError>, stackly_domain::RegistryError> {
    let key = resolved.package.key();
    let (verdict, package_sha256) = match resolved.source {
        PackageSource::LocalImport { .. } => {
            let digest = crate::catalog::local::package_sha256(resolved.package.files());
            (
                crate::catalog::guard_trust(view, key.provider(), &digest)?,
                digest.to_string(),
            )
        }
        PackageSource::Pinned => (
            crate::catalog::guard_pinned_trust(view, None, key)?,
            String::new(),
        ),
        PackageSource::BundledCatalog | PackageSource::DevelopmentCatalog => return Ok(None),
    };
    Ok(match verdict {
        crate::catalog::TrustVerdict::ApprovalRequired => Some(CreateError::ApprovalRequired {
            provider: key.provider().to_string(),
            provider_revision: key.revision().to_string(),
            package_sha256,
        }),
        crate::catalog::TrustVerdict::Revoked(info) => Some(CreateError::Revoked {
            provider: key.provider().to_string(),
            provider_revision: key.revision().to_string(),
            info,
        }),
        _ => None,
    })
}

/// Reads every bound reference through the guarded commit's own
/// transaction: its value was checked against the new field's rules in the
/// `secrets` step at the revision recorded there, but its use is written
/// only here and without the reference lock. A reference that is gone, or
/// whose revision moved since, so a `secret.set` stored a value the check
/// never saw, refuses the commit (spec 0011, AC-23).
fn recheck_bound(
    view: &dyn GuardView,
    bound: &[(SecretRef, ResourceRevision)],
) -> Result<Option<SecretError>, RegistryError> {
    for (reference, checked) in bound {
        let Some(stored) = view.secret_reference(reference)? else {
            return Ok(Some(SecretError::NotFound {
                reference: reference.clone(),
            }));
        };
        if stored.record.revision != *checked {
            return Ok(Some(SecretError::RevisionMismatch {
                reference: reference.clone(),
                expected: *checked,
                actual: stored.record.revision,
            }));
        }
    }
    Ok(None)
}

/// Reads the registry through the guarded commit's own transaction and
/// checks every rule that comes from it again: the name, `multi_instance`,
/// the runtime, and the fixed ports against the other instances (spec 0020,
/// AC-10). Returns the provider revision to insert when the registry still
/// lacks it.
fn recheck(
    view: &dyn GuardView,
    resolved: &ResolvedCreate,
    id: InstanceId,
    pinned: Option<stackly_domain::ProviderRevisionRecord>,
) -> Result<Option<stackly_domain::ProviderRevisionRecord>, CreateError> {
    let read = |error: RegistryError| CreateError::DataUnavailable {
        path: ".".to_owned(),
        reason: error.to_string(),
    };
    let instances = view
        .instances(stackly_domain::InstanceFilter {
            include_retained: false,
        })
        .map_err(read)?;
    let others: Vec<Instance> = instances
        .into_iter()
        .filter(|other| other.id() != id)
        .collect();
    if let Some(existing) = others.iter().find(|other| *other.name() == resolved.name) {
        return Err(CreateError::NameTaken {
            existing: existing.id(),
        });
    }
    let provider = resolved.package.key().provider();
    if !resolved
        .package
        .manifest()
        .capabilities()
        .contains(stackly_domain::Capability::MultiInstance)
        && let Some(existing) = others
            .iter()
            .find(|other| other.provider_revision().provider() == provider)
    {
        return Err(CreateError::CapabilityUnsupported {
            provider: provider.to_string(),
            existing: existing.id(),
        });
    }
    match view.runtime(resolved.runtime.id()).map_err(read)? {
        Some(runtime)
            if runtime.state() == stackly_domain::RuntimeState::Installed
                && runtime.software() == resolved.runtime.software() => {}
        Some(runtime) => {
            return Err(CreateError::RuntimeUnavailable {
                runtime_id: runtime.id(),
                state: runtime.state().as_code(),
            });
        }
        None => {
            return Err(CreateError::RuntimeNotFound {
                runtime_id: resolved.runtime.id(),
            });
        }
    }
    for endpoint in &resolved.endpoints {
        let stackly_domain::Endpoint::Ip(ip) = endpoint else {
            continue;
        };
        let stackly_domain::PortRequest::Fixed(port) = ip.request() else {
            continue;
        };
        let slot = port::Slot::of(ip);
        if let Some(other) = port::conflicts_with_instances(&others, &slot, port) {
            return Err(CreateError::EndpointConflict {
                endpoint: ip.name().to_string(),
                port,
                other,
            });
        }
    }
    let record = match pinned {
        Some(record) => view
            .provider_revision(record.revision.key())
            .map_err(read)?
            .is_none()
            .then_some(record),
        None => None,
    };
    Ok(record)
}

/// The errors the `layout` step reports that are not a [`CreateError`] row.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum CreateFault {
    #[error("the new instance folder is not empty")]
    DataNotEmpty { path: String },
    #[error("the new instance ID collided with another instance")]
    ShortKeyTaken { key: String },
    #[error("the new data folder overlaps another instance")]
    DataOverlap { path: String, other: InstanceId },
}

impl OperationError for CreateFault {
    fn code(&self) -> stackly_domain::ErrorCodeName {
        stackly_domain::ErrorCodeName::from_static(match self {
            Self::DataNotEmpty { .. } => "instance.data_not_empty",
            Self::ShortKeyTaken { .. } => "instance.short_key_taken",
            Self::DataOverlap { .. } => "instance.data_overlap",
        })
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(stackly_domain::DetailKey, String)> {
        let key = |text: &str| {
            text.parse::<stackly_domain::DetailKey>()
                .expect("the detail keys of spec 0020 are camelCase")
        };
        match self {
            Self::DataNotEmpty { path } => vec![(key("path"), path.clone())],
            Self::ShortKeyTaken { key: short } => vec![(key("shortKey"), short.clone())],
            Self::DataOverlap { path, other } => vec![
                (key("path"), path.clone()),
                (key("otherInstance"), other.to_string()),
            ],
        }
    }

    fn rule(&self) -> Option<stackly_domain::Rule> {
        match self {
            Self::DataNotEmpty { .. } => Some(stackly_domain::Rule::Br008),
            Self::DataOverlap { .. } => Some(stackly_domain::Rule::Br002),
            Self::ShortKeyTaken { .. } => None,
        }
    }
}

// The input of the journal.

/// The input of a `create_instance` journal: the draft without any secret
/// value, in the JSON the factory reads back.
///
/// A `value` secret is recorded as `value` only. When the result passes the
/// journal limit, the settings are replaced by `{"settingsOmitted": true,
/// "settingsSha256": "<hex>"}`, and the operation cannot be rebuilt.
///
/// # Errors
///
/// Returns the payload error when even the shortened input is too large.
pub fn encode_input(
    id: InstanceId,
    draft: &CreateDraft,
) -> Result<JournalPayload, stackly_domain::ValueError> {
    encode_bound_input(id, draft, None)
}

/// [`encode_input`] for a create that Up binds to an environment (spec
/// 0046 AC-7): the same JSON with a `binding` object, which
/// [`CreateWorkFactory`] reads back, so a create rebuilt after a restart
/// keeps its binding. Without a binding the JSON is exactly that of
/// [`encode_input`].
///
/// # Errors
///
/// Returns the payload error when even the shortened input is too large.
pub fn encode_bound_input(
    id: InstanceId,
    draft: &CreateDraft,
    binding: Option<&EnvironmentBinding>,
) -> Result<JournalPayload, stackly_domain::ValueError> {
    let endpoints: Vec<serde_json::Value> = draft
        .endpoints
        .iter()
        .map(|(name, request)| {
            let port = match request {
                super::draft::PortRequestDraft::Auto => serde_json::json!({ "type": "auto" }),
                super::draft::PortRequestDraft::Fixed(port) => {
                    serde_json::json!({ "type": "fixed", "content": port })
                }
            };
            serde_json::json!({ "name": name, "port": port })
        })
        .collect();
    let secrets: Vec<serde_json::Value> = draft
        .secrets
        .iter()
        .map(|(field, source)| {
            let source = match source {
                SecretSourceKind::Generate => serde_json::json!({ "type": "generate" }),
                SecretSourceKind::Value => serde_json::json!({ "type": "value" }),
                SecretSourceKind::CopyFrom(text) => {
                    serde_json::json!({ "type": "copyFrom", "content": text })
                }
                SecretSourceKind::Bind(text) => {
                    serde_json::json!({ "type": "bind", "content": text })
                }
            };
            serde_json::json!({ "field": field, "source": source })
        })
        .collect();
    let build = |settings: serde_json::Value| {
        let mut value = serde_json::json!({
            "instanceId": id.to_string(),
            "name": draft.name,
            "displayName": draft.display_name,
            "provider": { "name": draft.provider.name, "revision": draft.provider.revision },
            "runtimeId": draft.runtime_id.to_string(),
            "endpoints": endpoints,
            "settings": settings,
            "secrets": secrets,
            "start": draft.start,
            "startup": draft.startup.map(StartupPolicy::as_code),
        });
        if let (Some(binding), Some(object)) = (binding, value.as_object_mut()) {
            object.insert("binding".to_owned(), binding.to_json());
        }
        value.to_string()
    };
    let full = build(draft.settings.clone());
    JournalPayload::new(full, stackly_domain::OperationJournal::INPUT_MAX).or_else(|_| {
        let digest = Sha256::digest(draft.settings.to_string().as_bytes());
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        JournalPayload::new(
            build(serde_json::json!({ "settingsOmitted": true, "settingsSha256": hex })),
            stackly_domain::OperationJournal::INPUT_MAX,
        )
    })
}

/// Builds the work of a queued `create_instance` again after a restart
/// (spec 0020, *Operasi `create_instance`*). An input with a `value` secret
/// or with omitted settings is not rebuildable: the operation closes
/// `failed interrupted`.
pub struct CreateWorkFactory {
    deps: Arc<CreateWorkDeps>,
}

impl CreateWorkFactory {
    /// A factory over the same dependencies as the handler's work.
    #[must_use]
    pub fn new(deps: Arc<CreateWorkDeps>) -> Self {
        Self { deps }
    }
}

impl WorkFactory for CreateWorkFactory {
    fn rebuild(
        &self,
        operation: &Operation,
        journal: &stackly_domain::JournalView,
    ) -> Result<Box<dyn OperationWork>, RebuildError> {
        let OperationResource::Instance(id) = operation.resource() else {
            return Err(RebuildError::InvalidInput);
        };
        let (draft, binding) = decode_input(journal.journal.input().as_str())?;
        let work = CreateInstanceWork::new(
            id,
            draft,
            Vec::new(),
            SecretCaller::background(),
            Arc::clone(&self.deps),
        );
        Ok(Box::new(match binding {
            Some(binding) => work.with_binding(binding),
            None => work,
        }))
    }
}

/// The draft of a create journal's input and the environment it binds to,
/// if any.
fn decode_input(text: &str) -> Result<(CreateDraft, Option<EnvironmentBinding>), RebuildError> {
    let invalid = || RebuildError::InvalidInput;
    let not_rebuildable = |reason: &'static str| RebuildError::NotRebuildable {
        reason: ReasonCode::from_static(reason),
    };
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| invalid())?;
    let text_of = |value: &serde_json::Value| value.as_str().map(str::to_owned);
    let settings = value.get("settings").ok_or_else(invalid)?;
    if settings.get("settingsOmitted").is_some() {
        return Err(not_rebuildable("settings_omitted"));
    }
    let secrets = value
        .get("secrets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|row| {
            let field = text_of(row.get("field").ok_or_else(invalid)?).ok_or_else(invalid)?;
            let source = row.get("source").ok_or_else(invalid)?;
            let content = source.get("content").and_then(text_of);
            let kind = match source.get("type").and_then(serde_json::Value::as_str) {
                Some("generate") => SecretSourceKind::Generate,
                Some("value") => return Err(not_rebuildable("secret_value")),
                Some("copyFrom") => SecretSourceKind::CopyFrom(content.ok_or_else(invalid)?),
                Some("bind") => SecretSourceKind::Bind(content.ok_or_else(invalid)?),
                _ => return Err(invalid()),
            };
            Ok((field, kind))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let endpoints = value
        .get("endpoints")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|row| {
            let name = text_of(row.get("name").ok_or_else(invalid)?).ok_or_else(invalid)?;
            let port = row.get("port").ok_or_else(invalid)?;
            let request = match port.get("type").and_then(serde_json::Value::as_str) {
                Some("auto") => super::draft::PortRequestDraft::Auto,
                Some("fixed") => super::draft::PortRequestDraft::Fixed(
                    port.get("content")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|port| u16::try_from(port).ok())
                        .ok_or_else(invalid)?,
                ),
                _ => return Err(invalid()),
            };
            Ok((name, request))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let provider = value.get("provider").ok_or_else(invalid)?;
    let binding = match value.get("binding") {
        Some(binding) => Some(EnvironmentBinding::from_json(binding).ok_or_else(invalid)?),
        None => None,
    };
    let draft = CreateDraft {
        name: value.get("name").and_then(text_of).ok_or_else(invalid)?,
        display_name: value.get("displayName").and_then(text_of),
        provider: ProviderChoice {
            name: provider.get("name").and_then(text_of).ok_or_else(invalid)?,
            revision: provider.get("revision").and_then(text_of),
        },
        runtime_id: value
            .get("runtimeId")
            .and_then(text_of)
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?,
        endpoints,
        settings: settings.clone(),
        secrets,
        secret_values: Vec::new(),
        start: value
            .get("start")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        startup: value
            .get("startup")
            .and_then(serde_json::Value::as_str)
            .and_then(|code| code.parse().ok()),
        duplicate_of: None,
        data_location: None,
    };
    Ok((draft, binding))
}
