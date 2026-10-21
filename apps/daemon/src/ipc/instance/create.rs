//! The temporary `instance.create` (spec 0010, *Permukaan IPC*, create
//! sementara; `// provisional: F20 until F20 step 9`).
//!
//! The method validates what the registry and the pinned package decide
//! (name, provider, runtime, settings, endpoints), after the lookup of the
//! idempotency key, and then begins a `create_instance` operation with the
//! stages `validate`, `secrets`, `layout`, `register`, and `init`.
//! `validate`, `secrets`, and `layout` are reversible, so a failure there
//! is compensated by spec 0007; `register` is the first step that cannot be
//! undone, so a failure at `register` or `init` leaves the operation in
//! `needs_recovery` with its keys held until the user abandons it. The full
//! create of feature 20 replaces this handler without a contract change.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use jiff::Timestamp;
use stackly_core::operation::{
    BeginRequest, BoxFuture, BusyPolicy, FinalBatch, OperationContext, OperationError,
    OperationOutcome, OperationWork, Origin, StepSpec, WorkError, WorkOutcome, WorkResult,
};
use stackly_core::provider::{
    EndpointSpec, EndpointTransport, NameCheck, SettingsError, ValidatedPackage, read_package,
};
use stackly_core::secret::{
    FieldContext, PutRequest, PutTarget, SecretCaller, SecretFieldRule, SecretService,
    classify_create_failure,
};
use stackly_core::supervisor::{InitRequest, SupervisorError, SupervisorEvents};
use stackly_core::{PreparedLayout, create_instance_layout, daemon_info};
use stackly_domain::{
    ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, DetailKey, DisplayName, Endpoint,
    EndpointName, ErrorCodeName, InstallMode, Instance, InstanceId, InstanceManifest, InstanceName,
    IpEndpoint, JournalPayload, LockKey, ManifestRuntime, NewInstance, NewInstanceManifest,
    OperationKind, OperationResource, Port, PortRequest as DomainPortRequest, ProviderRevisionKey,
    Registry, ResourceRef, Runtime, RuntimeId, RuntimeState, SecretFieldName, SecretOwner,
    SecretPurpose, SecretRef, SecretUser, SocketName, StageName, StartupPolicy, Transport,
    UnixEndpoint, WriteOp, endpoints_conflict,
};
use stackly_platform::{FileKind, Platform, WriteMode};
use stackly_protocol::codes::{instance, request};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, InstanceActionAccepted, PortRequest as WirePort,
};
use stackly_protocol::json::FieldCode;
use stackly_protocol::message::Call;
use stackly_protocol::secret::SecretInput as WireInput;
use stackly_protocol::{ErrorBody, FieldError};
use stackly_storage::SqliteRegistry;

use super::{action_reply, registry_of};
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::{StartRequest as OperationStart, replay_operation, start_operation};
use crate::ipc::secret::{caller, create_input};
use crate::state::DaemonCore;
use crate::supervisor::BusSupervisorEvents;

/// The real `instance.create`.
pub fn create<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: CreateInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let secrets = core.secrets().service()?;
        let events = core.supervision().events();
        if params.provider.revision.is_none() {
            // provisional: F20 (an absent revision means the catalog's)
            return Err(Box::new(
                ErrorBody::new(
                    &request::INVALID_PARAMS,
                    "provider.revision is required until the full create exists",
                )
                .with_detail("method", "instance.create")
                .with_field(FieldError::new("provider.revision", FieldCode::Required)),
            ));
        }
        let fingerprint = request_fingerprint(&Call::InstanceCreate(params.clone()));
        let key = context.idempotency_key.as_deref();
        let start = OperationStart::new("instance.create", key, fingerprint);

        // A repeated request gets the reply it had, not a new refusal
        // (K12): the lookup of the key comes before any check of the
        // registry.
        if let Some(replay) = replay_operation(core, &context, start).await? {
            return Ok(InstanceActionAccepted {
                accepted: Some(replay.accepted),
                already_in_state: false,
            });
        }

        let platform = core.platform().clone();
        let prepared = core.prepared().clone();
        let plan = tokio::task::spawn_blocking({
            let registry = registry.clone();
            move || plan_create(&platform, &prepared, &registry, &params)
        })
        .await
        .map_err(|_| unexpected("the create validation task failed"))??;

        let id = plan.id;
        let name = plan.name.clone();
        let work = CreateWork {
            plan,
            supervisor: supervisor.clone(),
            secrets,
            events,
            caller: caller(&context),
        };
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::CreateInstance,
            resource: OperationResource::Instance(id),
            creates_resource: true,
            extra_locks: vec![LockKey::InstanceName(name)],
            busy: BusyPolicy::Reject,
            expected_revision: None,
            origin,
            // The params hold secret inputs, so nothing of them is stored
            // and a create is never rebuilt after a restart.
            input: JournalPayload::empty(),
            work: Box::new(work),
            precheck: None,
        };
        let outcome = start_operation(core, &context, start, request).await?;
        Ok(action_reply(outcome))
    })
}

