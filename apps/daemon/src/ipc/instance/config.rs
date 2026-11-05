//! The config methods (spec 0029, *Method IPC*): `instance.config.get`,
//! `instance.config.validate`, `instance.config.apply`,
//! `instance.config.keep_revision`, `instance.rename`, and
//! `instance.change_endpoints`.
//!
//! The two reads and the validation never write, spawn, or touch the
//! Keychain, the catalog, or the network: they read the registry, the
//! pinned package, and the history copies of the config folder (REQ-046).
//! A method that changes something follows the pattern of create: the
//! engine is asked for a recorded reply first (`replay_operation`), then
//! the same pure planner the operation runs reads the facts and refuses
//! what a state or a rule decides without any operation (AC-3, AC-6, AC-9,
//! AC-11), and only a valid change begins an operation. The operation
//! plans again from fresh facts after it queued behind any other work on
//! the instance.

use stackly_core::instance::config::{
    ConfigChange, ConfigError, ConfigFacts, EffectiveFile as CoreFile, EndpointIssue, Planned,
    Rejected, RuntimeFact, changed_lines, gather_config, preview, recorded_files, refusal_error,
};
use stackly_core::instance::create::{CreateError, FieldCode as CoreFieldCode, FieldIssue};
use stackly_core::operation::{
    BeginRequest, BusyPolicy, NoOpReply, Origin, Precheck, PrecheckOutcome,
};
use stackly_core::supervisor::drift::{self, ResolveDriftRequest};
use stackly_core::supervisor::{ChangeRequest, RenameRequest, SupervisorError};
use stackly_domain::{
    ConfigRevisionNumber as DomainNumber, EndpointName, InstanceFilter, InstanceId,
    InstanceLifecycle, InstanceName, JournalPayload, LockKey, OperationJournal, OperationKind,
    OperationResource, Port, PortRequest as DomainPort, rename_allowed,
};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::InstanceActionAccepted;
use stackly_protocol::instance::PortRequest;
use stackly_protocol::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ChangeEndpointsReply, ConfigApplyAccepted,
    ConfigChangeRequest, ConfigImpact, ConfigValidation, EndpointValidation, GetConfigParams,
    InstanceConfigView, KeepRevisionParams, RenameInstanceParams, ValidateConfigParams,
};
use stackly_protocol::message::Call;
use stackly_protocol::{ConfigRevisionNumber, ErrorBody};

use super::{
    action_reply, expected_revision, instance_id, read_for_precheck, read_instance, registry_of,
    reject, unexpected,
};
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::{
    Replay, StartOutcome, StartRequest as OperationStart, operation_error_body, replay_operation,
    start_operation,
};
use crate::ipc::secret::caller;
use crate::map;
use crate::state::DaemonCore;
use crate::supervisor::supervisor_error;

/// The method names the idempotency record keeps for the two config methods
/// that start an operation. A `MethodName` is one code, a dot, and one
/// code, so `instance.config.apply` becomes `instance.config_apply` here;
/// the wire name is unchanged.
const APPLY_KEY_METHOD: &str = "instance.config_apply";
const KEEP_KEY_METHOD: &str = "instance.config_keep_revision";

/// The error body of a config refusal, with the hint of its code.
fn config_error_body(error: &ConfigError) -> Box<ErrorBody> {
    Box::new(map::create::with_create_hint(operation_error_body(error)))
}

/// The error body of a port request that cannot be applied, with the free
/// port to try when there is one (AC-14).
fn endpoint_body(issue: &EndpointIssue) -> Box<ErrorBody> {
    let body = operation_error_body(&issue.error);
    let body = match issue.suggestion {
        Some(port) => body.with_detail("suggestedPort", port.get().to_string()),
        None => body,
    };
    Box::new(map::create::with_create_hint(body))
}

/// The error a rejected plan answers with.
fn rejection(rejected: Rejected) -> Box<ErrorBody> {
    match rejected {
        Rejected::Fields(issues) => Box::new(map::create::with_create_hint(
            map::create::create_error(&CreateError::SettingsInvalid, &issues),
        )),
        Rejected::Endpoints(issues) => issues.first().map_or_else(
            || unexpected("a port request was refused without a reason"),
            endpoint_body,
        ),
        Rejected::Error(error) => config_error_body(&error),
        _ => unexpected("a config change was refused for a reason this daemon cannot name"),
    }
}

