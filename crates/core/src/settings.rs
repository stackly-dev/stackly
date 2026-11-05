//! Daemon settings and storage locations as operations (spec 0032):
//! `update_daemon_settings` and `set_storage_location`.
//!
//! Both are one guarded commit that checks the revision the caller read
//! again, so a change made in between ends as `resource.revision_mismatch`
//! and writes nothing (AC-17). The location operation first judges the
//! folder with [`check_location`](crate::storage::check_location) and makes
//! its folders, which only affects what is created from then on: no data
//! moves.

use std::path::PathBuf;

use stackly_domain::{
    CommitGuard, DaemonSettings, DetailKey, ErrorCodeName, GuardDecision, Guarded, LocationKind,
    LocationRule, Registry, RegistryError, ResourceRevision, SettingsPatch, StageName,
    StorageLocation, StorageLocationId, WriteOp,
};

use crate::operation::codes as operation_codes;
use crate::operation::{
    BoxFuture, EngineError, OperationContext, OperationError, OperationOutcome, OperationWork,
    StepSpec, WorkError, WorkOutcome, WorkResult,
};
use crate::secret::registry_code;
use crate::storage::{
    LocationContext, LocationVerdict, check_location, gather_location_facts, prepare_location,
};

/// The code of a folder that breaks a location rule.
pub const LOCATION_INVALID: ErrorCodeName = ErrorCodeName::from_static("storage.location_invalid");
/// The code of an instance whose location folder is gone.
pub const STORAGE_LOCATION_MISSING: ErrorCodeName =
    ErrorCodeName::from_static("instance.storage_location_missing");

/// Why a settings or location operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    /// The settings changed since the caller read them.
    #[error("the settings are at revision {actual}, not {expected}")]
    RevisionMismatch {
        /// The revision the caller read.
        expected: ResourceRevision,
        /// The revision now.
        actual: ResourceRevision,
    },
    /// The folder breaks a location rule.
    #[error("{}", rule.sentence())]
    LocationInvalid {
        /// The first rule it broke.
        rule: LocationRule,
        /// The folder as given.
        path: String,
    },
    /// The registry failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// A blocking task failed.
    #[error("a blocking task failed: {0}")]
    Internal(String),
}

impl OperationError for SettingsError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::RevisionMismatch { .. } => operation_codes::REVISION_MISMATCH,
            Self::LocationInvalid { .. } => LOCATION_INVALID,
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
                .expect("the detail keys of spec 0032 are camelCase constants")
        };
        match self {
            Self::RevisionMismatch { expected, actual } => vec![
                (key("expected"), expected.to_string()),
                (key("actual"), actual.to_string()),
            ],
            Self::LocationInvalid { rule, path } => vec![
                (key("rule"), rule.as_code().to_owned()),
                (key("path"), path.clone()),
            ],
            Self::Registry(_) | Self::Internal(_) => Vec::new(),
        }
    }
}

fn failed(error: SettingsError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

/// What a precheck found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsCheck {
    /// The request changes something; start the operation.
    Proceed,
    /// The request changes nothing; answer with these settings.
    Unchanged(Box<DaemonSettings>),
}

/// Judges a settings update before an operation exists.
///
/// # Errors
///
/// Returns [`SettingsError::RevisionMismatch`] when `expected` is stale and
/// [`SettingsError::Registry`] when the read fails.
pub fn update_precheck(
    registry: &dyn Registry,
    patch: &SettingsPatch,
    expected: ResourceRevision,
) -> Result<SettingsCheck, SettingsError> {
    let settings = registry.daemon_settings()?.value;
    if settings.revision != expected {
        return Err(SettingsError::RevisionMismatch {
            expected,
            actual: settings.revision,
        });
    }
    if settings.patched(patch, settings.updated_at).is_none() {
        return Ok(SettingsCheck::Unchanged(Box::new(settings)));
    }
    Ok(SettingsCheck::Proceed)
}

/// What `update_daemon_settings` needs.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct UpdateRequest {
    /// What to change.
    pub patch: SettingsPatch,
    /// The revision the caller read.
    pub expected: ResourceRevision,
}

impl UpdateRequest {
    /// Asks to apply `patch` to the settings at `expected`.
    #[must_use]
    pub const fn new(patch: SettingsPatch, expected: ResourceRevision) -> Self {
        Self { patch, expected }
    }
}

/// Builds the work of `update_daemon_settings`.
#[must_use]
pub fn update_work(request: UpdateRequest) -> Box<dyn OperationWork> {
    Box::new(UpdateWork { request })
}

