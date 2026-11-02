//! Config drift (spec 0029, *Drift*): every render keeps a copy of each non
//! sensitive file it writes in `config-history/<revision-id>/`, and a file
//! in `config/` that differs from its copy, or is gone, is drift. Drift
//! blocks every render until the user keeps the revision, which writes the
//! copies back; Stackly never overwrites a drifted file on its own.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use stackly_domain::{
    ConfigRevisionId, InstanceBlocker, InstanceId, InstanceLifecycle, RelativePath, ResourceRef,
    StageName,
};
use stackly_platform::{FileKind, Filesystem, InstancePaths, PlatformError};

use super::error::SupervisorError;
use super::persist::{blocking, read_instance};
use super::steps::ensure_directories;
use super::{Supervisor, SupervisorDeps};
use crate::operation::{
    BoxFuture, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError, WorkOutcome,
    WorkResult,
};
use crate::{CoreError, check_owned_path};

/// Largest config file the drift check reads, copy or disk.
const MAX_CONFIG_FILE: u64 = 16 * 1024 * 1024;
/// Deepest folder nesting the drift check walks below a history folder.
const MAX_DEPTH: usize = 8;

/// Where one render writes: the config folder, and the history folder of
/// the revision it renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderTarget {
    /// The instance's `config/` folder.
    pub config: PathBuf,
    /// `config-history/<revision-id>/`.
    pub history: PathBuf,
}

impl RenderTarget {
    /// The target of a render of `revision` for the instance at `paths`.
    #[must_use]
    pub fn new(paths: &InstancePaths, revision: ConfigRevisionId) -> Self {
        Self {
            config: paths.config.clone(),
            history: history_dir(paths, revision),
        }
    }
}

/// The history folder of `revision`.
#[must_use]
pub fn history_dir(paths: &InstancePaths, revision: ConfigRevisionId) -> PathBuf {
    paths.config_history.join(revision.to_string())
}

/// How a config file drifted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriftKind {
    /// The file differs from the copy.
    Modified,
    /// The file is gone.
    Missing,
}

/// One drifted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftedFile {
    /// The path relative to `config/`.
    pub path: RelativePath,
    /// How it drifted.
    pub kind: DriftKind,
    /// The copy of the last render.
    pub copy: Vec<u8>,
    /// What is on disk, absent for a missing file.
    pub disk: Option<Vec<u8>>,
}

/// The drifted files of one check, ordered by path; empty means no drift.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriftReport {
    /// The drifted files.
    pub files: Vec<DriftedFile>,
}

impl DriftReport {
    /// Whether nothing drifted.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.files.is_empty()
    }

    /// The blocker this report puts on the instance, observed `at`, or
    /// `None` when nothing drifted.
    #[must_use]
    pub fn blocker(&self, at: Timestamp) -> Option<InstanceBlocker> {
        (!self.is_clean()).then(|| {
            InstanceBlocker::config_drift(self.files.iter().map(|file| file.path.clone()), at)
        })
    }

    /// The paths, as text.
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|file| file.path.to_string())
            .collect()
    }
}

/// Compares every file in the history folder of `revision` with the file
/// at the same path in `config/` (spec 0029, AC-10). A revision without a
/// history folder, such as one rendered before this feature, never drifts.
///
/// # Errors
///
/// Returns the [`PlatformError`] of a folder or file that cannot be read,
/// other than a missing config file, which is drift.
pub fn check(
    fs: &dyn Filesystem,
    paths: &InstancePaths,
    revision: ConfigRevisionId,
) -> Result<DriftReport, PlatformError> {
    check_dirs(fs, &paths.config, &history_dir(paths, revision))
}