/// One secret field of the new instance.
struct SecretPlan {
    field: SecretFieldName,
    reference: SecretRef,
    purpose: SecretPurpose,
    rule: SecretFieldRule,
    input: WireInput,
}

/// Everything the validation decided, for the operation to carry out.
struct Plan {
    id: InstanceId,
    name: InstanceName,
    display_name: Option<DisplayName>,
    provider: ProviderRevisionKey,
    runtime: Runtime,
    endpoints: Vec<Endpoint>,
    secrets: Vec<SecretPlan>,
    /// The settings with the reference text in each secret field.
    values: serde_json::Value,
}

/// Validates the request against the registry and the pinned package
/// (spec 0010, create sementara).
fn plan_create(
    platform: &Platform,
    prepared: &PreparedLayout,
    registry: &SqliteRegistry,
    params: &CreateInstanceParams,
) -> Result<Plan, Box<ErrorBody>> {
    let name: InstanceName = params.name.parse().map_err(|_| {
        invalid_field(
            "name",
            FieldCode::InvalidFormat,
            "the instance name is not valid",
        )
    })?;
    let display_name = params
        .display_name
        .as_deref()
        .map(str::parse::<DisplayName>)
        .transpose()
        .map_err(|_| {
            invalid_field(
                "displayName",
                FieldCode::InvalidFormat,
                "the display name is not valid",
            )
        })?;
    let existing = registry_error(registry.instance_by_name(&name))?.value;
    if let Some(existing) = existing {
        return Err(Box::new(name_taken(existing.id())));
    }

    let revision = params.provider.revision.as_deref().unwrap_or_default();
    let package = find_package(
        platform,
        prepared,
        registry,
        &params.provider.name,
        revision,
    )?;

    let runtime_id = RuntimeId::from_uuid(params.runtime_id).map_err(|_| {
        Box::new(
            ErrorBody::new(&instance::RUNTIME_NOT_FOUND, "no runtime has that id")
                .with_detail("runtimeId", params.runtime_id.to_string()),
        )
    })?;
    let runtime = registry_error(registry.runtime(runtime_id))?
        .value
        .ok_or_else(|| {
            Box::new(
                ErrorBody::new(&instance::RUNTIME_NOT_FOUND, "no runtime has that id")
                    .with_detail("runtimeId", runtime_id.to_string()),
            )
        })?;
    if runtime.software() != package.software() {
        return Err(Box::new(
            ErrorBody::new(
                &instance::RUNTIME_MISMATCH,
                "the runtime runs other software than the provider",
            )
            .with_detail("runtimeSoftware", runtime.software().to_string())
            .with_detail("providerSoftware", package.software().to_string()),
        ));
    }
    if runtime.state() != RuntimeState::Installed {
        return Err(Box::new(crate::supervisor::supervisor_error(
            &SupervisorError::RuntimeUnavailable {
                runtime_id: runtime.id().to_string(),
                state: runtime.state().as_code().to_owned(),
            },
        )));
    }

    let endpoints = endpoints_of(&package, &params.endpoints)?;

    // The id comes from the daemon; its short key must be free.
    let known = registry_error(registry.known_instance_ids())?.value;
    let id = loop {
        let id = InstanceId::generate();
        if known
            .iter()
            .all(|other| other.short_key() != id.short_key())
        {
            break id;
        }
    };

    let (secrets, values) = settings_of(&package, id, params)?;
    Ok(Plan {
        id,
        name,
        display_name,
        provider: package.key().clone(),
        runtime,
        endpoints,
        secrets,
        values,
    })
}

