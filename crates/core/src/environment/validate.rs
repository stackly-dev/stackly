//! `environment::validate` (spec 0044 AC-1 to AC-3): the manifest against
//! the catalog in memory, every rule of *Aturan validasi*, every finding at
//! once.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use sha2::{Digest, Sha256};
use stackly_domain::{
    Capability, DeclaredService, DependencyCondition, DependencyEdge, EndpointName,
    EnvironmentName, InstanceName, MemberAlias, Ownership, Port, PortRequest, ProviderName,
    RuntimeSelector, SecretFieldName, SecretRef, Sha256Digest,
};

use super::finding::{Finding, FindingCode, FindingLocation, Findings};
use super::manifest::{
    API_VERSION, RawDependency, RawEndpoint, RawEnvironment, RawPort, RawService, load, safe_path,
    safe_segment,
};
use crate::catalog::{Catalog, CatalogEntry, ProviderEntry};
use crate::instance::create::settings_with_references;
use crate::provider::{EndpointTransport, SettingsIssueKind};

/// Most services one manifest declares.
pub const MAX_SERVICES: usize = 32;
/// Most `dependsOn` entries of one service.
pub const MAX_DEPENDENCIES: usize = 31;
/// Longest `timeoutSeconds`.
pub const MAX_TIMEOUT_SECONDS: i64 = 3600;

/// One endpoint a dedicated service writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointRequest {
    /// The endpoint of the package.
    pub name: EndpointName,
    /// The port the manifest asks for.
    pub port: PortRequest,
}

/// One service of a valid manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    /// The alias, the key in `services`.
    pub alias: MemberAlias,
    /// The provider.
    pub provider: ProviderName,
    /// Dedicated or shared.
    pub ownership: Ownership,
    /// The selector text as written, if any.
    pub selector_text: Option<String>,
    /// The selector a dedicated service resolves: as written, or `stable`.
    /// None for a shared service.
    pub selector: Option<RuntimeSelector>,
    /// The instance a shared service borrows.
    pub instance_ref: Option<InstanceName>,
    /// The name a dedicated service's instance gets: `instanceName`, or
    /// `<metadata.name>-<alias>`.
    pub instance_name: Option<InstanceName>,
    /// `instanceName` as written, if any.
    pub written_instance_name: Option<InstanceName>,
    /// The endpoints the manifest writes, in manifest order.
    pub endpoints: Vec<EndpointRequest>,
    /// The settings document, without secret fields.
    pub settings: serde_json::Map<String, serde_json::Value>,
    /// The secret references, by field, in manifest order.
    pub secret_refs: Vec<(SecretFieldName, SecretRef)>,
    /// The services this one waits for.
    pub depends_on: Vec<DependencyEdge>,
}

impl ServiceSpec {
    /// The service as the registry snapshot keeps it.
    #[must_use]
    pub fn declared(&self) -> DeclaredService {
        DeclaredService::new(
            self.alias.clone(),
            self.provider.clone(),
            self.ownership,
            self.selector_text
                .as_deref()
                .and_then(|text| text.parse().ok()),
            self.instance_ref.clone(),
            self.written_instance_name.clone(),
            self.depends_on.clone(),
        )
    }
}

/// A manifest with no error finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentManifest {
    /// `metadata.name`.
    pub name: EnvironmentName,
    /// The services, in manifest order.
    pub services: Vec<ServiceSpec>,
}

impl EnvironmentManifest {
    /// The snapshot the registry keeps.
    #[must_use]
    pub fn declared(&self) -> Vec<DeclaredService> {
        self.services.iter().map(ServiceSpec::declared).collect()
    }

    /// Returns a service by alias.
    #[must_use]
    pub fn service(&self, alias: &MemberAlias) -> Option<&ServiceSpec> {
        self.services.iter().find(|service| &service.alias == alias)
    }
}

