//! The provider operations of spec 0037 that milestone 1 builds:
//! `import_provider` and `remove_provider_import`, their errors, the
//! compensator of the store folder an import makes, and the port through
//! which the work replaces the daemon's catalog snapshot.
//!
//! Both run on the manager resource with `LockKey::Provider` among their
//! extra locks (spec 0007, *Operasi provider*), so provider operations run
//! one at a time. Every registry write is one `commit_guarded`; the only
//! effect outside the registry before it is the content addressed store
//! folder, recorded as a custom item and removed if the operation fails.

use std::path::PathBuf;
use std::sync::Arc;

use stackly_domain::{
    CompensatorName, CreatedItem, CreatedRecord, CustomItemKey, DetailKey, ErrorCodeName,
    GuardDecision, Guarded, ProviderImportRecord, ProviderName, ProviderSourceId,
    ProviderSourceKind, ProviderSourceRecord, ReasonCode, Registry, RegistryError, Sha256Digest,
    StageName, WriteOp,
};
use stackly_platform::Platform;

use super::local::{ImportCheck, ImportLimit, import_limit};
use super::model::{Catalog, CatalogOptions};
use super::store::{StoreError, StoreMode, write_package};
use super::trust::RevokedInfo;
use crate::PreparedLayout;
use crate::operation::{
    AuditNote, BoxFuture, CompensationError, CompensationResult, CompensationScope, Compensator,
    OperationContext, OperationError, OperationOutcome, OperationWork, StepSpec, WorkError,
    WorkOutcome, WorkResult, codes as operation_codes,
};
use crate::provider::{NameCheck, check_package};
use crate::secret::registry_code;

/// The location of the one `local` provider source row (spec 0037, *Model
/// data*).
pub const LOCAL_SOURCE_LOCATION: &str = "imports";
/// The name the store folder compensator is registered under.
pub const STORE_COMPENSATOR: CompensatorName =
    CompensatorName::from_static("provider_import_store");

const READ: StageName = StageName::from_static("read");
const STORE: StageName = StageName::from_static("store");
const COMMIT: StageName = StageName::from_static("commit");
const CLEANUP: StageName = StageName::from_static("cleanup");

const IMPORT_CHANGED: ErrorCodeName = ErrorCodeName::from_static("provider.import_changed");
const IMPORT_LIMIT: ErrorCodeName = ErrorCodeName::from_static("provider.import_limit");
const IMPORT_NOT_FOUND: ErrorCodeName = ErrorCodeName::from_static("provider.import_not_found");
const STORE_FAILED: ErrorCodeName = ErrorCodeName::from_static("provider.store_failed");
const NOT_APPROVED: ErrorCodeName = ErrorCodeName::from_static("provider.not_approved");
const REVOKED: ErrorCodeName = ErrorCodeName::from_static("provider.revoked");
const NOT_INSTALLABLE: ErrorCodeName = ErrorCodeName::from_static("catalog.not_installable");

/// What happened to a provider, for the `catalog.changed` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LocalChange {
    /// A package was imported.
    Imported,
    /// An import was approved.
    Approved,
    /// An approval was revoked.
    Revoked,
    /// An import was removed.
    Removed,
}

impl LocalChange {
    /// Returns the wire code, such as `imported`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Approved => "approved",
            Self::Revoked => "revoked",
            Self::Removed => "removed",
        }
    }
}

/// Replaces the daemon's catalog snapshot after a provider operation
/// committed and announces the change (spec 0037, *Read model*). The work
/// calls it as its last step, after the commit and before the operation
/// finishes; a refresh that fails leaves the snapshot stale but never fails
/// the operation, because every gate reads the registry.
pub trait CatalogRefresher: Send + Sync + 'static {
    /// Rebuilds the snapshot from the registry and sends the event.
    fn refresh(&self, provider: ProviderName, change: LocalChange) -> BoxFuture<'static, ()>;
}