struct UpdateWork {
    request: UpdateRequest,
}

impl OperationWork for UpdateWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_update(self.request, ctx).await })
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

async fn run_update(request: UpdateRequest, mut ctx: OperationContext) -> WorkResult {
    ctx.begin_step(StepSpec::new(StageName::COMMIT)).await?;
    ctx.set_stage(StageName::COMMIT).await?;
    let now = ctx.platform().clock().now();
    let UpdateRequest { patch, expected } = request;
    let guard: CommitGuard = Box::new(move |view| {
        let settings = view.daemon_settings()?;
        if settings.revision != expected {
            return Ok(GuardDecision::Refuse(Box::new(
                SettingsError::RevisionMismatch {
                    expected,
                    actual: settings.revision,
                },
            )));
        }
        Ok(GuardDecision::Write(vec![WriteOp::UpdateDaemonSettings {
            patch,
            expected,
            at: now,
        }]))
    });
    commit(&mut ctx, guard).await?;
    Ok(settled())
}

async fn commit(ctx: &mut OperationContext, guard: CommitGuard) -> Result<(), WorkError> {
    match ctx.commit_guarded(guard).await {
        Ok(Guarded::Committed(_)) => Ok(()),
        Ok(Guarded::Refused(reason)) => Err(match reason.downcast::<SettingsError>() {
            Ok(error) => failed(*error),
            Err(_) => failed(SettingsError::Internal(
                "the settings guard refused with an unknown reason".to_owned(),
            )),
        }),
        Err(WorkError::Engine(EngineError::Registry(error))) => {
            Err(failed(SettingsError::Registry(error)))
        }
        Err(other) => Err(other),
    }
}

/// Where `storage.set_location` points a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationTarget {
    /// A folder; the location is its `Stackly` subfolder.
    Path(PathBuf),
    /// The location that comes with Stackly.
    BuiltIn,
}

/// What `set_storage_location` needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SetLocationRequest {
    /// Which default to change.
    pub kind: LocationKind,
    /// The new default.
    pub target: LocationTarget,
    /// The revision the caller read.
    pub expected: ResourceRevision,
    /// The home folder of the user, for the protected folders.
    pub home: Option<PathBuf>,
    /// Folders a location must not overlap besides the four owned roots,
    /// such as the app bundle.
    pub avoid: Vec<PathBuf>,
}

impl SetLocationRequest {
    /// Asks to point the default of `kind` at `target`, at `expected`.
    #[must_use]
    pub const fn new(
        kind: LocationKind,
        target: LocationTarget,
        expected: ResourceRevision,
    ) -> Self {
        Self {
            kind,
            target,
            expected,
            home: None,
            avoid: Vec::new(),
        }
    }

    /// This request with the home folder of the user.
    #[must_use]
    pub fn with_home(mut self, home: Option<PathBuf>) -> Self {
        self.home = home;
        self
    }

    /// This request with more folders a location must not overlap.
    #[must_use]
    pub fn with_avoid(mut self, avoid: Vec<PathBuf>) -> Self {
        self.avoid = avoid;
        self
    }
}

/// Judges a location change before an operation exists: the revision, and
/// whether the target is the default already.
///
/// # Errors
///
/// Returns [`SettingsError::RevisionMismatch`] when `expected` is stale and
/// [`SettingsError::Registry`] when a read fails.
pub fn set_location_precheck(
    registry: &dyn Registry,
    request: &SetLocationRequest,
) -> Result<SettingsCheck, SettingsError> {
    let settings = registry.daemon_settings()?.value;
    if settings.revision != request.expected {
        return Err(SettingsError::RevisionMismatch {
            expected: request.expected,
            actual: settings.revision,
        });
    }
    if request.target == LocationTarget::BuiltIn
        && settings.location_of(request.kind) == request.kind.built_in_id()
    {
        return Ok(SettingsCheck::Unchanged(Box::new(settings)));
    }
    Ok(SettingsCheck::Proceed)
}

/// Builds the work of `set_storage_location`.
#[must_use]
pub fn set_location_work(request: SetLocationRequest) -> Box<dyn OperationWork> {
    Box::new(SetLocationWork { request })
}

struct SetLocationWork {
    request: SetLocationRequest,
}

impl OperationWork for SetLocationWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_set_location(self.request, ctx).await })
    }
}