/// What [`validate`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation {
    /// The SHA-256 of the bytes checked.
    pub sha256: Sha256Digest,
    /// `apiVersion`, when the document is a mapping that has it and it is
    /// supported.
    pub api_version: Option<&'static str>,
    /// `metadata.name`, when it is a valid name.
    pub name: Option<EnvironmentName>,
    /// The services that decoded far enough to describe, in manifest
    /// order.
    pub services: Vec<DeclaredService>,
    /// Every finding.
    pub findings: Findings,
    /// The manifest, only when no finding is an error.
    pub manifest: Option<EnvironmentManifest>,
}

impl Validation {
    /// Returns whether the manifest has no error finding.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.manifest.is_some()
    }
}

/// Returns the SHA-256 of manifest bytes.
#[must_use]
pub fn manifest_digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

/// Checks manifest bytes against `catalog` (the providers' packages at the
/// catalog's current revision; pins are spec 0045). Reading a package of
/// a provider that is not approved runs nothing, so validation reads it
/// (spec 0037).
#[must_use]
pub fn validate(bytes: &[u8], catalog: &Catalog) -> Validation {
    let sha256 = manifest_digest(bytes);
    match load(bytes) {
        Err(findings) => Validation {
            sha256,
            api_version: None,
            name: None,
            services: Vec::new(),
            findings,
            manifest: None,
        },
        Ok(raw) => Checker::new(catalog).check(sha256, raw),
    }
}

struct Checker<'a> {
    catalog: &'a Catalog,
    findings: Findings,
}

/// What the per service rules decoded of one service, also when it has
/// findings, so the rules across services still see it.
struct Partial {
    spec: Option<ServiceSpec>,
    declared: Option<DeclaredService>,
    raw_alias: String,
    alias: Option<MemberAlias>,
    instance_ref: Option<InstanceName>,
    instance_name: Option<InstanceName>,
    instance_name_written: bool,
    /// Every fixed endpoint with its host.
    fixed: Vec<(EndpointName, IpAddr, Port)>,
    depends_on: Vec<DependencyEdge>,
    /// The provider of a dedicated service whose package loaded and is not
    /// `multiInstance`; `None` for every other service.
    single_instance: Option<ProviderName>,
}

impl<'a> Checker<'a> {
    fn new(catalog: &'a Catalog) -> Self {
        Self {
            catalog,
            findings: Findings::default(),
        }
    }

    fn push(&mut self, code: FindingCode, path: &str, service: Option<&MemberAlias>) {
        self.findings.push(
            Finding::error(code, FindingLocation::Field(path.to_owned())).for_service(service),
        );
    }

    fn push_detail(
        &mut self,
        code: FindingCode,
        path: &str,
        service: Option<&MemberAlias>,
        detail: &str,
    ) {
        self.findings.push(
            Finding::error(code, FindingLocation::Field(path.to_owned()))
                .for_service(service)
                .with_detail(detail),
        );
    }

