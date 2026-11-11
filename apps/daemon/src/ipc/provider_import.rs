//! The import methods of local providers (spec 0037, *API surface*):
//! `provider.preview_import`, `provider.import`, `provider.approve`,
//! `provider.revoke`, and `provider.remove`.
//!
//! A preview reads the folder through the platform filesystem and the
//! quarantine port only; it writes nothing, runs nothing, and is not an
//! operation. `provider.import` and `provider.remove` are operations on the
//! manager resource with `LockKey::Provider` (spec 0007, *Operasi
//! provider*): `replay_operation`, then the precheck, then
//! `start_operation`. Their work replaces the catalog snapshot and sends
//! `catalog.changed` before the operation finishes, so the reply reads the
//! new snapshot.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use stackly_core::catalog::import::{ImportRequest, import_work, remove_work};
use stackly_core::catalog::local::{ImportCheck, Permissions, import_limit, permission_changes};
use stackly_core::catalog::{Catalog, CatalogEntry};
use stackly_core::operation::{BeginRequest, BusyPolicy, NoOpReply, Origin, PrecheckOutcome};
use stackly_core::provider::{NameCheck, check_package};
use stackly_domain::{
    JournalPayload, LockKey, OperationId, OperationJournal, OperationKind, OperationResource,
    ProviderImportRecord, ProviderName, Registry,
};
use stackly_platform::{FileKind, PlatformError};
use stackly_protocol::catalog::ProviderDetail;
use stackly_protocol::codes::{internal, request};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::message::Call;
use stackly_protocol::provider::{
    DecideProviderParams, ExistingImport, ImportBlockReason, ImportPreview, ImportProviderParams,
    ImportVerdict, PREVIEW_FINDINGS_MAX, PREVIEW_IMPORT_ISSUES_MAX, PREVIEW_VERSIONS_MAX,
    PreviewImportParams, ProviderTargetParams, RemovedProvider,
};
use stackly_protocol::{Count, ErrorBody};
use stackly_storage::SqliteRegistry;

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::dto::failure_body;
use crate::ipc::operation::{StartOutcome, StartRequest as OperationStart, start_operation};
use crate::map;
use crate::state::DaemonCore;

/// How often a handler looks at the operation it waits for.
const POLL: Duration = Duration::from_millis(20);

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

/// What reading a package folder found, before any rule of an import.
struct Read {
    check: ImportCheck,
    file_count: u64,
    quarantined: Option<bool>,
    quarantine_agent: Option<String>,
}

/// Reads `path` as a package folder: the folder must exist, be a folder,
/// and not be a symlink, else `provider.import_unreadable`. The package
/// itself may be invalid; the check says so.
async fn read_folder(
    core: &DaemonCore,
    catalog: &Arc<Catalog>,
    path: &Path,
) -> Result<Read, Box<ErrorBody>> {
    let masked = core.home_mask().mask(&path.to_string_lossy()).into_owned();
    let platform = core.platform().clone();
    let options = core.catalog_options();
    let catalog = Arc::clone(catalog);
    let dir = path.to_path_buf();
    let read = tokio::task::spawn_blocking(move || -> Result<Read, &'static str> {
        match platform.fs().symlink_metadata(&dir) {
            Ok(None) | Err(PlatformError::NotFound { .. }) => return Err("not_found"),
            Ok(Some(info)) if info.kind == FileKind::Symlink => return Err("symlink"),
            Ok(Some(info)) if info.kind != FileKind::Dir => return Err("not_a_directory"),
            Ok(Some(_)) => {}
            Err(PlatformError::PermissionDenied { .. }) => return Err("permission_denied"),
            Err(_) => return Err("io"),
        }
        match platform.fs().read_dir(&dir) {
            Ok(_) => {}
            Err(PlatformError::PermissionDenied { .. }) => return Err("permission_denied"),
            Err(_) => return Err("io"),
        }
        let raw = check_package(platform.fs(), &dir, NameCheck::Skip);
        let file_count = raw
            .package
            .as_ref()
            .map_or(0, |package| package.files().iter().count() as u64);
        let check = ImportCheck::new(raw, options, Some(&catalog));
        let (quarantined, quarantine_agent) = match platform
            .code_signing()
            .quarantine(&dir.join("provider.yaml"))
        {
            Ok(Some(info)) => (Some(true), info.agent),
            Ok(None) => (Some(false), None),
            Err(_) => (None, None),
        };
        Ok(Read {
            check,
            file_count,
            quarantined,
            quarantine_agent,
        })
    })
    .await
    .map_err(|_| unexpected("the package read stopped"))?;
    read.map_err(|reason| Box::new(map::trust::import_unreadable(&masked, reason)))
}