/// Why a provider operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderOpError {
    /// The folder now holds other bytes than the client showed, or no
    /// longer validates.
    #[error("the provider package changed since it was previewed")]
    Changed {
        /// The provider.
        provider: String,
        /// The digest the client sent.
        expected: String,
        /// The digest read now, empty when the package no longer validates.
        actual: String,
    },
    /// An import limit is reached.
    #[error("too many imported providers")]
    Limit(ImportLimit),
    /// The provider is not imported.
    #[error("no imported provider {provider}")]
    NotFound {
        /// The provider.
        provider: String,
        /// `not_imported` or `not_local`.
        reason: &'static str,
    },
    /// The store copy could not be written. The text is the fixed one of
    /// the code table: the store error names paths under the data root, so
    /// it reaches only the daemon log and the `reason` detail (AC-17).
    #[error("the provider package could not be stored")]
    Store {
        /// The provider.
        provider: String,
        /// What failed.
        source: StoreError,
    },
    /// A revoke found no approval to revoke.
    #[error("{provider} has no approval to revoke")]
    NotApproved {
        /// The provider.
        provider: String,
        /// Where its stored bytes stand, such as `pending`.
        approval: &'static str,
    },
    /// The revision is revoked by the installed catalog, so it cannot be
    /// approved.
    #[error("{provider} {provider_revision} is revoked")]
    Revoked {
        /// The provider.
        provider: String,
        /// The revoked revision label.
        provider_revision: String,
        /// Who revoked it and since when.
        info: RevokedInfo,
    },
    /// The stored copy does not load or match, so it cannot be approved.
    #[error("the stored copy of {provider} failed validation")]
    PackageInvalid {
        /// The provider.
        provider: String,
    },
    /// The registry failed.
    #[error("the registry failed: {0}")]
    Registry(RegistryError),
    /// A bug.
    #[error("unexpected: {0}")]
    Internal(String),
}

impl OperationError for ProviderOpError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::Changed { .. } => IMPORT_CHANGED,
            Self::Limit(_) => IMPORT_LIMIT,
            Self::NotFound { .. } => IMPORT_NOT_FOUND,
            Self::Store { .. } => STORE_FAILED,
            Self::NotApproved { .. } => NOT_APPROVED,
            Self::Revoked { .. } => REVOKED,
            Self::PackageInvalid { .. } => NOT_INSTALLABLE,
            Self::Registry(error) => registry_code(error),
            Self::Internal(_) => operation_codes::INTERNAL_UNEXPECTED,
        }
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        let key = |text: &str| -> DetailKey {
            text.parse()
                .expect("the detail keys of spec 0037 are camelCase constants")
        };
        match self {
            Self::Changed {
                provider,
                expected,
                actual,
            } => vec![
                (key("provider"), provider.clone()),
                (key("expected"), expected.clone()),
                (key("actual"), actual.clone()),
            ],
            Self::Limit(limit) => vec![
                (key("limit"), limit.as_code().to_owned()),
                (key("max"), limit.max().to_string()),
            ],
            Self::NotFound { provider, reason } => vec![
                (key("provider"), provider.clone()),
                (key("reason"), (*reason).to_owned()),
            ],
            Self::Store { provider, source } => vec![
                (key("provider"), provider.clone()),
                (key("reason"), source.reason().to_owned()),
            ],
            Self::NotApproved { provider, approval } => vec![
                (key("provider"), provider.clone()),
                (key("approval"), (*approval).to_owned()),
            ],
            Self::Revoked {
                provider,
                provider_revision,
                info,
            } => revoked_details(provider, provider_revision, info),
            Self::PackageInvalid { provider } => vec![
                (key("provider"), provider.clone()),
                (key("reason"), "package_invalid".to_owned()),
            ],
            Self::Registry(_) | Self::Internal(_) => Vec::new(),
        }
    }
}

