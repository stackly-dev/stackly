//! The create form as the core describes it (spec 0020, *Evaluator create*
//! and *Nama*): the provider, the runtime, the endpoints, and the settings
//! fields of a package, in the order the form shows them. `stacklyd` maps it
//! to the wire `CreateForm`.

use stackly_domain::{
    CapabilitySet, Instance, InstanceName, ProviderName, RevisionLabel, Runtime, RuntimeState,
    Sha256Digest,
};

use super::draft::NAME_SUGGESTION_MAX_SUFFIX;
use super::facts::PackageSource;
use super::port::slots;
use crate::catalog::{CatalogEntry, CatalogLocation, CatalogMeta, CatalogState, ProviderEntry};
use crate::provider::{
    Category, EndpointPurpose, EndpointTransport, SettingField, ValidatedPackage,
};

/// The form's text for the data folder of the new instance.
pub const DATA_PATH_TEXT: &str = "instances/<new instance id>/data";

/// The longest instance name.
const NAME_MAX: usize = 63;

/// The provider a form is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormProvider {
    /// The provider ID.
    pub name: ProviderName,
    /// The revision label.
    pub revision: RevisionLabel,
    /// The digest of the manifest and templates.
    pub manifest_sha256: Sha256Digest,
    /// The name shown for it.
    pub display_name: String,
    /// Its category.
    pub category: Category,
    /// Where the package comes from.
    pub source: PackageSource,
}

/// One endpoint of the package, in form order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormEndpoint {
    /// The endpoint's name.
    pub name: String,
    /// `tcp`, `udp`, or `unix`.
    pub transport: EndpointTransport,
    /// What it is for.
    pub purpose: EndpointPurpose,
    /// The loopback host, for TCP and UDP.
    pub host: Option<String>,
    /// The suggested port, for TCP and UDP.
    pub default_port: Option<u16>,
    /// The `socketName` template as written, for a socket.
    pub socket_name: Option<String>,
}

/// The create form of one runtime.
#[derive(Debug, Clone)]
pub struct CreateFormModel {
    /// The provider.
    pub provider: FormProvider,
    /// The runtime.
    pub runtime: Runtime,
    /// The name the form starts with; none when every candidate is taken.
    pub suggested_name: Option<InstanceName>,
    /// The text for the data folder, [`DATA_PATH_TEXT`].
    pub data_path: &'static str,
    /// The endpoints: purpose `client`, `admin`, `monitoring`, then name.
    pub endpoints: Vec<FormEndpoint>,
    /// The settings fields, in the order their schema declares them.
    pub fields: Vec<SettingField>,
    /// The capabilities of the package.
    pub capabilities: CapabilitySet,
}

/// Why no form could be built (AC-1).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormError {
    /// The catalog is not loaded; `reason` is `not_configured`, `missing`,
    /// `index_invalid`, or `unsupported_schema`.
    CatalogUnavailable {
        /// Why.
        reason: &'static str,
    },
    /// No entry matches.
    ProviderNotFound,
    /// More than one entry matches.
    ProviderAmbiguous,
    /// The entry is planned or invalid.
    ProviderUnavailable {
        /// `planned` or `invalid`.
        state: &'static str,
    },
    /// The provider runs other software than the runtime.
    RuntimeMismatch,
    /// The runtime is not `installed`.
    RuntimeUnavailable {
        /// Its state.
        state: &'static str,
    },
}

/// Picks the catalog entry a form is built from: the entry named `provider`
/// when given, else the entry whose package runs the software of `runtime`
/// (spec 0020, *Resolusi provider dan revision*).
///
/// # Errors
///
/// Returns [`FormError`] for an unloaded catalog, no entry, more than one
/// entry, a planned or invalid entry, a provider of other software, and a
/// runtime that is not `installed`.
pub fn select_provider<'a>(
    catalog: &'a CatalogState,
    runtime: &Runtime,
    provider: Option<&str>,
) -> Result<(&'a ProviderEntry, &'a CatalogMeta), FormError> {
    let CatalogState::Loaded(catalog) = catalog else {
        return Err(FormError::CatalogUnavailable {
            reason: match catalog {
                CatalogState::Unavailable { reason, .. } => reason.as_code(),
                _ => "not_configured",
            },
        });
    };
    if runtime.state() != RuntimeState::Installed {
        return Err(FormError::RuntimeUnavailable {
            state: runtime.state().as_code(),
        });
    }
    let entry = match provider {
        Some(name) => {
            let id = name
                .parse::<ProviderName>()
                .map_err(|_| FormError::ProviderNotFound)?;
            let entry = catalog.entry(&id).ok_or(FormError::ProviderNotFound)?;
            match entry {
                CatalogEntry::Provider(entry) if entry.package.software() == runtime.software() => {
                    entry
                }
                CatalogEntry::Provider(_) => return Err(FormError::RuntimeMismatch),
                CatalogEntry::Planned(_) => {
                    return Err(FormError::ProviderUnavailable { state: "planned" });
                }
                CatalogEntry::Invalid(_) => {
                    return Err(FormError::ProviderUnavailable { state: "invalid" });
                }
            }
        }
        None => {
            let mut matching = catalog.entries().iter().filter_map(|entry| match entry {
                CatalogEntry::Provider(entry) if entry.package.software() == runtime.software() => {
                    Some(entry)
                }
                _ => None,
            });
            let first = matching.next().ok_or(FormError::ProviderNotFound)?;
            if matching.next().is_some() {
                return Err(FormError::ProviderAmbiguous);
            }
            first
        }
    };
    Ok((entry, catalog.meta()))
}

