//! The facts the config evaluator reads for one instance (spec 0029,
//! *Evaluator config*, *Value sourcing*): the registry rows, the pinned
//! package, and the runtime folder. No catalog, network, or Keychain.

use stackly_domain::{
    Capability, ConfigRevisionRecord, InstallMode, Instance, InstanceId, InstanceLifecycle,
    Registry,
};
use stackly_platform::{InstancePaths, Platform};

use super::CONFIG_HISTORY_KEEP;
use crate::PreparedLayout;
use crate::provider::{RuntimeContext, SettingsValues, ValidatedPackage};
use crate::supervisor::SupervisorError;
use crate::supervisor::prepare::load_package;

/// Whether the instance's runtime folder can render a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFact {
    /// The runtime is there: previews render with it.
    Present(RuntimeContext),
    /// The runtime row or folder is gone: `get` still answers, without a
    /// preview ("Runtime missing, preview unavailable").
    Missing,
}

/// Everything `get`, `validate`, and the apply precheck evaluate.
#[derive(Debug, Clone)]
pub struct ConfigFacts {
    /// The instance as the registry holds it.
    pub instance: Instance,
    /// Its active config revision.
    pub active: ConfigRevisionRecord,
    /// The newest revisions, newest number first, the active one included.
    pub history: Vec<ConfigRevisionRecord>,
    /// The last known good revision, if any.
    pub last_known_good: Option<ConfigRevisionRecord>,
    /// The pinned package.
    pub package: ValidatedPackage,
    /// The active settings, validated against the pinned schema.
    pub settings: SettingsValues,
    /// The instance's folders.
    pub paths: InstancePaths,
    /// The runtime, for previews.
    pub runtime: RuntimeFact,
    /// Whether the pinned provider revision declares `reload`.
    pub reload_supported: bool,
}

/// Reads the facts of `id` (spec 0029, *Evaluator config*).
///
/// # Errors
///
/// Returns [`SupervisorError::NotFound`] for an unknown instance,
/// [`SupervisorError::InstanceRetained`] for a retained one,
/// [`SupervisorError::PackageUnavailable`] with reason
/// `no_config_revision` when it has no revision or with the load error
/// when the pinned package is missing or changed,
/// [`SupervisorError::ConfigInvalid`] when the stored settings no longer
/// validate, and [`SupervisorError::Registry`] for a registry failure.
pub fn gather_config(
    platform: &Platform,
    layout: &PreparedLayout,
    registry: &dyn Registry,
    id: InstanceId,
) -> Result<ConfigFacts, SupervisorError> {
    let instance = registry
        .instance(id)?
        .value
        .ok_or(SupervisorError::NotFound(id))?;
    if instance.lifecycle() == InstanceLifecycle::Retained {
        return Err(SupervisorError::InstanceRetained);
    }
    let active = registry.active_config_revision(id)?.value.ok_or_else(|| {
        SupervisorError::PackageUnavailable {
            provider: instance.provider_revision().to_string(),
            reason: "no_config_revision".to_owned(),
        }
    })?;
    let history = registry.config_revisions(id, CONFIG_HISTORY_KEEP)?.value;
    let last_known_good = registry.last_known_good(id)?.value;
    let package = load_package(platform, layout, &instance)?;
    let settings = active_settings(&package, &active)?;
    let paths = layout.layout().instance_paths(id);
    let runtime = runtime_fact(platform, layout, registry, &instance)?;
    let reload_supported = package
        .provider_revision()
        .capabilities()
        .contains(Capability::Reload);
    Ok(ConfigFacts {
        instance,
        active,
        history,
        last_known_good,
        package,
        settings,
        paths,
        runtime,
        reload_supported,
    })
}

/// The settings of `revision`, validated against the pinned schema.
///
/// # Errors
///
/// Returns [`SupervisorError::ConfigInvalid`] when they are not JSON or no
/// longer validate.
pub(crate) fn active_settings(
    package: &ValidatedPackage,
    revision: &ConfigRevisionRecord,
) -> Result<SettingsValues, SupervisorError> {
    let invalid = |reason: String| SupervisorError::ConfigInvalid {
        field: "settings".to_owned(),
        reason,
    };
    let document: serde_json::Value = serde_json::from_str(revision.values.as_str())
        .map_err(|error| invalid(format!("the stored settings are not JSON: {error}")))?;
    package
        .settings_schema()
        .validate(&document)
        .map_err(|error| invalid(error.to_string()))
}

fn runtime_fact(
    platform: &Platform,
    layout: &PreparedLayout,
    registry: &dyn Registry,
    instance: &Instance,
) -> Result<RuntimeFact, SupervisorError> {
    let Some(runtime) = registry.runtime(instance.runtime_id())?.value else {
        return Ok(RuntimeFact::Missing);
    };
    let folder = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => layout.layout().runtime_dir(
            runtime.software(),
            runtime.version(),
            runtime.target(),
            artifact_sha256,
        ),
        InstallMode::External { root, .. } => root.clone(),
    };
    Ok(match platform.fs().canonicalize(&folder) {
        Ok(root) => RuntimeFact::Present(RuntimeContext {
            root,
            version: runtime.version().clone(),
            line: runtime.line().clone(),
        }),
        Err(_) => RuntimeFact::Missing,
    })
}
