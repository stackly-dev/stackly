//! Moving a validated staging tree to its place (spec 0018, *Commit atomik,
//! marker, dan folder yatim*): one rename that refuses an existing target,
//! after a leftover folder of an operation that never committed is moved
//! out of the way.
//!
//! Nothing here deletes anything in `runtimes/`: an orphan goes to the
//! scratch folder of the operation, which the engine removes with its own
//! scratch.

use std::path::{Path, PathBuf};

use stackly_domain::{
    ExactVersion, InstallMode, OperationId, Registry, RegistryError, Sha256Digest, SoftwareName,
    Target,
};
use stackly_platform::{FileKind, Platform, PlatformError};

use super::validate::MARKER_FILE;
use crate::layout::PreparedLayout;
use crate::ownership::check_owned_path;

/// The longest marker file read, in bytes.
const MARKER_READ_MAX: u64 = 4096;

/// What a commit needs.
pub struct CommitRequest<'a> {
    /// The platform ports.
    pub platform: &'a Platform,
    /// The checked layout.
    pub layout: &'a PreparedLayout,
    /// The registry, read to decide about a folder that is already there.
    pub registry: &'a dyn Registry,
    /// The software of the runtime.
    pub software: &'a SoftwareName,
    /// The exact version.
    pub version: &'a ExactVersion,
    /// The target of the artifact.
    pub target: Target,
    /// The SHA-256 of the artifact.
    pub sha256: &'a Sha256Digest,
    /// The staged tree.
    pub stage_tree: &'a Path,
    /// The scratch folder of the operation, where an orphan is moved.
    pub scratch: &'a Path,
    /// The operation that commits.
    pub operation_id: &'a str,
}

/// How a commit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOutcome {
    /// The staged tree was moved to its place.
    Moved,
    /// The folder was already there with the marker of this operation: the
    /// rename happened before a restart.
    AlreadyMoved,
    /// A leftover folder of an operation that never committed was moved out
    /// of the way, then the staged tree was moved to its place.
    MovedAfterOrphan,
}

/// Why a commit failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CommitError {
    /// The target folder exists and is not ours to take over: its marker is
    /// missing, broken, of another artifact, of an operation that is still
    /// open, or the artifact already has a runtime.
    #[error("{} belongs to something else", path.display())]
    Conflict {
        /// The folder.
        path: PathBuf,
    },
    /// A platform call failed.
    #[error("{0}")]
    Platform(PlatformError),
    /// The registry could not be read.
    #[error("the registry could not be read: {0}")]
    Registry(RegistryError),
}

impl From<PlatformError> for CommitError {
    fn from(error: PlatformError) -> Self {
        Self::Platform(error)
    }
}

/// The fields of the marker that matter for ownership.
struct MarkerFacts {
    operation_id: String,
    sha256: String,
}

fn read_marker(platform: &Platform, folder: &Path) -> Option<MarkerFacts> {
    let bytes = platform
        .fs()
        .read_file_limited(&folder.join(MARKER_FILE), MARKER_READ_MAX)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if value.get("schema")?.as_str()? != "stackly.runtime-marker/v1" {
        return None;
    }
    Some(MarkerFacts {
        operation_id: value.get("operationId")?.as_str()?.to_owned(),
        sha256: value.get("sha256")?.as_str()?.to_owned(),
    })
}

