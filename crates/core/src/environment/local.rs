//! `.stackly.local.yaml` (spec 0045, *Schema `.stackly.local.yaml`*): the
//! changes one Mac makes to a manifest without touching the team's file.
//!
//! Only two things can be overridden: the port of an IP endpoint and the
//! data location of a dedicated service. An override never changes the
//! manifest, its digest, the snapshot, the lock, or an instance that
//! exists (AC-10, AC-11); a plan applies it to a `create` draft only.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

use serde::Deserialize;
use stackly_domain::{EndpointName, MemberAlias, Port, PortRequest, Sha256Digest};
use stackly_platform::Filesystem;

use super::finding::{Finding, FindingCode, FindingLocation, Findings, Severity};
use super::manifest::{RawPort, load_document, safe_segment};
use super::register::UnreadableReason;
use super::validate::{EnvironmentManifest, ServiceSpec, manifest_digest};
use crate::catalog::{Catalog, CatalogEntry};
use crate::provider::raw::Ordered;

/// The `kind` of a local override file.
pub const LOCAL_KIND: &str = "EnvironmentLocal";
/// Largest local override file, in bytes.
pub const LOCAL_MAX_LEN: u64 = 64 * 1024;
/// Most services one file names.
pub const LOCAL_SERVICES_MAX: usize = 32;
/// Longest `dataLocation`, in bytes.
pub const DATA_LOCATION_MAX: usize = 1024;

/// `.stackly.local.yaml`, as it decodes.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "EnvironmentLocal"))]
pub(crate) struct RawLocal {
    /// Always `stackly/v1alpha1`.
    #[expect(
        dead_code,
        reason = "the loader checks it before decoding; the schema shows it"
    )]
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = r"^stackly/v1alpha1$"))
    )]
    pub(crate) api_version: String,
    /// Always `EnvironmentLocal`.
    #[expect(
        dead_code,
        reason = "the loader checks it before decoding; the schema shows it"
    )]
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = r"^EnvironmentLocal$"))
    )]
    pub(crate) kind: String,
    /// The overrides, by service alias: 0 to 32.
    pub(crate) services: Ordered<RawLocalService>,
}

/// The overrides of one service.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "LocalService"))]
pub(crate) struct RawLocalService {
    /// Ports by endpoint name; only IP endpoints of the package.
    #[serde(default)]
    pub(crate) endpoints: Option<Ordered<RawLocalEndpoint>>,
    /// Where the data of a new instance goes.
    #[serde(default)]
    pub(crate) storage: Option<RawLocalStorage>,
}

/// The port of one endpoint; nothing else may be written.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "LocalEndpoint"))]
pub(crate) struct RawLocalEndpoint {
    /// A port from 1 to 65535, or `auto`.
    pub(crate) port: RawPort,
}

/// `storage`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "LocalStorage"))]
pub(crate) struct RawLocalStorage {
    /// The absolute path of a registered data location: the folder chosen
    /// in Settings, or its `Stackly` subfolder.
    #[cfg_attr(feature = "json-schema", schemars(length(min = 1, max = 1024)))]
    pub(crate) data_location: String,
}

/// The JSON Schema of `.stackly.local.yaml` for editors, with the file
/// stem `local.v1alpha1` that `cargo xtask codegen` writes it under in
/// `schemas/environment/` (AC-20).
#[cfg(feature = "json-schema")]
#[must_use]
pub fn json_schemas() -> Vec<(&'static str, schemars::Schema)> {
    let generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    vec![(
        "local.v1alpha1",
        generator.into_root_schema_for::<RawLocal>(),
    )]
}

/// A local override file whose shape the loader accepted.
#[derive(Debug)]
pub struct LocalDocument(RawLocal);

/// What reading the local override file gave.
#[derive(Debug)]
pub enum LocalFile {
    /// There is no file.
    Absent,
    /// The file is there but can't be read.
    Unreadable(UnreadableReason),
    /// The loader refused its shape.
    Invalid {
        /// The findings of the shape.
        findings: Findings,
        /// The SHA-256 of the bytes read.
        sha256: Sha256Digest,
    },
    /// A document of the right shape.
    Loaded {
        /// The document.
        document: LocalDocument,
        /// The SHA-256 of the bytes read.
        sha256: Sha256Digest,
    },
}