/// The domain revision number a wire number names.
fn domain_number(number: ConfigRevisionNumber) -> Result<DomainNumber, Box<ErrorBody>> {
    DomainNumber::new(number.get()).map_err(|_| {
        Box::new(
            ErrorBody::new(
                &stackly_protocol::codes::request::INVALID_PARAMS,
                "a revision number starts at 1",
            )
            .with_field(stackly_protocol::FieldError::new(
                "baseRevision",
                stackly_protocol::json::FieldCode::OutOfRange,
            )),
        )
    })
}

/// The change a request asks for, in the words of core.
fn core_change(change: &ConfigChangeRequest) -> Result<ConfigChange, Box<ErrorBody>> {
    Ok(match change {
        ConfigChangeRequest::Settings(document) => {
            ConfigChange::Settings(serde_json::Value::Object(document.as_map().clone()))
        }
        ConfigChangeRequest::Restore(number) => ConfigChange::Restore(domain_number(*number)?),
        ConfigChangeRequest::Rollback => ConfigChange::Rollback,
    })
}

/// Reads the facts of `id` on a blocking thread.
async fn gather(core: &DaemonCore, id: InstanceId) -> Result<ConfigFacts, Box<ErrorBody>> {
    let registry = registry_of(core)?;
    let platform = core.platform().clone();
    let prepared = core.prepared().clone();
    tokio::task::spawn_blocking(move || gather_config(&platform, &prepared, &registry, id))
        .await
        .map_err(|_| unexpected("the config read task failed"))?
        .map_err(|error| Box::new(map::create::with_create_hint(supervisor_error(&error))))
}

/// The config files the page shows now: the copies of the last render of
/// the active revision, else a preview of its settings. The note says why
/// there is nothing to show when the runtime folder is gone.
async fn current_files(
    core: &DaemonCore,
    facts: &ConfigFacts,
) -> Result<(Vec<CoreFile>, Option<String>), Box<ErrorBody>> {
    let platform = core.platform().clone();
    let paths = facts.paths.clone();
    let revision = facts.active.id;
    let copies =
        tokio::task::spawn_blocking(move || recorded_files(platform.fs(), &paths, revision))
            .await
            .map_err(|_| unexpected("the config copies task failed"))?;
    match copies {
        Ok(files) if !files.is_empty() => return Ok((files, None)),
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(instance = %facts.instance.id(), %error, "the config copies cannot be read");
            return Ok((Vec::new(), Some("Config copies unreadable".to_owned())));
        }
    }
    Ok(match &facts.runtime {
        RuntimeFact::Present(runtime) => match preview(
            &facts.package,
            &facts.instance,
            &facts.paths,
            runtime,
            &facts.settings,
        ) {
            Ok(files) => (files, None),
            Err(_) => (Vec::new(), Some("Preview unavailable".to_owned())),
        },
        _ => (
            Vec::new(),
            Some(map::config::RUNTIME_MISSING_NOTE.to_owned()),
        ),
    })
}

/// The files of the drift the supervisor holds for `id`, or `None` when
/// there is none. The blocker is set by `instance.config.get`, by every
/// render, and once per instance when the daemon starts; `start`,
/// `restart`, and `reload` refuse with `config.drift_detected` before any
/// operation while it stands (spec 0029, AC-11). A file edited since the
/// last check is caught by the drift gate of the render, which fails the
/// operation before it writes a file. This reads memory only, so a start
/// never waits for the disk before it begins.
pub(super) fn drifted_files(
    supervisor: &stackly_core::supervisor::Supervisor,
    id: InstanceId,
) -> Option<Vec<String>> {
    match supervisor.status(id).blocker {
        Some(stackly_domain::InstanceBlocker::ConfigDrift { files, .. }) => {
            Some(files.iter().map(ToString::to_string).collect())
        }
        _ => None,
    }
}

