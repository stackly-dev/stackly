//! `settings.get`, `settings.update`, `storage.list_locations`, and
//! `storage.set_location` (spec 0032).
//!
//! Both writes follow `instance.set_startup`: a precheck answers what the
//! state decides (a stale revision, a change that changes nothing), only a
//! real change begins an operation, and the method waits for it and answers
//! with the settings or the location as they are then. `settings.changed`
//! goes out once per accepted change, after the commit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use stackly_core::logs::{LOG_SEGMENT_MAX_BYTES, LOG_SEGMENTS_PER_STREAM};
use stackly_core::operation::{
    BeginRequest, BusyPolicy, NoOpReply, Origin, Precheck, PrecheckOutcome,
};
use stackly_core::settings::{
    LocationTarget as CoreTarget, SetLocationRequest, SettingsCheck, SettingsError, UpdateRequest,
    set_location_precheck, set_location_work, update_precheck, update_work,
};
use stackly_domain::{
    AUDIT_MAX_ROWS, AuditRetentionDays, DaemonSettings, JournalPayload, LocationKind as DomainKind,
    OperationId, OperationJournal, OperationKind, OperationResource, Registry, RegistryError,
    ResourceRevision, SettingsPatch, StartupPolicy, StorageLocation,
    UpdateChannel as DomainChannel,
};
use stackly_platform::FileKind;
use stackly_protocol::audit::AuditRetention;
use stackly_protocol::codes::{internal, request};
use stackly_protocol::events::EventKind;
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::StartupPolicyKind;
use stackly_protocol::json::FieldCode;
use stackly_protocol::logs::LogRetention;
use stackly_protocol::message::Call;
use stackly_protocol::settings::{
    DaemonSettingsView, DefaultLocations, LocationKind, LocationList, LocationTarget,
    SetLocationParams, SettingsChanged, StorageLocationView, UpdateChannel, UpdateSettingsParams,
};
use stackly_protocol::{Count, ErrorBody, FieldError, Revision, Timestamp};
use stackly_storage::SqliteRegistry;

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::dto::failure_body;
use crate::ipc::operation::{StartOutcome, StartRequest as OperationStart, start_operation};
use crate::map;
use crate::state::DaemonCore;

/// How often a handler looks at the operation it waits for.
const POLL: Duration = Duration::from_millis(20);

fn registry_of(core: &DaemonCore) -> Result<SqliteRegistry, Box<ErrorBody>> {
    core.registry().ok_or_else(|| {
        Box::new(ErrorBody::new(
            &stackly_protocol::codes::registry::UNAVAILABLE,
            "the registry is not available",
        ))
    })
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&internal::UNEXPECTED, message))
}

fn registry_failure(error: &RegistryError) -> Box<ErrorBody> {
    Box::new(map::registry_error(error))
}

/// The wire revision of a domain one.
fn wire_revision(revision: ResourceRevision) -> Result<Revision, Box<ErrorBody>> {
    Revision::new(revision.get())
        .ok_or_else(|| unexpected("a settings revision is above the largest safe number"))
}

/// The domain revision a request expects.
fn domain_revision(revision: Revision) -> Result<ResourceRevision, Box<ErrorBody>> {
    ResourceRevision::new(revision.get()).map_err(|_| {
        Box::new(
            ErrorBody::new(
                &request::INVALID_PARAMS,
                "expectedRevision is not a revision",
            )
            .with_field(FieldError::new("expectedRevision", FieldCode::OutOfRange)),
        )
    })
}

fn kind_of(kind: LocationKind) -> Option<DomainKind> {
    match kind {
        LocationKind::Data => Some(DomainKind::Data),
        LocationKind::Logs => Some(DomainKind::Logs),
        LocationKind::Cache => Some(DomainKind::Cache),
        LocationKind::Unrecognized => None,
    }
}

const fn wire_kind(kind: DomainKind) -> LocationKind {
    match kind {
        DomainKind::Data => LocationKind::Data,
        DomainKind::Logs => LocationKind::Logs,
        DomainKind::Cache => LocationKind::Cache,
    }
}

