//! `environment::register` (spec 0044, AC-11 to AC-13): the two registry
//! only operations on an environment, `import_environment` and
//! `remove_environment`, their guards, and the reads of a manifest file
//! they share with validate and plan.
//!
//! Both run on the resource `Environment(id)` with
//! `LockKey::EnvironmentName(name)` among their extra locks (spec 0007), so
//! an import, a remove, and later Up of the same environment exclude each
//! other. Each writes the registry once, through `commit_guarded`; nothing
//! outside the registry changes, so an interruption before the commit
//! leaves nothing to compensate. Spec 0046 calls [`register`] from Up for a
//! manifest that is not registered yet.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::Arc;

use stackly_domain::{
    DeclaredService, DetailKey, Environment, EnvironmentId, EnvironmentName, ErrorCodeName,
    FailureItem, GuardDecision, GuardView, Guarded, InstanceFilter, InstanceId, LeaseFilter,
    LeaseHolder, LeaseState, ManifestLocation, NotDeletableReason, ReasonCode, RegistryError,
    ResourceRevision, Sha256Digest, StageName, StoredEnvironment, WriteOp,
};
use stackly_platform::{Filesystem, PlatformError};

use super::manifest::MANIFEST_MAX_LEN;
use super::validate::{manifest_digest, validate};
use crate::catalog::Catalog;
use crate::diagnostics::HomeMask;
use crate::operation::{
    AuditNote, BoxFuture, OperationContext, OperationError, OperationOutcome, OperationWork,
    StepSpec, WorkError, WorkOutcome, WorkResult, codes as operation_codes,
};
use crate::secret::registry_code;

/// The most environments the registry holds (spec 0044 AC-12).
pub const ENVIRONMENT_LIMIT: usize = 64;
/// The most instances `env.lease_active` lists.
pub const LEASE_ITEMS_MAX: usize = 32;

const READ: StageName = StageName::from_static("read");
const COMMIT: StageName = StageName::from_static("commit");

const MANIFEST_UNREADABLE: ErrorCodeName = ErrorCodeName::from_static("env.manifest_unreadable");
const MANIFEST_INVALID: ErrorCodeName = ErrorCodeName::from_static("env.manifest_invalid");
const MANIFEST_CHANGED: ErrorCodeName = ErrorCodeName::from_static("env.manifest_changed");
const NAME_TAKEN: ErrorCodeName = ErrorCodeName::from_static("env.name_taken");
const LIMIT_REACHED: ErrorCodeName = ErrorCodeName::from_static("env.limit_reached");
const NOT_FOUND: ErrorCodeName = ErrorCodeName::from_static("env.not_found");
const LEASE_ACTIVE: ErrorCodeName = ErrorCodeName::from_static("env.lease_active");
const RELINK_MISMATCH: ErrorCodeName = ErrorCodeName::from_static("env.relink_mismatch");

/// Why a manifest file could not be read: the `reason` of
/// `env.manifest_unreadable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnreadableReason {
    /// Nothing is at the path, or a folder on the way is missing.
    NotFound,
    /// The path is a symlink, a folder, or anything but a regular file.
    NotAFile,
    /// macOS refused the read.
    PermissionDenied,
    /// The file is above [`MANIFEST_MAX_LEN`].
    TooLarge,
    /// Any other read failure.
    Io,
}

impl UnreadableReason {
    /// Returns the wire code, such as `not_found`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::NotAFile => "not_a_file",
            Self::PermissionDenied => "permission_denied",
            Self::TooLarge => "too_large",
            Self::Io => "io",
        }
    }

    fn of(error: &PlatformError) -> Self {
        match error {
            PlatformError::NotFound { .. } | PlatformError::NotADirectory { .. } => Self::NotFound,
            PlatformError::NotARegularFile { .. } => Self::NotAFile,
            PlatformError::PermissionDenied { .. } => Self::PermissionDenied,
            PlatformError::FileTooLarge { .. } => Self::TooLarge,
            _ => Self::Io,
        }
    }
}

