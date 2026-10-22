//! Gathering what a plan needs, right before an action (spec 0010,
//! *Value sourcing*): the instance, its runtime and the runtimes that one
//! depends on, the pinned provider package, the instance folder, and the
//! settings of the active config revision.
//!
//! Plans are built again before every action from the pinned package,
//! never read back, so every action calls [`prepare`] first.

use std::collections::BTreeMap;
use std::path::PathBuf;

use stackly_domain::{
    Endpoint, EndpointName, InstallMode, Instance, InstanceId, InstanceLifecycle, Port, Runtime,
    RuntimeState, ShortKey,
};
use stackly_platform::{InstancePaths, check_private_dir};

use super::Supervisor;
use super::error::SupervisorError;
use super::persist::{blocking, read_instance};
use crate::provider::{
    NameCheck, PlanContext, ResolvedEndpoint, RuntimeContext, SettingsValues, ValidatedPackage,
    read_package,
};

/// Everything the plans of one action are built from.
pub(crate) struct Prepared {
    /// The instance as the registry holds it now.
    pub(crate) instance: Instance,
    /// Its runtime.
    pub(crate) runtime: Runtime,
    /// The canonical root of that runtime's folder.
    pub(crate) runtime_root: PathBuf,
    /// The pinned package, with the digest of the instance's revision.
    pub(crate) package: ValidatedPackage,
    /// The instance's folders.
    pub(crate) paths: InstancePaths,
    /// The short key of the run folder.
    pub(crate) key: ShortKey,
    /// The validated settings of the active config revision.
    pub(crate) settings: SettingsValues,
}

impl Prepared {
    /// Returns the context of a plan, with `ports` as the port of each IP
    /// endpoint that needs one decided now; every other IP endpoint keeps
    /// its effective port, and an `Auto` endpoint with neither has none.
    pub(crate) fn context(&self, ports: &BTreeMap<EndpointName, Port>) -> PlanContext {
        let endpoints = self
            .instance
            .endpoints()
            .iter()
            .map(|endpoint| match endpoint {
                Endpoint::Ip(ip) => (
                    ip.name().clone(),
                    ResolvedEndpoint::Ip {
                        host: ip.address(),
                        port: ports
                            .get(ip.name())
                            .copied()
                            .or_else(|| ip.effective_port()),
                    },
                ),
                Endpoint::Unix(unix) => (unix.name().clone(), ResolvedEndpoint::Unix),
            })
            .collect();
        PlanContext {
            instance_id: self.instance.id(),
            instance_name: self.instance.name().clone(),
            paths: self.paths.clone(),
            runtime: RuntimeContext {
                root: self.runtime_root.clone(),
                version: self.runtime.version().clone(),
                line: self.runtime.line().clone(),
            },
            endpoints,
            settings: self.settings.clone(),
        }
    }
}

/// How much of the instance and its surroundings an action needs in order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Checks {
    /// Everything a start checks: runtimes installed, package pinned and
    /// intact, instance folder private and the user's, settings valid.
    Full,
    /// What a stop or a reload of a running process needs: the package and
    /// the folders to build its plan; the runtime may be gone or changed.
    Plan,
}

/// Gathers what the plans of `id` need.
///
/// # Errors
///
/// Returns the first failure of the checks, with the code of spec 0010
/// `Kode error` for it.
pub(crate) async fn prepare(
    sup: &Supervisor,
    id: InstanceId,
    checks: Checks,
) -> Result<Prepared, SupervisorError> {
    let deps = sup.deps();
    let instance = read_instance(deps, id).await?;
    if instance.lifecycle() == InstanceLifecycle::Retained {
        return Err(SupervisorError::InstanceRetained);
    }
    let platform = deps.platform.clone();
    let registry = deps.registry.clone();
    let layout = deps.layout.clone();
    let loaded = blocking({
        let instance = instance.clone();
        move || -> Result<Loaded, SupervisorError> {
            let runtime = registry
                .runtime(instance.runtime_id())
                .map_err(SupervisorError::Registry)?
                .value
                .ok_or_else(|| runtime_unavailable(instance.runtime_id().to_string(), "missing"))?;
            if checks == Checks::Full {
                require_installed(&*registry, &runtime)?;
            }
            let runtime_root = runtime_root(&platform, &layout, &runtime)?;
            let package = load_package(&platform, &layout, &instance)?;
            let paths = layout.layout().instance_paths(instance.id());
            if checks == Checks::Full {
                check_instance_folder(&platform, &paths)?;
            }
            let settings = settings(&*registry, &package, instance.id())?;
            Ok(Loaded {
                runtime,
                runtime_root,
                package,
                paths,
                settings,
            })
        }
    })
    .await??;
    Ok(Prepared {
        key: id.short_key(),
        instance,
        runtime: loaded.runtime,
        runtime_root: loaded.runtime_root,
        package: loaded.package,
        paths: loaded.paths,
        settings: loaded.settings,
    })
}

