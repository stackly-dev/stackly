//! The pure evaluation of a create draft over the facts [`gather`] read
//! (spec 0020, *Evaluator create*, *Pemeriksaan port*, and *Field error*).
//! Preflight shows its rows; the create precheck refuses on the first row
//! that is an error, so both give the same answer for the same facts.
//!
//! [`gather`]: super::gather

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;

use stackly_domain::{
    Capability, DisplayName, Endpoint, EndpointName, InstallMode, InstanceId, InstanceName,
    IpEndpoint, Port, PortRequest, ProviderSourceId, Runtime, RuntimeState, SecretFieldName,
    SecretPurpose, SecretRef, SocketName, Transport, UnixEndpoint,
};
use stackly_platform::SecretBackend;

use super::check::{Check, CheckKind, CheckStatus, CreateError, FieldCode, FieldIssue};
use super::command::{CommandSpec, command_spec};
use super::draft::{
    CREATE_MIN_FREE_BYTES, CreateDraft, PortRequestDraft, SecretSourceKind, placeholder_instance_id,
};
use super::facts::{
    CreateFacts, DirFact, PackageSource, ProbeFact, ProgramCheck, ProviderFound, ProviderProblem,
    SourceFact,
};
use super::port::{self, Slot};
use crate::provider::{
    EndpointTransport, Location, PlanContext, PlanError, ResolvedEndpoint, RuntimeContext,
    SettingsIssueKind, SettingsValues, ValidatedPackage,
};
use crate::secret::SecretFieldRule;
use crate::supervisor::BindOutcome;

/// The result of evaluating one draft.
#[derive(Debug, Clone)]
pub struct Evaluation {
    /// The preflight rows, in order: `provider`, `runtime`, `name`, the
    /// endpoints, `settings`, `data`, `disk`, then the secrets.
    pub checks: Vec<Check>,
    /// Every problem with one field, ordered by path.
    pub field_issues: Vec<FieldIssue>,
    /// What the operation carries out; there only when no row is an error.
    pub resolved: Option<ResolvedCreate>,
    /// The equivalent command.
    pub command: CommandSpec,
}

impl Evaluation {
    /// Whether no row is an error: the Create button is on, and the
    /// precheck lets the request through.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.first_error().is_none()
    }

    /// Returns the first row that is an error.
    #[must_use]
    pub fn first_error_check(&self) -> Option<&Check> {
        self.checks
            .iter()
            .find(|check| check.status == CheckStatus::Error)
    }

    /// Returns the error the first failing row stands for: the one
    /// `instance.create` refuses with.
    #[must_use]
    pub fn first_error(&self) -> Option<&CreateError> {
        self.first_error_check()
            .and_then(|check| check.error.as_ref())
    }
}

/// One secret field of the new instance and where its value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretPlan {
    /// The field's name.
    pub field: SecretFieldName,
    /// The field's form label.
    pub title: String,
    /// The purpose its reference is derived from.
    pub purpose: SecretPurpose,
    /// The reference the instance uses: derived from the new instance ID,
    /// or the one a `bind` names.
    pub reference: SecretRef,
    /// The rules of the field.
    pub rule: SecretFieldRule,
    /// Where the value comes from.
    pub source: SecretSourceKind,
}

/// What the create operation carries out.
#[derive(Debug, Clone)]
pub struct ResolvedCreate {
    /// The package.
    pub package: Arc<ValidatedPackage>,
    /// Where it comes from, which decides whether the `pin` step runs.
    pub source: PackageSource,
    /// The row of the loaded catalog in `provider_sources`.
    pub source_id: Option<ProviderSourceId>,
    /// The runtime.
    pub runtime: Runtime,
    /// The exact name.
    pub name: InstanceName,
    /// The display name.
    pub display_name: Option<DisplayName>,
    /// The endpoints of the new instance, each with its stored form.
    pub endpoints: Vec<Endpoint>,
    /// The validated settings; secret fields hold their reference.
    pub settings: SettingsValues,
    /// One plan per secret field, in schema order.
    pub secrets: Vec<SecretPlan>,
}

/// Evaluates `draft` over `facts`, with the ID dry run plans use.
#[must_use]
pub fn evaluate(draft: &CreateDraft, facts: &CreateFacts) -> Evaluation {
    evaluate_for(draft, facts, placeholder_instance_id())
}

/// Evaluates `draft` over `facts` for the instance `id`, whose reference
/// every `generate`, `value`, and `copyFrom` secret takes.
#[must_use]
pub fn evaluate_for(draft: &CreateDraft, facts: &CreateFacts, id: InstanceId) -> Evaluation {
    let mut judge = Judge {
        draft,
        facts,
        id,
        checks: Vec::new(),
        issues: Vec::new(),
    };
    judge.run()
}

struct Judge<'a> {
    draft: &'a CreateDraft,
    facts: &'a CreateFacts,
    id: InstanceId,
    checks: Vec<Check>,
    issues: Vec<FieldIssue>,
}