/// Reads a manifest through `read_file_limited`: at most 256 KiB, a
/// regular file, never through a symlink in the last component.
///
/// # Errors
///
/// Returns the [`UnreadableReason`] of the failed read.
pub fn read_manifest(fs: &dyn Filesystem, path: &Path) -> Result<Vec<u8>, UnreadableReason> {
    fs.read_file_limited(path, MANIFEST_MAX_LEN)
        .map_err(|error| UnreadableReason::of(&error))
}

/// The location of the manifest at `path` as the registry stores and
/// compares it (AC-11): the canonical parent folder, resolved through the
/// `Filesystem` port, plus the file name as that folder's listing spells
/// it, whose last component is never followed. `/tmp/x/stackly.yaml` and
/// `/private/tmp/x/stackly.yaml` give the same location, and so do
/// `Stackly.yaml` and `stackly.yaml` for one file on APFS.
///
/// # Errors
///
/// Returns the [`UnreadableReason`] when the parent folder can't be
/// resolved or the path has no file name.
pub fn manifest_location(
    fs: &dyn Filesystem,
    path: &Path,
) -> Result<ManifestLocation, UnreadableReason> {
    let (Some(parent), Some(file)) = (path.parent(), path.file_name()) else {
        return Err(UnreadableReason::NotAFile);
    };
    let parent = fs
        .canonicalize(parent)
        .map_err(|error| UnreadableReason::of(&error))?;
    let file = stored_name(fs, &parent, file);
    ManifestLocation::new(parent.join(file)).map_err(|_| UnreadableReason::NotAFile)
}

/// The name the folder `parent` stores for `file`: the entry with the same
/// device and inode, read without following it, so a different letter case
/// or Unicode normalization on APFS gives the one stored name. The name as
/// written when it is in the listing, when the file doesn't exist yet, or
/// when the folder can't be listed.
fn stored_name(fs: &dyn Filesystem, parent: &Path, file: &OsStr) -> OsString {
    let written = parent.join(file);
    let Ok(Some(info)) = fs.symlink_metadata(&written) else {
        return file.to_owned();
    };
    let Ok(entries) = fs.read_dir(parent) else {
        return file.to_owned();
    };
    if entries.iter().any(|entry| entry.name == file) {
        return file.to_owned();
    }
    entries
        .into_iter()
        .find(|entry| entry.info.identity == info.identity)
        .map_or_else(|| file.to_owned(), |entry| entry.name)
}

/// Why an environment operation failed. Every message is fixed text; a
/// value from the manifest only reaches a detail, and a location only with
/// the home folder written `~`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EnvOpError {
    /// `env.manifest_unreadable`.
    #[error("the manifest file can't be read")]
    Unreadable(UnreadableReason),
    /// `env.manifest_changed`: the file holds other bytes than the client
    /// planned.
    #[error("the manifest changed since it was planned")]
    Changed,
    /// `env.manifest_invalid`: the file has error findings.
    #[error("the manifest has errors")]
    Invalid {
        /// How many error findings.
        errors: usize,
    },
    /// `env.name_taken`.
    #[error("the environment name is registered at another location")]
    NameTaken {
        /// The name.
        name: EnvironmentName,
        /// Where it is registered, with `~`, if anywhere.
        location: Option<String>,
    },
    /// `env.limit_reached`.
    #[error("the registry already holds the most environments")]
    LimitReached,
    /// `env.not_found`.
    #[error("no environment has that id")]
    NotFound,
    /// `resource.revision_mismatch`.
    #[error("the environment changed since it was read")]
    RevisionMismatch {
        /// The revision the client sent.
        expected: ResourceRevision,
        /// The revision the registry holds.
        actual: ResourceRevision,
    },
    /// `env.lease_active`: the environment still holds an `active` lease.
    #[error("the environment still holds an active lease")]
    LeaseActive {
        /// Each instance it leases, with its name when it is listed.
        instances: Vec<(InstanceId, Option<String>)>,
    },
    /// `env.relink_mismatch`.
    #[error("the file declares another name than the environment")]
    RelinkMismatch {
        /// The name the file declares.
        declared: EnvironmentName,
        /// The name of the environment.
        registered: EnvironmentName,
    },
    /// A registry read or write failed.
    #[error("the registry failed")]
    Registry(RegistryError),
    /// A bug.
    #[error("an environment operation failed unexpectedly")]
    Internal(&'static str),
}