/// The details of `provider.revoked` (spec 0037, *Kode error di wire*).
#[must_use]
pub fn revoked_details(
    provider: &str,
    provider_revision: &str,
    info: &RevokedInfo,
) -> Vec<(DetailKey, String)> {
    let key = |text: &str| -> DetailKey {
        text.parse()
            .expect("the detail keys of spec 0037 are camelCase constants")
    };
    let mut details = vec![
        (key("provider"), provider.to_owned()),
        (key("providerRevision"), provider_revision.to_owned()),
        (key("by"), info.by.as_code().to_owned()),
        (
            key("since"),
            info.since.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        ),
    ];
    if let Some(revision) = info.catalog_revision {
        details.push((key("catalogRevision"), revision.to_string()));
    }
    details
}

fn failed(error: ProviderOpError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

fn engine_registry(error: WorkError) -> WorkError {
    match error {
        WorkError::Engine(crate::operation::EngineError::Registry(error)) => {
            failed(ProviderOpError::Registry(error))
        }
        other => other,
    }
}

/// What `import_provider` needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ImportRequest {
    /// The provider the handler read from the folder, the one the lock
    /// names.
    pub provider: ProviderName,
    /// The package folder.
    pub path: PathBuf,
    /// The `packageSha256` the client showed.
    pub expected: Sha256Digest,
    /// The folder path as the row stores it, with the home folder `~`.
    pub origin_path: String,
    /// What the quarantine attribute of `provider.yaml` showed.
    pub quarantined: Option<bool>,
    /// The quarantine agent, when one was read.
    pub quarantine_agent: Option<String>,
    /// The loader options, `testing` only in tests.
    pub options: CatalogOptions,
    /// The catalog of this run, for the ID rule of the test option.
    pub catalog: Option<Arc<Catalog>>,
}

impl ImportRequest {
    /// An import of `provider` from `path`, expecting the bytes `expected`,
    /// with no quarantine attribute read, the default options, and no
    /// catalog.
    #[must_use]
    pub const fn new(
        provider: ProviderName,
        path: PathBuf,
        expected: Sha256Digest,
        origin_path: String,
    ) -> Self {
        Self {
            provider,
            path,
            expected,
            origin_path,
            quarantined: None,
            quarantine_agent: None,
            options: CatalogOptions {
                allow_testing_namespace: false,
            },
            catalog: None,
        }
    }
}

/// The `import_provider` work.
#[must_use]
pub fn import_work(
    request: ImportRequest,
    refresher: Arc<dyn CatalogRefresher>,
) -> Box<dyn OperationWork> {
    Box::new(ImportWork { request, refresher })
}

/// The `remove_provider_import` work for `provider`.
#[must_use]
pub fn remove_work(
    provider: ProviderName,
    refresher: Arc<dyn CatalogRefresher>,
) -> Box<dyn OperationWork> {
    Box::new(RemoveWork {
        provider,
        refresher,
    })
}

/// The `approve_provider` work: approves exactly the stored bytes with the
/// digest `expected`, which the client showed. `catalog` is the catalog of
/// this run, whose revocation of the revision refuses the approval.
#[must_use]
pub fn approve_work(
    provider: ProviderName,
    expected: Sha256Digest,
    catalog: Option<Arc<Catalog>>,
    refresher: Arc<dyn CatalogRefresher>,
) -> Box<dyn OperationWork> {
    Box::new(DecideWork {
        provider,
        approve: Some((expected, catalog)),
        refresher,
    })
}

/// The `revoke_provider` work.
#[must_use]
pub fn revoke_work(
    provider: ProviderName,
    refresher: Arc<dyn CatalogRefresher>,
) -> Box<dyn OperationWork> {
    Box::new(DecideWork {
        provider,
        approve: None,
        refresher,
    })
}

struct DecideWork {
    provider: ProviderName,
    approve: Option<(Sha256Digest, Option<Arc<Catalog>>)>,
    refresher: Arc<dyn CatalogRefresher>,
}

impl OperationWork for DecideWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            match self.approve {
                Some((expected, catalog)) => {
                    run_approve(self.provider, expected, catalog, self.refresher, ctx).await
                }
                None => run_revoke(self.provider, self.refresher, ctx).await,
            }
        })
    }
}