struct Loaded {
    runtime: Runtime,
    runtime_root: PathBuf,
    package: ValidatedPackage,
    paths: InstancePaths,
    settings: SettingsValues,
}

fn runtime_unavailable(runtime_id: String, state: &str) -> SupervisorError {
    SupervisorError::RuntimeUnavailable {
        runtime_id,
        state: state.to_owned(),
    }
}

/// The runtime and every runtime it depends on must be `installed`.
pub(crate) fn require_installed(
    registry: &dyn stackly_domain::Registry,
    runtime: &Runtime,
) -> Result<(), SupervisorError> {
    if runtime.state() != RuntimeState::Installed {
        return Err(runtime_unavailable(
            runtime.id().to_string(),
            runtime.state().as_code(),
        ));
    }
    for dependency in runtime.depends_on() {
        let found = registry
            .runtime(*dependency)
            .map_err(SupervisorError::Registry)?
            .value;
        match found {
            Some(found) if found.state() == RuntimeState::Installed => {}
            Some(found) => {
                return Err(runtime_unavailable(
                    dependency.to_string(),
                    found.state().as_code(),
                ));
            }
            None => return Err(runtime_unavailable(dependency.to_string(), "missing")),
        }
    }
    Ok(())
}

/// The canonical folder of the runtime: the content addressed folder of a
/// managed one, the folder the user registered for an external one.
fn runtime_root(
    platform: &stackly_platform::Platform,
    layout: &crate::PreparedLayout,
    runtime: &Runtime,
) -> Result<PathBuf, SupervisorError> {
    let folder = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => layout.layout().runtime_dir(
            runtime.software(),
            runtime.version(),
            runtime.target(),
            artifact_sha256,
        ),
        InstallMode::External { root, .. } => root.clone(),
    };
    platform
        .fs()
        .canonicalize(&folder)
        .map_err(|_| runtime_unavailable(runtime.id().to_string(), "missing"))
}

/// Reads the pinned package of the instance's provider revision and checks
/// that its digest is the one the instance records.
pub(crate) fn load_package(
    platform: &stackly_platform::Platform,
    layout: &crate::PreparedLayout,
    instance: &Instance,
) -> Result<ValidatedPackage, SupervisorError> {
    let key = instance.provider_revision();
    let unavailable = |reason: String| SupervisorError::PackageUnavailable {
        provider: key.to_string(),
        reason,
    };
    let dir = layout.layout().provider_revision_dir(key);
    let files = read_package(platform.fs(), &dir)
        .map_err(|error| unavailable(format!("the pinned package cannot be read: {error}")))?;
    let package = ValidatedPackage::from_files(files, NameCheck::Skip)
        .map_err(|error| unavailable(format!("the pinned package is invalid: {error}")))?;
    if package.key() != key {
        return Err(unavailable(
            "the pinned package has another digest than the instance records".to_owned(),
        ));
    }
    Ok(package)
}

/// The instance folder must be a private folder of the current user.
pub(crate) fn check_instance_folder(
    platform: &stackly_platform::Platform,
    paths: &InstancePaths,
) -> Result<(), SupervisorError> {
    let unavailable = |reason: String| SupervisorError::DataUnavailable {
        path: paths.root.display().to_string(),
        reason,
    };
    let info = platform
        .fs()
        .symlink_metadata(&paths.root)
        .map_err(|error| unavailable(error.to_string()))?
        .ok_or_else(|| unavailable("the folder is missing".to_owned()))?;
    let user = platform
        .identity()
        .current_user()
        .map_err(|error| unavailable(error.to_string()))?;
    check_private_dir(&info, &user).map_err(|violation| unavailable(violation.to_string()))
}

/// The settings of the active config revision, validated again against the
/// schema of the pinned package.
fn settings(
    registry: &dyn stackly_domain::Registry,
    package: &ValidatedPackage,
    id: InstanceId,
) -> Result<SettingsValues, SupervisorError> {
    let invalid = |reason: String| SupervisorError::ConfigInvalid {
        field: "settings".to_owned(),
        reason,
    };
    let revision = registry
        .active_config_revision(id)
        .map_err(SupervisorError::Registry)?
        .value
        .ok_or_else(|| invalid("the instance has no active config revision".to_owned()))?;
    let document: serde_json::Value = serde_json::from_str(revision.values.as_str())
        .map_err(|error| invalid(format!("the stored settings are not JSON: {error}")))?;
    package
        .settings_schema()
        .validate(&document)
        .map_err(|error| invalid(error.to_string()))
}
