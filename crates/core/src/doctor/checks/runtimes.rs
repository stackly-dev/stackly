//! `runtimes`: managed runtimes the registry marks `missing`, `drifted`,
//! or `failed`, and, for every installed one, whether its folder and each
//! executable its install recorded are still in place as regular files of
//! this user that can run. It never changes a state, computes a digest, or
//! offers a reinstall (feature 55).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};
use stackly_domain::{InstallMode, InstanceName, RuntimeState};
use stackly_platform::{FileKind, UserId};

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Finding, FindingFacts};

/// The owner may run it.
const OWNER_EXECUTE: u32 = 0o100;

#[derive(Deserialize)]
struct Signature {
    #[serde(default)]
    executables: Vec<Executable>,
}

#[derive(Deserialize)]
struct Executable {
    path: String,
}

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let registry = context.registry()?;
    let uid = context.uid()?;
    let runtimes = registry
        .runtimes()
        .map_err(|error| CheckFailure::registry(&error))?
        .value;
    let instances = context.all_instances()?;
    let layout = context.sources.layout.layout();
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for runtime in runtimes {
        if stopped(stop) {
            break;
        }
        let InstallMode::Managed { artifact_sha256 } = runtime.install() else {
            continue;
        };
        checked += 1;
        let used_by: Vec<InstanceName> = instances
            .iter()
            .filter(|instance| instance.runtime_id() == runtime.id())
            .map(|instance| instance.name().clone())
            .collect();
        let severity = if used_by.is_empty() {
            DoctorSeverity::Warning
        } else {
            DoctorSeverity::Error
        };
        let folder = layout.runtime_dir(
            runtime.software(),
            runtime.version(),
            runtime.target(),
            artifact_sha256,
        );
        let subject = format!("{} {}", runtime.software(), runtime.version());
        let (code, missing) = match runtime.state() {
            RuntimeState::Missing | RuntimeState::Drifted | RuntimeState::Failed => {
                (DoctorFindingCode::RuntimeState, Vec::new())
            }
            RuntimeState::Installed => {
                let executables = registry
                    .artifact(artifact_sha256)
                    .ok()
                    .and_then(|versioned| versioned.value)
                    .and_then(|artifact| artifact.signature_info)
                    .and_then(|json| serde_json::from_str::<Signature>(json.as_str()).ok())
                    .map(|signature| signature.executables)
                    .unwrap_or_default();
                let missing = missing_files(context, &folder, &executables, &uid);
                if missing.is_empty() {
                    continue;
                }
                (DoctorFindingCode::RuntimeFilesMissing, missing)
            }
            _ => continue,
        };
        findings.push(
            Finding::new(
                code,
                severity,
                FindingFacts::Runtime {
                    runtime: Box::new(runtime.clone()),
                    folder,
                    used_by,
                    missing,
                },
            )
            .subject(subject),
        );
    }
    Ok(CheckFindings::checked(findings, checked))
}

/// The paths that are not in place: the folder itself (an empty string)
/// when it is not a folder, else each executable that is not a regular
/// file of `uid` with the owner's execute bit.
fn missing_files(
    context: &CheckContext,
    folder: &Path,
    executables: &[Executable],
    uid: &UserId,
) -> Vec<String> {
    let fs = context.sources.platform.fs();
    match fs.symlink_metadata(folder) {
        Ok(Some(info)) if info.kind == FileKind::Dir => {}
        _ => return vec![String::new()],
    }
    executables
        .iter()
        .filter(|executable| {
            let relative = Path::new(&executable.path);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return false;
            }
            !matches!(
                fs.symlink_metadata(&folder.join(relative)),
                Ok(Some(info)) if info.kind == FileKind::File
                    && info.owner.as_ref().is_none_or(|owner| owner == uid)
                    && info.mode.is_none_or(|mode| mode & OWNER_EXECUTE != 0)
            )
        })
        .map(|executable| executable.path.clone())
        .collect()
}