/// Finds the one pinned package of `provider` at `revision`: the folder
/// under the provider root, read and checked against its digest and the
/// registry (spec 0010 *Value sourcing*).
fn find_package(
    platform: &Platform,
    prepared: &PreparedLayout,
    registry: &SqliteRegistry,
    provider: &str,
    revision: &str,
) -> Result<ValidatedPackage, Box<ErrorBody>> {
    let not_found = || {
        Box::new(
            ErrorBody::new(
                &instance::PROVIDER_NOT_FOUND,
                "no pinned package has that provider and revision",
            )
            .with_detail("provider", provider)
            .with_detail("revision", revision),
        )
    };
    let Some((namespace, name)) = provider.split_once('/') else {
        return Err(not_found());
    };
    if [namespace, name, revision]
        .iter()
        .any(|part| part.is_empty() || part.contains(['/', '\\']) || *part == "..")
    {
        return Err(not_found());
    }
    let base = prepared
        .layout()
        .providers_dir()
        .join(namespace)
        .join(name)
        .join(revision);
    let entries = match platform.fs().read_dir(&base) {
        Ok(entries) => entries,
        Err(_) => return Err(not_found()),
    };
    let digests: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.info.kind == FileKind::Dir)
        .collect();
    match digests.as_slice() {
        [] => Err(not_found()),
        [one] => {
            let dir = base.join(&one.name);
            let package = read_package(platform.fs(), &dir)
                .ok()
                .and_then(|files| ValidatedPackage::from_files(files, NameCheck::Skip).ok())
                .ok_or_else(not_found)?;
            let key = package.key();
            let digest_matches = one.name.to_string_lossy() == key.manifest_sha256().to_string();
            let named =
                key.provider().to_string() == provider && key.revision().to_string() == revision;
            let known = registry_error(registry.provider_revision(key))?
                .value
                .is_some();
            if digest_matches && named && known {
                Ok(package)
            } else {
                Err(not_found())
            }
        }
        _ => Err(Box::new(
            ErrorBody::new(
                &instance::PROVIDER_AMBIGUOUS,
                "more than one package digest has that provider and revision",
            )
            .with_detail("provider", provider)
            .with_detail("revision", revision),
        )),
    }
}

/// The endpoints of the new instance: one per manifest endpoint, with the
/// port the request asks for (default `auto`) for a TCP or UDP endpoint.
fn endpoints_of(
    package: &ValidatedPackage,
    requests: &[EndpointRequest],
) -> Result<Vec<Endpoint>, Box<ErrorBody>> {
    let invalid = |endpoint: &str, message: &str| {
        Box::new(
            ErrorBody::new(&instance::ENDPOINT_INVALID, message).with_detail("endpoint", endpoint),
        )
    };
    let specs: Vec<&EndpointSpec> = package.endpoints().collect();
    let mut asked: BTreeMap<&str, DomainPortRequest> = BTreeMap::new();
    for request in requests {
        let Some(spec) = specs
            .iter()
            .find(|spec| spec.name().as_str() == request.name)
        else {
            return Err(invalid(
                &request.name,
                "the manifest has no endpoint with that name",
            ));
        };
        if !spec.transport().is_ip() {
            return Err(invalid(
                &request.name,
                "a unix endpoint takes no port request",
            ));
        }
        let port = match request.port {
            WirePort::Auto => DomainPortRequest::Auto,
            WirePort::Fixed(port) => DomainPortRequest::Fixed(
                Port::new(port)
                    .map_err(|_| invalid(&request.name, "the port is outside 1 to 65535"))?,
            ),
        };
        if asked.insert(request.name.as_str(), port).is_some() {
            return Err(invalid(&request.name, "the endpoint is asked for twice"));
        }
    }
    specs
        .iter()
        .map(|spec| {
            let name = spec.name().clone();
            match spec.transport() {
                EndpointTransport::Tcp | EndpointTransport::Udp => {
                    let transport = if spec.transport() == EndpointTransport::Tcp {
                        Transport::Tcp
                    } else {
                        Transport::Udp
                    };
                    let request = asked
                        .get(name.as_str())
                        .copied()
                        .unwrap_or(DomainPortRequest::Auto);
                    let host = spec
                        .default_host()
                        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
                    Ok(Endpoint::Ip(IpEndpoint::new(
                        name, transport, host, request, None,
                    )))
                }
                _ => {
                    // provisional: F20 (the socket name is rendered from
                    // the package at read time; the stored one only fills
                    // the column, spec 0010 *Status dan event*)
                    let mut text = format!("{}.sock", name.as_str());
                    text.truncate(15);
                    let socket: SocketName = text.parse().map_err(|_| {
                        invalid(name.as_str(), "the endpoint has no usable socket name")
                    })?;
                    Ok(Endpoint::Unix(UnixEndpoint::new(name, socket)))
                }
            }
        })
        .collect()
}