fn patch_of(params: &UpdateSettingsParams) -> Result<SettingsPatch, Box<ErrorBody>> {
    let invalid = |path: &str| {
        Box::new(
            ErrorBody::new(
                &request::INVALID_PARAMS,
                "the patch holds a value this build does not know",
            )
            .with_field(FieldError::new(path, FieldCode::UnknownValue)),
        )
    };
    let default_startup = match params.patch.default_startup {
        None => None,
        Some(StartupPolicyKind::Manual) => Some(StartupPolicy::Manual),
        Some(StartupPolicyKind::AtLogin) => Some(StartupPolicy::AtLogin),
        Some(StartupPolicyKind::Unrecognized) => return Err(invalid("patch.defaultStartup")),
    };
    let update_channel = match params.patch.update_channel {
        None => None,
        Some(UpdateChannel::Stable) => Some(DomainChannel::Stable),
        Some(UpdateChannel::Beta) => Some(DomainChannel::Beta),
        Some(UpdateChannel::Unrecognized) => return Err(invalid("patch.updateChannel")),
    };
    let audit_retention = match params.patch.audit_retention_days {
        None => None,
        Some(days) => Some(AuditRetentionDays::from_days(days).ok_or_else(|| {
            Box::new(
                ErrorBody::new(
                    &request::INVALID_PARAMS,
                    "auditRetentionDays must be 7, 30, or 90",
                )
                .with_field(FieldError::new(
                    "patch.auditRetentionDays",
                    FieldCode::UnknownValue,
                )),
            )
        })?),
    };
    Ok(SettingsPatch {
        default_startup,
        update_channel,
        telemetry: params.patch.telemetry,
        audit_retention,
    })
}

/// The view of a location: its path resolved, whether it is the default,
/// how many instances use it, and whether its folder is there now.
fn location_view(
    core: &DaemonCore,
    settings: &DaemonSettings,
    location: &StorageLocation,
    usage: &[(stackly_domain::StorageLocationId, u64)],
) -> Result<StorageLocationView, Box<ErrorBody>> {
    let base = core
        .prepared()
        .locations()
        .base(location.id())
        .or_else(|| location.path().map(PathBuf::from))
        .ok_or_else(|| unexpected("a storage location has no folder"))?;
    let reachable = matches!(
        core.platform().fs().symlink_metadata(&base),
        Ok(Some(info)) if info.kind == FileKind::Dir
    );
    let count = if location.kind() == DomainKind::Cache {
        0
    } else {
        usage
            .iter()
            .find(|(id, _)| *id == location.id())
            .map_or(0, |(_, count)| *count)
    };
    Ok(StorageLocationView {
        id: location.id().as_uuid(),
        kind: wire_kind(location.kind()),
        path: base.display().to_string(),
        built_in: location.is_built_in(),
        is_default: settings.location_of(location.kind()) == location.id(),
        instance_count: Count::saturating(count),
        reachable,
    })
}

/// Everything the views need, read in one go.
struct Snapshot {
    settings: DaemonSettings,
    locations: Vec<StorageLocation>,
    usage: Vec<(stackly_domain::StorageLocationId, u64)>,
}

fn read_snapshot(registry: &SqliteRegistry) -> Result<Snapshot, Box<ErrorBody>> {
    Ok(Snapshot {
        settings: registry
            .daemon_settings()
            .map_err(|error| registry_failure(&error))?
            .value,
        locations: registry
            .storage_locations()
            .map_err(|error| registry_failure(&error))?
            .value,
        usage: registry
            .location_usage()
            .map_err(|error| registry_failure(&error))?
            .value,
    })
}