/// Whether the snapshot holds a valid local entry for `provider` built from
/// the bytes of `record`.
fn stored_copy_valid(catalog: &Catalog, record: &ProviderImportRecord) -> bool {
    matches!(
        catalog.entry(&record.provider),
        Some(CatalogEntry::Provider(entry))
            if entry.local().is_some_and(|info| info.record.package_sha256 == record.package_sha256)
    )
}

/// The preview of a folder: what [`read_folder`] found, against the stored
/// import rows.
fn preview_of(
    read: &Read,
    rows: &[ProviderImportRecord],
    catalog: &Catalog,
    origin_path: String,
) -> ImportPreview {
    let check = &read.check;
    let package = check.package();
    let digest = check.package_sha256;
    let mut versions: Vec<String> = Vec::new();
    if let Some(package) = package {
        for artifact in package.versions().artifacts() {
            let version = artifact.version.to_string();
            if !versions.contains(&version) {
                versions.push(version);
            }
        }
    }
    let version_count = versions.len() as u64;
    versions.truncate(PREVIEW_VERSIONS_MAX);
    let provider = package.map(|package| package.key().provider().clone());
    let stored = provider
        .as_ref()
        .and_then(|provider| rows.iter().find(|row| &row.provider == provider));
    let limit = provider
        .as_ref()
        .and_then(|provider| import_limit(rows, provider, check.package_bytes));
    let verdict = if !check.valid() {
        ImportVerdict {
            importable: false,
            reason: Some(ImportBlockReason::PackageInvalid),
        }
    } else if limit.is_some() {
        ImportVerdict {
            importable: false,
            reason: Some(ImportBlockReason::ImportLimit),
        }
    } else {
        ImportVerdict {
            importable: true,
            reason: None,
        }
    };
    let changes = match (
        stored.and_then(|row| row.decision.as_ref()),
        &check.permissions,
        package,
    ) {
        (Some(decision), Some(current), Some(package))
            if Some(decision.package_sha256) != digest =>
        {
            Permissions::from_json(&decision.permissions_json)
                .map(|decided| {
                    permission_changes(
                        &decided,
                        &decision.provider_revision,
                        current,
                        package.key().revision(),
                    )
                    .into_iter()
                    .map(map::trust::change)
                    .collect()
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let findings: Vec<_> = check
        .findings()
        .iter()
        .take(PREVIEW_FINDINGS_MAX)
        .map(crate::ipc::provider::finding)
        .collect();
    ImportPreview {
        provider: provider.as_ref().map(ToString::to_string),
        display_name: package.map(|package| package.manifest().display_name().to_owned()),
        software: package.map(|package| package.software().to_string()),
        category: package.map(|package| {
            map::catalog::wire(stackly_core::catalog::category_code(
                package.manifest().category(),
            ))
        }),
        description: package
            .and_then(|package| package.manifest().description().map(str::to_owned)),
        provider_revision: package.map(|package| package.key().revision().to_string()),
        publisher: provider
            .as_ref()
            .map(|provider| provider.namespace().to_owned()),
        manifest_sha256: package.map(|package| package.key().manifest_sha256().to_string()),
        versions_sha256: package
            .and_then(|package| stackly_core::catalog::versions_sha256(package.files())),
        package_sha256: digest.map(|digest| digest.to_string()),
        package_bytes: Count::saturating(check.package_bytes),
        file_count: Count::saturating(read.file_count),
        origin_path,
        quarantined: read.quarantined,
        quarantine_agent: read.quarantine_agent.clone(),
        versions,
        version_count: Count::saturating(version_count),
        permissions: check.permissions.as_ref().map(map::trust::permissions),
        omitted_findings: Count::saturating((check.findings().len() - findings.len()) as u64),
        findings,
        import_issues: check
            .import_issues
            .iter()
            .take(PREVIEW_IMPORT_ISSUES_MAX)
            .map(map::catalog::catalog_issue)
            .collect(),
        verdict,
        existing: stored.map(|row| ExistingImport {
            provider_revision: row.provider_revision.to_string(),
            package_sha256: row.package_sha256.to_string(),
            approval: map::trust::approval(row.approval()),
            same_package: Some(row.package_sha256) == digest,
            stored_copy_valid: stored_copy_valid(catalog, row),
        }),
        changes,
        revocation: package
            .and_then(|package| stackly_core::catalog::catalog_revocation(catalog, package.key()))
            .as_ref()
            .map(map::trust::revocation),
    }
}

async fn import_rows(
    registry: &SqliteRegistry,
) -> Result<Vec<ProviderImportRecord>, Box<ErrorBody>> {
    let registry = registry.clone();
    tokio::task::spawn_blocking(move || registry.provider_imports())
        .await
        .map_err(|_| unexpected("the registry read stopped"))?
        .map(|rows| rows.value)
        .map_err(|error| Box::new(map::registry_error(&error)))
}

/// `provider.preview_import`.
pub fn preview<'a>(
    core: &'a DaemonCore,
    params: PreviewImportParams,
) -> HandlerFuture<'a, Box<ImportPreview>> {
    Box::pin(async move {
        let catalog = crate::catalog::loaded(core)?;
        let registry = registry_of(core)?;
        let path = PathBuf::from(&params.path);
        let read = read_folder(core, &catalog, &path).await?;
        let rows = import_rows(&registry).await?;
        let origin = core.home_mask().mask(&params.path).into_owned();
        Ok(Box::new(preview_of(&read, &rows, &catalog, origin)))
    })
}

/// The detail of `provider` from the snapshot of now.
fn detail_of(
    core: &DaemonCore,
    provider: &ProviderName,
) -> Result<Box<ProviderDetail>, Box<ErrorBody>> {
    let catalog = crate::catalog::loaded(core)?;
    let entry = catalog.entry(provider).ok_or_else(|| {
        Box::new(map::trust::import_not_found(
            provider.as_str(),
            "not_imported",
        ))
    })?;
    Ok(Box::new(map::catalog::provider_detail(
        &catalog,
        entry,
        &core.platform().info().host_facts(),
    )))
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

fn input_of(value: &serde_json::Value) -> JournalPayload {
    JournalPayload::new(value.to_string(), OperationJournal::INPUT_MAX)
        .unwrap_or_else(|_| JournalPayload::empty())
}

/// `provider.import`.
pub fn import<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ImportProviderParams,
) -> HandlerFuture<'a, Box<ProviderDetail>> {
    let inner = import_inner(core, context, params);
    Box::pin(async move {
        inner
            .await
            .map_err(|error| Box::new(map::trust::with_area_hint(*error)))
    })
}

fn import_inner<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ImportProviderParams,
) -> HandlerFuture<'a, Box<ProviderDetail>> {
    let context = context.clone();
    Box::pin(async move {
        let catalog = crate::catalog::loaded(core)?;
        let registry = registry_of(core)?;
        let refresher = core
            .catalog_refresher()
            .ok_or_else(|| unexpected("the catalog refresher is not set"))?;
        let fingerprint = request_fingerprint(&Call::ProviderImport(params.clone()));
        let path = PathBuf::from(&params.path);
        let read = read_folder(core, &catalog, &path).await?;
        let check = &read.check;
        let (Some(package), Some(digest)) = (check.package(), check.package_sha256) else {
            let issues: Vec<_> = check
                .findings()
                .iter()
                .map(|finding| finding.issue.clone())
                .collect();
            return Err(Box::new(map::package_issues_error(
                &check.check.folder,
                &issues,
            )));
        };
        let provider = package.key().provider().clone();
        if !check.valid() {
            let issues: Vec<_> = check
                .findings()
                .iter()
                .filter(|finding| finding.severity == stackly_core::provider::Severity::Error)
                .map(|finding| finding.issue.clone())
                .chain(check.import_issues.iter().cloned())
                .collect();
            return Err(Box::new(map::package_issues_error(
                &check.check.folder,
                &issues,
            )));
        }
        if digest.to_string() != params.expected_package_sha256 {
            return Err(Box::new(map::trust::import_changed(
                provider.as_str(),
                &params.expected_package_sha256,
                &digest.to_string(),
            )));
        }
        let expected = digest;
        let origin_path = core.home_mask().mask(&params.path).into_owned();
        let input = input_of(&serde_json::json!({
            "provider": provider.as_str(),
            "path": origin_path,
            "packageSha256": digest.to_string(),
        }));
        let check_registry = registry.clone();
        let check_catalog = Arc::clone(&catalog);
        let check_provider = provider.clone();
        let bytes = check.package_bytes;
        let precheck = Box::new(move || -> PrecheckOutcome {
            let rows = match check_registry.provider_imports() {
                Ok(rows) => rows.value,
                Err(error) => {
                    return PrecheckOutcome::Reject(Box::new(
                        stackly_core::catalog::import::ProviderOpError::Registry(error),
                    ));
                }
            };
            if let Some(row) = rows.iter().find(|row| row.provider == check_provider)
                && row.package_sha256 == expected
                && stored_copy_valid(&check_catalog, row)
            {
                return PrecheckOutcome::NoOp(NoOpReply::nothing());
            }
            if let Some(limit) = import_limit(&rows, &check_provider, bytes) {
                return PrecheckOutcome::Reject(Box::new(
                    stackly_core::catalog::import::ProviderOpError::Limit(limit),
                ));
            }
            PrecheckOutcome::Proceed
        }) as Box<dyn stackly_core::operation::Precheck>;
        let work_request = {
            let mut request = ImportRequest::new(provider.clone(), path, expected, origin_path);
            request.quarantined = read.quarantined;
            request.quarantine_agent = read.quarantine_agent.clone();
            request.options = core.catalog_options();
            request.catalog = Some(Arc::clone(&catalog));
            request
        };
        let lock = LockKey::Provider(provider.clone());
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::ImportProvider,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: vec![lock],
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work: import_work(work_request, refresher),
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "provider.import",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        if let StartOutcome::Accepted(accepted) = &outcome {
            wait_final(core, accepted.operation.id).await?;
        }
        detail_of(core, &provider)
    })
}