/// Builds the form of `entry` for `runtime`. `taken` are the names of the
/// instances that are not retained, which the suggested name avoids.
#[must_use]
pub fn build_form(
    entry: &ProviderEntry,
    runtime: &Runtime,
    taken: &[InstanceName],
    meta: &CatalogMeta,
) -> CreateFormModel {
    let source = match meta.location {
        CatalogLocation::Bundled(_) => PackageSource::BundledCatalog,
        CatalogLocation::Development(_) => PackageSource::DevelopmentCatalog,
    };
    build_form_from_package(
        &entry.package,
        source,
        runtime,
        suggested_name(runtime.software().as_str(), runtime.line().as_str(), taken),
    )
}

/// Builds the form of `package` for `runtime`, whichever way the package
/// was found: a catalog entry for create, the pinned package of an
/// instance for duplicate (spec 0024). `suggested_name` is the name the
/// form starts with.
#[must_use]
pub fn build_form_from_package(
    package: &ValidatedPackage,
    source: PackageSource,
    runtime: &Runtime,
    suggested_name: Option<InstanceName>,
) -> CreateFormModel {
    let key = package.key();
    CreateFormModel {
        provider: FormProvider {
            name: key.provider().clone(),
            revision: key.revision().clone(),
            manifest_sha256: *key.manifest_sha256(),
            display_name: package.manifest().display_name().to_owned(),
            category: package.manifest().category(),
            source,
        },
        runtime: runtime.clone(),
        suggested_name,
        data_path: DATA_PATH_TEXT,
        endpoints: form_endpoints(package),
        fields: package.settings_schema().fields().cloned().collect(),
        capabilities: package.manifest().capabilities().clone(),
    }
}

/// The names of the instances in `instances`, for [`build_form`].
#[must_use]
pub fn names_of(instances: &[Instance]) -> Vec<InstanceName> {
    instances
        .iter()
        .map(|instance| instance.name().clone())
        .collect()
}

fn form_endpoints(package: &ValidatedPackage) -> Vec<FormEndpoint> {
    let ip: Vec<_> = slots(package);
    let specs: Vec<_> = package.endpoints().collect();
    let mut endpoints: Vec<FormEndpoint> = ip
        .iter()
        .filter_map(|slot| {
            let spec = specs.iter().find(|spec| *spec.name() == slot.name)?;
            Some(FormEndpoint {
                name: slot.name.to_string(),
                transport: spec.transport(),
                purpose: slot.purpose,
                host: Some(slot.host.to_string()),
                default_port: slot.default_port.map(stackly_domain::Port::get),
                socket_name: None,
            })
        })
        .collect();
    endpoints.extend(
        specs
            .iter()
            .filter(|spec| spec.transport() == EndpointTransport::Unix)
            .map(|spec| FormEndpoint {
                name: spec.name().to_string(),
                transport: EndpointTransport::Unix,
                purpose: spec.purpose(),
                host: None,
                default_port: None,
                socket_name: spec.socket_name_template(),
            }),
    );
    endpoints
}

/// `<software>-<line>` with each dot of the line a hyphen (`postgresql-18`,
/// `mysql-8-4`); when a name is taken, `-2` to `-99` after it. The software
/// part is cut so the whole name fits 63 characters. None when every
/// candidate is taken or none is a valid name.
#[must_use]
pub fn suggested_name(software: &str, line: &str, taken: &[InstanceName]) -> Option<InstanceName> {
    let line = line.replace('.', "-");
    (1..=NAME_SUGGESTION_MAX_SUFFIX)
        .map(|n| {
            if n == 1 {
                String::new()
            } else {
                format!("-{n}")
            }
        })
        .find_map(|suffix| {
            let room = NAME_MAX.saturating_sub(1 + line.len() + suffix.len());
            let software: String = software.chars().take(room).collect();
            let software = software.trim_end_matches('-');
            let candidate: InstanceName = format!("{software}-{line}{suffix}").parse().ok()?;
            (!taken.contains(&candidate)).then_some(candidate)
        })
}