fn settings_view(
    core: &DaemonCore,
    snapshot: &Snapshot,
) -> Result<DaemonSettingsView, Box<ErrorBody>> {
    let default_of = |kind: DomainKind| -> Result<StorageLocationView, Box<ErrorBody>> {
        let id = snapshot.settings.location_of(kind);
        let location = snapshot
            .locations
            .iter()
            .find(|location| location.id() == id)
            .ok_or_else(|| unexpected("a default location is not in the registry"))?;
        location_view(core, &snapshot.settings, location, &snapshot.usage)
    };
    let settings = &snapshot.settings;
    Ok(DaemonSettingsView {
        revision: wire_revision(settings.revision)?,
        default_startup: match settings.default_startup {
            StartupPolicy::Manual => StartupPolicyKind::Manual,
            StartupPolicy::AtLogin => StartupPolicyKind::AtLogin,
        },
        update_channel: match settings.update_channel {
            DomainChannel::Stable => UpdateChannel::Stable,
            DomainChannel::Beta => UpdateChannel::Beta,
        },
        telemetry: settings.telemetry,
        telemetry_copy_version: Count::saturating(u64::from(settings.telemetry_copy_version)),
        locations: DefaultLocations {
            data: default_of(DomainKind::Data)?,
            logs: default_of(DomainKind::Logs)?,
            cache: default_of(DomainKind::Cache)?,
        },
        log_retention: LogRetention {
            segments_per_stream: Count::saturating(LOG_SEGMENTS_PER_STREAM as u64),
            segment_max_bytes: Count::saturating(LOG_SEGMENT_MAX_BYTES),
        },
        audit_retention: Some(AuditRetention {
            days: settings.audit_retention.days(),
            max_rows: Count::saturating(u64::from(AUDIT_MAX_ROWS)),
        }),
        updated_at: Timestamp::from(settings.updated_at),
    })
}

async fn current_settings(core: &DaemonCore) -> Result<Box<DaemonSettingsView>, Box<ErrorBody>> {
    let registry = registry_of(core)?;
    let snapshot = tokio::task::spawn_blocking(move || read_snapshot(&registry))
        .await
        .map_err(|_| unexpected("the settings task failed"))??;
    Ok(Box::new(settings_view(core, &snapshot)?))
}

/// The real `settings.get`.
pub fn get(core: &DaemonCore) -> HandlerFuture<'_, Box<DaemonSettingsView>> {
    Box::pin(async move { current_settings(core).await })
}

/// The real `storage.list_locations`.
pub fn list_locations(core: &DaemonCore) -> HandlerFuture<'_, LocationList> {
    Box::pin(async move {
        let registry = registry_of(core)?;
        let snapshot = tokio::task::spawn_blocking(move || read_snapshot(&registry))
            .await
            .map_err(|_| unexpected("the locations task failed"))??;
        let items = snapshot
            .locations
            .iter()
            .map(|location| location_view(core, &snapshot.settings, location, &snapshot.usage))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LocationList { items })
    })
}

fn precheck_of(outcome: Result<SettingsCheck, SettingsError>) -> PrecheckOutcome {
    match outcome {
        Ok(SettingsCheck::Proceed) => PrecheckOutcome::Proceed,
        Ok(SettingsCheck::Unchanged(_)) => PrecheckOutcome::NoOp(NoOpReply::nothing()),
        Err(error) => PrecheckOutcome::Reject(Box::new(error)),
    }
}

fn input_of(text: String) -> JournalPayload {
    JournalPayload::new(text, OperationJournal::INPUT_MAX)
        .unwrap_or_else(|_| JournalPayload::empty())
}

/// The real `settings.update`.
pub fn update<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: UpdateSettingsParams,
) -> HandlerFuture<'a, Box<DaemonSettingsView>> {
    let context = context.clone();
    Box::pin(async move {
        let registry = registry_of(core)?;
        let patch = patch_of(&params)?;
        let expected = domain_revision(params.expected_revision)?;
        let fingerprint = request_fingerprint(&Call::SettingsUpdate(params));
        let input = input_of(
            serde_json::json!({
                "defaultStartup": patch.default_startup.map(StartupPolicy::as_code),
                "updateChannel": patch.update_channel.map(DomainChannel::as_code),
                "telemetry": patch.telemetry,
                "auditRetentionDays": patch.audit_retention.map(AuditRetentionDays::days),
            })
            .to_string(),
        );
        let check = {
            let registry = registry.clone();
            Box::new(move || precheck_of(update_precheck(&registry, &patch, expected)))
                as Box<dyn Precheck>
        };
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::UpdateDaemonSettings,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work: update_work(UpdateRequest::new(patch, expected)),
            precheck: Some(check),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "settings.update",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        finish(core, &outcome).await?;
        // A shorter retention drops older rows right away, not at the next
        // daily prune (spec 0035, AC-6): only after the commit landed.
        if patch.audit_retention.is_some() && matches!(outcome, StartOutcome::Accepted(_)) {
            core.wake_prune();
        }
        current_settings(core).await
    })
}