/// The secret plans and the validated settings of the new instance. A
/// secret field takes the reference the daemon makes for it; the client
/// never sends a secret value in `settings`.
fn settings_of(
    package: &ValidatedPackage,
    id: InstanceId,
    params: &CreateInstanceParams,
) -> Result<(Vec<SecretPlan>, serde_json::Value), Box<ErrorBody>> {
    let schema = package.settings_schema();
    let mut document = params.settings.as_map().clone();
    let mut fields = Vec::new();
    let mut secrets = Vec::new();
    for (index, secret) in params.secrets.iter().enumerate() {
        let at = format!("secrets[{index}].field");
        let setting = schema.field(&secret.field);
        let Some(rule) = setting.and_then(SecretFieldRule::from_setting) else {
            fields.push(FieldError::new(at, FieldCode::UnknownValue));
            continue;
        };
        let (Ok(field), Ok(purpose)) = (
            secret.field.parse::<SecretFieldName>(),
            SecretPurpose::from_field_name(&secret.field),
        ) else {
            fields.push(FieldError::new(at, FieldCode::InvalidFormat));
            continue;
        };
        if document.contains_key(&secret.field)
            || secrets.iter().any(|plan: &SecretPlan| plan.field == field)
        {
            fields.push(FieldError::new(at, FieldCode::InvalidValue));
            continue;
        }
        let reference = SecretRef::for_instance(id, &purpose);
        document.insert(
            secret.field.clone(),
            serde_json::Value::String(reference.to_string()),
        );
        secrets.push(SecretPlan {
            field,
            reference,
            purpose,
            rule,
            input: secret.input.clone(),
        });
    }
    // A secret field the client did not mention is generated (spec 0011
    // AC-6), within the field's length bounds.
    for setting in schema.fields() {
        let Some(rule) = SecretFieldRule::from_setting(setting) else {
            continue;
        };
        let name = setting.name().as_str();
        if document.contains_key(name) {
            continue;
        }
        let (Ok(field), Ok(purpose)) = (
            name.parse::<SecretFieldName>(),
            SecretPurpose::from_field_name(name),
        ) else {
            continue;
        };
        if secrets.iter().any(|plan: &SecretPlan| plan.field == field) {
            continue;
        }
        let reference = SecretRef::for_instance(id, &purpose);
        document.insert(
            name.to_owned(),
            serde_json::Value::String(reference.to_string()),
        );
        secrets.push(SecretPlan {
            field,
            reference,
            purpose,
            rule,
            input: WireInput::Generate,
        });
    }
    if !fields.is_empty() {
        return Err(settings_invalid(fields));
    }
    let document = serde_json::Value::Object(document);
    match schema.validate(&document) {
        Ok(values) => Ok((secrets, values.to_json())),
        Err(error) => Err(settings_invalid(settings_fields(&error))),
    }
}

fn settings_fields(error: &SettingsError) -> Vec<FieldError> {
    error
        .issues
        .iter()
        .map(|issue| {
            let path = match &issue.location {
                stackly_core::provider::Location::Field(path) if !path.is_empty() => {
                    format!("settings.{path}")
                }
                _ => "settings".to_owned(),
            };
            let code = if issue.code == stackly_core::provider::IssueCode::UnknownSetting {
                FieldCode::UnknownValue
            } else {
                FieldCode::InvalidValue
            };
            FieldError::new(path, code)
        })
        .collect()
}

fn settings_invalid(fields: Vec<FieldError>) -> Box<ErrorBody> {
    Box::new(
        ErrorBody::new(
            &instance::SETTINGS_INVALID,
            "the settings break the schema of the package",
        )
        .with_fields(fields),
    )
}

fn invalid_field(field: &str, code: FieldCode, message: &str) -> Box<ErrorBody> {
    Box::new(
        ErrorBody::new(&request::INVALID_PARAMS, message)
            .with_detail("method", "instance.create")
            .with_field(FieldError::new(field, code)),
    )
}

fn name_taken(existing: InstanceId) -> ErrorBody {
    ErrorBody::new(&instance::NAME_TAKEN, "another instance has that name")
        .with_detail("existing", existing.to_string())
}

