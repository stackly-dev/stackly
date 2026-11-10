//! `diagnostics.preview_bundle` and `diagnostics.export_bundle` (spec
//! 0034, AC-12, AC-13).
//!
//! A preview runs the doctor, takes every other entry from the same read
//! handlers a client calls (`daemon.get_info`, `daemon.get_health`,
//! `storage.get_capacity`, `instance.list`, `instance.get_detail`,
//! `runtime.list`, `operation.list`, `operation.get`), reads the agent
//! files and the logs, and hands them to `stackly_core::diagnostics`, which
//! redacts, masks the home folder, writes the zip, and stages it. An export
//! copies that exact zip and logs one `diagnostics_exported` line. Neither
//! is an operation (spec 0007) nor audited (spec 0035); a preview writes
//! only its staged zip, and nothing is sent over the network.

use std::sync::Arc;

use serde_json::{Value, json};
use stackly_core::diagnostics::{
    AgentFile, BundleEntryKind as CoreKind, BundleInput, ExportError, InstanceLogInput,
    StagedPreview, WriteReason, build, daemon_log, export, instance_log, suggested_file_name,
};
use stackly_domain::{InstanceId, OperationState, Registry as _};
use stackly_platform::PlatformError;
use stackly_protocol::codes::{diagnostics, internal};
use stackly_protocol::diagnostics::{
    BundleEntry, BundleEntryKind, BundlePreview, ExportBundleParams, ExportedBundle,
};
use stackly_protocol::doctor::DoctorParams;
use stackly_protocol::instance::{GetInstanceDetailParams, ListInstancesParams};
use stackly_protocol::operation::{GetOperationParams, ListOperationsParams, OperationSummary};
use stackly_protocol::runtime::ListRuntimesParams;
use stackly_protocol::{Count, ErrorBody, ProtocolVersion, Timestamp};
use uuid::Uuid;

use crate::dispatch::{HandlerFuture, Handlers, RequestContext};
use crate::state::{DaemonCore, RegistryState};

/// The most bytes of an agent file the bundle reads.
const AGENT_FILE_MAX: u64 = 4096;
/// The newest operations `operations.json` holds besides the ones that
/// need recovery.
const OPERATIONS_NEWEST: u32 = 100;

/// Builds and stages a bundle, then answers what it holds.
pub fn preview<'a, H: Handlers + ?Sized>(
    handlers: &'a H,
    core: &'a DaemonCore,
    context: &'a RequestContext,
) -> HandlerFuture<'a, BundlePreview> {
    Box::pin(async move {
        let created_at = core.platform().clock().now();
        let preview_id = Uuid::now_v7();
        let recovery = matches!(core.registry_state(), RegistryState::Recovery { .. });
        let protocol = format!(
            "{}.{}",
            ProtocolVersion::CURRENT.major,
            ProtocolVersion::CURRENT.minor
        );
        let info = handlers.get_info(core, context).await?;
        let mut input = BundleInput::new(created_at, info.version.clone(), protocol, recovery);
        let report = crate::ipc::doctor::run(core, DoctorParams::default()).await?;
        input.doctor = to_value(&report);
        input.system = system(handlers, core, context, &info, recovery).await;

        let mut instance_ids = Vec::new();
        if !recovery && core.registry().is_some() {
            let (instances, ids) = instances(handlers, core, context, &mut input.notes).await;
            input.instances = instances;
            instance_ids = ids;
            input.runtimes = runtimes(handlers, core, context, &mut input.notes).await;
            input.operations = operations(handlers, core, context, &mut input.notes).await;
        } else if recovery {
            input.notes.push(
                "logs/instances/ is left out: the registry is in recovery mode, so Stackly cannot name the instances."
                    .to_owned(),
            );
        }
        input.agent = agent_files(core);
        let gathered = gather_logs(core, instance_ids).await;
        input.daemon_log = gathered.daemon;
        input.instance_logs = gathered.instances;
        input.notes.extend(gathered.notes);

        let redactor = Arc::clone(core.redactor());
        let home = core.home_mask().clone();
        let bundle = tokio::task::spawn_blocking(move || build(input, &redactor, &home))
            .await
            .map_err(|_| unexpected("the bundle task failed"))?
            .map_err(|error| bundle_failed(WriteReason::Io, &error.to_string()))?;
        let staged = core
            .bundles()
            .stage(core.platform().fs(), preview_id, created_at, &bundle)
            .map_err(|error| bundle_failed(error.reason, &error.message))?;
        Ok(BundlePreview {
            preview_id,
            created_at: Timestamp::from_jiff(staged.created_at),
            expires_at: Timestamp::from_jiff(staged.expires_at),
            suggested_file_name: suggested_file_name(created_at),
            entries: bundle
                .entries
                .iter()
                .map(|entry| BundleEntry {
                    path: entry.path.clone(),
                    kind: kind(entry.kind),
                    description: entry.description.clone(),
                    bytes: Count::saturating(entry.bytes),
                    lines: entry.lines.map(Count::saturating),
                    truncated: entry.truncated,
                })
                .collect(),
            total_bytes: Count::saturating(bundle.total_bytes),
            zip_bytes: Count::saturating(staged.bytes),
            recovery,
        })
    })
}