    fn check(mut self, sha256: Sha256Digest, raw: RawEnvironment) -> Validation {
        let name = raw.metadata.name.parse::<EnvironmentName>().ok();
        if name.is_none() {
            self.push(FindingCode::InvalidName, "metadata.name", None);
        }
        let count = raw.services.len();
        if count == 0 {
            self.push_detail(
                FindingCode::InvalidValue,
                "services",
                None,
                "declare at least one service",
            );
        } else if count > MAX_SERVICES {
            self.push(FindingCode::TooManyServices, "services", None);
        }
        let aliases: BTreeSet<String> = raw.services.keys().cloned().collect();
        let mut partials = Vec::new();
        for (alias, service) in raw.services {
            let partial = self.service(name.as_ref(), &alias, service, &aliases);
            partials.push(partial);
        }
        self.cross_service(&partials);
        let services = partials
            .iter()
            .filter_map(|partial| partial.declared.clone())
            .collect();
        let manifest = if self.findings.has_errors() {
            None
        } else {
            name.clone().map(|name| EnvironmentManifest {
                name,
                services: partials.into_iter().filter_map(|p| p.spec).collect(),
            })
        };
        Validation {
            sha256,
            api_version: Some(API_VERSION),
            name,
            services,
            findings: self.findings,
            manifest,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn service(
        &mut self,
        environment: Option<&EnvironmentName>,
        raw_alias: &str,
        raw: RawService,
        aliases: &BTreeSet<String>,
    ) -> Partial {
        let base = format!("services.{}", safe_segment(raw_alias));
        let at = |field: &str| format!("{base}.{field}");
        let alias = raw_alias.parse::<MemberAlias>().ok();
        if alias.is_none() {
            self.push(FindingCode::InvalidName, &base, None);
        }
        let service = alias.as_ref();
        let errors_before = self.findings.count();

        // Provider and its package.
        let provider = raw.provider.parse::<ProviderName>().ok();
        let entry = match &provider {
            None => {
                self.push(FindingCode::InvalidValue, &at("provider"), service);
                None
            }
            Some(provider) => match self.catalog.entry(provider) {
                Some(CatalogEntry::Provider(entry)) => Some(entry.clone()),
                Some(CatalogEntry::Invalid(_)) => {
                    self.push(FindingCode::ProviderUnreadable, &at("provider"), service);
                    None
                }
                Some(CatalogEntry::Planned(_)) | None => {
                    self.push(FindingCode::UnknownProvider, &at("provider"), service);
                    None
                }
            },
        };

        // Ownership and instanceRef.
        let instance_ref = match &raw.instance_ref {
            None => None,
            Some(text) => {
                let parsed = text.parse::<InstanceName>().ok();
                if parsed.is_none() {
                    self.push(FindingCode::InvalidValue, &at("instanceRef"), service);
                }
                parsed
            }
        };
        let ownership = match raw.ownership.as_deref() {
            None if raw.instance_ref.is_some() => Some(Ownership::Shared),
            None => Some(Ownership::Dedicated),
            Some(text) => match text.parse::<Ownership>() {
                Ok(ownership) => Some(ownership),
                Err(_) => {
                    self.push(FindingCode::InvalidValue, &at("ownership"), service);
                    None
                }
            },
        };
        if let Some(ownership) = ownership
            && (ownership == Ownership::Shared) != raw.instance_ref.is_some()
        {
            self.push(FindingCode::OwnershipConflict, &at("ownership"), service);
        }
        let shared = raw.instance_ref.is_some();
        let single_instance = match (&provider, &entry) {
            (Some(provider), Some(entry))
                if !shared
                    && !entry
                        .package
                        .manifest()
                        .capabilities()
                        .contains(Capability::MultiInstance) =>
            {
                Some(provider.clone())
            }
            _ => None,
        };
        if shared {
            let forbidden = [
                ("version", raw.version.is_some()),
                ("instanceName", raw.instance_name.is_some()),
                ("endpoints", raw.endpoints.is_some()),
                ("settings", raw.settings.is_some()),
                ("secretRefs", raw.secret_refs.is_some()),
                ("storage", raw.storage.is_some()),
            ];
            for (field, written) in forbidden {
                if written {
                    self.push_detail(FindingCode::InstanceRefConflict, &at(field), service, field);
                }
            }
        }

        // Selector.
        let selector = if shared {
            None
        } else {
            match raw.version.as_deref() {
                None => Some(RuntimeSelector::Stable),
                Some(text) => match text.parse::<RuntimeSelector>() {
                    Ok(selector) => Some(selector),
                    Err(_) => {
                        self.push(FindingCode::InvalidValue, &at("version"), service);
                        None
                    }
                },
            }
        };

        // Instance name.
        let written_instance_name = match &raw.instance_name {
            None => None,
            Some(text) => {
                let parsed = text.parse::<InstanceName>().ok();
                if parsed.is_none() {
                    self.push(
                        FindingCode::InstanceNameInvalid,
                        &at("instanceName"),
                        service,
                    );
                }
                parsed
            }
        };
        let instance_name = if shared {
            None
        } else if let Some(name) = &written_instance_name {
            Some(name.clone())
        } else if raw.instance_name.is_some() {
            None
        } else {
            match (environment, &alias) {
                (Some(environment), Some(alias)) => {
                    let derived = format!("{environment}-{alias}")
                        .parse::<InstanceName>()
                        .ok();
                    if derived.is_none() {
                        self.push(FindingCode::InstanceNameInvalid, &base, service);
                    }
                    derived
                }
                _ => None,
            }
        };

        // Endpoints.
        let mut endpoints = Vec::new();
        if let (Some(written), false) = (&raw.endpoints, shared) {
            for (name, endpoint) in written {
                let path = format!("{}.{}", at("endpoints"), safe_segment(name));
                if let Some(request) = self.endpoint(entry.as_ref(), name, endpoint, &path, service)
                {
                    endpoints.push(request);
                }
            }
        }

        // Secrets.
        let mut secret_refs = Vec::new();
        if let (Some(written), false) = (&raw.secret_refs, shared) {
            for (field, value) in written {
                let path = format!("{}.{}", at("secretRefs"), safe_segment(field));
                if let Some(entry) = &entry {
                    let is_secret = entry
                        .package
                        .settings_schema()
                        .field(field)
                        .is_some_and(crate::provider::SettingField::is_secret);
                    if !is_secret {
                        self.push(FindingCode::UnknownSecret, &path, service);
                        continue;
                    }
                }
                let reference = match SecretRef::parse(value) {
                    Err(_) => {
                        self.push(FindingCode::SecretLiteral, &path, service);
                        continue;
                    }
                    Ok(reference)
                        if matches!(reference.first_segment(), "instance" | "stackly") =>
                    {
                        self.push(FindingCode::SecretReferenceReserved, &path, service);
                        continue;
                    }
                    Ok(reference) => reference,
                };
                if let Ok(field) = field.parse::<SecretFieldName>() {
                    secret_refs.push((field, reference));
                }
            }
        }

        // Settings.
        let mut settings = serde_json::Map::new();
        if let (Some(written), false) = (&raw.settings, shared) {
            settings = self.settings(
                entry.as_ref(),
                written,
                &secret_refs,
                &at("settings"),
                service,
            );
        }

        // Storage.
        if let (Some(storage), false) = (&raw.storage, shared) {
            if storage
                .mode
                .as_deref()
                .is_some_and(|mode| mode != "managed")
            {
                self.push(FindingCode::UnsupportedValue, &at("storage.mode"), service);
            }
            if storage.persistent == Some(false) {
                self.push(
                    FindingCode::UnsupportedValue,
                    &at("storage.persistent"),
                    service,
                );
            }
        }

        // Dependencies.
        let depends_on = self.dependencies(
            raw_alias,
            raw.depends_on.as_deref().unwrap_or_default(),
            aliases,
            &at("dependsOn"),
            service,
        );

        let declared = match (&alias, &provider, ownership) {
            (Some(alias), Some(provider), Some(ownership)) => Some(DeclaredService::new(
                alias.clone(),
                provider.clone(),
                ownership,
                if shared {
                    None
                } else {
                    raw.version.as_deref().and_then(|v| v.parse().ok())
                },
                instance_ref.clone(),
                written_instance_name.clone(),
                depends_on.clone(),
            )),
            _ => None,
        };
        let fixed = entry
            .as_ref()
            .map(|entry| {
                endpoints
                    .iter()
                    .filter_map(|endpoint| {
                        let PortRequest::Fixed(port) = endpoint.port else {
                            return None;
                        };
                        let host = entry
                            .package
                            .manifest()
                            .endpoints
                            .get(&endpoint.name)
                            .and_then(crate::provider::EndpointSpec::default_host)?;
                        Some((endpoint.name.clone(), host, port))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let partial_facts = (
            alias.clone(),
            instance_ref.clone(),
            instance_name.clone(),
            written_instance_name.is_some(),
            fixed,
            depends_on.clone(),
        );
        let clean = self.findings.count() == errors_before;
        let spec = match (clean, alias, provider, ownership) {
            (true, Some(alias), Some(provider), Some(ownership)) => Some(ServiceSpec {
                alias,
                provider,
                ownership,
                selector_text: if shared { None } else { raw.version.clone() },
                selector,
                instance_ref,
                instance_name,
                written_instance_name,
                endpoints,
                settings,
                secret_refs,
                depends_on,
            }),
            _ => None,
        };
        let (alias, instance_ref, instance_name, instance_name_written, fixed, depends_on) =
            partial_facts;
        Partial {
            spec,
            declared,
            raw_alias: raw_alias.to_owned(),
            alias,
            instance_ref,
            instance_name,
            instance_name_written,
            fixed,
            depends_on,
            single_instance,
        }
    }

    fn endpoint(
        &mut self,
        entry: Option<&ProviderEntry>,
        name: &str,
        endpoint: &RawEndpoint,
        path: &str,
        service: Option<&MemberAlias>,
    ) -> Option<EndpointRequest> {
        let port = match &endpoint.port {
            None => Some(PortRequest::Auto),
            Some(RawPort::Text(text)) if text == "auto" => Some(PortRequest::Auto),
            Some(RawPort::Fixed(number)) => u16::try_from(*number)
                .ok()
                .and_then(|number| Port::new(number).ok())
                .map(PortRequest::Fixed),
            Some(RawPort::Text(_)) => None,
        };
        if port.is_none() {
            self.push_detail(
                FindingCode::InvalidValue,
                &format!("{path}.port"),
                service,
                "a port is 1 to 65535 or auto",
            );
        }
        let entry = entry?;
        let Some(spec) = entry
            .package
            .manifest()
            .endpoints
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, spec)| spec)
        else {
            self.push(FindingCode::UnknownEndpoint, path, service);
            return None;
        };
        if !spec.transport().is_ip() {
            self.push(FindingCode::EndpointNotIp, path, service);
            return None;
        }
        if let Some(host) = &endpoint.host {
            let same = host
                .parse::<IpAddr>()
                .ok()
                .is_some_and(|host| Some(host) == spec.default_host());
            if !same {
                self.push(
                    FindingCode::EndpointHostUnsupported,
                    &format!("{path}.host"),
                    service,
                );
            }
        }
        if let Some(transport) = &endpoint.transport {
            let expected = match spec.transport() {
                EndpointTransport::Tcp => "tcp",
                EndpointTransport::Udp => "udp",
                EndpointTransport::Unix => "unix",
            };
            if transport != expected {
                self.push(
                    FindingCode::EndpointTransportMismatch,
                    &format!("{path}.transport"),
                    service,
                );
            }
        }
        Some(EndpointRequest {
            name: spec.name().clone(),
            port: port?,
        })
    }

    fn settings(
        &mut self,
        entry: Option<&ProviderEntry>,
        written: &serde_json::Map<String, serde_json::Value>,
        secret_refs: &[(SecretFieldName, SecretRef)],
        path: &str,
        service: Option<&MemberAlias>,
    ) -> serde_json::Map<String, serde_json::Value> {
        let Some(entry) = entry else {
            return written.clone();
        };
        let schema = entry.package.settings_schema();
        let mut document = serde_json::Map::new();
        for (key, value) in written {
            if schema
                .field(key)
                .is_some_and(crate::provider::SettingField::is_secret)
            {
                self.push(
                    FindingCode::SecretInSettings,
                    &format!("{path}.{}", safe_segment(key)),
                    service,
                );
            } else {
                document.insert(key.clone(), value.clone());
            }
        }
        let references: BTreeMap<String, SecretRef> = secret_refs
            .iter()
            .map(|(field, reference)| (field.to_string(), reference.clone()))
            .collect();
        let placeholder = crate::instance::create::placeholder_instance_id();
        let fallback = |setting: &crate::provider::SettingField| {
            stackly_domain::SecretPurpose::from_field_name(setting.name().as_str())
                .ok()
                .map(|purpose| SecretRef::for_instance(placeholder, &purpose))
        };
        if let Err(error) = settings_with_references(schema, &document, &references, fallback) {
            for issue in &error.issues {
                let code = if issue.settings_kind == Some(SettingsIssueKind::Unknown) {
                    FindingCode::UnknownSetting
                } else {
                    FindingCode::SettingsValue
                };
                let field = match &issue.location {
                    crate::provider::Location::Field(field) if !field.is_empty() => {
                        format!("{path}.{}", safe_path(field))
                    }
                    _ => path.to_owned(),
                };
                self.push(code, &field, service);
            }
        }
        document
    }

    fn dependencies(
        &mut self,
        raw_alias: &str,
        written: &[RawDependency],
        aliases: &BTreeSet<String>,
        path: &str,
        service: Option<&MemberAlias>,
    ) -> Vec<DependencyEdge> {
        if written.len() > MAX_DEPENDENCIES {
            self.push(FindingCode::TooManyDependencies, path, service);
        }
        let mut seen = BTreeSet::new();
        let mut edges = Vec::new();
        for (index, dependency) in written.iter().enumerate() {
            let at = format!("{path}[{index}]");
            if dependency.service == raw_alias {
                self.push(
                    FindingCode::SelfDependency,
                    &format!("{at}.service"),
                    service,
                );
                continue;
            }
            if !aliases.contains(&dependency.service) {
                self.push(
                    FindingCode::UnknownDependency,
                    &format!("{at}.service"),
                    service,
                );
                continue;
            }
            if !seen.insert(dependency.service.clone()) {
                self.push(
                    FindingCode::DuplicateDependency,
                    &format!("{at}.service"),
                    service,
                );
                continue;
            }
            let condition = match dependency.condition.as_deref() {
                None | Some("ready") => Some(DependencyCondition::Ready),
                Some(_) => {
                    self.push(
                        FindingCode::UnsupportedValue,
                        &format!("{at}.condition"),
                        service,
                    );
                    None
                }
            };
            let timeout = match dependency.timeout_seconds {
                None => Some(None),
                Some(seconds) if (1..=MAX_TIMEOUT_SECONDS).contains(&seconds) => {
                    Some(u32::try_from(seconds).ok())
                }
                Some(_) => {
                    self.push_detail(
                        FindingCode::InvalidValue,
                        &format!("{at}.timeoutSeconds"),
                        service,
                        "timeoutSeconds is 1 to 3600",
                    );
                    None
                }
            };
            if let (Ok(target), Some(condition), Some(timeout)) = (
                dependency.service.parse::<MemberAlias>(),
                condition,
                timeout,
            ) {
                edges.push(DependencyEdge::new(target, condition, timeout));
            }
        }
        edges
    }

    fn cross_service(&mut self, partials: &[Partial]) {
        // Two services borrowing one instance, or creating one name.
        let mut refs: BTreeSet<&InstanceName> = BTreeSet::new();
        let mut names: BTreeSet<&InstanceName> = BTreeSet::new();
        for partial in partials {
            let base = format!("services.{}", safe_segment(&partial.raw_alias));
            let alias = partial.alias.as_ref();
            if let Some(reference) = &partial.instance_ref
                && !refs.insert(reference)
            {
                self.push(
                    FindingCode::DuplicateInstanceRef,
                    &format!("{base}.instanceRef"),
                    alias,
                );
            }
            if let Some(name) = &partial.instance_name
                && !names.insert(name)
            {
                let path = if partial.instance_name_written {
                    format!("{base}.instanceName")
                } else {
                    base.clone()
                };
                self.push(FindingCode::InstanceNameDuplicate, &path, alias);
            }
        }

        // A second dedicated service of a provider that runs one instance
        // at a time, in manifest order: the first one passes.
        let mut single: BTreeSet<&ProviderName> = BTreeSet::new();
        for partial in partials {
            if let Some(provider) = &partial.single_instance
                && !single.insert(provider)
            {
                self.push(
                    FindingCode::ProviderSingleInstance,
                    &format!("services.{}.provider", safe_segment(&partial.raw_alias)),
                    partial.alias.as_ref(),
                );
            }
        }

        // Two fixed endpoints on one host and port: each one is named.
        let fixed: Vec<(IpAddr, Port, String, Option<&MemberAlias>)> = partials
            .iter()
            .flat_map(|partial| {
                partial.fixed.iter().map(move |(name, host, port)| {
                    (
                        *host,
                        *port,
                        format!(
                            "services.{}.endpoints.{name}.port",
                            safe_segment(&partial.raw_alias)
                        ),
                        partial.alias.as_ref(),
                    )
                })
            })
            .collect();
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
                self.push(FindingCode::EndpointConflict, path, *alias);
            }
        }

        // Cycles, over the edges that name a declared service.
        let graph: BTreeMap<&MemberAlias, Vec<&MemberAlias>> = partials
            .iter()
            .filter_map(|partial| {
                partial.alias.as_ref().map(|alias| {
                    (
                        alias,
                        partial
                            .depends_on
                            .iter()
                            .map(DependencyEdge::service)
                            .collect(),
                    )
                })
            })
            .collect();
        let order: Vec<&MemberAlias> = partials.iter().filter_map(|p| p.alias.as_ref()).collect();
        for cycle in cycles(&order, &graph) {
            let path = cycle
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" -> ");
            let first = cycle[0];
            self.findings.push(
                Finding::error(
                    FindingCode::DependencyCycle,
                    FindingLocation::Field(format!("services.{first}.dependsOn")),
                )
                .for_service(Some(first))
                .with_detail(path),
            );
        }
    }
}

/// Every elementary cycle found by a depth first walk in manifest order,
/// each once, as a path that returns to its first alias.
fn cycles<'a>(
    order: &[&'a MemberAlias],
    graph: &BTreeMap<&'a MemberAlias, Vec<&'a MemberAlias>>,
) -> Vec<Vec<&'a MemberAlias>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        New,
        Open,
        Done,
    }
    fn walk<'a>(
        node: &'a MemberAlias,
        graph: &BTreeMap<&'a MemberAlias, Vec<&'a MemberAlias>>,
        marks: &mut BTreeMap<&'a MemberAlias, Mark>,
        stack: &mut Vec<&'a MemberAlias>,
        found: &mut Vec<Vec<&'a MemberAlias>>,
    ) {
        marks.insert(node, Mark::Open);
        stack.push(node);
        for next in graph.get(node).into_iter().flatten() {
            match marks.get(next).copied().unwrap_or(Mark::New) {
                Mark::New => walk(next, graph, marks, stack, found),
                Mark::Open => {
                    if let Some(start) = stack.iter().position(|alias| alias == next) {
                        let mut cycle = stack[start..].to_vec();
                        cycle.push(next);
                        found.push(cycle);
                    }
                }
                Mark::Done => {}
            }
        }
        stack.pop();
        marks.insert(node, Mark::Done);
    }
    let mut marks = BTreeMap::new();
    let mut found = Vec::new();
    for node in order {
        if marks.get(node).copied().unwrap_or(Mark::New) == Mark::New {
            walk(node, graph, &mut marks, &mut Vec::new(), &mut found);
        }
    }
    found
}
