//! Compensation (spec 0007, "Kompensasi dan primitive hapus"): undoing,
//! newest first, only what the operation owns by construction (BR-023,
//! VAL-007). Paths are always derived again from the IDs in the records
//! and removed through `remove_owned_tree`, never from a stored path.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use stackly_domain::{
    CompensatorName, CreatedItem, CreatedRecord, CreatedState, EnvironmentId, InstanceId,
    LockState, OperationId, ReasonCode, Registry,
};
use stackly_platform::{Platform, PlatformError};

use super::Inner;
use super::state::{OpHandle, OpRecord};
use super::work::BoxFuture;
use crate::layout::PreparedLayout;
use crate::ownership::remove_owned_tree;
use crate::secret::SECRET_REF_COMPENSATOR;
use crate::{CoreError, read_instance_manifest};

/// The name the built in compensator of instance folders reports.
pub const INSTANCE_TREE_COMPENSATOR: &str = "instance_tree";
/// The name the built in compensator of environment folders reports.
pub const ENVIRONMENT_TREE_COMPENSATOR: &str = "environment_tree";

/// What a compensator may use.
#[derive(Clone)]
pub struct CompensationScope {
    /// The operation being compensated.
    pub operation: OperationId,
    /// The registry, to read.
    pub registry: Arc<dyn Registry>,
    /// The platform ports.
    pub platform: Arc<Platform>,
    /// The checked layout.
    pub layout: Arc<PreparedLayout>,
}

impl fmt::Debug for CompensationScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompensationScope")
            .field("operation", &self.operation)
            .finish_non_exhaustive()
    }
}

/// How one item was compensated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompensationResult {
    /// It is undone, or was already gone.
    Compensated,
    /// It is not this operation's, so it was left alone.
    NotOwned {
        /// Why, for example `created_by_other_operation`.
        reason: ReasonCode,
    },
}

/// Why undoing one item failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("compensation failed ({code}): {message}")]
pub struct CompensationError {
    /// A short code, stored as the record's detail.
    pub code: ReasonCode,
    /// What failed, for logs.
    pub message: String,
}