/// Moves the staged tree to `runtime_dir(software, version, target, sha256)`.
///
/// 1. A target with the marker of this operation is the rename that
///    happened before a restart.
/// 2. A target with the marker of the same artifact, while the registry has
///    no runtime for it and the operation named in the marker is final or
///    unknown, is an orphan: it moves to `scratch/orphan/`.
/// 3. Any other target is a conflict.
/// 4. The parent folders are made one by one, then `rename_no_replace`
///    moves the tree.
///
/// # Errors
///
/// Returns [`CommitError::Conflict`] for a target that belongs to something
/// else, and [`CommitError::Platform`] or [`CommitError::Registry`] when a
/// call fails.
pub fn commit_tree(request: &CommitRequest<'_>) -> Result<CommitOutcome, CommitError> {
    let platform = request.platform;
    let fs = platform.fs();
    let layout = request.layout.layout();
    let target = layout.runtime_dir(
        request.software,
        request.version,
        request.target,
        request.sha256,
    );
    let mut outcome = CommitOutcome::Moved;
    if fs.symlink_metadata(&target)?.is_some() {
        let Some(marker) = read_marker(platform, &target) else {
            return Err(CommitError::Conflict { path: target });
        };
        if marker.operation_id == request.operation_id {
            return Ok(CommitOutcome::AlreadyMoved);
        }
        if marker.sha256 == request.sha256.to_string()
            && !has_runtime(request.registry, request.sha256)?
            && operation_ended(request.registry, &marker.operation_id)?
        {
            move_orphan(request, &target)?;
            outcome = CommitOutcome::MovedAfterOrphan;
        } else {
            return Err(CommitError::Conflict { path: target });
        }
    }
    make_parents(request, &target)?;
    let from = check_owned_path(platform, request.layout, request.stage_tree)
        .map_err(|error| core_error("check the staged tree", &error))?;
    let to = check_owned_path(platform, request.layout, &target)
        .map_err(|error| core_error("check the runtime folder", &error))?;
    if from.root() != to.root() {
        return Err(CommitError::Platform(PlatformError::CrossesDevice {
            path: target,
        }));
    }
    match fs.rename_no_replace(request.stage_tree, &target) {
        Ok(()) => Ok(outcome),
        Err(PlatformError::AlreadyExists { .. }) => Err(CommitError::Conflict { path: target }),
        Err(error) => Err(error.into()),
    }
}

fn core_error(op: &'static str, error: &crate::CoreError) -> CommitError {
    CommitError::Platform(PlatformError::Io {
        op,
        path: PathBuf::new(),
        kind: std::io::ErrorKind::InvalidInput,
        message: error.to_string(),
    })
}

fn has_runtime(registry: &dyn Registry, sha256: &Sha256Digest) -> Result<bool, CommitError> {
    let runtimes = registry.runtimes().map_err(CommitError::Registry)?;
    Ok(runtimes.value.iter().any(|runtime| {
        matches!(runtime.install(), InstallMode::Managed { artifact_sha256 } if artifact_sha256 == sha256)
    }))
}

/// Whether the operation a marker names is final, or gone from the registry
/// (retention pruned it).
fn operation_ended(registry: &dyn Registry, operation_id: &str) -> Result<bool, CommitError> {
    let Ok(id) = operation_id.parse::<OperationId>() else {
        return Ok(false);
    };
    let found = registry.operation(id).map_err(CommitError::Registry)?;
    Ok(found
        .value
        .is_none_or(|operation| operation.state().is_final()))
}

fn move_orphan(request: &CommitRequest<'_>, target: &Path) -> Result<(), CommitError> {
    let fs = request.platform.fs();
    let orphans = request.scratch.join("orphan");
    match fs.create_dir_private(&orphans) {
        Ok(()) | Err(PlatformError::AlreadyExists { .. }) => {}
        Err(error) => return Err(error.into()),
    }
    let destination = (0..)
        .map(|n| orphans.join(format!("runtime-{n}")))
        .find(|path| matches!(fs.symlink_metadata(path), Ok(None)))
        .expect("an unbounded range holds a free name");
    fs.rename_no_replace(target, &destination)?;
    Ok(())
}

/// Makes `runtimes/<software>/<version>/<os>-<arch>/` one folder at a time;
/// a folder that is already there is accepted when it is a plain folder.
fn make_parents(request: &CommitRequest<'_>, target: &Path) -> Result<(), CommitError> {
    let fs = request.platform.fs();
    let root = request.layout.layout().runtimes_dir();
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    let mut chain: Vec<&Path> = parent
        .ancestors()
        .take_while(|path| *path != root.as_path())
        .collect();
    chain.reverse();
    for dir in chain {
        match fs.create_dir_private(dir) {
            Ok(()) => {}
            Err(PlatformError::AlreadyExists { .. }) => {
                let plain = fs
                    .symlink_metadata(dir)?
                    .is_some_and(|info| info.kind == FileKind::Dir);
                if !plain {
                    return Err(CommitError::Conflict {
                        path: dir.to_path_buf(),
                    });
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