fn key(text: &'static str) -> DetailKey {
    text.parse()
        .expect("the detail keys of spec 0044 are camelCase constants")
}

impl OperationError for EnvOpError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::Unreadable(_) => MANIFEST_UNREADABLE,
            Self::Changed => MANIFEST_CHANGED,
            Self::Invalid { .. } => MANIFEST_INVALID,
            Self::NameTaken { .. } => NAME_TAKEN,
            Self::LimitReached => LIMIT_REACHED,
            Self::NotFound => NOT_FOUND,
            Self::RevisionMismatch { .. } => operation_codes::REVISION_MISMATCH,
            Self::LeaseActive { .. } => LEASE_ACTIVE,
            Self::RelinkMismatch { .. } => RELINK_MISMATCH,
            Self::Registry(error) => registry_code(error),
            Self::Internal(_) => operation_codes::INTERNAL_UNEXPECTED,
        }
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        match self {
            Self::Unreadable(reason) => vec![(key("reason"), reason.as_code().to_owned())],
            Self::Invalid { errors } => vec![(key("errors"), errors.to_string())],
            Self::NameTaken { name, location } => {
                let mut details = vec![(key("name"), name.to_string())];
                if let Some(location) = location {
                    details.push((key("location"), location.clone()));
                }
                details
            }
            Self::LimitReached => vec![(key("limit"), ENVIRONMENT_LIMIT.to_string())],
            Self::RevisionMismatch { expected, actual } => vec![
                (key("expected"), expected.get().to_string()),
                (key("actual"), actual.get().to_string()),
            ],
            Self::LeaseActive { instances } => vec![
                (key("count"), instances.len().to_string()),
                (
                    key("omitted"),
                    instances.len().saturating_sub(LEASE_ITEMS_MAX).to_string(),
                ),
            ],
            Self::RelinkMismatch {
                declared,
                registered,
            } => vec![
                (key("declared"), declared.to_string()),
                (key("registered"), registered.to_string()),
            ],
            Self::Changed | Self::NotFound | Self::Registry(_) | Self::Internal(_) => Vec::new(),
        }
    }

    fn items(&self) -> Vec<FailureItem> {
        let Self::LeaseActive { instances } = self else {
            return Vec::new();
        };
        instances
            .iter()
            .take(LEASE_ITEMS_MAX)
            .filter_map(|(id, name)| {
                let mut fields = vec![(key("instanceId"), id.to_string())];
                if let Some(name) = name {
                    fields.push((key("name"), name.clone()));
                }
                FailureItem::new(ReasonCode::from_static("instance"), fields).ok()
            })
            .collect()
    }
}

fn failed(error: EnvOpError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

fn engine_registry(error: WorkError) -> WorkError {
    match error {
        WorkError::Engine(crate::operation::EngineError::Registry(error)) => {
            failed(EnvOpError::Registry(error))
        }
        other => other,
    }
}

/// What [`register`] writes: the environment of a valid manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterInput {
    /// The ID the operation runs on: the registered environment of the
    /// name, or a new one.
    pub id: EnvironmentId,
    /// `metadata.name`.
    pub name: EnvironmentName,
    /// Where the manifest lives ([`manifest_location`]).
    pub location: ManifestLocation,
    /// The digest of its bytes.
    pub sha256: Sha256Digest,
    /// The snapshot of its services.
    pub declared: Vec<DeclaredService>,
    /// The environment the request relinks to this file, if any.
    pub relink: Option<EnvironmentId>,
}