/// The real `instance.config.get`.
pub fn get_config(
    core: &DaemonCore,
    params: GetConfigParams,
) -> HandlerFuture<'_, Box<InstanceConfigView>> {
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let id = instance_id(params.instance_id)?;
        let facts = gather(core, id).await?;
        let report = drift::observe_active(&supervisor, id)
            .await
            .map_err(|error| Box::new(supervisor_error(&error)))?;
        let observed_at = core.platform().clock().now();
        let (files, note) = current_files(core, &facts).await?;
        Ok(Box::new(map::config::view(
            &facts,
            &files,
            note,
            map::config::drift(&report, observed_at),
        )))
    })
}

/// The real `instance.config.validate`: judges the change against the
/// revision the client read, and writes nothing (AC-2).
pub fn validate_config(
    core: &DaemonCore,
    params: ValidateConfigParams,
) -> HandlerFuture<'_, Box<ConfigValidation>> {
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let id = instance_id(params.instance_id)?;
        let base = domain_number(params.base_revision)?;
        let change = core_change(&params.change)?;
        let planned = match supervisor.plan_change(id, base, &change, false).await {
            Ok(planned) => planned,
            Err(Rejected::Fields(issues)) => {
                return Ok(Box::new(ConfigValidation {
                    valid: false,
                    field_errors: map::create::field_errors(&issues),
                    changes: Vec::new(),
                    endpoint_changes: Vec::new(),
                    impact: ConfigImpact::None,
                    preview: Vec::new(),
                }));
            }
            Err(other) => return Err(rejection(other)),
        };
        let Planned::Change(plan) = planned else {
            return Ok(Box::new(ConfigValidation {
                valid: true,
                field_errors: Vec::new(),
                changes: Vec::new(),
                endpoint_changes: Vec::new(),
                impact: ConfigImpact::None,
                preview: Vec::new(),
            }));
        };
        let facts = gather(core, id).await?;
        let files = proposed_files(&facts, plan.values.as_str());
        Ok(Box::new(ConfigValidation {
            valid: true,
            field_errors: Vec::new(),
            changes: plan
                .changes
                .iter()
                .map(map::config::setting_change)
                .collect(),
            endpoint_changes: map::config::endpoint_changes(&plan.endpoint_changes),
            impact: map::config::impact(plan.impact),
            preview: files.iter().map(map::config::effective_file).collect(),
        }))
    })
}

/// The files the proposed settings would render, with the lines that
/// differ from the active settings rendered the same way marked. Nothing
/// without the runtime folder.
fn proposed_files(facts: &ConfigFacts, proposed: &str) -> Vec<CoreFile> {
    let RuntimeFact::Present(runtime) = &facts.runtime else {
        return Vec::new();
    };
    let Ok(document) = serde_json::from_str::<serde_json::Value>(proposed) else {
        return Vec::new();
    };
    let Ok(settings) = facts.package.settings_schema().validate(&document) else {
        return Vec::new();
    };
    let render = |settings| {
        preview(
            &facts.package,
            &facts.instance,
            &facts.paths,
            runtime,
            settings,
        )
        .unwrap_or_default()
    };
    let before = render(&facts.settings);
    let old = map::config::contents(&before);
    let mut after = render(&settings);
    for file in &mut after {
        file.changed_lines = changed_lines(
            old.get(file.path.as_str()).copied().unwrap_or(""),
            &file.content,
        );
    }
    after
}

/// The journal input of a config change (AC-20): the old and new revision,
/// the origin, the names of the settings that change, the old and new port
/// of each endpoint that moves, and the impact. No setting value.
fn change_input(plan: &stackly_core::instance::config::ChangePlan) -> JournalPayload {
    let mut audit = plan.audit();
    if let Some(object) = audit.as_object_mut() {
        object.insert(
            "impact".to_owned(),
            serde_json::Value::from(map::config::impact_text(plan.impact)),
        );
    }
    JournalPayload::new(audit.to_string(), OperationJournal::INPUT_MAX)
        .unwrap_or_else(|_| JournalPayload::empty())
}