fn registry_error<T>(
    result: Result<T, stackly_domain::RegistryError>,
) -> Result<T, Box<ErrorBody>> {
    result.map_err(|error| Box::new(crate::map::registry_error(&error)))
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(
        &stackly_protocol::codes::internal::UNEXPECTED,
        message,
    ))
}

/// The name of another instance holds the name this operation wants.
#[derive(Debug, thiserror::Error)]
#[error("another instance has the name {name}")]
struct NameTaken {
    name: String,
    existing: InstanceId,
}

impl OperationError for NameTaken {
    fn code(&self) -> ErrorCodeName {
        ErrorCodeName::from_static("instance.name_taken")
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        "existing"
            .parse::<DetailKey>()
            .map(|key| vec![(key, self.existing.to_string())])
            .unwrap_or_default()
    }
}

/// The work of `create_instance`.
struct CreateWork {
    plan: Plan,
    supervisor: stackly_core::supervisor::Supervisor,
    secrets: Arc<SecretService>,
    events: Option<Arc<BusSupervisorEvents>>,
    caller: SecretCaller,
}

impl OperationWork for CreateWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_create(*self, ctx).await })
    }
}

fn failed(error: impl OperationError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

/// Begins a step and shows its stage.
async fn enter(
    ctx: &mut OperationContext,
    stage: StageName,
    irreversible: bool,
) -> Result<(), WorkError> {
    let spec = StepSpec::new(stage.clone());
    ctx.begin_step(if irreversible {
        spec.irreversible()
    } else {
        spec
    })
    .await?;
    ctx.set_stage(stage).await
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, WorkError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| failed(SupervisorError::internal("a create task stopped")))
}

async fn run_create(work: CreateWork, mut ctx: OperationContext) -> WorkResult {
    let CreateWork {
        plan,
        supervisor,
        secrets,
        events,
        caller,
    } = work;
    let id = plan.id;
    let caller = caller.in_operation(ctx.id());
    let registry: Arc<dyn Registry> = Arc::clone(ctx.registry());

    // validate: the checks again, now that the operation holds the name,
    // plus the fixed ports against every other instance.
    enter(&mut ctx, StageName::VALIDATE, false).await?;
    {
        let registry = Arc::clone(&registry);
        let name = plan.name.clone();
        let endpoints = plan.endpoints.clone();
        let runtime_id = plan.runtime.id();
        blocking(move || validate_again(&*registry, &name, &endpoints, runtime_id))
            .await?
            .map_err(WorkError::Failed)?;
    }

    // secrets: each reference is recorded before its value is stored.
    enter(&mut ctx, StageName::SECRETS, false).await?;
    for secret in &plan.secrets {
        let handle = ctx.record_secret_intent(secret.reference.clone()).await?;
        let input = create_input(&secret.input).map_err(|error| {
            tracing::error!(code = error.code(), "a secret input could not be read");
            failed(SupervisorError::internal(
                "a secret input could not be read",
            ))
        })?;
        let target = PutTarget::New {
            reference: secret.reference.clone(),
            owner: SecretOwner::Instance(id),
            purpose: secret.purpose.clone(),
            field: Some(FieldContext::new(secret.field.clone(), secret.rule)),
        };
        let request = PutRequest::new(target, input, WriteMode::Create);
        let service = Arc::clone(&secrets);
        let who = caller.clone();
        let put = blocking(move || service.put(request, &who)).await?;
        if let Err(error) = put {
            return Err(failed(SupervisorError::Secret(classify_create_failure(
                error,
                &secret.reference,
            ))));
        }
        ctx.confirm_created(handle).await?;
    }

    // layout: the folders, claimed before the first write.
    enter(&mut ctx, StageName::LAYOUT, false).await?;
    let now = ctx.platform().clock().now();
    let handle = ctx.claim_resource().await?;
    let manifest = manifest_of(&plan, ctx.id(), now);
    {
        let platform = Arc::clone(ctx.platform());
        let layout = Arc::clone(ctx.layout());
        let registry = Arc::clone(&registry);
        let created = blocking(move || {
            let known = registry
                .known_instance_ids()
                .map(|known| known.value)
                .map_err(SupervisorError::Registry)?;
            create_instance_layout(&platform, &layout, &manifest, &known).map_err(|error| {
                SupervisorError::DataUnavailable {
                    path: layout
                        .layout()
                        .instance_paths(id)
                        .root
                        .display()
                        .to_string(),
                    reason: error.to_string(),
                }
            })
        })
        .await?;
        created.map_err(failed)?;
    }
    ctx.confirm_created(handle).await?;

    // register: the first step that cannot be undone.
    enter(&mut ctx, StageName::REGISTER, true).await?;
    let instance = Instance::new(NewInstance {
        id,
        name: plan.name.clone(),
        display_name: plan.display_name.clone(),
        provider_revision: plan.provider.clone(),
        software: plan.runtime.software().clone(),
        runtime_id: plan.runtime.id(),
        data_line: plan.runtime.line().clone(),
        data_version: plan.runtime.version().clone(),
        endpoints: plan.endpoints.clone(),
        startup: StartupPolicy::Manual,
        created_at: now,
        created_by: ctx.id(),
    })
    .map_err(|error| failed(SupervisorError::internal(error.to_string())))?;
    let values = ConfigValuesJson::new(plan.values.to_string())
        .map_err(|error| failed(SupervisorError::internal(error.to_string())))?;
    let revision = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        id,
        values,
        Some(ctx.id()),
        now,
    );
    let uses: BTreeMap<SecretFieldName, SecretRef> = plan
        .secrets
        .iter()
        .map(|secret| (secret.field.clone(), secret.reference.clone()))
        .collect();
    ctx.commit(FinalBatch {
        expected_generation: None,
        ops: vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertConfigRevision(revision),
            WriteOp::ReplaceSecretUses {
                user: SecretUser::Instance(id),
                uses,
            },
        ],
    })
    .await?;
    if let Some(events) = &events {
        events.instance_changed(&instance);
    }

    // init: the provider's init plan, under the instance lock.
    enter(&mut ctx, StageName::INIT, false).await?;
    supervisor
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