/// The local entry `provider` of the snapshot, or the error a decision on
/// it answers: `not_local` for a catalog entry, `not_imported` for none.
fn local_entry<'c>(
    catalog: &'c Catalog,
    provider: &ProviderName,
    registry_has_row: bool,
) -> Result<Option<&'c CatalogEntry>, Box<ErrorBody>> {
    match catalog.entry(provider) {
        Some(entry) if !entry.is_local() => Err(Box::new(map::trust::import_not_found(
            provider.as_str(),
            "not_local",
        ))),
        Some(entry) => Ok(Some(entry)),
        None if registry_has_row => Ok(None),
        None => Err(Box::new(map::trust::import_not_found(
            provider.as_str(),
            "not_imported",
        ))),
    }
}

fn provider_param(text: &str) -> Result<ProviderName, Box<ErrorBody>> {
    text.parse().map_err(|_| {
        Box::new(ErrorBody::new(
            &request::INVALID_PARAMS,
            "provider is not a provider ID",
        ))
    })
}

async fn import_row(
    registry: &SqliteRegistry,
    provider: &ProviderName,
) -> Result<Option<ProviderImportRecord>, Box<ErrorBody>> {
    let registry = registry.clone();
    let provider = provider.clone();
    tokio::task::spawn_blocking(move || registry.provider_import(&provider))
        .await
        .map_err(|_| unexpected("the registry read stopped"))?
        .map(|row| row.value)
        .map_err(|error| Box::new(map::registry_error(&error)))
}