struct ImportWork {
    request: ImportRequest,
    refresher: Arc<dyn CatalogRefresher>,
}

impl OperationWork for ImportWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_import(self.request, self.refresher, ctx).await })
    }
}

struct RemoveWork {
    provider: ProviderName,
    refresher: Arc<dyn CatalogRefresher>,
}

impl OperationWork for RemoveWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_remove(self.provider, self.refresher, ctx).await })
    }
}

fn settled() -> WorkOutcome {
    WorkOutcome {
        result: OperationOutcome {
            resources: Vec::new(),
            partial: Vec::new(),
        },
        finalize: None,
    }
}

/// The audit note of an import or a remove (spec 0037 AC-17).
fn note(record: &ProviderImportRecord, changes: &[&'static str]) -> AuditNote {
    let mut note = AuditNote::new(None)
        .with("provider", record.provider.as_str())
        .with("providerRevision", record.provider_revision.as_str())
        .with("packageSha256", record.package_sha256)
        .with("manifestSha256", record.manifest_sha256);
    if !changes.is_empty() {
        note = note.with("changes", changes.join(","));
    }
    note
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, WorkError> {
    tokio::task::spawn_blocking(job).await.map_err(|_| {
        failed(ProviderOpError::Internal(
            "a blocking task stopped".to_owned(),
        ))
    })
}

async fn run_import(
    request: ImportRequest,
    refresher: Arc<dyn CatalogRefresher>,
    mut ctx: OperationContext,
) -> WorkResult {
    let provider = request.provider.clone();
    ctx.begin_step(StepSpec::new(READ)).await?;
    ctx.set_stage(READ).await?;
    let platform = Arc::clone(ctx.platform());
    let path = request.path.clone();
    let options = request.options;
    let catalog = request.catalog.clone();
    let check = blocking(move || {
        ImportCheck::new(
            check_package(platform.fs(), &path, NameCheck::Skip),
            options,
            catalog.as_deref(),
        )
    })
    .await?;
    let changed = |actual: String| ProviderOpError::Changed {
        provider: provider.to_string(),
        expected: request.expected.to_string(),
        actual,
    };
    let (Some(package), Some(digest)) = (check.package(), check.package_sha256) else {
        return Err(failed(changed(String::new())));
    };
    if digest != request.expected || !check.valid() || package.key().provider() != &provider {
        return Err(failed(changed(digest.to_string())));
    }
    let files = package.files().clone();
    let key = package.key().clone();
    let software = package.software().clone();
    let versions_sha256: Sha256Digest = super::index::versions_sha256(package.files())
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| {
            failed(ProviderOpError::Internal(
                "versions.json has no digest".to_owned(),
            ))
        })?;
    let bytes = check.package_bytes;
    ctx.finish_step(None).await?;

    ctx.begin_step(StepSpec::new(STORE)).await?;
    ctx.set_stage(STORE).await?;
    let item = ctx
        .record_custom_intent(STORE_COMPENSATOR, store_key(&digest), None)
        .await?;
    let platform = Arc::clone(ctx.platform());
    let layout = Arc::clone(ctx.layout());
    let (absent, stored) = blocking(move || {
        let dirs = layout.layout();
        let dir = dirs.provider_import_dir(&digest);
        // A folder that was not there before this write is this import's
        // own, also when the write fails after making it (a full disk): the
        // compensator removes it then. A folder that was there, or one that
        // cannot be inspected, is never claimed.
        let absent = matches!(platform.fs().symlink_metadata(&dir), Ok(None));
        let stored = write_package(
            &platform,
            &layout,
            &dirs.provider_imports_dir(),
            &dir,
            &files,
            StoreMode::Import(&digest),
        );
        (absent, stored)
    })
    .await?;
    match stored {
        Ok(stored) if stored.created_dir => ctx.confirm_created(item).await?,
        Ok(_) => {
            ctx.not_created(item, ReasonCode::from_static("already_exists"))
                .await?;
        }
        Err(source) => {
            tracing::warn!(error = %source, reason = source.reason(), "a provider package could not be stored");
            if absent {
                ctx.confirm_created(item).await?;
            } else {
                ctx.not_created(item, ReasonCode::from_static("store_failed"))
                    .await?;
            }
            return Err(failed(ProviderOpError::Store {
                provider: provider.to_string(),
                source,
            }));
        }
    }
    ctx.finish_step(None).await?;

    ctx.begin_step(StepSpec::new(COMMIT)).await?;
    ctx.set_stage(COMMIT).await?;
    let registry = Arc::clone(ctx.registry());
    let source = blocking(move || {
        registry.provider_source_by_location(ProviderSourceKind::Local, LOCAL_SOURCE_LOCATION)
    })
    .await?
    .map_err(|error| failed(ProviderOpError::Registry(error)))?
    .value;
    let now = ctx.platform().clock().now();
    let (source_id, insert_source) = match source {
        Some(source) => (source.id, None),
        None => {
            let id = ProviderSourceId::generate();
            (
                id,
                Some(ProviderSourceRecord::new(
                    id,
                    ProviderSourceKind::Local,
                    LOCAL_SOURCE_LOCATION.to_owned(),
                    now,
                )),
            )
        }
    };
    let record = ProviderImportRecord::new(
        provider.clone(),
        source_id,
        key.revision().clone(),
        software,
        *key.manifest_sha256(),
        versions_sha256,
        digest,
        bytes,
        request.origin_path.clone(),
        now,
    )
    .with_quarantine(request.quarantined, request.quarantine_agent.clone());
    ctx.set_audit_note(note(&record, &[]));
    let written = record.clone();
    let guard_provider = provider.clone();
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            let rows = view.provider_imports()?;
            if let Some(limit) = import_limit(&rows, &guard_provider, written.package_bytes) {
                return Ok(GuardDecision::Refuse(Box::new(ProviderOpError::Limit(
                    limit,
                ))));
            }
            let stored = rows.iter().find(|row| row.provider == guard_provider);
            let mut put = written;
            let expected = stored.map(|row| row.revision);
            if let Some(revision) = expected {
                put.revision = revision.next();
            }
            let mut ops = Vec::new();
            if let Some(source) = insert_source {
                ops.push(WriteOp::InsertProviderSource(source));
            }
            ops.push(WriteOp::PutProviderImport {
                record: Box::new(put),
                expected,
            });
            Ok(GuardDecision::Write(ops))
        }))
        .await
        .map_err(engine_registry)?;
    match outcome {
        Guarded::Committed(_) => {}
        Guarded::Refused(reason) => {
            return Err(failed(match reason.downcast::<ProviderOpError>() {
                Ok(error) => *error,
                Err(_) => ProviderOpError::Internal("the import guard refused".to_owned()),
            }));
        }
    }
    ctx.finish_step(None).await?;

    ctx.begin_step(StepSpec::new(CLEANUP).repeatable()).await?;
    ctx.set_stage(CLEANUP).await?;
    remove_unreferenced(&ctx).await;
    refresher.refresh(provider, LocalChange::Imported).await;
    ctx.finish_step(None).await?;
    Ok(settled())
}