/// What [`register`] decided.
#[derive(Debug)]
#[non_exhaustive]
pub enum RegisterDecision {
    /// Write these ops in the guarded commit.
    Write(Vec<WriteOp>),
    /// The registry holds this manifest already: nothing to write.
    Unchanged,
    /// The import is refused.
    Refuse(EnvOpError),
}

/// The import guard (AC-11, AC-12), a pure function over the registry
/// inside the guarded commit: it refuses a name registered at another
/// location unless the request relinks that environment, a relink whose
/// file declares another name, and the 65th environment; it clears the
/// location of the environment that held this file before; and it writes
/// nothing when the stored row holds the same location, digest, and
/// snapshot.
///
/// # Errors
///
/// Returns a [`RegistryError`] when the view can't be read.
pub fn register(
    view: &dyn GuardView,
    input: &RegisterInput,
    home: &HomeMask,
) -> Result<RegisterDecision, RegistryError> {
    let environments = view.environments()?;
    let named = environments.iter().find(|env| env.name() == &input.name);
    let target = match input.relink {
        Some(id) => {
            let Some(target) = environments.iter().find(|env| env.id() == id) else {
                return Ok(RegisterDecision::Refuse(EnvOpError::NotFound));
            };
            if target.name() != &input.name {
                return Ok(RegisterDecision::Refuse(EnvOpError::RelinkMismatch {
                    declared: input.name.clone(),
                    registered: target.name().clone(),
                }));
            }
            Some(target)
        }
        None => match named {
            Some(env) if env.manifest_location() == Some(&input.location) => Some(env),
            Some(env) => {
                return Ok(RegisterDecision::Refuse(EnvOpError::NameTaken {
                    name: env.name().clone(),
                    location: env
                        .manifest_location()
                        .map(|location| home.mask(location.as_str()).into_owned()),
                }));
            }
            None => None,
        },
    };
    let target_id = target.map(Environment::id);
    let holder = environments.iter().find(|env| {
        Some(env.id()) != target_id && env.manifest_location() == Some(&input.location)
    });
    let mut ops = Vec::new();
    // The unique index on the location is checked per statement: the
    // environment that held this file lets go of it first.
    if let Some(holder) = holder {
        let mut cleared = holder.clone();
        cleared.clear_manifest_location();
        ops.push(WriteOp::UpdateEnvironment {
            environment: cleared,
            expected: holder.revision(),
        });
    }
    match target {
        Some(target) => {
            if holder.is_none()
                && target.manifest_location() == Some(&input.location)
                && target.manifest_sha256() == Some(&input.sha256)
                && target.declared() == input.declared.as_slice()
            {
                return Ok(RegisterDecision::Unchanged);
            }
            let mut updated = target.clone();
            if updated
                .set_manifest(input.location.clone(), input.sha256, input.declared.clone())
                .is_err()
            {
                return Ok(RegisterDecision::Refuse(EnvOpError::Internal(
                    "a validated snapshot broke an environment invariant",
                )));
            }
            ops.push(WriteOp::UpdateEnvironment {
                environment: updated,
                expected: target.revision(),
            });
        }
        None => {
            if environments.len() >= ENVIRONMENT_LIMIT {
                return Ok(RegisterDecision::Refuse(EnvOpError::LimitReached));
            }
            let Ok(created) = Environment::restore(StoredEnvironment {
                id: input.id,
                name: input.name.clone(),
                manifest_location: Some(input.location.clone()),
                manifest_sha256: Some(input.sha256),
                declared: input.declared.clone(),
                members: Vec::new(),
                revision: ResourceRevision::INITIAL,
            }) else {
                return Ok(RegisterDecision::Refuse(EnvOpError::Internal(
                    "a validated snapshot broke an environment invariant",
                )));
            };
            ops.push(WriteOp::InsertEnvironment(created));
        }
    }
    Ok(RegisterDecision::Write(ops))
}