/// Starts a provider decision operation and waits for it.
async fn run_decision(
    core: &DaemonCore,
    context: &RequestContext,
    start: OperationStart<'_>,
    kind: OperationKind,
    provider: &ProviderName,
    precheck: Box<dyn stackly_core::operation::Precheck>,
    work: Box<dyn stackly_core::operation::OperationWork>,
) -> Result<(), Box<ErrorBody>> {
    let input = input_of(&serde_json::json!({ "provider": provider.as_str() }));
    let lock = LockKey::Provider(provider.clone());
    let request = move |origin: Origin| BeginRequest {
        kind,
        resource: OperationResource::Manager,
        creates_resource: false,
        extra_locks: vec![lock],
        busy: BusyPolicy::Queue,
        expected_revision: None,
        origin,
        input,
        work,
        precheck: Some(precheck),
    };
    let outcome = start_operation(core, context, start, request).await?;
    if let StartOutcome::Accepted(accepted) = &outcome {
        wait_final(core, accepted.operation.id).await?;
    }
    Ok(())
}

/// `provider.approve`.
pub fn approve<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: DecideProviderParams,
) -> HandlerFuture<'a, Box<ProviderDetail>> {
    let context = context.clone();
    Box::pin(async move {
        let result = async {
            let catalog = crate::catalog::loaded(core)?;
            let registry = registry_of(core)?;
            let refresher = core
                .catalog_refresher()
                .ok_or_else(|| unexpected("the catalog refresher is not set"))?;
            let fingerprint = request_fingerprint(&Call::ProviderApprove(params.clone()));
            let provider = provider_param(&params.provider)?;
            let expected: stackly_domain::Sha256Digest =
                params.expected_package_sha256.parse().map_err(|_| {
                    Box::new(ErrorBody::new(
                        &request::INVALID_PARAMS,
                        "expectedPackageSha256 is not a digest",
                    ))
                })?;
            let row = import_row(&registry, &provider).await?;
            let entry = local_entry(&catalog, &provider, row.is_some())?;
            let invalid = matches!(entry, Some(CatalogEntry::Invalid(_)));
            let check_provider = provider.clone();
            let precheck = Box::new(move || -> PrecheckOutcome {
                use stackly_core::catalog::import::ProviderOpError;
                let Some(row) = &row else {
                    return PrecheckOutcome::Reject(Box::new(ProviderOpError::NotFound {
                        provider: check_provider.to_string(),
                        reason: "not_imported",
                    }));
                };
                if row.package_sha256 != expected {
                    return PrecheckOutcome::Reject(Box::new(ProviderOpError::Changed {
                        provider: check_provider.to_string(),
                        expected: expected.to_string(),
                        actual: row.package_sha256.to_string(),
                    }));
                }
                if invalid {
                    return PrecheckOutcome::Reject(Box::new(ProviderOpError::PackageInvalid {
                        provider: check_provider.to_string(),
                    }));
                }
                if row.approval() == stackly_domain::ApprovalState::Approved {
                    return PrecheckOutcome::NoOp(NoOpReply::nothing());
                }
                PrecheckOutcome::Proceed
            }) as Box<dyn stackly_core::operation::Precheck>;
            let work = stackly_core::catalog::import::approve_work(
                provider.clone(),
                expected,
                Some(Arc::clone(&catalog)),
                refresher,
            );
            run_decision(
                core,
                &context,
                OperationStart::new(
                    "provider.approve",
                    context.idempotency_key.as_deref(),
                    fingerprint,
                ),
                OperationKind::ApproveProvider,
                &provider,
                precheck,
                work,
            )
            .await?;
            detail_of(core, &provider)
        }
        .await;
        result.map_err(|error| Box::new(map::trust::with_area_hint(*error)))
    })
}