/// The location a request settled on, and whether the registry lacks it.
struct Chosen {
    id: StorageLocationId,
    new: Option<StorageLocation>,
}

async fn run_set_location(request: SetLocationRequest, mut ctx: OperationContext) -> WorkResult {
    let SetLocationRequest {
        kind,
        target,
        expected,
        home,
        avoid,
    } = request;

    let chosen = match target {
        LocationTarget::BuiltIn => Chosen {
            id: kind.built_in_id(),
            new: None,
        },
        LocationTarget::Path(path) => {
            ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
            ctx.set_stage(StageName::VALIDATE).await?;
            let platform = ctx.platform().clone();
            let layout = ctx.layout().clone();
            let registry = ctx.registry().clone();
            let now = platform.clock().now();
            tokio::task::spawn_blocking(move || {
                judge_and_prepare(
                    &platform,
                    &layout,
                    registry.as_ref(),
                    kind,
                    &path,
                    home,
                    &avoid,
                    now,
                )
            })
            .await
            .map_err(|error| failed(SettingsError::Internal(error.to_string())))?
            .map_err(failed)?
        }
    };

    ctx.begin_step(StepSpec::new(StageName::COMMIT)).await?;
    ctx.set_stage(StageName::COMMIT).await?;
    let now = ctx.platform().clock().now();
    let id = chosen.id;
    let new = chosen.new.clone();
    let guard: CommitGuard = Box::new(move |view| {
        let settings = view.daemon_settings()?;
        if settings.revision != expected {
            return Ok(GuardDecision::Refuse(Box::new(
                SettingsError::RevisionMismatch {
                    expected,
                    actual: settings.revision,
                },
            )));
        }
        let mut ops = Vec::new();
        if let Some(location) = &new
            && !view
                .storage_locations()?
                .iter()
                .any(|known| known.id() == location.id())
        {
            ops.push(WriteOp::InsertStorageLocation(location.clone()));
        }
        ops.push(WriteOp::SetDefaultLocation {
            kind,
            id,
            expected,
            at: now,
        });
        Ok(GuardDecision::Write(ops))
    });
    commit(&mut ctx, guard).await?;

    // New instances and downloads follow the default from now on.
    let book = ctx.layout().locations();
    if let Some(location) = &chosen.new
        && let Some(path) = location.path()
    {
        book.add_location(location.id(), kind, PathBuf::from(path));
    }
    if kind == LocationKind::Cache {
        book.set_cache(chosen.id);
    }
    Ok(settled())
}

#[expect(
    clippy::too_many_arguments,
    reason = "one argument per input of the judgment"
)]
fn judge_and_prepare(
    platform: &stackly_platform::Platform,
    layout: &crate::PreparedLayout,
    registry: &dyn Registry,
    kind: LocationKind,
    path: &std::path::Path,
    home: Option<PathBuf>,
    avoid: &[PathBuf],
    now: jiff::Timestamp,
) -> Result<Chosen, SettingsError> {
    let invalid = |rule| SettingsError::LocationInvalid {
        rule,
        path: path.display().to_string(),
    };
    let known = registry.storage_locations()?.value;
    let existing = known
        .iter()
        .filter_map(|location| {
            Some((
                location.id(),
                location.kind(),
                PathBuf::from(location.path()?),
            ))
        })
        .collect();
    let mut roots: Vec<PathBuf> = layout
        .canonical_roots()
        .into_iter()
        .map(|(_, root)| root.to_path_buf())
        .collect();
    roots.extend(avoid.iter().cloned());
    let context = LocationContext::new(home, roots, existing);
    let facts = gather_location_facts(platform, path, &context)
        .map_err(|error| SettingsError::Internal(error.to_string()))?;
    let verdict = check_location(kind, &facts, &context).map_err(invalid)?;

    let folder = facts.canonical.as_deref().unwrap_or(path);
    let id = match verdict {
        LocationVerdict::New => StorageLocationId::generate(),
        LocationVerdict::Reuse(id) => id,
    };
    let location_folder = prepare_location(platform, folder, kind, id).map_err(invalid)?;
    let new = match verdict {
        LocationVerdict::New => {
            let text = location_folder.to_str().ok_or(RegistryError::NotUtf8Path {
                field: "location_path",
            })?;
            Some(
                StorageLocation::custom(id, kind, text, now)
                    .map_err(|error| SettingsError::Internal(error.to_string()))?,
            )
        }
        LocationVerdict::Reuse(_) => None,
    };
    Ok(Chosen { id, new })
}