/// Copies the staged preview to the destination the client chose.
pub fn export_bundle(
    core: &DaemonCore,
    params: ExportBundleParams,
) -> HandlerFuture<'_, ExportedBundle> {
    Box::pin(async move {
        let now = core.platform().clock().now();
        let preview: Arc<StagedPreview> = core
            .bundles()
            .get(core.platform().fs(), params.preview_id, now)
            .ok_or_else(|| {
                let mut body = ErrorBody::new(
                    &diagnostics::PREVIEW_NOT_FOUND,
                    "the bundle preview is not staged anymore",
                )
                .with_detail("previewId", params.preview_id.to_string());
                if let Some(hint) = diagnostics::hint(diagnostics::PREVIEW_NOT_FOUND.code) {
                    body = body.with_hint(hint);
                }
                Box::new(body)
            })?;
        let platform = core.platform().clone();
        let destination = params.destination.clone();
        let replace = params.replace;
        let staged = Arc::clone(&preview);
        let exported = tokio::task::spawn_blocking(move || {
            export(platform.fs(), &staged, &destination, replace)
        })
        .await
        .map_err(|_| unexpected("the export task failed"))?
        .map_err(|error| export_error(core, &error))?;
        let shown = core
            .home_mask()
            .mask(&exported.path.display().to_string())
            .into_owned();
        tracing::info!(
            path = %shown,
            bytes = exported.bytes,
            sha256 = %exported.sha256,
            "diagnostics_exported",
        );
        Ok(ExportedBundle {
            path: exported.path.display().to_string(),
            bytes: Count::saturating(exported.bytes),
            sha256: exported.sha256,
        })
    })
}

/// `system.json`: what the daemon knows about itself and its host.
async fn system<H: Handlers + ?Sized>(
    handlers: &H,
    core: &DaemonCore,
    context: &RequestContext,
    info: &stackly_protocol::daemon::DaemonInfo,
    recovery: bool,
) -> Value {
    let health = handlers.get_health(core, context).await;
    let capacity = handlers.get_capacity(core, context).await;
    let host = core.platform().info().host_facts();
    let roots = core.prepared().layout().roots();
    json!({
        "stacklyVersion": info.version,
        "protocol": {
            "major": ProtocolVersion::CURRENT.major,
            "minor": ProtocolVersion::CURRENT.minor,
        },
        "daemon": to_value(info),
        "health": result_value(health),
        "recovery": recovery,
        "host": {
            "target": host.target.to_string(),
            "osVersion": host.os_version.to_string(),
            "osBuild": host.os_build,
        },
        "capacity": result_value(capacity),
        "roots": {
            "data": roots.data().display().to_string(),
            "logs": roots.logs().display().to_string(),
            "cache": roots.cache().display().to_string(),
            "runtime": roots.runtime().display().to_string(),
        },
    })
}

/// `instances.json`, through `instance.list` and `instance.get_detail`,
/// and the instances whose logs the bundle takes.
async fn instances<H: Handlers + ?Sized>(
    handlers: &H,
    core: &DaemonCore,
    context: &RequestContext,
    notes: &mut Vec<String>,
) -> (Option<Value>, Vec<(InstanceId, String)>) {
    let mut summaries = Vec::new();
    let mut before = None;
    loop {
        let page = handlers
            .list_instances(
                core,
                context,
                ListInstancesParams {
                    limit: None,
                    before: before.clone(),
                },
            )
            .await;
        match page {
            Ok(page) => {
                summaries.extend(page.items);
                match page.next {
                    Some(next) if before.as_ref() != Some(&next) => before = Some(next),
                    _ => break,
                }
            }
            Err(error) => {
                notes.push(format!(
                    "instances.json is left out: instance.list answered {}.",
                    error.code()
                ));
                return (None, Vec::new());
            }
        }
    }
    let mut items = Vec::with_capacity(summaries.len());
    let mut ids = Vec::with_capacity(summaries.len());
    for summary in summaries {
        let detail = handlers
            .get_instance_detail(
                core,
                context,
                GetInstanceDetailParams {
                    instance_id: summary.id,
                },
            )
            .await;
        if let Ok(id) = InstanceId::from_uuid(summary.id) {
            ids.push((id, summary.name.clone()));
        }
        items.push(json!({
            "summary": to_value(&summary),
            "detail": result_value(detail),
        }));
    }
    (Some(Value::Array(items)), ids)
}