/// The remove guard (AC-13): refuses an unknown ID, another revision, and
/// an environment that still holds an `active` lease; otherwise deletes
/// it, which takes its members, its `released` demand rows, and its secret
/// uses along.
///
/// # Errors
///
/// Returns a [`RegistryError`] when the view can't be read.
pub fn unregister(
    view: &dyn GuardView,
    id: EnvironmentId,
    expected: ResourceRevision,
) -> Result<RegisterDecision, RegistryError> {
    let environments = view.environments()?;
    let Some(environment) = environments.iter().find(|env| env.id() == id) else {
        return Ok(RegisterDecision::Refuse(EnvOpError::NotFound));
    };
    if environment.revision() != expected {
        return Ok(RegisterDecision::Refuse(EnvOpError::RevisionMismatch {
            expected,
            actual: environment.revision(),
        }));
    }
    let mut leased = Vec::new();
    for member in environment.members() {
        let active = view
            .leases(member.instance_id(), LeaseFilter::Unreleased)?
            .iter()
            .any(|lease| {
                lease.holder() == LeaseHolder::Environment(id)
                    && lease.state() == LeaseState::Active
            });
        if active {
            leased.push(member.instance_id());
        }
    }
    if !leased.is_empty() {
        let instances = view.instances(InstanceFilter {
            include_retained: true,
        })?;
        let named = leased
            .into_iter()
            .map(|leased| {
                let name = instances
                    .iter()
                    .find(|instance| instance.id() == leased)
                    .map(|instance| instance.name().to_string());
                (leased, name)
            })
            .collect();
        return Ok(RegisterDecision::Refuse(EnvOpError::LeaseActive {
            instances: named,
        }));
    }
    Ok(RegisterDecision::Write(vec![WriteOp::DeleteEnvironment {
        id,
        expected,
    }]))
}

/// What `import_environment` works on.
#[derive(Debug, Clone)]
pub struct ImportRequest {
    /// The register input, minus what the work reads again from the file.
    pub id: EnvironmentId,
    /// The path the client sent, absolute.
    pub path: std::path::PathBuf,
    /// Its location ([`manifest_location`]).
    pub location: ManifestLocation,
    /// The digest the client planned.
    pub expected: Sha256Digest,
    /// The environment to relink, if any.
    pub relink: Option<EnvironmentId>,
    /// The catalog to validate against.
    pub catalog: Arc<Catalog>,
    /// The home folder to write `~` in notes and details.
    pub home: HomeMask,
}

/// The `import_environment` work for `request`.
#[must_use]
pub fn import_work(request: ImportRequest) -> Box<dyn OperationWork> {
    Box::new(ImportWork { request })
}

/// The `remove_environment` work: removes `id` at revision `expected`.
#[must_use]
pub fn remove_work(
    id: EnvironmentId,
    expected: ResourceRevision,
    name: EnvironmentName,
) -> Box<dyn OperationWork> {
    Box::new(RemoveWork { id, expected, name })
}

struct ImportWork {
    request: ImportRequest,
}

impl OperationWork for ImportWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_import(self.request, ctx).await })
    }
}

struct RemoveWork {
    id: EnvironmentId,
    expected: ResourceRevision,
    name: EnvironmentName,
}

impl OperationWork for RemoveWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run_remove(self.id, self.expected, self.name, ctx).await })
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

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, WorkError> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| failed(EnvOpError::Internal("a blocking task stopped")))
}