async fn run_remove(
    provider: ProviderName,
    refresher: Arc<dyn CatalogRefresher>,
    mut ctx: OperationContext,
) -> WorkResult {
    ctx.begin_step(StepSpec::new(COMMIT)).await?;
    ctx.set_stage(COMMIT).await?;
    let registry = Arc::clone(ctx.registry());
    let read = provider.clone();
    let stored = blocking(move || registry.provider_import(&read))
        .await?
        .map_err(|error| failed(ProviderOpError::Registry(error)))?
        .value;
    if let Some(record) = &stored {
        ctx.set_audit_note(note(record, &[]));
    }
    let now = ctx.platform().clock().now();
    let guard_provider = provider.clone();
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            if view.provider_import(&guard_provider)?.is_none() {
                return Ok(GuardDecision::Refuse(Box::new(ProviderOpError::NotFound {
                    provider: guard_provider.to_string(),
                    reason: "not_imported",
                })));
            }
            Ok(GuardDecision::Write(vec![
                WriteOp::DeleteProviderImport(guard_provider.clone()),
                WriteOp::RevokeProviderApprovals {
                    provider: guard_provider,
                    at: now,
                },
            ]))
        }))
        .await
        .map_err(engine_registry)?;
    if let Guarded::Refused(reason) = outcome {
        return Err(failed(match reason.downcast::<ProviderOpError>() {
            Ok(error) => *error,
            Err(_) => ProviderOpError::Internal("the remove guard refused".to_owned()),
        }));
    }
    ctx.finish_step(None).await?;

    ctx.begin_step(StepSpec::new(CLEANUP).repeatable()).await?;
    ctx.set_stage(CLEANUP).await?;
    remove_unreferenced(&ctx).await;
    refresher.refresh(provider, LocalChange::Removed).await;
    ctx.finish_step(None).await?;
    Ok(settled())
}