/// Waits for the operation of an accepted request, and announces the change
/// once, for the request that made it.
async fn finish(core: &DaemonCore, outcome: &StartOutcome) -> Result<(), Box<ErrorBody>> {
    let StartOutcome::Accepted(accepted) = outcome else {
        return Ok(());
    };
    wait_final(core, accepted.operation.id).await?;
    if !accepted.replayed {
        let settings = current_settings(core).await?;
        core.bus()
            .publish_daemon(EventKind::SettingsChanged(SettingsChanged {
                revision: settings.revision,
            }));
    }
    Ok(())
}

/// Waits until the operation is final; a failed one answers with its own
/// error.
async fn wait_final(core: &DaemonCore, operation: uuid::Uuid) -> Result<(), Box<ErrorBody>> {
    let engine = core.operations().writer()?;
    let id = OperationId::from_uuid(operation)
        .map_err(|_| unexpected("the operation ID is not a version 7 UUID"))?;
    loop {
        match engine.get(id).await {
            Ok(Some(detail)) if detail.view.operation.state().is_final() => {
                return match detail.view.journal.failure_error() {
                    Some(error) => Err(Box::new(failure_body(error, id))),
                    None => Ok(()),
                };
            }
            Ok(Some(_)) => {}
            Ok(None) => return Err(unexpected("the operation vanished")),
            Err(_) => return Err(unexpected("the operation cannot be read")),
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The real `storage.set_location`.
pub fn set_location<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: SetLocationParams,
) -> HandlerFuture<'a, Box<StorageLocationView>> {
    let context = context.clone();
    Box::pin(async move {
        let registry = registry_of(core)?;
        let kind = kind_of(params.kind).ok_or_else(|| {
            Box::new(
                ErrorBody::new(&request::INVALID_PARAMS, "kind is not a location kind")
                    .with_field(FieldError::new("kind", FieldCode::UnknownValue)),
            )
        })?;
        let expected = domain_revision(params.expected_revision)?;
        let target = match &params.target {
            LocationTarget::Path(path) => CoreTarget::Path(PathBuf::from(path)),
            LocationTarget::BuiltIn => CoreTarget::BuiltIn,
        };
        let fingerprint = request_fingerprint(&Call::StorageSetLocation(params.clone()));
        let input = input_of(
            serde_json::json!({
                "kind": kind.as_code(),
                "target": match &target {
                    CoreTarget::Path(path) => path.display().to_string(),
                    CoreTarget::BuiltIn => "built_in".to_owned(),
                },
            })
            .to_string(),
        );
        let avoid = bundle_roots();
        let work_request = SetLocationRequest::new(kind, target, expected)
            .with_home(core.home().map(Path::to_path_buf))
            .with_avoid(avoid);
        let check = {
            let registry = registry.clone();
            let work_request = work_request.clone();
            Box::new(move || precheck_of(set_location_precheck(&registry, &work_request)))
                as Box<dyn Precheck>
        };
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::SetStorageLocation,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work: set_location_work(work_request),
            precheck: Some(check),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "storage.set_location",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        finish(core, &outcome).await?;
        let registry = registry_of(core)?;
        let snapshot = tokio::task::spawn_blocking(move || read_snapshot(&registry))
            .await
            .map_err(|_| unexpected("the locations task failed"))??;
        let id = snapshot.settings.location_of(kind);
        let location = snapshot
            .locations
            .iter()
            .find(|location| location.id() == id)
            .ok_or_else(|| unexpected("a default location is not in the registry"))?;
        Ok(Box::new(location_view(
            core,
            &snapshot.settings,
            location,
            &snapshot.usage,
        )?))
    })
}

/// The app bundle this daemon runs from, which a location must not
/// overlap.
fn bundle_roots() -> Vec<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| stackly_platform::bundle::AppBundle::enclosing(&exe))
        .map(|bundle| vec![bundle.root().to_path_buf()])
        .unwrap_or_default()
}