/// [`check`] for a config folder and a history folder.
fn check_dirs(
    fs: &dyn Filesystem,
    config: &Path,
    history: &Path,
) -> Result<DriftReport, PlatformError> {
    let mut copies = Vec::new();
    if fs
        .symlink_metadata(history)?
        .is_some_and(|info| info.kind == FileKind::Dir)
    {
        walk(fs, history, Path::new(""), 0, &mut copies)?;
    }
    let mut files = Vec::new();
    for relative in copies {
        let copy = fs.read_file_limited(&history.join(&relative), MAX_CONFIG_FILE)?;
        let on_disk = config.join(&relative);
        let disk = match fs.symlink_metadata(&on_disk)? {
            None => None,
            Some(_) => Some(fs.read_file_limited(&on_disk, MAX_CONFIG_FILE)?),
        };
        let kind = match &disk {
            None => DriftKind::Missing,
            Some(bytes) if *bytes != copy => DriftKind::Modified,
            Some(_) => continue,
        };
        let Some(path) = relative
            .to_str()
            .and_then(|text| text.parse::<RelativePath>().ok())
        else {
            continue;
        };
        files.push(DriftedFile {
            path,
            kind,
            copy,
            disk,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(DriftReport { files })
}

/// Collects the regular files below `dir`, as paths relative to the
/// history folder.
fn walk(
    fs: &dyn Filesystem,
    dir: &Path,
    relative: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
) -> Result<(), PlatformError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for entry in fs.read_dir(&dir.join(relative))? {
        let name = relative.join(&entry.name);
        match entry.info.kind {
            FileKind::Dir => walk(fs, dir, &name, depth + 1, out)?,
            FileKind::File => out.push(name),
            _ => {}
        }
    }
    Ok(())
}

/// Writes the history copy of one rendered file, atomically, before the
/// file in `config/` is written (spec 0029, AC-10). A file outside
/// `config/` keeps no copy.
///
/// # Errors
///
/// Returns the [`CoreError`] of the path check, the folders, or the write.
pub(crate) fn write_copy(
    deps: &SupervisorDeps,
    target: &RenderTarget,
    path: &Path,
    bytes: &[u8],
) -> Result<(), CoreError> {
    let Ok(relative) = path.strip_prefix(&target.config) else {
        return Ok(());
    };
    let copy = target.history.join(relative);
    if let Some(parent) = copy.parent() {
        ensure_directories(deps, &[parent.to_path_buf()])?;
    }
    check_owned_path(&deps.platform, &deps.layout, &copy)?;
    deps.platform.fs().write_file_atomic(&copy, bytes)?;
    Ok(())
}

/// Writes every drifted file of `report` back from its copy, atomically:
/// the work of `resolve_config_drift` (spec 0029, AC-11).
///
/// # Errors
///
/// Returns the [`CoreError`] of the first file that cannot be written.
pub(crate) fn restore(
    deps: &SupervisorDeps,
    target: &RenderTarget,
    report: &DriftReport,
) -> Result<(), CoreError> {
    for file in &report.files {
        let path = target.config.join(file.path.as_str());
        if let Some(parent) = path.parent() {
            ensure_directories(deps, &[parent.to_path_buf()])?;
        }
        check_owned_path(&deps.platform, &deps.layout, &path)?;
        deps.platform.fs().write_file_atomic(&path, &file.copy)?;
    }
    Ok(())
}

/// The drift gate every render passes first (spec 0029, *Drift*): checks
/// `target` and puts the result in the instance's blocker slot, set when
/// something drifted and cleared when nothing did.
///
/// # Errors
///
/// Returns [`SupervisorError::ConfigDrift`] when something drifted, before
/// any file is written, and [`SupervisorError::DataUnavailable`] when the
/// check cannot read a file.
pub(crate) async fn gate(
    sup: &Supervisor,
    id: InstanceId,
    target: &RenderTarget,
) -> Result<(), SupervisorError> {
    let report = observe(sup, id, target).await?;
    if report.is_clean() {
        Ok(())
    } else {
        Err(SupervisorError::ConfigDrift {
            files: report.paths(),
        })
    }
}

/// Runs the drift check of `target` and puts the result in the blocker
/// slot of `id`, without failing on drift.
///
/// # Errors
///
/// Returns [`SupervisorError::DataUnavailable`] when the check cannot
/// read a file.
pub(crate) async fn observe(
    sup: &Supervisor,
    id: InstanceId,
    target: &RenderTarget,
) -> Result<DriftReport, SupervisorError> {
    let deps = sup.deps();
    let platform = deps.platform.clone();
    let history = target.history.clone();
    let config = target.config.clone();
    let report = blocking(move || check_dirs(platform.fs(), &config, &history))
        .await?
        .map_err(|error| SupervisorError::DataUnavailable {
            path: target.config.display().to_string(),
            reason: error.to_string(),
        })?;
    let at = deps.platform.clock().now();
    sup.actor(id)
        .update(deps, |state| state.observe_drift(report.blocker(at)));
    Ok(report)
}

/// Runs the drift check of the active revision of `id` and puts the result
/// in its blocker slot (spec 0029: once per instance when the daemon
/// starts, and on `instance.config.get`). An instance without an active
/// revision has nothing to check.
///
/// # Errors
///
/// Returns [`SupervisorError::Registry`] when the revision cannot be read,
/// and the errors of [`observe`].
pub async fn observe_active(
    sup: &Supervisor,
    id: InstanceId,
) -> Result<DriftReport, SupervisorError> {
    let deps = sup.deps();
    let registry = deps.registry.clone();
    let active = blocking(move || registry.active_config_revision(id))
        .await??
        .value;
    let Some(active) = active else {
        return Ok(DriftReport::default());
    };
    let paths = deps.layout.layout().instance_paths(id);
    observe(sup, id, &RenderTarget::new(&paths, active.id)).await
}

/// What [`Supervisor::resolve_drift_work`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ResolveDriftRequest {
    /// The instance whose config files go back to the active revision.
    pub instance: InstanceId,
}

impl ResolveDriftRequest {
    /// Asks to keep the active revision of `instance`.
    #[must_use]
    pub const fn new(instance: InstanceId) -> Self {
        Self { instance }
    }
}

impl Supervisor {
    /// Builds the operation work of `resolve_config_drift` for the engine
    /// (spec 0029, AC-11): every drifted file is written back from the copy
    /// of the active revision, the check runs again, and the blocker goes.
    /// A running process is not reloaded; it reads the files at its next
    /// reload or restart.
    #[must_use]
    pub fn resolve_drift_work(&self, request: ResolveDriftRequest) -> Box<dyn OperationWork> {
        Box::new(ResolveDriftWork {
            sup: self.clone(),
            request,
        })
    }
}

struct ResolveDriftWork {
    sup: Supervisor,
    request: ResolveDriftRequest,
}

impl OperationWork for ResolveDriftWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { resolve(&self.sup, self.request.instance, ctx).await })
    }
}