/// `runtimes.json`, through `runtime.list`.
async fn runtimes<H: Handlers + ?Sized>(
    handlers: &H,
    core: &DaemonCore,
    context: &RequestContext,
    notes: &mut Vec<String>,
) -> Option<Value> {
    let mut items = Vec::new();
    let mut after = None;
    loop {
        let page = handlers
            .list_runtimes(core, context, ListRuntimesParams { limit: None, after })
            .await;
        match page {
            Ok(page) => {
                items.extend(page.items.iter().map(to_value));
                match page.next {
                    Some(next) if after != Some(next) => after = Some(next),
                    _ => break,
                }
            }
            Err(error) => {
                notes.push(format!(
                    "runtimes.json is left out: runtime.list answered {}.",
                    error.code()
                ));
                return None;
            }
        }
    }
    Some(Value::Array(items))
}

/// `operations.json`: the 100 newest operations and every one that needs
/// recovery, without a duplicate, newest first by `(createdAt, id)`.
async fn operations<H: Handlers + ?Sized>(
    handlers: &H,
    core: &DaemonCore,
    context: &RequestContext,
    notes: &mut Vec<String>,
) -> Option<Value> {
    let mut items: Vec<OperationSummary> = Vec::new();
    let mut before = None;
    while items.len() < OPERATIONS_NEWEST as usize {
        let page = handlers
            .list_operations(
                core,
                context,
                ListOperationsParams {
                    active_only: false,
                    resource: None,
                    limit: Some(OPERATIONS_NEWEST - u32::try_from(items.len()).unwrap_or(0)),
                    before: before.clone(),
                },
            )
            .await;
        match page {
            Ok(page) => {
                let read = page.items.len();
                items.extend(page.items);
                match page.next {
                    Some(next) if read > 0 && before.as_ref() != Some(&next) => {
                        before = Some(next);
                    }
                    _ => break,
                }
            }
            Err(error) => {
                notes.push(format!(
                    "operations.json is left out: operation.list answered {}.",
                    error.code()
                ));
                return None;
            }
        }
    }
    items.truncate(OPERATIONS_NEWEST as usize);
    for id in needing_recovery(core) {
        if items.iter().any(|item| item.id == id) {
            continue;
        }
        if let Ok(detail) = handlers
            .get_operation(core, context, GetOperationParams { operation_id: id })
            .await
        {
            items.push(detail.operation);
        }
    }
    items.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    Some(Value::Array(items.iter().map(to_value).collect()))
}

/// The IDs of the operations that need recovery, from the registry the
/// daemon holds; none when it cannot be read.
fn needing_recovery(core: &DaemonCore) -> Vec<Uuid> {
    let Some(registry) = core.registry() else {
        return Vec::new();
    };
    registry
        .unfinished_operations()
        .map(|versioned| {
            versioned
                .value
                .into_iter()
                .filter(|(operation, _)| operation.state() == OperationState::NeedsRecovery)
                .map(|(operation, _)| operation.id().as_uuid())
                .collect()
        })
        .unwrap_or_default()
}

/// The launch record and the start failure record, when they are regular
/// files the bundle can read.
fn agent_files(core: &DaemonCore) -> Vec<AgentFile> {
    let fs = core.platform().fs();
    let layout = core.prepared().layout();
    [
        layout.agent_launch_record(),
        layout.daemon_start_failure_record(),
    ]
    .into_iter()
    .filter_map(|path| {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let bytes = fs.read_file_limited(&path, AGENT_FILE_MAX).ok()?;
        Some(AgentFile::new(
            name,
            String::from_utf8_lossy(&bytes).into_owned(),
        ))
    })
    .collect()
}

/// The logs a bundle takes.
struct GatheredLogs {
    daemon: Option<stackly_core::diagnostics::LogText>,
    instances: Vec<InstanceLogInput>,
    notes: Vec<String>,
}