impl CompensationError {
    /// An error with its code and message.
    #[must_use]
    pub fn new(code: ReasonCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Undoes the custom items a feature records. A compensator must prove the
/// item is its operation's before it removes anything, return `NotOwned`
/// when it cannot, treat an item that is already gone as `Compensated`, and
/// give the same result when it runs twice.
pub trait Compensator: Send + Sync + 'static {
    /// Undoes `record`.
    fn compensate(
        &self,
        scope: CompensationScope,
        record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>>;
}

const fn code(text: &'static str) -> ReasonCode {
    ReasonCode::from_static(text)
}

/// Why an item was not this operation's: its instance manifest names
/// another operation.
pub(crate) const CREATED_BY_OTHER_OPERATION: ReasonCode = code("created_by_other_operation");
const REMOVE_FAILED: ReasonCode = code("remove_failed");
/// The instance manifest exists but cannot be read, so ownership is unknown.
const MANIFEST_UNREADABLE: ReasonCode = code("manifest_unreadable");
/// A process still holds the instance lock: an init helper that outlived
/// the daemon (spec 0007, K10).
const INSTANCE_LOCKED: ReasonCode = code("instance_locked");
/// The instance lock could not be inspected, so nothing is removed.
const LOCK_PROBE_FAILED: ReasonCode = code("lock_probe_failed");
const COMPENSATOR_MISSING: ReasonCode = code("compensator_missing");
const UNKNOWN_ITEM: ReasonCode = code("unknown_item");
const TASK_STOPPED: ReasonCode = code("task_stopped");

/// Which records a compensation visits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Every record not settled, then the scratch folder.
    All,
    /// Only records of reversible steps, and the scratch folder only before
    /// the point of no return (abandon).
    Reversible,
}

/// What one compensation left.
#[derive(Debug, Clone, Default)]
pub(crate) struct Summary {
    /// The first record that failed: its item and the detail.
    pub(crate) failed: Option<(String, ReasonCode)>,
}

/// Compensates the operation's records, newest first, and then its scratch
/// folder when nothing failed. Every settled record is committed when it
/// settles; a commit that fails keeps the result in `record` for the final
/// batch. The caller holds the writer and the compensation claim.
pub(crate) async fn compensate(
    inner: &Inner,
    handle: &OpHandle,
    record: &mut OpRecord,
    mode: Mode,
) -> Summary {
    if record.journal.compensation_started_at().is_none() {
        let written = inner
            .write(handle, record, |next, now| {
                next.journal.mark_compensation_started(now);
                Ok(Vec::new())
            })
            .await;
        if let Err(error) = written {
            let now = inner.now();
            record.journal.mark_compensation_started(now);
            tracing::warn!(operation_id = %handle.id, %error, "compensation start not committed");
        }
    }
    let reversible = |step_seq| {
        record
            .steps
            .iter()
            .find(|step| step.seq() == step_seq)
            .is_none_or(|step| step.reversible())
    };
    let mut pending: Vec<usize> = record
        .created
        .iter()
        .enumerate()
        .filter(|(_, created)| {
            matches!(
                created.state(),
                CreatedState::Intended | CreatedState::Created | CreatedState::CompensationFailed
            ) && (mode == Mode::All || reversible(created.step_seq()))
        })
        .map(|(index, _)| index)
        .collect();
    pending.sort_by_key(|index| std::cmp::Reverse(record.created[*index].seq()));

    let mut summary = Summary::default();
    for index in pending {
        let created = record.created[index].clone();
        let outcome = undo(inner, handle.id, &created).await;
        let (state, detail) = match &outcome {
            Ok(CompensationResult::Compensated) => (CreatedState::Compensated, None),
            Ok(CompensationResult::NotOwned { reason }) => {
                (CreatedState::NotOwned, Some(reason.clone()))
            }
            Err(error) => {
                tracing::warn!(
                    operation_id = %handle.id,
                    item = %created.item().item_ref(),
                    code = %error.code,
                    "compensation of an item failed"
                );
                (CreatedState::CompensationFailed, Some(error.code.clone()))
            }
        };
        if state == CreatedState::CompensationFailed && summary.failed.is_none() {
            summary.failed = Some((
                format!(
                    "{}:{}",
                    created.item().kind_code(),
                    created.item().item_ref()
                ),
                detail.clone().unwrap_or(REMOVE_FAILED),
            ));
        }
        let written = inner
            .write(handle, record, |next, now| {
                next.created[index]
                    .settle(state, detail.clone(), now)
                    .map_err(|_| super::EngineError::Internal {
                        reason: "a created record refused to settle",
                    })?;
                Ok(Vec::new())
            })
            .await;
        if written.is_err() {
            let now = inner.now();
            // Kept in memory; the next batch, at the latest the final one,
            // writes it.
            if record.created[index].settle(state, detail, now).is_ok() {
                let seq = record.created[index].seq();
                record.mark_unwritten_created(seq);
            }
        }
    }
    let scratch = mode == Mode::All || record.journal.point_of_no_return_at().is_none();
    if summary.failed.is_none() && scratch {
        remove_scratch(inner, handle.id).await;
    }
    summary
}

/// Removes the operation's scratch folder, best effort.
pub(crate) async fn remove_scratch(inner: &Inner, id: OperationId) {
    let platform = Arc::clone(&inner.deps.platform);
    let layout = Arc::clone(&inner.deps.layout);
    let removed = tokio::task::spawn_blocking(move || {
        let path = layout.layout().operations_dir().join(id.to_string());
        remove_owned_tree(&platform, &layout, &path)
    })
    .await;
    match removed {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(operation_id = %id, %error, "the scratch folder was not removed");
        }
        Err(_) => tracing::warn!(operation_id = %id, "the scratch removal task stopped"),
    }
}

/// Undoes one record with its compensator.
async fn undo(
    inner: &Inner,
    operation: OperationId,
    record: &CreatedRecord,
) -> Result<CompensationResult, CompensationError> {
    let scope = CompensationScope {
        operation,
        registry: Arc::clone(&inner.deps.registry),
        platform: Arc::clone(&inner.deps.platform),
        layout: Arc::clone(&inner.deps.layout),
    };
    match record.item() {
        CreatedItem::InstanceTree(id) => instance_tree(scope, *id).await,
        CreatedItem::EnvironmentTree(id) => environment_tree(scope, *id).await,
        CreatedItem::Custom { compensator, .. } => {
            registered(inner, compensator, scope, record).await
        }
        CreatedItem::SecretRef(_) => {
            registered(inner, &SECRET_REF_COMPENSATOR, scope, record).await
        }
        _ => Err(CompensationError::new(
            UNKNOWN_ITEM,
            "the engine has no compensator for this kind of item",
        )),
    }
}

