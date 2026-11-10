//! The provider remedy of a `start_failed` or `not_ready` finding (spec
//! 0034, AC-10, *Entri PostgreSQL*).
//!
//! The entry comes from the package revision of the provider in the
//! catalog, since a remedy is only text and a newer revision may know more;
//! when the catalog has no usable package for the provider, or its entry
//! does not render for the instance, from the package the instance pins;
//! when neither loads, there is no remedy (`package_unreadable` explains
//! it). The first entry in manifest order whose `when` matches wins.
//!
//! The PID file of an entry is read passively: its canonical folder must
//! lie under the canonical data or config folder of the instance, the file
//! is opened without following a symlink, at most 4 KiB are read, and only
//! a decimal PID on the first line is taken from it. The PID is looked up
//! with `inspect`; nothing is signalled, locked, removed, or written.

use std::io::Read;
use std::path::{Path, PathBuf};

use stackly_domain::{Endpoint, InstallMode, Instance, Pid, Runtime};
use stackly_platform::{PlatformError, ProcessError};

use super::CheckContext;
use crate::catalog::CatalogEntry;
use crate::doctor::types::{PidFileFacts, PidFileState, RemedyFacts};
use crate::provider::{
    PlanContext, RemedySubject, ResolvedEndpoint, RuntimeContext, SettingsValues, ValidatedPackage,
    render_remedy,
};
use crate::supervisor::prepare::load_package;

/// Most bytes read from a PID file.
const PID_FILE_READ_MAX: u64 = 4096;

/// The remedy for `instance` and `subject`, if a package has one that
/// matches and renders for the instance.
pub(super) fn find(
    context: &CheckContext,
    instance: &Instance,
    subject: &RemedySubject<'_>,
) -> Option<RemedyFacts> {
    let registry = context.registry().ok()?;
    let runtime = registry.runtime(instance.runtime_id()).ok()?.value?;
    let plan = plan_context(context, instance, &runtime);
    if let Some(package) = catalog_package(context, instance)
        && let Some(found) = from_package(context, &package, &plan, subject)
    {
        return found;
    }
    let pinned = load_package(&context.sources.platform, &context.sources.layout, instance).ok()?;
    from_package(context, &pinned, &plan, subject).flatten()
}

/// The package of the instance's provider in the loaded catalog.
fn catalog_package(context: &CheckContext, instance: &Instance) -> Option<ValidatedPackage> {
    let catalog = context.sources.catalog.as_ref()?;
    match catalog.entry(instance.provider_revision().provider())? {
        CatalogEntry::Provider(entry) => Some((*entry.package).clone()),
        _ => None,
    }
}

/// The first entry of `package` that matches `subject`, rendered: `None`
/// when the package cannot serve the instance (an entry does not render
/// for it), `Some(None)` when no entry matches.
fn from_package(
    context: &CheckContext,
    package: &ValidatedPackage,
    plan: &PlanContext,
    subject: &RemedySubject<'_>,
) -> Option<Option<RemedyFacts>> {
    let manifest = package.manifest();
    let Some(spec) = manifest
        .remedies()
        .iter()
        .find(|remedy| remedy.matches(subject))
    else {
        return Some(None);
    };
    let remedy = render_remedy(manifest, spec, plan).ok()?;
    let pid_file = remedy
        .pid_file
        .as_deref()
        .map(|path| read_pid_file(context, plan, path, &executables(package, plan)));
    Some(Some(RemedyFacts {
        package: package.key().clone(),
        display_name: manifest.display_name().to_owned(),
        remedy,
        pid_file,
    }))
}

/// The plan context of a render without settings: the instance's folders,
/// its runtime, and the effective port of every endpoint, as the detail of
/// spec 0021 builds it.
fn plan_context(context: &CheckContext, instance: &Instance, runtime: &Runtime) -> PlanContext {
    let layout = &context.sources.layout;
    let folder = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => layout.layout().runtime_dir(
            runtime.software(),
            runtime.version(),
            runtime.target(),
            artifact_sha256,
        ),
        InstallMode::External { root, .. } => root.clone(),
    };
    let root = context
        .sources
        .platform
        .fs()
        .canonicalize(&folder)
        .unwrap_or(folder);
    let endpoints = instance
        .endpoints()
        .iter()
        .map(|endpoint| match endpoint {
            Endpoint::Ip(ip) => (
                ip.name().clone(),
                ResolvedEndpoint::Ip {
                    host: ip.address(),
                    port: ip.effective_port(),
                },
            ),
            Endpoint::Unix(unix) => (unix.name().clone(), ResolvedEndpoint::Unix),
        })
        .collect();
    PlanContext {
        instance_id: instance.id(),
        instance_name: instance.name().clone(),
        paths: layout.instance_paths(instance.id()),
        runtime: RuntimeContext {
            root,
            version: runtime.version().clone(),
            line: runtime.line().clone(),
        },
        endpoints,
        settings: SettingsValues::default(),
    }
}