impl LocalFile {
    /// The file from the result of a read: nothing at the path is
    /// [`LocalFile::Absent`].
    #[must_use]
    pub fn from_read(read: Result<Vec<u8>, UnreadableReason>) -> Self {
        match read {
            Err(UnreadableReason::NotFound) => Self::Absent,
            Err(reason) => Self::Unreadable(reason),
            Ok(bytes) => {
                let sha256 = manifest_digest(&bytes);
                match load_document::<RawLocal>(&bytes, LOCAL_KIND, "kind must be EnvironmentLocal")
                {
                    Ok(raw) => Self::Loaded {
                        document: LocalDocument(raw),
                        sha256,
                    },
                    Err(findings) => Self::Invalid { findings, sha256 },
                }
            }
        }
    }

    /// Reads the file at `path` through the `Filesystem` port (a regular
    /// file, at most [`LOCAL_MAX_LEN`] bytes).
    #[must_use]
    pub fn read(fs: &dyn Filesystem, path: &std::path::Path) -> Self {
        Self::from_read(
            fs.read_file_limited(path, LOCAL_MAX_LEN)
                .map_err(|error| UnreadableReason::of(&error)),
        )
    }

    /// Returns the digest of the bytes read, if any.
    #[must_use]
    pub const fn sha256(&self) -> Option<Sha256Digest> {
        match self {
            Self::Invalid { sha256, .. } | Self::Loaded { sha256, .. } => Some(*sha256),
            Self::Absent | Self::Unreadable(_) => None,
        }
    }
}

/// The local override file a plan reads: its name and what reading it
/// gave.
#[derive(Debug)]
pub struct LocalInput {
    /// The file name, such as `.stackly.local.yaml`.
    pub file: String,
    /// What reading it gave.
    pub content: LocalFile,
}

/// The state of a local override file that is there. Open on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalState {
    /// It has no error finding.
    Present,
    /// It has an error finding.
    Invalid,
    /// It can't be read.
    Unreadable,
}

impl LocalState {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Invalid => "invalid",
            Self::Unreadable => "unreadable",
        }
    }
}

/// What became of one override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverrideState {
    /// The `create` draft uses it.
    Applied,
    /// A port of a service with a member: only the `differences` compare
    /// against it.
    Compared,
    /// A data location of a service with a member: never applied.
    NotAppliedExisting,
    /// The alias is not in the manifest.
    Ignored,
}

impl OverrideState {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Compared => "compared",
            Self::NotAppliedExisting => "not_applied_existing",
            Self::Ignored => "ignored",
        }
    }
}

/// The value of one override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideValue {
    /// The port of an endpoint.
    Port {
        /// The endpoint.
        endpoint: EndpointName,
        /// Its port.
        port: PortRequest,
    },
    /// `storage.dataLocation`, as written.
    DataLocation(String),
}

/// One override that passed the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    /// The alias as the file wrote it.
    pub service: String,
    /// The service, when the manifest declares it.
    pub alias: Option<MemberAlias>,
    /// What it overrides.
    pub value: OverrideValue,
}

impl Override {
    /// `endpoints.<name>.port` or `storage.dataLocation`.
    #[must_use]
    pub fn field(&self) -> String {
        match &self.value {
            OverrideValue::Port { endpoint, .. } => format!("endpoints.{endpoint}.port"),
            OverrideValue::DataLocation(_) => "storage.dataLocation".to_owned(),
        }
    }

    /// The value as text: the port, `auto`, or the path as written.
    #[must_use]
    pub fn value_text(&self) -> String {
        match &self.value {
            OverrideValue::Port {
                port: PortRequest::Fixed(port),
                ..
            } => port.to_string(),
            OverrideValue::Port { .. } => "auto".to_owned(),
            OverrideValue::DataLocation(path) => path.clone(),
        }
    }

    /// What a plan does with it, by whether its service has a member.
    #[must_use]
    pub const fn state(&self, has_member: bool) -> OverrideState {
        match (&self.alias, &self.value, has_member) {
            (None, _, _) => OverrideState::Ignored,
            (Some(_), _, false) => OverrideState::Applied,
            (Some(_), OverrideValue::Port { .. }, true) => OverrideState::Compared,
            (Some(_), OverrideValue::DataLocation(_), true) => OverrideState::NotAppliedExisting,
        }
    }
}