/// Undoes one record with the compensator registered as `name`.
async fn registered(
    inner: &Inner,
    name: &CompensatorName,
    scope: CompensationScope,
    record: &CreatedRecord,
) -> Result<CompensationResult, CompensationError> {
    match inner.compensator(name) {
        Some(compensator) => compensator.compensate(scope, record).await,
        None => Err(CompensationError::new(
            COMPENSATOR_MISSING,
            format!("no compensator is registered as {name}"),
        )),
    }
}

fn remove_failed(error: &CoreError) -> CompensationError {
    CompensationError::new(REMOVE_FAILED, error.to_string())
}

/// Refuses the removal while a process holds the instance lock (K10).
///
/// A lock file that is not there means no process holds it, so the probe is
/// skipped; the ordinary path (a create that stopped before `init`) never
/// calls `probe_lock`. An error from either call is `lock_probe_failed`, and
/// anything but `Free` is `instance_locked`; both leave the tree alone.
fn check_instance_lock(platform: &Platform, lock: &Path) -> Result<(), CompensationError> {
    let probe_failed =
        |error: &dyn fmt::Display| CompensationError::new(LOCK_PROBE_FAILED, error.to_string());
    match platform.fs().symlink_metadata(lock) {
        Ok(None) => return Ok(()),
        Ok(Some(_)) => {}
        Err(error) => return Err(probe_failed(&error)),
    }
    match platform.processes().probe_lock(lock) {
        Ok(LockState::Free) => Ok(()),
        Ok(_) => Err(CompensationError::new(
            INSTANCE_LOCKED,
            "a process still holds the instance lock",
        )),
        Err(error) => Err(probe_failed(&error)),
    }
}

/// The built in compensator of an instance's folders: its root, its log
/// folder, and its run folder unless another known instance shares its
/// short key. A manifest naming another operation leaves it all alone.
async fn instance_tree(
    scope: CompensationScope,
    id: InstanceId,
) -> Result<CompensationResult, CompensationError> {
    let known = {
        let registry = Arc::clone(&scope.registry);
        tokio::task::spawn_blocking(move || registry.known_instance_ids())
            .await
            .map_err(|_| CompensationError::new(TASK_STOPPED, "the registry read stopped"))?
            .map_err(|error| CompensationError::new(REMOVE_FAILED, error.to_string()))?
            .value
    };
    tokio::task::spawn_blocking(move || {
        let layout = scope.layout.layout();
        let paths = layout.instance_paths(id);
        match read_instance_manifest(&scope.platform, &paths) {
            Ok(manifest) if manifest.created_by_operation() != scope.operation => {
                return Ok(CompensationResult::NotOwned {
                    reason: CREATED_BY_OTHER_OPERATION,
                });
            }
            // No manifest: the operation stopped before it wrote one, or the
            // tree is already gone.
            Ok(_) | Err(CoreError::Platform(PlatformError::NotFound { .. })) => {}
            // A manifest that is there but cannot be read might name
            // another owner, so nothing is removed.
            Err(error) => {
                return Err(CompensationError::new(
                    MANIFEST_UNREADABLE,
                    error.to_string(),
                ));
            }
        }
        check_instance_lock(&scope.platform, &paths.lock)?;
        let platform = &scope.platform;
        let prepared = &scope.layout;
        remove_owned_tree(platform, prepared, &paths.root).map_err(|e| remove_failed(&e))?;
        remove_owned_tree(platform, prepared, &layout.instance_log_dir(id))
            .map_err(|e| remove_failed(&e))?;
        let shared = known
            .iter()
            .any(|other| *other != id && other.short_key() == id.short_key());
        if !shared {
            remove_owned_tree(platform, prepared, &layout.instance_run_dir(id.short_key()))
                .map_err(|e| remove_failed(&e))?;
        }
        Ok(CompensationResult::Compensated)
    })
    .await
    .map_err(|_| CompensationError::new(TASK_STOPPED, "the removal task stopped"))?
}

/// The built in compensator of an environment's folder.
async fn environment_tree(
    scope: CompensationScope,
    id: EnvironmentId,
) -> Result<CompensationResult, CompensationError> {
    tokio::task::spawn_blocking(move || {
        let path = scope.layout.layout().environment_dir(id);
        remove_owned_tree(&scope.platform, &scope.layout, &path)
            .map(|_| CompensationResult::Compensated)
            .map_err(|e| remove_failed(&e))
    })
    .await
    .map_err(|_| CompensationError::new(TASK_STOPPED, "the removal task stopped"))?
}
