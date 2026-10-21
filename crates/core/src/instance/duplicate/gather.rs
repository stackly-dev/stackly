//! What a duplicate form reads (spec 0024, *Draft duplicate*):
//! [`gather_duplicate`] is its only IO.

use stackly_domain::{
    Capability, InstanceFilter, InstanceId, InstanceLifecycle, InstanceName, Registry,
    RegistryError, Runtime, RuntimeState, SecretUse, SecretUser,
};
use stackly_platform::Platform;

use super::DuplicateError;
use crate::PreparedLayout;
use crate::catalog::{CatalogEntry, CatalogState};
use crate::provider::ValidatedPackage;
use crate::supervisor::SupervisorError;
use crate::supervisor::prepare::load_package;

/// What [`gather_duplicate`] reads from.
pub struct DuplicateDeps<'a> {
    /// The registry.
    pub registry: &'a dyn Registry,
    /// The catalog the daemon loaded.
    pub catalog: &'a CatalogState,
    /// The platform ports.
    pub platform: &'a Platform,
    /// The checked layout.
    pub prepared: &'a PreparedLayout,
}

/// Everything [`build_duplicate`](super::build_duplicate) needs.
#[derive(Debug, Clone)]
pub struct DuplicateFacts {
    /// The instance to duplicate.
    pub source: stackly_domain::Instance,
    /// Its pinned package.
    pub package: ValidatedPackage,
    /// Its runtime, `installed`.
    pub runtime: Runtime,
    /// The `values` of its active config revision, a JSON object, or none
    /// while it has no active revision.
    pub values: Option<serde_json::Value>,
    /// The secret references it uses.
    pub uses: Vec<SecretUse>,
    /// The names of the instances that are not retained.
    pub taken: Vec<InstanceName>,
}

/// Reads the facts of a duplicate of `source`, checking in this order that
/// the instance exists and is not retained, that its pinned package reads
/// with the digest the instance records, that the loaded catalog does not
/// hold another package under the same revision label, that the package
/// allows a second instance, and that the runtime exists and is `installed`.
/// It stops at the first check that fails.
///
/// It reads the registry, the catalog in memory, and the pinned package
/// folder. It never reads the data, log, or run folder of the source, and
/// it writes nothing. It blocks, so async callers run it in
/// `spawn_blocking`.
///
/// # Errors
///
/// Returns [`DuplicateError`] for the first check that fails, and
/// [`DuplicateError::Registry`] when the registry fails or the active
/// settings are not a JSON object.
pub fn gather_duplicate(
    deps: &DuplicateDeps<'_>,
    source: InstanceId,
) -> Result<DuplicateFacts, DuplicateError> {
    let registry = deps.registry;
    let instance = registry
        .instance(source)?
        .value
        .ok_or(DuplicateError::NotFound)?;
    if instance.lifecycle() == InstanceLifecycle::Retained {
        return Err(DuplicateError::Retained);
    }
    let package =
        load_package(deps.platform, deps.prepared, &instance).map_err(|error| match error {
            SupervisorError::PackageUnavailable { provider, reason } => {
                DuplicateError::PackageUnavailable { provider, reason }
            }
            other => DuplicateError::PackageUnavailable {
                provider: instance.provider_revision().to_string(),
                reason: other.to_string(),
            },
        })?;
    let key = instance.provider_revision();
    if catalog_has_another_digest(deps.catalog, &package) {
        return Err(DuplicateError::ProviderAmbiguous {
            provider: key.provider().clone(),
            revision: key.revision().clone(),
        });
    }
    if !package
        .manifest()
        .capabilities()
        .contains(Capability::MultiInstance)
    {
        return Err(DuplicateError::CapabilityUnsupported {
            provider: key.provider().clone(),
            existing: instance.name().clone(),
        });
    }
    let runtime_id = instance.runtime_id();
    let runtime = registry
        .runtime(runtime_id)?
        .value
        .ok_or(DuplicateError::RuntimeNotFound { runtime_id })?;
    if runtime.state() != RuntimeState::Installed {
        return Err(DuplicateError::RuntimeUnavailable {
            runtime_id,
            state: runtime.state(),
        });
    }
    let values = registry
        .active_config_revision(source)?
        .value
        .map(|revision| settings_object(revision.values.as_str()))
        .transpose()?;
    let uses = registry.secret_uses_by(SecretUser::Instance(source))?.value;
    let taken = registry
        .instances(InstanceFilter {
            include_retained: false,
        })?
        .value
        .iter()
        .map(|instance| instance.name().clone())
        .collect();
    Ok(DuplicateFacts {
        source: instance,
        package,
        runtime,
        values,
        uses,
        taken,
    })
}

/// Whether the loaded catalog has a provider entry with the name and
/// revision label of `package` and another manifest digest. A catalog that
/// is not loaded, or has no such entry, is no problem: create then uses the
/// pinned package.
fn catalog_has_another_digest(catalog: &CatalogState, package: &ValidatedPackage) -> bool {
    let CatalogState::Loaded(catalog) = catalog else {
        return false;
    };
    let key = package.key();
    match catalog.entry(key.provider()) {
        Some(CatalogEntry::Provider(entry)) => {
            let other = entry.package.key();
            other.revision() == key.revision() && other.manifest_sha256() != key.manifest_sha256()
        }
        _ => false,
    }
}

/// The stored `values` text as a JSON object.
fn settings_object(text: &str) -> Result<serde_json::Value, RegistryError> {
    let corrupt = |detail: String| RegistryError::Corrupt {
        table: "config_revisions",
        column: "values",
        detail,
    };
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| corrupt(format!("the stored settings are not JSON: {error}")))?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(corrupt(
            "the stored settings are not a JSON object".to_owned(),
        ))
    }
}
