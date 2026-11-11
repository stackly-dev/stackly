//! The create form as the core describes it (spec 0020, *Evaluator create*
//! and *Nama*): the provider, the runtime, the endpoints, and the settings
//! fields of a package, in the order the form shows them. `stacklyd` maps it
//! to the wire `CreateForm`.

use stackly_domain::{
    CapabilitySet, Instance, InstanceName, ProviderName, Registry, RegistryError, RevisionLabel,
    Runtime, RuntimeState, Sha256Digest,
};

use super::draft::NAME_SUGGESTION_MAX_SUFFIX;
use super::facts::PackageSource;
use super::port::slots;
use crate::catalog::{
    CatalogEntry, CatalogLocation, CatalogMeta, CatalogState, ProviderEntry, TrustVerdict,
    require_entry_trust,
};
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
    /// More than one entry matches; the client asks the user to pick one
    /// of `candidates` (spec 0037, *Software yang sama*).
    ProviderAmbiguous {
        /// The IDs of the matching entries, at most
        /// [`CANDIDATES_MAX`](FormError::CANDIDATES_MAX).
        candidates: Vec<String>,
    },
    /// The entry is planned or invalid.
    ProviderUnavailable {
        /// `planned` or `invalid`.
        state: &'static str,
    },
    /// A local entry whose stored bytes are not approved (spec 0037).
    ApprovalRequired {
        /// The provider.
        provider: String,
        /// Its revision label.
        provider_revision: String,
        /// The digest of its stored bytes.
        package_sha256: String,
    },
    /// The revision is revoked (spec 0037).
    Revoked {
        /// The provider.
        provider: String,
        /// The revoked revision label.
        provider_revision: String,
        /// Who revoked it and since when.
        info: crate::catalog::RevokedInfo,
    },
    /// The provider runs other software than the runtime.
    RuntimeMismatch,
    /// The runtime is not `installed`.
    RuntimeUnavailable {
        /// Its state.
        state: &'static str,
    },
}

impl FormError {
    /// The most candidates an ambiguous choice names.
    pub const CANDIDATES_MAX: usize = 8;
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
            // Spec 0037, *Software yang sama*: a catalog entry of the
            // software wins; without one, the only trusted local entry;
            // anything else is for the user to choose.
            let matching: Vec<&ProviderEntry> = catalog
                .entries()
                .iter()
                .filter_map(|entry| match entry {
                    CatalogEntry::Provider(entry)
                        if entry.package.software() == runtime.software() =>
                    {
                        Some(entry)
                    }
                    _ => None,
                })
                .collect();
            let from_catalog: Vec<&ProviderEntry> = matching
                .iter()
                .copied()
                .filter(|entry| entry.local().is_none())
                .collect();
            let trusted_local: Vec<&ProviderEntry> = matching
                .iter()
                .copied()
                .filter(|entry| {
                    entry.local().is_some()
                        && crate::catalog::entry_trust(catalog, entry).is_trusted()
                })
                .collect();
            match (from_catalog.as_slice(), trusted_local.as_slice()) {
                ([one], _) | ([], [one]) => *one,
                ([], []) if matching.is_empty() => return Err(FormError::ProviderNotFound),
                (several, _) => {
                    let pool = if several.is_empty() {
                        &matching
                    } else {
                        &from_catalog
                    };
                    return Err(FormError::ProviderAmbiguous {
                        candidates: pool
                            .iter()
                            .take(FormError::CANDIDATES_MAX)
                            .map(|entry| entry.package.key().provider().to_string())
                            .collect(),
                    });
                }
            }
        }
    };
    Ok((entry, catalog.meta()))
}

/// The trust gate of a form without a revision, after [`select_provider`]
/// and before [`build_form`] (spec 0037, *Gate trust*): a local entry's
/// import row is read again from `registry`.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a failed read. The inner result is
/// [`FormError::ApprovalRequired`] or [`FormError::Revoked`] for a verdict
/// that is not trusted.
pub fn form_trust(
    catalog: &CatalogState,
    registry: &dyn Registry,
    entry: &ProviderEntry,
) -> Result<Result<(), FormError>, RegistryError> {
    let CatalogState::Loaded(catalog) = catalog else {
        return Ok(Ok(()));
    };
    let key = entry.package.key();
    Ok(match require_entry_trust(catalog, registry, entry)? {
        TrustVerdict::Trusted => Ok(()),
        TrustVerdict::Revoked(info) => Err(FormError::Revoked {
            provider: key.provider().to_string(),
            provider_revision: key.revision().to_string(),
            info,
        }),
        _ => Err(FormError::ApprovalRequired {
            provider: key.provider().to_string(),
            provider_revision: key.revision().to_string(),
            package_sha256: entry
                .local()
                .map(|local| local.record.package_sha256.to_string())
                .unwrap_or_default(),
        }),
    })
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
    let source = match (entry.local(), &meta.location) {
        (Some(info), _) => PackageSource::LocalImport {
            source_id: info.record.source_id,
            approved_at: info
                .record
                .decision
                .as_ref()
                .map_or(info.record.updated_at, |decision| decision.at),
        },
        (None, CatalogLocation::Bundled(_)) => PackageSource::BundledCatalog,
        (None, CatalogLocation::Development(_)) => PackageSource::DevelopmentCatalog,
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