/// Every executable of the instance's runtime the package names: the main
/// one and each alias.
fn executables(package: &ValidatedPackage, plan: &PlanContext) -> Vec<PathBuf> {
    let manifest = package.manifest();
    std::iter::once(&manifest.executable)
        .chain(manifest.aliases.values())
        .map(|components| {
            let mut path = plan.runtime.root.clone();
            path.extend(components);
            path
        })
        .collect()
}

/// Reads the PID file at `path` passively and says what its PID is.
fn read_pid_file(
    context: &CheckContext,
    plan: &PlanContext,
    path: &Path,
    executables: &[PathBuf],
) -> PidFileFacts {
    let facts = |state, pid| PidFileFacts {
        path: path.to_path_buf(),
        pid,
        state,
        process: None,
        other_user: false,
    };
    let pid = match pid_of(context, plan, path) {
        Ok(pid) => pid,
        Err(state) => return facts(state, None),
    };
    let uid = context.uid().ok();
    match context.sources.platform.processes().inspect(pid) {
        Ok(None) => facts(PidFileState::NotRunning, Some(pid)),
        Ok(Some(process)) if uid.as_ref() != Some(&process.uid) => PidFileFacts {
            process: Some(process),
            other_user: true,
            ..facts(PidFileState::OtherProcess, Some(pid))
        },
        Ok(Some(process)) => {
            let state = if executables.contains(&process.executable) {
                PidFileState::RuntimeProcess
            } else {
                PidFileState::OtherProcess
            };
            PidFileFacts {
                process: Some(process),
                ..facts(state, Some(pid))
            }
        }
        Err(ProcessError::PermissionDenied { .. }) => PidFileFacts {
            other_user: true,
            ..facts(PidFileState::OtherProcess, Some(pid))
        },
        Err(_) => facts(PidFileState::Unreadable, Some(pid)),
    }
}

/// The PID on the first line of the file, or the state of a file that has
/// none.
fn pid_of(context: &CheckContext, plan: &PlanContext, path: &Path) -> Result<Pid, PidFileState> {
    let fs = context.sources.platform.fs();
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(PidFileState::Unreadable);
    };
    let parent = fs.canonicalize(parent).map_err(|error| match error {
        PlatformError::NotFound { .. } => PidFileState::Missing,
        _ => PidFileState::Unreadable,
    })?;
    let inside = [&plan.paths.data, &plan.paths.config]
        .into_iter()
        .filter_map(|folder| fs.canonicalize(folder).ok())
        .any(|folder| parent.starts_with(folder));
    if !inside {
        return Err(PidFileState::Unreadable);
    }
    let file = fs
        .open_read(&parent.join(name))
        .map_err(|error| match error {
            PlatformError::NotFound { .. } => PidFileState::Missing,
            _ => PidFileState::Unreadable,
        })?;
    let mut bytes = Vec::new();
    file.take(PID_FILE_READ_MAX)
        .read_to_end(&mut bytes)
        .map_err(|_| PidFileState::Unreadable)?;
    parse_pid(&bytes).ok_or(PidFileState::Unreadable)
}

/// A decimal `u32` PID on the first line, around which only ASCII spaces,
/// tabs, and a carriage return may stand.
fn parse_pid(bytes: &[u8]) -> Option<Pid> {
    let line = bytes.split(|byte| *byte == b'\n').next()?;
    let line = line.trim_ascii();
    if line.is_empty() || !line.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let number: u32 = std::str::from_utf8(line).ok()?.parse().ok()?;
    Pid::new(number).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_decimal_pid_on_the_first_line_is_taken() {
        assert_eq!(parse_pid(b"4242\n/data\n").map(Pid::get), Some(4242));
        assert_eq!(parse_pid(b" 77 \r\nrest").map(Pid::get), Some(77));
        assert_eq!(parse_pid(b"4242").map(Pid::get), Some(4242));
        assert_eq!(parse_pid(b"-1\n"), None);
        assert_eq!(parse_pid(b"0\n"), None);
        assert_eq!(parse_pid(b"12ab\n"), None);
        assert_eq!(parse_pid(b"4294967296\n"), None);
        assert_eq!(parse_pid(b"\n4242\n"), None);
        assert_eq!(parse_pid(b""), None);
        assert_eq!(parse_pid("٤٢".as_bytes()), None);
    }
}