/// The reply for a request the engine already answered: the revision and
/// the impact come back from the journal input of the first request.
fn replayed(replay: Replay, base: ConfigRevisionNumber) -> ConfigApplyAccepted {
    let input: serde_json::Value =
        serde_json::from_str(replay.input.as_str()).unwrap_or(serde_json::Value::Null);
    let revision = input
        .get("newRevision")
        .and_then(serde_json::Value::as_u64)
        .and_then(ConfigRevisionNumber::new)
        .or_else(|| base.next())
        .unwrap_or(base);
    ConfigApplyAccepted {
        accepted: Some(replay.accepted),
        unchanged: false,
        revision,
        impact: input
            .get("impact")
            .and_then(serde_json::Value::as_str)
            .map_or(ConfigImpact::Unrecognized, map::config::impact_of_text),
    }
}

/// The reply for a change equal to the active revision.
fn unchanged(base: ConfigRevisionNumber) -> ConfigApplyAccepted {
    ConfigApplyAccepted {
        accepted: None,
        unchanged: true,
        revision: base,
        impact: ConfigImpact::None,
    }
}

/// The reply of a change that began an operation.
fn applied(
    outcome: StartOutcome,
    plan: &stackly_core::instance::config::ChangePlan,
    base: ConfigRevisionNumber,
) -> ConfigApplyAccepted {
    match outcome {
        StartOutcome::Accepted(accepted) => ConfigApplyAccepted {
            accepted: Some(*accepted),
            unchanged: false,
            revision: map::config::number(plan.active.next()),
            impact: map::config::impact(plan.impact),
        },
        StartOutcome::NothingToDo => unchanged(base),
    }
}

/// The precheck of an operation that changes an instance: the instance
/// exists and is at the revision the request expects.
fn expected_precheck(
    core: &DaemonCore,
    id: InstanceId,
    expected: Option<stackly_domain::ResourceRevision>,
) -> Result<Box<dyn Precheck>, Box<ErrorBody>> {
    let registry = registry_of(core)?;
    Ok(Box::new(move || {
        match read_for_precheck(&registry, id, expected) {
            Ok(_) => PrecheckOutcome::Proceed,
            Err(rejected) => rejected,
        }
    }))
}

/// The real `instance.config.apply`: apply, roll back, and restore.
pub fn apply_config<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ApplyConfigParams,
) -> HandlerFuture<'a, Box<ConfigApplyAccepted>> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let id = instance_id(params.instance_id)?;
        let base_number = domain_number(params.base_revision)?;
        let expected = expected_revision(params.expected_revision)?;
        let change = core_change(&params.change)?;
        let base = params.base_revision;
        let fingerprint = request_fingerprint(&Call::InstanceConfigApply(params));
        let start = OperationStart::new(
            APPLY_KEY_METHOD,
            context.idempotency_key.as_deref(),
            fingerprint,
        );
        if let Some(replay) = replay_operation(core, &context, start).await? {
            return Ok(Box::new(replayed(replay, base)));
        }
        let precheck = expected_precheck(core, id, expected)?;
        let plan = match supervisor
            .plan_change(id, base_number, &change, true)
            .await
            .map_err(rejection)?
        {
            Planned::Unchanged => {
                // Nothing queues, so the precheck never runs: a stale
                // `expectedRevision` is refused here instead of answered
                // `unchanged`.
                if let Some(expected) = expected {
                    let actual = read_instance(core, id).await?.revision();
                    if expected != actual {
                        return Err(Box::new(supervisor_error(
                            &SupervisorError::RevisionMismatch {
                                instance: id,
                                expected,
                                actual,
                            },
                        )));
                    }
                }
                return Ok(Box::new(unchanged(base)));
            }
            Planned::Change(plan) => plan,
            _ => return Err(unexpected("a config plan this daemon cannot read")),
        };
        let input = change_input(&plan);
        let work = supervisor.change_work(ChangeRequest::new(
            id,
            base_number,
            change,
            caller(&context),
        ));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::ApplyConfig,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            // A change waits behind the operation that holds the instance,
            // then plans again from fresh facts (AC-9).
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(core, &context, start, request).await?;
        Ok(Box::new(applied(outcome, &plan, base)))
    })
}