async fn run_approve(
    provider: ProviderName,
    expected: Sha256Digest,
    catalog: Option<Arc<Catalog>>,
    refresher: Arc<dyn CatalogRefresher>,
    mut ctx: OperationContext,
) -> WorkResult {
    ctx.begin_step(StepSpec::new(READ)).await?;
    ctx.set_stage(READ).await?;
    // The permissions of the decision come from the store copy, never from
    // the client (spec 0037, *Value sourcing*).
    let platform = Arc::clone(ctx.platform());
    let layout = Arc::clone(ctx.layout());
    let checked = blocking(move || {
        let dir = layout.layout().provider_import_dir(&expected);
        check_package(platform.fs(), &dir, NameCheck::Skip)
    })
    .await?;
    let package = checked
        .package
        .filter(|package| super::local::package_sha256(package.files()) == expected)
        .ok_or_else(|| {
            failed(ProviderOpError::PackageInvalid {
                provider: provider.to_string(),
            })
        })?;
    let key = package.key().clone();
    let permissions = super::local::permissions(&package);
    if let Some(info) = catalog
        .as_deref()
        .and_then(|catalog| super::trust::catalog_revocation(catalog, &key))
    {
        return Err(failed(ProviderOpError::Revoked {
            provider: provider.to_string(),
            provider_revision: key.revision().to_string(),
            info,
        }));
    }
    ctx.finish_step(None).await?;

    ctx.begin_step(StepSpec::new(COMMIT)).await?;
    ctx.set_stage(COMMIT).await?;
    let registry = Arc::clone(ctx.registry());
    let read = provider.clone();
    let stored = blocking(move || registry.provider_import(&read))
        .await?
        .map_err(|error| failed(ProviderOpError::Registry(error)))?
        .value;
    let mut changes: Vec<&'static str> = Vec::new();
    if let Some(record) = &stored {
        if let Some(decided) = record
            .decision
            .as_ref()
            .filter(|decided| decided.package_sha256 != expected)
            && let Ok(previous) = super::local::Permissions::from_json(&decided.permissions_json)
        {
            changes = super::local::permission_changes(
                &previous,
                &decided.provider_revision,
                &permissions,
                key.revision(),
            )
            .into_iter()
            .map(super::local::PermissionChange::as_code)
            .collect();
        }
        ctx.set_audit_note(note(record, &changes));
    }
    let now = ctx.platform().clock().now();
    let permissions_json = permissions.to_json();
    let guard_provider = provider.clone();
    let guard_key = key.clone();
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            let Some(row) = view.provider_import(&guard_provider)? else {
                return Ok(GuardDecision::Refuse(Box::new(ProviderOpError::NotFound {
                    provider: guard_provider.to_string(),
                    reason: "not_imported",
                })));
            };
            if row.package_sha256 != expected {
                return Ok(GuardDecision::Refuse(Box::new(ProviderOpError::Changed {
                    provider: guard_provider.to_string(),
                    expected: expected.to_string(),
                    actual: row.package_sha256.to_string(),
                })));
            }
            if row.approval() == stackly_domain::ApprovalState::Approved {
                return Ok(GuardDecision::Write(Vec::new()));
            }
            Ok(GuardDecision::Write(vec![
                WriteOp::DecideProviderImport {
                    provider: guard_provider,
                    decision: stackly_domain::ImportDecision::new(
                        stackly_domain::TrustDecision::Approved,
                        expected,
                        guard_key.revision().clone(),
                        permissions_json,
                        now,
                    ),
                },
                WriteOp::SetProviderRevisionApproval {
                    key: guard_key,
                    approval: stackly_domain::TrustDecision::Approved,
                    at: now,
                },
            ]))
        }))
        .await
        .map_err(engine_registry)?;
    refused(outcome, "the approve guard refused")?;
    refresher.refresh(provider, LocalChange::Approved).await;
    ctx.finish_step(None).await?;
    Ok(settled())
}