/// The checks of the `validate` stage, run inside the operation.
fn validate_again(
    registry: &dyn Registry,
    name: &InstanceName,
    endpoints: &[Endpoint],
    runtime: RuntimeId,
) -> Result<(), Box<dyn OperationError>> {
    let registry_failure =
        |error| -> Box<dyn OperationError> { Box::new(SupervisorError::Registry(error)) };
    if let Some(existing) = registry
        .instance_by_name(name)
        .map_err(registry_failure)?
        .value
    {
        return Err(Box::new(NameTaken {
            name: name.to_string(),
            existing: existing.id(),
        }));
    }
    let found = registry.runtime(runtime).map_err(registry_failure)?.value;
    match found {
        Some(found) if found.state() == RuntimeState::Installed => {}
        Some(found) => {
            return Err(Box::new(SupervisorError::RuntimeUnavailable {
                runtime_id: runtime.to_string(),
                state: found.state().as_code().to_owned(),
            }));
        }
        None => {
            return Err(Box::new(SupervisorError::RuntimeUnavailable {
                runtime_id: runtime.to_string(),
                state: "missing".to_owned(),
            }));
        }
    }
    let others = registry
        .instances(stackly_domain::InstanceFilter {
            include_retained: false,
        })
        .map_err(registry_failure)?
        .value;
    for endpoint in endpoints {
        let Endpoint::Ip(ip) = endpoint else {
            continue;
        };
        if !matches!(ip.request(), DomainPortRequest::Fixed(_)) {
            continue;
        }
        for other in &others {
            if other
                .endpoints()
                .iter()
                .any(|theirs| endpoints_conflict(endpoint, theirs))
            {
                return Err(Box::new(SupervisorError::EndpointConflict {
                    endpoint: endpoint_name(endpoint).to_string(),
                    port: ip.effective_port().map_or(0, Port::get),
                    other: other.id(),
                }));
            }
        }
    }
    Ok(())
}

fn endpoint_name(endpoint: &Endpoint) -> &EndpointName {
    endpoint.name()
}

/// The manifest the instance folder starts with.
fn manifest_of(
    plan: &Plan,
    operation: stackly_domain::OperationId,
    now: Timestamp,
) -> InstanceManifest {
    let runtime = &plan.runtime;
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
        instance_id: plan.id,
        name: plan.name.clone(),
        display_name: plan.display_name.clone(),
        software: runtime.software().clone(),
        provider: plan.provider.clone(),
        runtime: manifest_runtime,
        created_at: now,
        created_by_operation: operation,
        stackly_version: daemon_info().version.to_owned(),
    })
}