/// The real `instance.config.keep_revision`: write the config files of the
/// active revision back over a drift (AC-11).
pub fn keep_revision<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: KeepRevisionParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let id = instance_id(params.instance_id)?;
        let fingerprint = request_fingerprint(&Call::InstanceConfigKeepRevision(params));
        let start = OperationStart::new(
            KEEP_KEY_METHOD,
            context.idempotency_key.as_deref(),
            fingerprint,
        );
        if let Some(replay) = replay_operation(core, &context, start).await? {
            return Ok(InstanceActionAccepted {
                accepted: Some(replay.accepted),
                already_in_state: false,
            });
        }
        let instance = read_instance(core, id).await?;
        if instance.lifecycle() == InstanceLifecycle::Retained {
            return Err(Box::new(supervisor_error(
                &stackly_core::supervisor::SupervisorError::InstanceRetained,
            )));
        }
        let report = drift::observe_active(&supervisor, id)
            .await
            .map_err(|error| Box::new(supervisor_error(&error)))?;
        if report.is_clean() {
            return Ok(InstanceActionAccepted {
                accepted: None,
                already_in_state: true,
            });
        }
        let work = supervisor.resolve_drift_work(ResolveDriftRequest::new(id));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::ResolveConfigDrift,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            // Another operation that holds the instance answers
            // `resource.busy`, and the user tries again (spec 0029).
            busy: BusyPolicy::Reject,
            expected_revision: None,
            origin,
            input: JournalPayload::empty(),
            work,
            precheck: None,
        };
        let outcome = start_operation(core, &context, start, request).await?;
        Ok(action_reply(outcome))
    })
}

/// The real `instance.rename` (AC-13).
pub fn rename<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: RenameInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let expected = expected_revision(params.expected_revision)?;
        let name: InstanceName = params.name.parse().map_err(|_| {
            Box::new(map::create::with_create_hint(map::create::create_error(
                &CreateError::NameInvalid,
                &[FieldIssue::new("name", CoreFieldCode::InvalidFormat)],
            )))
        })?;
        let fingerprint = request_fingerprint(&Call::InstanceRename(params));
        let work = supervisor.rename_work(RenameRequest::new(id, name.clone()));
        let input = JournalPayload::new(
            serde_json::json!({ "newName": name.as_str() }).to_string(),
            OperationJournal::INPUT_MAX,
        )
        .unwrap_or_else(|_| JournalPayload::empty());
        let taken = name.clone();
        let precheck: Box<dyn Precheck> =
            Box::new(move || rename_precheck(&registry, id, expected, &taken));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::RenameInstance,
            resource: OperationResource::Instance(id),
            // The same lock as create: a create and a rename to one name
            // never both win.
            creates_resource: false,
            extra_locks: vec![LockKey::InstanceName(name)],
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work,
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "instance.rename",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        Ok(action_reply(outcome))
    })
}

/// Refuses a rename the state or the name decides, and answers the same
/// name with nothing to do (AC-13). The commit checks the name again.
fn rename_precheck(
    registry: &stackly_storage::SqliteRegistry,
    id: InstanceId,
    expected: Option<stackly_domain::ResourceRevision>,
    name: &InstanceName,
) -> PrecheckOutcome {
    use stackly_domain::Registry as _;

    let instance = match read_for_precheck(registry, id, expected) {
        Ok(instance) => instance,
        Err(rejected) => return rejected,
    };
    if let Err(refusal) = rename_allowed(instance.lifecycle()) {
        return reject(refusal_error(refusal, instance.lifecycle()));
    }
    if instance.name() == name {
        return PrecheckOutcome::NoOp(NoOpReply::nothing());
    }
    let others = match registry.instances(InstanceFilter {
        include_retained: false,
    }) {
        Ok(others) => others.value,
        Err(error) => return reject(stackly_core::supervisor::SupervisorError::Registry(error)),
    };
    match others
        .iter()
        .find(|other| other.id() != id && other.name() == name)
    {
        Some(taken) => PrecheckOutcome::Reject(Box::new(CreateError::NameTaken {
            existing: taken.id(),
        })),
        None => PrecheckOutcome::Proceed,
    }
}