async fn run_import(request: ImportRequest, mut ctx: OperationContext) -> WorkResult {
    let masked = request.home.mask(request.location.as_str()).into_owned();
    ctx.set_audit_note(
        AuditNote::new(None)
            .with("manifestSha256", request.expected)
            .with("location", &masked),
    );
    ctx.begin_step(StepSpec::new(READ)).await?;
    ctx.set_stage(READ).await?;
    // The file is read and validated again under the locks: the bytes the
    // client planned are the bytes that register.
    let platform = Arc::clone(ctx.platform());
    let path = request.path.clone();
    let catalog = Arc::clone(&request.catalog);
    let read = blocking(move || {
        read_manifest(platform.fs(), &path).map(|bytes| {
            let digest = manifest_digest(&bytes);
            (digest, validate(&bytes, &catalog))
        })
    })
    .await?;
    let (digest, validation) = read.map_err(|reason| failed(EnvOpError::Unreadable(reason)))?;
    if digest != request.expected {
        return Err(failed(EnvOpError::Changed));
    }
    let errors = validation
        .findings
        .count_of(super::finding::Severity::Error);
    let (Some(name), true) = (validation.name.clone(), errors == 0) else {
        return Err(failed(EnvOpError::Invalid { errors }));
    };
    ctx.set_audit_note(
        AuditNote::new(None)
            .with("name", &name)
            .with("manifestSha256", digest)
            .with("location", &masked),
    );
    ctx.finish_step(None).await?;

    ctx.begin_step(StepSpec::new(COMMIT)).await?;
    ctx.set_stage(COMMIT).await?;
    let input = RegisterInput {
        id: request.id,
        name,
        location: request.location,
        sha256: digest,
        declared: validation.services,
        relink: request.relink,
    };
    let home = request.home;
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            Ok(match register(view, &input, &home)? {
                RegisterDecision::Write(ops) => GuardDecision::Write(ops),
                RegisterDecision::Unchanged => GuardDecision::Refuse(Box::new(Unchanged)),
                RegisterDecision::Refuse(error) => GuardDecision::Refuse(Box::new(error)),
            })
        }))
        .await
        .map_err(engine_registry)?;
    settle(outcome)?;
    ctx.finish_step(None).await?;
    Ok(settled())
}

async fn run_remove(
    id: EnvironmentId,
    expected: ResourceRevision,
    name: EnvironmentName,
    mut ctx: OperationContext,
) -> WorkResult {
    ctx.set_audit_note(
        AuditNote::new(None)
            .with("name", &name)
            .with("environmentId", id),
    );
    ctx.begin_step(StepSpec::new(COMMIT)).await?;
    ctx.set_stage(COMMIT).await?;
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            Ok(match unregister(view, id, expected)? {
                RegisterDecision::Write(ops) => GuardDecision::Write(ops),
                RegisterDecision::Unchanged => GuardDecision::Refuse(Box::new(Unchanged)),
                RegisterDecision::Refuse(error) => GuardDecision::Refuse(Box::new(error)),
            })
        }))
        .await
        .map_err(|error| match error {
            // The delete itself still refuses a lease the guard did not see.
            WorkError::Engine(crate::operation::EngineError::Registry(
                RegistryError::NotDeletable {
                    reason: NotDeletableReason::ActiveLease,
                    ..
                },
            )) => failed(EnvOpError::LeaseActive {
                instances: Vec::new(),
            }),
            other => engine_registry(other),
        })?;
    settle(outcome)?;
    ctx.finish_step(None).await?;
    Ok(settled())
}

/// The guard found nothing to write: not a failure.
struct Unchanged;

fn settle(outcome: Guarded) -> Result<(), WorkError> {
    match outcome {
        Guarded::Committed(_) => Ok(()),
        Guarded::Refused(reason) if reason.is::<Unchanged>() => Ok(()),
        Guarded::Refused(reason) => Err(failed(match reason.downcast::<EnvOpError>() {
            Ok(error) => *error,
            Err(_) => EnvOpError::Internal("the environment guard refused"),
        })),
    }
}