async fn run_revoke(
    provider: ProviderName,
    refresher: Arc<dyn CatalogRefresher>,
    mut ctx: OperationContext,
) -> WorkResult {
    ctx.begin_step(StepSpec::new(COMMIT)).await?;
    ctx.set_stage(COMMIT).await?;
    let registry = Arc::clone(ctx.registry());
    let read = provider.clone();
    let stored = blocking(move || registry.provider_import(&read))
        .await?
        .map_err(|error| failed(ProviderOpError::Registry(error)))?
        .value;
    if let Some(record) = &stored {
        ctx.set_audit_note(note(record, &[]));
    }
    let now = ctx.platform().clock().now();
    let guard_provider = provider.clone();
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            let Some(row) = view.provider_import(&guard_provider)? else {
                return Ok(GuardDecision::Refuse(Box::new(ProviderOpError::NotFound {
                    provider: guard_provider.to_string(),
                    reason: "not_imported",
                })));
            };
            let approved_decision = row
                .decision
                .as_ref()
                .filter(|decided| decided.decision == stackly_domain::TrustDecision::Approved);
            if approved_decision.is_none()
                && !view.provider_has_approved_revision(&guard_provider)?
            {
                return Ok(GuardDecision::Refuse(Box::new(
                    ProviderOpError::NotApproved {
                        provider: guard_provider.to_string(),
                        approval: row.approval().as_code(),
                    },
                )));
            }
            let mut ops = Vec::new();
            if let Some(decided) = approved_decision {
                ops.push(WriteOp::DecideProviderImport {
                    provider: guard_provider.clone(),
                    decision: stackly_domain::ImportDecision::new(
                        stackly_domain::TrustDecision::Revoked,
                        decided.package_sha256,
                        decided.provider_revision.clone(),
                        decided.permissions_json.clone(),
                        now,
                    ),
                });
            }
            ops.push(WriteOp::RevokeProviderApprovals {
                provider: guard_provider,
                at: now,
            });
            Ok(GuardDecision::Write(ops))
        }))
        .await
        .map_err(engine_registry)?;
    refused(outcome, "the revoke guard refused")?;
    refresher.refresh(provider, LocalChange::Revoked).await;
    ctx.finish_step(None).await?;
    Ok(settled())
}

/// The error of a guard that refused, or nothing for a commit.
fn refused(outcome: Guarded, fallback: &str) -> Result<(), WorkError> {
    match outcome {
        Guarded::Committed(_) => Ok(()),
        Guarded::Refused(reason) => Err(failed(match reason.downcast::<ProviderOpError>() {
            Ok(error) => *error,
            Err(_) => ProviderOpError::Internal(fallback.to_owned()),
        })),
    }
}

/// The custom item key of the store folder of `digest`.
fn store_key(digest: &Sha256Digest) -> CustomItemKey {
    digest
        .to_string()
        .parse()
        .expect("a lowercase hex digest is a valid custom item key")
}