/// What `settings` produced.
struct SettingsOutcome {
    values: SettingsValues,
    secrets: Vec<SecretPlan>,
}

impl Judge<'_> {
    fn run(&mut self) -> Evaluation {
        let found = self.facts.provider.as_ref().ok();
        self.provider_row();
        self.runtime_row();
        let name = self.name_row();
        let settings = found.and_then(|found| {
            let endpoints_ok = self.endpoint_rows(found);
            let settings = self.settings_row(found, name.as_ref());
            endpoints_ok.then_some(settings).flatten()
        });
        self.data_row();
        self.disk_row();
        if let Some(found) = found {
            self.secret_rows(found);
        }
        let mut issues = std::mem::take(&mut self.issues);
        issues.sort_by(|a, b| a.path.cmp(&b.path));
        issues.dedup();
        let resolved = if self.checks.iter().any(|c| c.status == CheckStatus::Error) {
            None
        } else {
            self.resolve(found, name, settings)
        };
        Evaluation {
            checks: std::mem::take(&mut self.checks),
            field_issues: issues,
            resolved,
            command: command_spec(self.draft, self.facts),
        }
    }

    fn push(&mut self, check: Check) {
        self.checks.push(check);
    }

    fn issue(&mut self, path: impl Into<String>, code: FieldCode) {
        self.issues.push(FieldIssue::new(path, code));
    }

    /// An error row: its status, `error`, and detail.
    fn error_row(
        &mut self,
        id: impl Into<String>,
        kind: CheckKind,
        title: impl Into<String>,
        detail: impl Into<String>,
        subject: Option<String>,
        error: CreateError,
    ) {
        self.push(Check {
            id: id.into(),
            kind,
            status: CheckStatus::Error,
            title: title.into(),
            detail: detail.into(),
            subject,
            suggested_port: None,
            error: Some(error),
        });
    }

    fn row(
        &mut self,
        id: impl Into<String>,
        kind: CheckKind,
        status: CheckStatus,
        title: impl Into<String>,
        detail: impl Into<String>,
        subject: Option<String>,
    ) {
        self.push(Check {
            id: id.into(),
            kind,
            status,
            title: title.into(),
            detail: detail.into(),
            subject,
            suggested_port: None,
            error: None,
        });
    }

    // provider

    fn provider_row(&mut self) {
        let draft = self.draft;
        match &self.facts.provider {
            Err(problem) => {
                let title = format!("Provider {}", draft.provider.name);
                let revision_path = if draft.provider.revision.is_some() {
                    "provider.revision"
                } else {
                    "provider.name"
                };
                let (error, detail) = match problem {
                    ProviderProblem::CatalogUnavailable { reason } => (
                        CreateError::CatalogUnavailable { reason },
                        "The provider catalog is not available.".to_owned(),
                    ),
                    ProviderProblem::NotFound => {
                        self.issue(revision_path, FieldCode::UnknownValue);
                        (
                            CreateError::ProviderNotFound {
                                provider: draft.provider.name.clone(),
                                revision: draft.provider.revision.clone(),
                            },
                            "No provider matches that name and revision.".to_owned(),
                        )
                    }
                    ProviderProblem::Ambiguous => {
                        self.issue("provider.name", FieldCode::InvalidValue);
                        (
                            CreateError::ProviderAmbiguous {
                                provider: draft.provider.name.clone(),
                                revision: draft.provider.revision.clone(),
                            },
                            "More than one package matches that revision.".to_owned(),
                        )
                    }
                    ProviderProblem::Unavailable { state } => {
                        self.issue("provider.name", FieldCode::InvalidValue);
                        (
                            CreateError::ProviderUnavailable {
                                provider: draft.provider.name.clone(),
                                state,
                            },
                            if *state == "planned" {
                                "This provider is not in this release.".to_owned()
                            } else {
                                "This provider failed validation.".to_owned()
                            },
                        )
                    }
                };
                self.error_row("provider", CheckKind::Provider, title, detail, None, error);
            }
            Ok(found) => {
                let package = &found.package;
                if package
                    .manifest()
                    .capabilities()
                    .contains(Capability::MultiInstance)
                {
                    return;
                }
                let id = package.key().provider();
                let existing = self
                    .facts
                    .instances
                    .iter()
                    .find(|instance| instance.provider_revision().provider() == id);
                if let Some(existing) = existing {
                    self.issue("provider.name", FieldCode::InvalidValue);
                    let title = format!(
                        "{} revision {}",
                        found.display_name,
                        package.key().revision()
                    );
                    self.error_row(
                        "provider",
                        CheckKind::Provider,
                        title,
                        format!("{} runs one instance at a time.", found.display_name),
                        None,
                        CreateError::CapabilityUnsupported {
                            provider: id.to_string(),
                            existing: existing.id(),
                        },
                    );
                }
            }
        }
    }

    // runtime

    fn runtime_row(&mut self) {
        let facts = self.facts;
        let Some(runtime) = &facts.runtime else {
            self.issue("runtimeId", FieldCode::UnknownValue);
            self.error_row(
                "runtime",
                CheckKind::Runtime,
                "Runtime",
                "No runtime has that ID.",
                None,
                CreateError::RuntimeNotFound {
                    runtime_id: self.draft.runtime_id,
                },
            );
            return;
        };
        let shown = facts.provider.as_ref().map_or_else(
            |_| runtime.software().to_string(),
            |found| found.display_name.clone(),
        );
        let title = format!("Runtime {shown} {}", runtime.version());
        if let Ok(found) = &facts.provider
            && runtime.software() != found.package.software()
        {
            self.issue("runtimeId", FieldCode::InvalidValue);
            self.error_row(
                "runtime",
                CheckKind::Runtime,
                title,
                "This runtime runs other software than the provider.",
                None,
                CreateError::RuntimeMismatch {
                    runtime_software: runtime.software().to_string(),
                    provider_software: found.package.software().to_string(),
                },
            );
            return;
        }
        if runtime.state() != RuntimeState::Installed {
            self.issue("runtimeId", FieldCode::InvalidValue);
            self.error_row(
                "runtime",
                CheckKind::Runtime,
                title,
                format!("This runtime is {}.", runtime.state().as_code()),
                None,
                CreateError::RuntimeUnavailable {
                    runtime_id: runtime.id(),
                    state: runtime.state().as_code(),
                },
            );
            return;
        }
        if let Some(ProgramCheck::Missing(executable)) = &facts.programs {
            self.issue("runtimeId", FieldCode::InvalidValue);
            self.error_row(
                "runtime",
                CheckKind::Runtime,
                title,
                "The runtime lacks a program the provider needs.",
                None,
                CreateError::RuntimeIncompatible {
                    runtime_id: runtime.id(),
                    executable: executable.clone(),
                },
            );
            return;
        }
        match runtime.install() {
            InstallMode::External { .. } => self.row(
                "runtime",
                CheckKind::Runtime,
                CheckStatus::Caution,
                title,
                "External runtime · not reproducible",
                None,
            ),
            InstallMode::Managed { artifact_sha256 } => {
                let digest = artifact_sha256.to_string();
                self.row(
                    "runtime",
                    CheckKind::Runtime,
                    CheckStatus::Ok,
                    title,
                    format!("Verified {}", &digest[..digest.len().min(12)]),
                    None,
                );
            }
        }
    }

    // name

    fn name_row(&mut self) -> Option<(InstanceName, Option<DisplayName>)> {
        let draft = self.draft;
        let name = draft.name.parse::<InstanceName>();
        let display = draft.display_name.as_deref().map(str::parse::<DisplayName>);
        let mut invalid_detail = None;
        if name.is_err() {
            self.issue("name", FieldCode::InvalidFormat);
            invalid_detail =
                Some("Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter.");
        }
        if matches!(display, Some(Err(_))) {
            self.issue("displayName", FieldCode::InvalidFormat);
            invalid_detail.get_or_insert("Use up to 128 characters without control characters.");
        }
        if let Some(detail) = invalid_detail {
            self.error_row(
                "name",
                CheckKind::Name,
                "Name available",
                detail,
                None,
                CreateError::NameInvalid,
            );
            return None;
        }
        let name = name.ok()?;
        let display = display.transpose().ok()?;
        if let Some(existing) = self
            .facts
            .instances
            .iter()
            .find(|instance| *instance.name() == name)
        {
            self.issue("name", FieldCode::InvalidValue);
            self.error_row(
                "name",
                CheckKind::Name,
                "Name available",
                format!("An instance named {name} already exists."),
                None,
                CreateError::NameTaken {
                    existing: existing.id(),
                },
            );
            return None;
        }
        self.row(
            "name",
            CheckKind::Name,
            CheckStatus::Ok,
            "Name available",
            name.to_string(),
            None,
        );
        Some((name, display))
    }

    // endpoints

    /// Judges every IP endpoint of the manifest and every request that
    /// names none; returns whether all passed.
    fn endpoint_rows(&mut self, found: &ProviderFound) -> bool {
        let before = self.error_count();
        let slots = port::slots(&found.package);
        for slot in &slots {
            self.endpoint_row(&slots, slot);
        }
        self.request_rows(found);
        self.error_count() == before
    }

    fn error_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == CheckStatus::Error)
            .count()
    }

    fn request_index(&self, name: &EndpointName) -> Option<usize> {
        self.draft
            .endpoints
            .iter()
            .position(|(requested, _)| requested == name.as_str())
    }

    fn endpoint_row(&mut self, slots: &[Slot], slot: &Slot) {
        let id = format!("endpoint.{}", slot.name);
        let subject = Some(slot.name.to_string());
        let index = self.request_index(&slot.name);
        let request = index.map(|i| self.draft.endpoints[i].1);
        let port_path = index.map(|i| format!("endpoints[{i}].port"));
        let Some(PortRequestDraft::Fixed(raw)) = request else {
            self.row(
                id,
                CheckKind::Endpoint,
                CheckStatus::Ok,
                format!("Endpoint {} · {}", slot.host, slot.name),
                "Port assigned when the instance starts",
                subject,
            );
            return;
        };
        let title = |port: u16| format!("Endpoint {}:{port}", slot.host);
        let invalid = |this: &mut Self, code: FieldCode, detail: &str| {
            if let Some(path) = &port_path {
                this.issue(path.clone(), code);
            }
            this.error_row(
                id.clone(),
                CheckKind::Endpoint,
                title(raw),
                detail,
                subject.clone(),
                CreateError::EndpointInvalid {
                    endpoint: slot.name.to_string(),
                },
            );
        };
        // Step 1.
        let Ok(port) = Port::new(raw) else {
            invalid(self, FieldCode::OutOfRange, "Use a port from 1 to 65535.");
            return;
        };
        // Step 2.
        if port::conflicts_in_draft(self.draft, slots, slot, port) {
            invalid(
                self,
                FieldCode::InvalidValue,
                "Another endpoint of this instance uses this port.",
            );
            return;
        }
        let facts = self.facts.ports.get(&slot.name).copied();
        let suggestion = facts.and_then(|facts| facts.suggestion).map(Port::get);
        let failing = |this: &mut Self, detail: String, error: CreateError| {
            if let Some(path) = &port_path {
                this.issue(path.clone(), FieldCode::InvalidValue);
            }
            this.push(Check {
                id: id.clone(),
                kind: CheckKind::Endpoint,
                status: CheckStatus::Error,
                title: title(raw),
                detail,
                subject: subject.clone(),
                suggested_port: suggestion,
                error: Some(error),
            });
        };
        // Step 3.
        if let Some(other) = port::conflicts_with_instances(&self.facts.instances, slot, port) {
            let owner = self
                .facts
                .instances
                .iter()
                .find(|instance| instance.id() == other)
                .map_or_else(String::new, |instance| instance.name().to_string());
            failing(
                self,
                format!("In use by {owner}"),
                CreateError::EndpointConflict {
                    endpoint: slot.name.to_string(),
                    port,
                    other,
                },
            );
            return;
        }
        // Steps 4 to 6.
        let Some(facts) = facts else {
            // Nothing probed it: the draft was gathered before it asked
            // for this port, so the row cannot be judged.
            self.row(
                id,
                CheckKind::Endpoint,
                CheckStatus::Caution,
                title(raw),
                "Couldn’t check this port",
                subject,
            );
            return;
        };
        match facts.exact {
            BindOutcome::InUse => failing(
                self,
                "In use by another process".to_owned(),
                CreateError::EndpointInUse {
                    endpoint: slot.name.to_string(),
                    address: slot.host,
                    port,
                },
            ),
            outcome @ (BindOutcome::Denied | BindOutcome::Unavailable) => failing(
                self,
                "macOS won’t let Stackly use this port".to_owned(),
                CreateError::EndpointUnavailable {
                    endpoint: slot.name.to_string(),
                    address: slot.host,
                    port,
                    reason: outcome.as_code(),
                },
            ),
            BindOutcome::Free => {
                let (status, detail) = port_notes(slot.host, port, &facts);
                self.row(id, CheckKind::Endpoint, status, title(raw), detail, subject);
            }
        }
    }

    /// One more row for each request that names no endpoint of the
    /// manifest, a socket, or an endpoint already named.
    fn request_rows(&mut self, found: &ProviderFound) {
        let specs: Vec<_> = found.package.endpoints().collect();
        for (index, (name, _)) in self.draft.endpoints.iter().enumerate() {
            let spec = specs.iter().find(|spec| spec.name().as_str() == name);
            let repeated = self.draft.endpoints[..index]
                .iter()
                .any(|(earlier, _)| earlier == name);
            let (code, detail) = match spec {
                None => (
                    FieldCode::UnknownValue,
                    "This provider has no endpoint by that name.",
                ),
                Some(spec) if spec.transport() == EndpointTransport::Unix => {
                    (FieldCode::InvalidValue, "Sockets don’t take a port.")
                }
                Some(_) if repeated => (FieldCode::InvalidValue, "This endpoint is listed twice."),
                Some(_) => continue,
            };
            self.issue(format!("endpoints[{index}].name"), code);
            self.error_row(
                format!("endpoint.request.{index}"),
                CheckKind::Endpoint,
                format!("Endpoint {name}"),
                detail,
                Some(name.clone()),
                CreateError::EndpointInvalid {
                    endpoint: name.clone(),
                },
            );
        }
    }

    // settings

    fn settings_row(
        &mut self,
        found: &ProviderFound,
        name: Option<&(InstanceName, Option<DisplayName>)>,
    ) -> Option<SettingsOutcome> {
        let package = &found.package;
        let schema = package.settings_schema();
        let before = self.issues.len();
        let mut bad_endpoint: Option<CreateError> = None;

        let document = match &self.draft.settings {
            serde_json::Value::Object(map) => map.clone(),
            _ => {
                self.issue("settings", FieldCode::InvalidValue);
                serde_json::Map::new()
            }
        };

        // The secret entries of the draft.
        let mut sources: BTreeMap<String, SecretSourceKind> = BTreeMap::new();
        for (index, (field, source)) in self.draft.secrets.iter().enumerate() {
            let at = format!("secrets[{index}].field");
            let is_secret = schema
                .field(field)
                .is_some_and(|setting| SecretFieldRule::from_setting(setting).is_some());
            if !is_secret {
                self.issue(at, FieldCode::UnknownValue);
            } else if sources.contains_key(field) {
                self.issue(at, FieldCode::InvalidValue);
            } else {
                sources.insert(field.clone(), source.clone());
            }
        }

        // The secret fields of the schema and their references.
        let mut plans = Vec::new();
        let mut doc = document.clone();
        for setting in schema.fields() {
            let Some(rule) = SecretFieldRule::from_setting(setting) else {
                continue;
            };
            if document.contains_key(setting.name().as_str()) {
                self.issue(
                    format!("settings.{}", setting.name()),
                    FieldCode::InvalidValue,
                );
            }
            let (Ok(field), Ok(purpose)) = (
                setting.name().as_str().parse::<SecretFieldName>(),
                SecretPurpose::from_field_name(setting.name().as_str()),
            ) else {
                continue;
            };
            let source = sources
                .get(setting.name().as_str())
                .cloned()
                .unwrap_or(SecretSourceKind::Generate);
            let reference = match &source {
                SecretSourceKind::Bind(text) => SecretRef::parse(text).ok(),
                _ => None,
            }
            .unwrap_or_else(|| SecretRef::for_instance(self.id, &purpose));
            doc.insert(
                setting.name().to_string(),
                serde_json::Value::String(reference.to_string()),
            );
            plans.push(SecretPlan {
                field,
                title: setting.title().to_owned(),
                purpose,
                reference,
                rule,
                source,
            });
        }

        let mut values = None;
        if self.issues.len() == before {
            match schema.validate(&serde_json::Value::Object(doc)) {
                Ok(valid) => values = Some(valid),
                Err(error) => {
                    for issue in &error.issues {
                        let code = issue
                            .settings_kind
                            .map_or(FieldCode::InvalidValue, field_code);
                        self.issue(settings_path(&issue.location), code);
                    }
                }
            }
        }
        if let Some(valid) = &values {
            bad_endpoint = self.dry_run(found, valid, name);
        }

        let count = self.issues.len() - before;
        if count == 0 && bad_endpoint.is_none() {
            return values.map(|values| SettingsOutcome {
                values,
                secrets: plans,
            });
        }
        let error = bad_endpoint.unwrap_or(CreateError::SettingsInvalid);
        self.error_row(
            "settings",
            CheckKind::Settings,
            "Settings",
            format!("{count} settings need attention"),
            None,
            error,
        );
        None
    }

    /// Builds the init and run plans for the placeholder instance, as the
    /// operation will, and turns a plan error into field issues. Returns an
    /// endpoint error when an endpoint is what the plan refuses.
    fn dry_run(
        &mut self,
        found: &ProviderFound,
        values: &SettingsValues,
        name: Option<&(InstanceName, Option<DisplayName>)>,
    ) -> Option<CreateError> {
        let facts = self.facts;
        let package = &found.package;
        let id = placeholder_instance_id();
        let layout = &facts.layout;
        let endpoints: BTreeMap<EndpointName, ResolvedEndpoint> = package
            .endpoints()
            .map(|spec| {
                let resolved = match spec.transport() {
                    EndpointTransport::Unix => ResolvedEndpoint::Unix,
                    _ => {
                        let requested = self
                            .draft
                            .endpoints
                            .iter()
                            .find(|(requested, _)| requested == spec.name().as_str())
                            .and_then(|(_, request)| match request {
                                PortRequestDraft::Fixed(port) => Port::new(*port).ok(),
                                PortRequestDraft::Auto => None,
                            });
                        ResolvedEndpoint::Ip {
                            host: spec
                                .default_host()
                                .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                            port: requested.or(spec.default_port()).or(Port::new(49_152).ok()),
                        }
                    }
                };
                (spec.name().clone(), resolved)
            })
            .collect();
        let runtime_root = facts
            .runtime_root
            .clone()
            .unwrap_or_else(|| layout.roots().runtime().to_path_buf());
        let (Ok(version), Ok(line)) = (
            facts
                .runtime
                .as_ref()
                .map_or_else(|| "0".parse(), |runtime| Ok(runtime.version().clone())),
            facts
                .runtime
                .as_ref()
                .map_or_else(|| "0".parse(), |runtime| Ok(runtime.line().clone())),
        ) else {
            return None;
        };
        let context = PlanContext {
            instance_id: id,
            instance_name: name.map_or_else(
                || {
                    "instance"
                        .parse()
                        .expect("instance is a valid instance name")
                },
                |(name, _)| name.clone(),
            ),
            paths: layout.instance_paths(id),
            runtime: RuntimeContext {
                root: runtime_root,
                version,
                line,
            },
            endpoints,
            settings: values.clone(),
        };
        let error = package
            .init_plan(&context)
            .map(drop)
            .and_then(|()| package.run_plan(&context).map(drop))
            .err()?;
        match error {
            PlanError::Settings(settings) => {
                for issue in &settings.issues {
                    let code = issue
                        .settings_kind
                        .map_or(FieldCode::InvalidValue, field_code);
                    self.issue(settings_path(&issue.location), code);
                }
                None
            }
            PlanError::EndpointUnresolved { endpoint }
            | PlanError::EndpointNotLoopback { endpoint } => {
                if let Some(index) = self.request_index(&endpoint) {
                    self.issue(format!("endpoints[{index}].port"), FieldCode::InvalidValue);
                }
                Some(CreateError::EndpointInvalid {
                    endpoint: endpoint.to_string(),
                })
            }
            other => {
                let path = match &other {
                    PlanError::SecretUnavailable { field } => format!("settings.{field}"),
                    _ => "settings".to_owned(),
                };
                self.issue(path, FieldCode::InvalidValue);
                None
            }
        }
    }

    // data and disk

    fn data_row(&mut self) {
        let data = &self.facts.data;
        let title = "Empty data directory";
        let problem = [("instances", &data.instances), ("retained", &data.retained)]
            .into_iter()
            .find_map(|(path, fact)| match fact {
                DirFact::Private => None,
                DirFact::Problem(reason) => Some((path, reason.clone())),
            });
        if let Some((path, reason)) = problem {
            self.error_row(
                "data",
                CheckKind::Data,
                title,
                format!("The {path} folder is not a private folder of yours."),
                None,
                CreateError::DataUnavailable {
                    path: path.to_owned(),
                    reason,
                },
            );
            return;
        }
        match &data.volume {
            Err(reason) => self.error_row(
                "data",
                CheckKind::Data,
                title,
                "Stackly can’t read the volume of its data folder.",
                None,
                CreateError::DataUnavailable {
                    path: ".".to_owned(),
                    reason: reason.clone(),
                },
            ),
            Ok(volume) if !volume.local || !matches!(volume.fs_type.as_str(), "apfs" | "hfs") => {
                self.error_row(
                    "data",
                    CheckKind::Data,
                    title,
                    "Keep the Stackly data folder on a local APFS volume.",
                    None,
                    CreateError::StorageUnsupported {
                        fs_type: volume.fs_type.clone(),
                        root: ".".to_owned(),
                    },
                );
            }
            Ok(_) => self.row(
                "data",
                CheckKind::Data,
                CheckStatus::Ok,
                title,
                "instances/<new instance id>/data · created new for this instance",
                None,
            ),
        }
    }

    fn disk_row(&mut self) {
        match &self.facts.disk {
            Err(reason) => self.error_row(
                "disk",
                CheckKind::Disk,
                "Disk space",
                "Stackly can’t read the free space of its data folder.",
                None,
                CreateError::DataUnavailable {
                    path: "instances".to_owned(),
                    reason: reason.clone(),
                },
            ),
            Ok(available) if *available < CREATE_MIN_FREE_BYTES => self.error_row(
                "disk",
                CheckKind::Disk,
                "Disk space",
                format!(
                    "{} free · needs at least {}",
                    human_bytes(*available),
                    human_bytes(CREATE_MIN_FREE_BYTES)
                ),
                None,
                CreateError::InsufficientSpace {
                    needed: CREATE_MIN_FREE_BYTES,
                    available: *available,
                },
            ),
            Ok(available) => self.row(
                "disk",
                CheckKind::Disk,
                CheckStatus::Ok,
                "Disk space",
                format!(
                    "{} free · needs at least {}",
                    human_bytes(*available),
                    human_bytes(CREATE_MIN_FREE_BYTES)
                ),
                None,
            ),
        }
    }

    // secrets

    fn secret_rows(&mut self, found: &ProviderFound) {
        let facts = self.facts;
        let draft = self.draft;
        let schema = found.package.settings_schema();
        let mut seen: Vec<&str> = Vec::new();
        for setting in schema.fields() {
            let Some(rule) = SecretFieldRule::from_setting(setting) else {
                continue;
            };
            let field = setting.name().to_string();
            let entry = draft
                .secrets
                .iter()
                .enumerate()
                .find(|(_, (name, _))| *name == field);
            seen.push(setting.name().as_str());
            let source = entry.map_or(&SecretSourceKind::Generate, |(_, (_, source))| source);
            let id = format!("secret.{field}");
            let title = format!("{} stored in Keychain", setting.title());
            let subject = Some(field.clone());
            if facts.secrets.backend == SecretBackend::Unsupported {
                self.error_row(
                    id,
                    CheckKind::Secret,
                    title,
                    "This Mac has no secret store Stackly can use.",
                    subject,
                    CreateError::SecretStoreUnsupported,
                );
                continue;
            }
            let ok_detail;
            match source {
                SecretSourceKind::Generate => {
                    ok_detail = "Generated on create, never logged".to_owned();
                }
                SecretSourceKind::Value => {
                    ok_detail = "Entered value, never logged".to_owned();
                    let shape = draft
                        .secret_values
                        .iter()
                        .find(|(name, _)| *name == field)
                        .map(|(_, shape)| *shape);
                    if let Some(shape) = shape
                        && let Err(reason) = rule.check(shape.chars, shape.has_control)
                    {
                        if let Some((index, _)) = entry {
                            self.issue(
                                format!("secrets[{index}].input.content"),
                                FieldCode::InvalidValue,
                            );
                        }
                        self.error_row(
                            id,
                            CheckKind::Secret,
                            title,
                            "This value breaks the rules of the field.",
                            subject,
                            CreateError::SecretInvalidValue {
                                field: field.clone(),
                                reason,
                            },
                        );
                        continue;
                    }
                }
                SecretSourceKind::CopyFrom(text) | SecretSourceKind::Bind(text) => {
                    let verb = if matches!(source, SecretSourceKind::CopyFrom(_)) {
                        "Copied from"
                    } else {
                        "Uses"
                    };
                    ok_detail = format!("{verb} {text}");
                    let fact = facts.secrets.sources.get(text).copied();
                    let reference = SecretRef::parse(text).ok();
                    let failure = match (fact, reference) {
                        (Some(SourceFact::Invalid) | None, _) | (_, None) => Some((
                            format!("{text} is not a secret reference."),
                            CreateError::SecretInvalidReference {
                                field: field.clone(),
                            },
                        )),
                        (Some(SourceFact::Unregistered), Some(reference)) => Some((
                            format!("{text} is not a known secret."),
                            CreateError::SecretNotFound { reference },
                        )),
                        (Some(SourceFact::Registered(ProbeFact::Missing)), Some(reference)) => {
                            Some((
                                format!("{text} has no stored value yet"),
                                CreateError::SecretNeedsSecret { reference },
                            ))
                        }
                        _ => None,
                    };
                    if let Some((detail, error)) = failure {
                        self.error_row(id, CheckKind::Secret, title, detail, subject, error);
                        continue;
                    }
                    if let Some(SourceFact::Registered(
                        probe @ (ProbeFact::Busy | ProbeFact::Failed),
                    )) = fact
                    {
                        let _ = probe;
                        self.row(
                            id,
                            CheckKind::Secret,
                            CheckStatus::Caution,
                            title,
                            "Keychain is busy; checked again when you create",
                            subject,
                        );
                        continue;
                    }
                    if fact == Some(SourceFact::Registered(ProbeFact::Locked)) {
                        self.row(
                            id,
                            CheckKind::Secret,
                            CheckStatus::Caution,
                            title,
                            "Keychain is locked; macOS asks you to unlock it when you create",
                            subject,
                        );
                        continue;
                    }
                }
            }
            if facts.secrets.state == stackly_platform::StoreState::Locked {
                self.row(
                    id,
                    CheckKind::Secret,
                    CheckStatus::Caution,
                    title,
                    "Keychain is locked; macOS asks you to unlock it when you create",
                    subject,
                );
            } else {
                self.row(
                    id,
                    CheckKind::Secret,
                    CheckStatus::Ok,
                    title,
                    ok_detail,
                    subject,
                );
            }
        }
    }

    // result

    fn resolve(
        &self,
        found: Option<&ProviderFound>,
        name: Option<(InstanceName, Option<DisplayName>)>,
        settings: Option<SettingsOutcome>,
    ) -> Option<ResolvedCreate> {
        let found = found?;
        let (name, display_name) = name?;
        let settings = settings?;
        let runtime = self.facts.runtime.clone()?;
        let endpoints = self.build_endpoints(found)?;
        Some(ResolvedCreate {
            package: Arc::clone(&found.package),
            source: found.source,
            source_id: self.facts.source_id,
            runtime,
            name,
            display_name,
            endpoints,
            settings: settings.values,
            secrets: settings.secrets,
        })
    }

    /// The endpoints the new instance holds: an IP endpoint with the
    /// request of the draft (none means `Auto`), and a Unix endpoint with
    /// its socket name rendered once (spec 0020, *Socket Unix dengan token
    /// port*).
    fn build_endpoints(&self, found: &ProviderFound) -> Option<Vec<Endpoint>> {
        let request_of = |name: &EndpointName| {
            self.draft
                .endpoints
                .iter()
                .find(|(requested, _)| requested == name.as_str())
                .map_or(Some(PortRequest::Auto), |(_, request)| match request {
                    PortRequestDraft::Auto => Some(PortRequest::Auto),
                    PortRequestDraft::Fixed(port) => Port::new(*port).ok().map(PortRequest::Fixed),
                })
        };
        found
            .package
            .endpoints()
            .map(|spec| match spec.transport() {
                EndpointTransport::Unix => {
                    let socket =
                        rendered_socket_name(spec.name(), spec.socket_name_template(), |port| {
                            request_of(port)
                        })?;
                    Some(Endpoint::Unix(UnixEndpoint::new(
                        spec.name().clone(),
                        socket,
                    )))
                }
                transport => Some(Endpoint::Ip(IpEndpoint::new(
                    spec.name().clone(),
                    if transport == EndpointTransport::Tcp {
                        Transport::Tcp
                    } else {
                        Transport::Udp
                    },
                    spec.default_host()
                        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                    request_of(spec.name())?,
                    None,
                ))),
            })
            .collect()
    }
}