/// `provider.revoke`.
pub fn revoke<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ProviderTargetParams,
) -> HandlerFuture<'a, Box<ProviderDetail>> {
    let context = context.clone();
    Box::pin(async move {
        let result = async {
            let catalog = crate::catalog::loaded(core)?;
            let registry = registry_of(core)?;
            let refresher = core
                .catalog_refresher()
                .ok_or_else(|| unexpected("the catalog refresher is not set"))?;
            let fingerprint = request_fingerprint(&Call::ProviderRevoke(params.clone()));
            let provider = provider_param(&params.provider)?;
            let row = import_row(&registry, &provider).await?;
            local_entry(&catalog, &provider, row.is_some())?;
            let check_registry = registry.clone();
            let check_provider = provider.clone();
            let precheck = Box::new(move || -> PrecheckOutcome {
                use stackly_core::catalog::import::ProviderOpError;
                let Some(row) = &row else {
                    return PrecheckOutcome::Reject(Box::new(ProviderOpError::NotFound {
                        provider: check_provider.to_string(),
                        reason: "not_imported",
                    }));
                };
                let approved = row.decision.as_ref().is_some_and(|decided| {
                    decided.decision == stackly_domain::TrustDecision::Approved
                });
                let pinned = match check_registry.provider_has_approved_revision(&check_provider) {
                    Ok(found) => found.value,
                    Err(error) => {
                        return PrecheckOutcome::Reject(Box::new(ProviderOpError::Registry(error)));
                    }
                };
                if !approved && !pinned {
                    return PrecheckOutcome::Reject(Box::new(ProviderOpError::NotApproved {
                        provider: check_provider.to_string(),
                        approval: row.approval().as_code(),
                    }));
                }
                PrecheckOutcome::Proceed
            }) as Box<dyn stackly_core::operation::Precheck>;
            let work = stackly_core::catalog::import::revoke_work(provider.clone(), refresher);
            run_decision(
                core,
                &context,
                OperationStart::new(
                    "provider.revoke",
                    context.idempotency_key.as_deref(),
                    fingerprint,
                ),
                OperationKind::RevokeProvider,
                &provider,
                precheck,
                work,
            )
            .await?;
            detail_of(core, &provider)
        }
        .await;
        result.map_err(|error| Box::new(map::trust::with_area_hint(*error)))
    })
}