/// Reads the daemon log and the log of each instance on the blocking pool.
async fn gather_logs(core: &DaemonCore, instances: Vec<(InstanceId, String)>) -> GatheredLogs {
    let platform = core.platform().clone();
    let daemon_dir = core.prepared().layout().daemon_log_dir();
    let store = core
        .logs()
        .map(|logs| (logs.log().config().segments_per_stream, logs.log().clone()));
    let read = tokio::task::spawn_blocking(move || {
        let mut notes = Vec::new();
        let daemon = match daemon_log(platform.fs(), &daemon_dir) {
            Ok(log) => Some(log),
            Err(error) => {
                notes.push(format!(
                    "logs/stacklyd.log is left out: the stacklyd log cannot be read ({}).",
                    reason_of(&error)
                ));
                None
            }
        };
        let mut logs = Vec::new();
        match &store {
            Some((segments, store)) => {
                for (id, name) in instances {
                    match instance_log(platform.fs(), &store.dir(id), *segments) {
                        Ok((log, newest)) if !log.lines.is_empty() => {
                            logs.push(InstanceLogInput::new(name, newest, log));
                        }
                        Ok(_) => {}
                        Err(error) => notes.push(format!(
                            "logs/instances/{name}.log is left out: its log store cannot be read ({error})."
                        )),
                    }
                }
            }
            None if !instances.is_empty() => {
                notes.push("Instance logs are left out: the log store is not open.".to_owned());
            }
            None => {}
        }
        GatheredLogs {
            daemon,
            instances: logs,
            notes,
        }
    })
    .await;
    read.unwrap_or_else(|_| GatheredLogs {
        daemon: None,
        instances: Vec::new(),
        notes: vec!["The logs are left out: their read failed.".to_owned()],
    })
}

/// The short reason of a filesystem error for `README.txt`.
fn reason_of(error: &PlatformError) -> &'static str {
    WriteReason::of(error).as_code()
}

/// A DTO as JSON; one that does not serialize becomes null.
fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A handler result as JSON: the reply, or the code and message of its
/// error.
fn result_value<T: serde::Serialize>(result: Result<T, Box<ErrorBody>>) -> Value {
    match result {
        Ok(value) => to_value(&value),
        Err(error) => json!({ "error": { "code": error.code(), "message": error.message() } }),
    }
}

/// The wire kind of an entry.
const fn kind(kind: CoreKind) -> BundleEntryKind {
    match kind {
        CoreKind::Readme => BundleEntryKind::Readme,
        CoreKind::Doctor => BundleEntryKind::Doctor,
        CoreKind::System => BundleEntryKind::System,
        CoreKind::Instances => BundleEntryKind::Instances,
        CoreKind::Runtimes => BundleEntryKind::Runtimes,
        CoreKind::Operations => BundleEntryKind::Operations,
        CoreKind::DaemonLog => BundleEntryKind::DaemonLog,
        CoreKind::InstanceLog => BundleEntryKind::InstanceLog,
        CoreKind::Agent => BundleEntryKind::Agent,
        _ => BundleEntryKind::Unrecognized,
    }
}

fn bundle_failed(reason: WriteReason, message: &str) -> Box<ErrorBody> {
    tracing::warn!(
        reason = reason.as_code(),
        error = message,
        "cannot build the diagnostic bundle"
    );
    Box::new(
        ErrorBody::new(
            &diagnostics::BUNDLE_FAILED,
            format!(
                "the diagnostic bundle cannot be prepared ({})",
                reason.as_code()
            ),
        )
        .with_detail("reason", reason.as_code()),
    )
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&internal::UNEXPECTED, message))
}

/// The error body of a failed export, every path with `~`.
fn export_error(core: &DaemonCore, error: &ExportError) -> Box<ErrorBody> {
    let home = core.home_mask();
    let shown = |path: &std::path::Path| home.mask(&path.display().to_string()).into_owned();
    let body = match error {
        ExportError::Invalid { reason, path } => ErrorBody::new(
            &diagnostics::DESTINATION_INVALID,
            format!(
                "the destination {} is refused ({})",
                shown(path),
                reason.as_code()
            ),
        )
        .with_detail("reason", reason.as_code())
        .with_detail("path", shown(path)),
        ExportError::Exists { path } => {
            let body = ErrorBody::new(
                &diagnostics::DESTINATION_EXISTS,
                format!("{} already exists", shown(path)),
            )
            .with_detail("path", shown(path));
            match diagnostics::hint(diagnostics::DESTINATION_EXISTS.code) {
                Some(hint) => body.with_hint(hint),
                None => body,
            }
        }
        ExportError::Write { reason, path, .. } => ErrorBody::new(
            &diagnostics::WRITE_FAILED,
            format!(
                "the bundle cannot be written to {} ({})",
                shown(path),
                reason.as_code()
            ),
        )
        .with_detail("reason", reason.as_code())
        .with_detail("path", shown(path)),
        _ => ErrorBody::new(&internal::UNEXPECTED, "the export failed"),
    };
    Box::new(body)
}