/// Renders the stored name of a Unix endpoint's socket: each
/// `${endpoint.<name>.port}` token becomes the fixed port of that endpoint,
/// or `auto` when it is `Auto`; the name is never written again.
fn rendered_socket_name(
    endpoint: &EndpointName,
    template: Option<String>,
    request_of: impl Fn(&EndpointName) -> Option<PortRequest>,
) -> Option<SocketName> {
    let Some(template) = template else {
        let mut text = format!("{endpoint}.sock");
        text.truncate(15);
        return text.parse().ok();
    };
    let mut text = String::new();
    let mut rest = template.as_str();
    while let Some(start) = rest.find("${endpoint.") {
        text.push_str(&rest[..start]);
        let after = &rest[start + "${endpoint.".len()..];
        let end = after.find('}')?;
        let token = &after[..end];
        let name: EndpointName = token.strip_suffix(".port")?.parse().ok()?;
        match request_of(&name)? {
            PortRequest::Fixed(port) => text.push_str(&port.to_string()),
            _ => text.push_str("auto"),
        }
        rest = &after[end + 1..];
    }
    text.push_str(rest);
    text.parse().ok()
}

/// The status and sentence of a fixed port that step 4 found free.
fn port_notes(
    host: IpAddr,
    port: Port,
    facts: &crate::instance::create::PortFacts,
) -> (CheckStatus, String) {
    match facts.wildcard {
        Some(BindOutcome::InUse) => {
            return (
                CheckStatus::Caution,
                "In use on another address of this Mac".to_owned(),
            );
        }
        Some(BindOutcome::Denied | BindOutcome::Unavailable) => {
            return (CheckStatus::Caution, "Couldn’t check this port".to_owned());
        }
        _ => {}
    }
    match facts.other_loopback {
        Some(BindOutcome::InUse) => {
            let other = if host.is_ipv4() {
                format!("[::1]:{port}")
            } else {
                format!("127.0.0.1:{port}")
            };
            (
                CheckStatus::Caution,
                format!(
                    "Also in use on {other}; clients that connect to localhost may reach that server"
                ),
            )
        }
        Some(BindOutcome::Denied | BindOutcome::Unavailable) => {
            (CheckStatus::Caution, "Couldn’t check this port".to_owned())
        }
        _ if host.is_loopback() => (CheckStatus::Ok, "Free on IPv4 and IPv6 loopback".to_owned()),
        _ => (CheckStatus::Ok, "Free".to_owned()),
    }
}