/// The endpoint requests of the params, in the words of core.
fn endpoint_requests(
    params: &ChangeEndpointsParams,
) -> Result<Vec<(EndpointName, DomainPort)>, Box<ErrorBody>> {
    let invalid = |endpoint: &str| {
        Box::new(map::create::with_create_hint(operation_error_body(
            &CreateError::EndpointInvalid {
                endpoint: endpoint.to_owned(),
            },
        )))
    };
    params
        .endpoints
        .iter()
        .map(|request| {
            let name: EndpointName = request.name.parse().map_err(|_| invalid(&request.name))?;
            let port = match request.port {
                PortRequest::Auto => DomainPort::Auto,
                PortRequest::Fixed(port) => {
                    DomainPort::Fixed(Port::new(port).map_err(|_| invalid(&request.name))?)
                }
            };
            Ok((name, port))
        })
        .collect()
}

/// The real `instance.change_endpoints` (AC-14): the operation that
/// changes the ports, or the verdict of a dry run.
pub fn change_endpoints<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ChangeEndpointsParams,
) -> HandlerFuture<'a, Box<ChangeEndpointsReply>> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let id = instance_id(params.instance_id)?;
        let base_number = domain_number(params.base_revision)?;
        let base = params.base_revision;
        let requests = endpoint_requests(&params)?;
        let change = ConfigChange::Endpoints(requests);
        if params.dry_run {
            return dry_run(&supervisor, id, base_number, &change)
                .await
                .map(|verdict| Box::new(ChangeEndpointsReply::DryRun(verdict)));
        }
        let fingerprint = request_fingerprint(&Call::InstanceChangeEndpoints(params));
        let start = OperationStart::new(
            "instance.change_endpoints",
            context.idempotency_key.as_deref(),
            fingerprint,
        );
        if let Some(replay) = replay_operation(core, &context, start).await? {
            return Ok(Box::new(ChangeEndpointsReply::Applied(Box::new(replayed(
                replay, base,
            )))));
        }
        let plan = match supervisor
            .plan_change(id, base_number, &change, true)
            .await
            .map_err(rejection)?
        {
            Planned::Unchanged => {
                return Ok(Box::new(ChangeEndpointsReply::Applied(Box::new(
                    unchanged(base),
                ))));
            }
            Planned::Change(plan) => plan,
            _ => return Err(unexpected("a config plan this daemon cannot read")),
        };
        let input = change_input(&plan);
        let work = supervisor.change_work(ChangeRequest::new(
            id,
            base_number,
            change,
            caller(&context),
        ));
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::ChangeEndpoints,
            resource: OperationResource::Instance(id),
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work,
            precheck: None,
        };
        let outcome = start_operation(core, &context, start, request).await?;
        Ok(Box::new(ChangeEndpointsReply::Applied(Box::new(applied(
            outcome, &plan, base,
        )))))
    })
}

/// The verdict of a dry run: every request that cannot be applied, or the
/// ports that would change.
async fn dry_run(
    supervisor: &stackly_core::supervisor::Supervisor,
    id: InstanceId,
    base: DomainNumber,
    change: &ConfigChange,
) -> Result<EndpointValidation, Box<ErrorBody>> {
    match supervisor.plan_change(id, base, change, false).await {
        Ok(Planned::Change(plan)) => Ok(EndpointValidation {
            valid: true,
            problems: Vec::new(),
            changes: map::config::endpoint_changes(&plan.endpoint_changes),
            impact: map::config::impact(plan.impact),
        }),
        Ok(_) => Ok(EndpointValidation {
            valid: true,
            problems: Vec::new(),
            changes: Vec::new(),
            impact: ConfigImpact::None,
        }),
        Err(Rejected::Endpoints(issues)) => Ok(EndpointValidation {
            valid: false,
            problems: issues
                .iter()
                .map(|issue| {
                    map::config::endpoint_problem(
                        issue,
                        stackly_core::operation::OperationError::code(&issue.error).as_str(),
                    )
                })
                .collect(),
            changes: Vec::new(),
            impact: ConfigImpact::None,
        }),
        Err(other) => Err(rejection(other)),
    }
}