/// `provider.remove`.
pub fn remove<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ProviderTargetParams,
) -> HandlerFuture<'a, RemovedProvider> {
    let inner = remove_inner(core, context, params);
    Box::pin(async move {
        inner
            .await
            .map_err(|error| Box::new(map::trust::with_area_hint(*error)))
    })
}

fn remove_inner<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ProviderTargetParams,
) -> HandlerFuture<'a, RemovedProvider> {
    let context = context.clone();
    Box::pin(async move {
        let catalog = crate::catalog::loaded(core)?;
        let registry = registry_of(core)?;
        let refresher = core
            .catalog_refresher()
            .ok_or_else(|| unexpected("the catalog refresher is not set"))?;
        let fingerprint = request_fingerprint(&Call::ProviderRemove(params.clone()));
        let provider: ProviderName = params.provider.parse().map_err(|_| {
            Box::new(ErrorBody::new(
                &request::INVALID_PARAMS,
                "provider is not a provider ID",
            ))
        })?;
        let not_local = catalog
            .entry(&provider)
            .is_some_and(|entry| !entry.is_local());
        let check_registry = registry.clone();
        let check_provider = provider.clone();
        let precheck = Box::new(move || -> PrecheckOutcome {
            if not_local {
                return PrecheckOutcome::Reject(Box::new(
                    stackly_core::catalog::import::ProviderOpError::NotFound {
                        provider: check_provider.to_string(),
                        reason: "not_local",
                    },
                ));
            }
            match check_registry.provider_import(&check_provider) {
                Ok(found) if found.value.is_some() => PrecheckOutcome::Proceed,
                Ok(_) => PrecheckOutcome::Reject(Box::new(
                    stackly_core::catalog::import::ProviderOpError::NotFound {
                        provider: check_provider.to_string(),
                        reason: "not_imported",
                    },
                )),
                Err(error) => PrecheckOutcome::Reject(Box::new(
                    stackly_core::catalog::import::ProviderOpError::Registry(error),
                )),
            }
        }) as Box<dyn stackly_core::operation::Precheck>;
        let input = input_of(&serde_json::json!({ "provider": provider.as_str() }));
        let lock = LockKey::Provider(provider.clone());
        let work_provider = provider.clone();
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::RemoveProviderImport,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: vec![lock],
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin,
            input,
            work: remove_work(work_provider, refresher),
            precheck: Some(precheck),
        };
        let outcome = start_operation(
            core,
            &context,
            OperationStart::new(
                "provider.remove",
                context.idempotency_key.as_deref(),
                fingerprint,
            ),
            request,
        )
        .await?;
        if let StartOutcome::Accepted(accepted) = &outcome {
            wait_final(core, accepted.operation.id).await?;
        }
        Ok(RemovedProvider {
            provider: provider.to_string(),
        })
    })
}