/// The path of a settings problem as a field issue.
fn settings_path(location: &Location) -> String {
    match location {
        Location::Field(path) if !path.is_empty() && path != "." => format!("settings.{path}"),
        _ => "settings".to_owned(),
    }
}

/// The `FieldCode` of a kind of settings problem (spec 0020, *Field
/// error*).
const fn field_code(kind: SettingsIssueKind) -> FieldCode {
    match kind {
        SettingsIssueKind::Unknown | SettingsIssueKind::Enum => FieldCode::UnknownValue,
        SettingsIssueKind::Pattern => FieldCode::InvalidFormat,
        SettingsIssueKind::TooShort | SettingsIssueKind::TooFewItems => FieldCode::TooShort,
        SettingsIssueKind::TooLong => FieldCode::TooLong,
        SettingsIssueKind::OutOfRange => FieldCode::OutOfRange,
        SettingsIssueKind::TooManyItems => FieldCode::TooMany,
        SettingsIssueKind::Required => FieldCode::Required,
        _ => FieldCode::InvalidValue,
    }
}

/// A size in the decimal units of the catalog ("41 GB", "256 MB").
fn human_bytes(bytes: u64) -> String {
    const KB: u64 = 1_000;
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;
    if bytes >= GB {
        format!("{} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{} KB", bytes / KB)
    }
}