async fn resolve(sup: &Supervisor, id: InstanceId, mut ctx: OperationContext) -> WorkResult {
    let failed = |error: SupervisorError| WorkError::Failed(Box::new(error));
    let deps = sup.deps();
    ctx.begin_step(StepSpec::new(StageName::VALIDATE)).await?;
    ctx.set_stage(StageName::VALIDATE).await?;
    let instance = read_instance(deps, id).await.map_err(failed)?;
    if instance.lifecycle() == InstanceLifecycle::Retained {
        return Err(failed(SupervisorError::InstanceRetained));
    }
    let registry = deps.registry.clone();
    let active = blocking(move || registry.active_config_revision(id))
        .await
        .map_err(failed)?
        .map_err(|error| failed(error.into()))?
        .value
        .ok_or_else(|| {
            failed(SupervisorError::PackageUnavailable {
                provider: instance.provider_revision().to_string(),
                reason: "no_config_revision".to_owned(),
            })
        })?;
    let paths = deps.layout.layout().instance_paths(id);
    let target = RenderTarget::new(&paths, active.id);
    let report = observe(sup, id, &target).await.map_err(failed)?;

    ctx.begin_step(StepSpec::new(StageName::RENDER).irreversible())
        .await?;
    ctx.set_stage(StageName::RENDER).await?;
    if !report.is_clean() {
        let deps = deps.clone();
        let target = target.clone();
        blocking(move || restore(&deps, &target, &report))
            .await
            .map_err(failed)?
            .map_err(|error| {
                failed(SupervisorError::DataUnavailable {
                    path: paths.config.display().to_string(),
                    reason: error.to_string(),
                })
            })?;
    }

    ctx.begin_step(StepSpec::new(StageName::VERIFY)).await?;
    ctx.set_stage(StageName::VERIFY).await?;
    let after = observe(sup, id, &target).await.map_err(failed)?;
    if !after.is_clean() {
        return Err(failed(SupervisorError::ConfigDrift {
            files: after.paths(),
        }));
    }
    Ok(WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Instance(id)],
            partial: Vec::new(),
        },
        finalize: None,
    })
}