/// The overrides of a file that loaded, with every finding (*Aturan
/// override*).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalOverrides {
    /// Every override, the ignored ones included.
    pub overrides: Vec<Override>,
    /// Every finding.
    pub findings: Findings,
}

impl LocalOverrides {
    /// The port override of `alias`'s `endpoint`, if any.
    #[must_use]
    pub fn port(&self, alias: &MemberAlias, endpoint: &EndpointName) -> Option<PortRequest> {
        self.overrides.iter().find_map(|o| match &o.value {
            OverrideValue::Port {
                endpoint: own,
                port,
            } if o.alias.as_ref() == Some(alias) && own == endpoint => Some(*port),
            _ => None,
        })
    }

    /// Every port override of `alias`.
    #[must_use]
    pub fn ports(&self, alias: &MemberAlias) -> Vec<(EndpointName, PortRequest)> {
        self.overrides
            .iter()
            .filter(|o| o.alias.as_ref() == Some(alias))
            .filter_map(|o| match &o.value {
                OverrideValue::Port { endpoint, port } => Some((endpoint.clone(), *port)),
                OverrideValue::DataLocation(_) => None,
            })
            .collect()
    }

    /// The data location override of `alias`, if any.
    #[must_use]
    pub fn data_location(&self, alias: &MemberAlias) -> Option<&str> {
        self.overrides.iter().find_map(|o| match &o.value {
            OverrideValue::DataLocation(path) if o.alias.as_ref() == Some(alias) => {
                Some(path.as_str())
            }
            _ => None,
        })
    }

    /// Whether a finding is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.findings.count_of(Severity::Error) > 0
    }
}