/// Removes every store folder no import row refers to, after a commit,
/// while the operation still holds its provider lock. A failure is only
/// logged: the operation already committed, and a leftover folder is for
/// garbage collection (feature 55).
async fn remove_unreferenced(ctx: &OperationContext) {
    let registry = Arc::clone(ctx.registry());
    let platform = Arc::clone(ctx.platform());
    let layout = Arc::clone(ctx.layout());
    let removed =
        tokio::task::spawn_blocking(move || prune_store(&*registry, &platform, &layout)).await;
    match removed {
        Ok(Ok(())) => {}
        Ok(Err(code)) => {
            tracing::warn!(
                code,
                "an unreferenced provider store folder was not removed"
            );
        }
        Err(_) => tracing::warn!("the provider store cleanup task stopped"),
    }
}

fn prune_store(
    registry: &dyn Registry,
    platform: &Platform,
    prepared: &PreparedLayout,
) -> Result<(), &'static str> {
    let rows = registry
        .provider_imports()
        .map_err(|_| "registry_read_failed")?;
    let dir = prepared.layout().provider_imports_dir();
    let entries = match platform.fs().read_dir(&dir) {
        Ok(entries) => entries,
        Err(stackly_platform::PlatformError::NotFound { .. }) => return Ok(()),
        Err(_) => return Err("list_failed"),
    };
    let mut result = Ok(());
    for entry in entries {
        let Some(name) = entry.name.to_str() else {
            continue;
        };
        let Ok(digest) = name.parse::<Sha256Digest>() else {
            continue;
        };
        if rows.value.iter().any(|row| row.package_sha256 == digest) {
            continue;
        }
        if crate::remove_owned_tree(platform, prepared, &dir.join(name)).is_err() {
            result = Err("remove_failed");
        }
    }
    result
}

/// Undoes the store folder an import made: removes it unless an import row
/// refers to its digest, which means another import owns it. A folder that
/// is gone already is compensated, so a second run is the same.
#[derive(Debug, Clone, Copy, Default)]
pub struct StoreFolderCompensator;

impl Compensator for StoreFolderCompensator {
    fn compensate(
        &self,
        scope: CompensationScope,
        record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        let digest = match record.item() {
            CreatedItem::Custom { key, .. } => key.as_str().parse::<Sha256Digest>().ok(),
            _ => None,
        };
        Box::pin(async move {
            let digest = digest.ok_or_else(|| {
                CompensationError::new(
                    ReasonCode::from_static("unknown_item"),
                    "the item does not name a package digest",
                )
            })?;
            tokio::task::spawn_blocking(move || undo_store(&scope, &digest))
                .await
                .map_err(|_| {
                    CompensationError::new(
                        ReasonCode::from_static("task_stopped"),
                        "the removal task stopped",
                    )
                })?
        })
    }
}

fn undo_store(
    scope: &CompensationScope,
    digest: &Sha256Digest,
) -> Result<CompensationResult, CompensationError> {
    let rows = scope.registry.provider_imports().map_err(|error| {
        CompensationError::new(
            ReasonCode::from_static("registry_read_failed"),
            error.to_string(),
        )
    })?;
    if rows.value.iter().any(|row| row.package_sha256 == *digest) {
        return Ok(CompensationResult::NotOwned {
            reason: ReasonCode::from_static("referenced"),
        });
    }
    let dir = scope.layout.layout().provider_import_dir(digest);
    match scope.platform.fs().symlink_metadata(&dir) {
        Ok(None) => return Ok(CompensationResult::Compensated),
        Ok(Some(_)) => {}
        Err(error) => {
            return Err(CompensationError::new(
                ReasonCode::from_static("inspect_failed"),
                error.to_string(),
            ));
        }
    }
    crate::remove_owned_tree(&scope.platform, &scope.layout, &dir)
        .map(|_| CompensationResult::Compensated)
        .map_err(|error| {
            CompensationError::new(ReasonCode::from_static("remove_failed"), error.to_string())
        })
}