/// Whether `path` is an absolute path of at most 1024 bytes without NUL
/// and without a `.` or `..` segment (the rule of `import_path_code`).
fn path_fits(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= DATA_LOCATION_MAX
        && !path.contains('\0')
        && !path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

fn port_of(raw: &RawPort) -> Option<PortRequest> {
    match raw {
        RawPort::Fixed(number) => u16::try_from(*number)
            .ok()
            .and_then(|number| Port::new(number).ok())
            .map(PortRequest::Fixed),
        RawPort::Text(text) if text == "auto" => Some(PortRequest::Auto),
        RawPort::Text(_) => None,
    }
}

/// Checks a loaded document against the manifest and the catalog, every
/// row of *Aturan override*. `has_member` says which services already
/// have an instance, so the endpoint conflict check covers only the
/// services a plan would create.
#[must_use]
pub fn validate(
    document: &LocalDocument,
    manifest: &EnvironmentManifest,
    catalog: &Catalog,
    has_member: &dyn Fn(&MemberAlias) -> bool,
) -> LocalOverrides {
    let mut out = LocalOverrides::default();
    let raw = &document.0;
    if raw.services.len() > LOCAL_SERVICES_MAX {
        out.findings.push(Finding::error(
            FindingCode::InvalidValue,
            FindingLocation::Field("services".into()),
        ));
        return out;
    }
    for (key, service) in &raw.services {
        let base = format!("services.{}", safe_segment(key));
        let at = |path: &str| FindingLocation::Field(format!("{base}{path}"));
        let declared = key
            .parse::<MemberAlias>()
            .ok()
            .and_then(|alias| manifest.service(&alias));
        let Some(spec) = declared else {
            out.findings.push(Finding::warning(
                FindingCode::OverrideUnknownService,
                at(""),
            ));
            for (name, endpoint) in service.endpoints.iter().flatten() {
                if let (Ok(endpoint_name), Some(port)) =
                    (name.parse::<EndpointName>(), port_of(&endpoint.port))
                {
                    out.overrides.push(Override {
                        service: key.clone(),
                        alias: None,
                        value: OverrideValue::Port {
                            endpoint: endpoint_name,
                            port,
                        },
                    });
                }
            }
            if let Some(storage) = &service.storage {
                out.overrides.push(Override {
                    service: key.clone(),
                    alias: None,
                    value: OverrideValue::DataLocation(storage.data_location.clone()),
                });
            }
            continue;
        };
        let alias = Some(&spec.alias);
        if spec.instance_ref.is_some() {
            out.findings
                .push(Finding::error(FindingCode::OverrideNotAllowed, at("")).for_service(alias));
            continue;
        }
        let package = match catalog.entry(&spec.provider) {
            Some(CatalogEntry::Provider(entry)) => Some(&entry.package),
            _ => None,
        };
        for (name, endpoint) in service.endpoints.iter().flatten() {
            let path = format!(".endpoints.{}", safe_segment(name));
            let found = package.and_then(|package| {
                package
                    .endpoints()
                    .find(|candidate| candidate.name().as_str() == name)
            });
            let Some(found) = found else {
                out.findings.push(
                    Finding::error(FindingCode::UnknownEndpoint, at(&path)).for_service(alias),
                );
                continue;
            };
            if !found.transport().is_ip() {
                out.findings
                    .push(Finding::error(FindingCode::EndpointNotIp, at(&path)).for_service(alias));
                continue;
            }
            let Some(port) = port_of(&endpoint.port) else {
                out.findings.push(
                    Finding::error(FindingCode::InvalidValue, at(&format!("{path}.port")))
                        .for_service(alias),
                );
                continue;
            };
            out.overrides.push(Override {
                service: key.clone(),
                alias: Some(spec.alias.clone()),
                value: OverrideValue::Port {
                    endpoint: found.name().clone(),
                    port,
                },
            });
        }
        if let Some(storage) = &service.storage {
            if path_fits(&storage.data_location) {
                out.overrides.push(Override {
                    service: key.clone(),
                    alias: Some(spec.alias.clone()),
                    value: OverrideValue::DataLocation(storage.data_location.clone()),
                });
            } else {
                out.findings.push(
                    Finding::error(
                        FindingCode::StorageLocationInvalid,
                        at(".storage.dataLocation"),
                    )
                    .for_service(alias),
                );
            }
        }
    }
    endpoint_conflicts(&mut out, manifest, catalog, has_member);
    out
}

/// `override_endpoint_conflict`: after the overrides, two fixed endpoints
/// of services a plan would create share a host and port, by the overlap
/// rule of `endpoint_conflict` (spec 0044). Each one is named.
fn endpoint_conflicts(
    out: &mut LocalOverrides,
    manifest: &EnvironmentManifest,
    catalog: &Catalog,
    has_member: &dyn Fn(&MemberAlias) -> bool,
) {
    let mut fixed: Vec<(IpAddr, Port, String, MemberAlias)> = Vec::new();
    let creates = manifest
        .services
        .iter()
        .filter(|service| service.instance_ref.is_none() && !has_member(&service.alias));
    for service in creates {
        let Some(CatalogEntry::Provider(entry)) = catalog.entry(&service.provider) else {
            continue;
        };
        for spec in entry
            .package
            .endpoints()
            .filter(|spec| spec.transport().is_ip())
        {
            let port = effective_port(out, service, spec.name());
            if let PortRequest::Fixed(port) = port {
                fixed.push((
                    spec.default_host()
                        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                    port,
                    format!("services.{}.endpoints.{}.port", service.alias, spec.name()),
                    service.alias.clone(),
                ));
            }
        }
    }
    let mut conflicting = BTreeSet::new();
    for (i, a) in fixed.iter().enumerate() {
        for b in &fixed[i + 1..] {
            if a.1 == b.1 && (a.0 == b.0 || a.0.is_unspecified() || b.0.is_unspecified()) {
                conflicting.insert(a.2.clone());
                conflicting.insert(b.2.clone());
            }
        }
    }
    for (_, _, path, alias) in &fixed {
        if conflicting.contains(path) {
            out.findings.push(
                Finding::error(
                    FindingCode::OverrideEndpointConflict,
                    FindingLocation::Field(path.clone()),
                )
                .for_service(Some(alias)),
            );
        }
    }
}

/// The port a service asks for at `endpoint` after the overrides: the
/// override, else the manifest, else `auto`.
#[must_use]
pub fn effective_port(
    overrides: &LocalOverrides,
    service: &ServiceSpec,
    endpoint: &EndpointName,
) -> PortRequest {
    overrides
        .port(&service.alias, endpoint)
        .or_else(|| {
            service
                .endpoints
                .iter()
                .find(|written| &written.name == endpoint)
                .map(|written| written.port)
        })
        .unwrap_or(PortRequest::Auto)
}
