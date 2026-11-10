//! The pure plan of one config change (spec 0029, *Operasi*): what `apply`,
//! `rollback`, `restore`, and `change_endpoints` all turn into before any
//! side effect. The precheck of the daemon and the first step of the
//! operation call [`plan_change`] with the same facts, so they always agree.

use std::collections::BTreeMap;

use stackly_domain::{
    ApplyGate, ConfigRevisionNumber, ConfigRevisionOrigin, ConfigValuesJson, Endpoint,
    EndpointName, EndpointRequests, Instance, InstanceLifecycle, IpEndpoint, LifecycleRefusal,
    Port, PortRequest, apply_allowed,
};

use super::error::ConfigError;
use super::evaluate::{ConfigImpact, SettingChange, evaluate_change};
use super::gather::ConfigFacts;
use crate::instance::create::{CreateError, FieldIssue, PortProblem, judge_change};
use crate::operation::AuditNote;
use crate::supervisor::{PortProbe, SupervisorError};

/// What a caller asks to change on the active config revision.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConfigChange {
    /// New settings: the whole proposed settings document. The ports stay.
    Settings(serde_json::Value),
    /// The settings and ports of the last known good revision (`origin`
    /// `rollback`).
    Rollback,
    /// The settings and ports of an older revision of the history (`origin`
    /// `restored`).
    Restore(ConfigRevisionNumber),
    /// New port requests for some IP endpoints, by name. The settings stay.
    Endpoints(Vec<(EndpointName, PortRequest)>),
}

/// A port request that cannot be applied, with the free port to try
/// instead for a fixed port that is taken (spec 0029, AC-14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointIssue {
    /// The endpoint the request names.
    pub endpoint: String,
    /// Why, as the create error of the same case.
    pub error: CreateError,
    /// The first free port after the requested one.
    pub suggestion: Option<Port>,
}

/// A valid change that differs from the active revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlan {
    /// The active revision it follows.
    pub active: ConfigRevisionNumber,
    /// The settings of the new revision.
    pub values: ConfigValuesJson,
    /// The port requests of the new revision.
    pub endpoints: EndpointRequests,
    /// How the new revision comes to be.
    pub origin: ConfigRevisionOrigin,
    /// The revision it copies, for `rollback` and `restored`.
    pub source: Option<ConfigRevisionNumber>,
    /// The settings that change, in schema order.
    pub changes: Vec<SettingChange>,
    /// The endpoints whose request changes, as `(name, old, new)`.
    pub endpoint_changes: Vec<(EndpointName, PortRequest, PortRequest)>,
    /// The impact now.
    pub impact: ConfigImpact,
    /// Whether a running instance reloads instead of restarting: every
    /// changed setting is `apply: reload`, no port changes, and the provider
    /// can reload.
    pub reloads: bool,
}

impl ChangePlan {
    /// The impact of applying the plan to an instance in `lifecycle` (spec
    /// 0029, *State transitions*).
    ///
    /// # Errors
    ///
    /// Returns the [`LifecycleRefusal`] of a lifecycle that refuses a
    /// change.
    pub fn impact_at(
        &self,
        lifecycle: InstanceLifecycle,
    ) -> Result<ConfigImpact, LifecycleRefusal> {
        Ok(match apply_allowed(lifecycle)? {
            ApplyGate::Save => ConfigImpact::NextStart,
            ApplyGate::Reload if self.reloads => ConfigImpact::Reload,
            ApplyGate::Reload | ApplyGate::Restart => ConfigImpact::Restart,
        })
    }

    /// Whether any port request changes.
    #[must_use]
    pub fn changes_endpoints(&self) -> bool {
        !self.endpoint_changes.is_empty()
    }

    /// What the journal of the operation records (spec 0029, AC-20): the
    /// old and new revision numbers, the origin, the names of the settings
    /// that change, and the old and new port of each endpoint that moves.
    /// It holds no setting value, so no secret reaches the journal.
    #[must_use]
    pub fn audit(&self) -> serde_json::Value {
        let request = |request: PortRequest| match request {
            PortRequest::Fixed(port) => serde_json::Value::from(port.get()),
            PortRequest::Auto => serde_json::Value::from("auto"),
        };
        serde_json::json!({
            "oldRevision": self.active.get(),
            "newRevision": self.active.next().get(),
            "origin": self.origin.as_code(),
            "settings": self.changes.iter().map(|change| change.key.as_str()).collect::<Vec<_>>(),
            "ports": self.endpoint_changes.iter().map(|(name, old, new)| serde_json::json!({
                "endpoint": name.as_str(),
                "old": request(*old),
                "new": request(*new),
            })).collect::<Vec<_>>(),
        })
    }
}

impl ChangePlan {
    /// The audit note of the operation once the commit wrote the new
    /// revision (spec 0035, *Catatan audit pekerjaan*): its number as the
    /// config revision, then `oldRevision`, the names of the settings that
    /// change, each endpoint that moves as `<endpoint> <old> → <new>`
    /// (`auto` for a port Stackly picks), all joined by commas, and the
    /// impact. Like [`ChangePlan::audit`] it holds no setting value. The
    /// journal input keeps [`ChangePlan::audit`].
    #[must_use]
    pub fn audit_note(&self) -> AuditNote {
        let request = |request: PortRequest| match request {
            PortRequest::Fixed(port) => port.get().to_string(),
            PortRequest::Auto => "auto".to_owned(),
        };
        let mut note =
            AuditNote::new(Some(self.active.next())).with("oldRevision", self.active.get());
        if !self.changes.is_empty() {
            let settings: Vec<&str> = self
                .changes
                .iter()
                .map(|change| change.key.as_str())
                .collect();
            note = note.with("settings", settings.join(", "));
        }
        if !self.endpoint_changes.is_empty() {
            let ports: Vec<String> = self
                .endpoint_changes
                .iter()
                .map(|(name, old, new)| {
                    format!("{name} {} \u{2192} {}", request(*old), request(*new))
                })
                .collect();
            note = note.with("ports", ports.join(", "));
        }
        note.with("impact", impact_code(self.impact))
    }
}

/// The code of an impact in an audit note.
const fn impact_code(impact: ConfigImpact) -> &'static str {
    match impact {
        ConfigImpact::None => "none",
        ConfigImpact::NextStart => "next_start",
        ConfigImpact::Reload => "reload",
        ConfigImpact::Restart => "restart",
    }
}

/// What [`plan_change`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Planned {
    /// The change equals the active revision: no operation, no revision.
    Unchanged,
    /// A change to apply.
    Change(Box<ChangePlan>),
}

/// Why [`plan_change`] refuses.
#[derive(Debug)]
#[non_exhaustive]
pub enum Rejected {
    /// The settings break the schema or the rules of a changed field; one
    /// issue for each (`instance.settings_invalid`).
    Fields(Vec<FieldIssue>),
    /// A port request is not valid or not free (`instance.endpoint_invalid`
    /// and the port codes of spec 0020).
    Endpoints(Vec<EndpointIssue>),
    /// Any other refusal.
    Error(ConfigError),
}

impl From<ConfigError> for Rejected {
    fn from(error: ConfigError) -> Self {
        Self::Error(error)
    }
}

impl From<SupervisorError> for Rejected {
    fn from(error: SupervisorError) -> Self {
        Self::Error(error.into())
    }
}

/// The supervisor error of a lifecycle that refuses a change or a rename
/// (spec 0029, AC-6, AC-13): the codes of spec 0010 and spec 0026.
#[must_use]
pub fn refusal_error(refusal: LifecycleRefusal, lifecycle: InstanceLifecycle) -> SupervisorError {
    match refusal {
        LifecycleRefusal::Starting => SupervisorError::InstanceNotRunning { lifecycle },
        LifecycleRefusal::Stopping => SupervisorError::InstanceStopping,
        LifecycleRefusal::Unknown => SupervisorError::IdentityUnproven { pid: None },
        LifecycleRefusal::Initializing => SupervisorError::InstanceInitializing,
        LifecycleRefusal::Retained => SupervisorError::InstanceRetained,
    }
}

/// Plans `change` on the instance of `facts`, read at revision `base`
/// (spec 0029, AC-2, AC-3, AC-6, AC-9, AC-14). `others` are the other
/// instances that are not retained, for the port checks. The checks run in
/// this order: the base revision, the lifecycle, the target (a roll back or
/// a restore reads its revision), the settings, and the ports. Nothing is
/// written.
///
/// # Errors
///
/// Returns [`Rejected::Error`] with `config.revision_conflict` for a stale
/// `base`, the lifecycle code of AC-6, `config.no_last_known_good`, or a
/// restore of a revision the history no longer holds; [`Rejected::Fields`]
/// for settings that do not validate; and [`Rejected::Endpoints`] for a
/// port request that is not valid or not free.
pub fn plan_change(
    facts: &ConfigFacts,
    base: ConfigRevisionNumber,
    change: &ConfigChange,
    others: &[Instance],
    probe: &dyn PortProbe,
) -> Result<Planned, Rejected> {
    if base != facts.active.number {
        return Err(ConfigError::RevisionConflict {
            base,
            active: facts.active.number,
        }
        .into());
    }
    let lifecycle = facts.instance.lifecycle();
    if let Err(refusal) = apply_allowed(lifecycle) {
        return Err(refusal_error(refusal, lifecycle).into());
    }
    let current = EndpointRequests::of(&facts.instance);
    let target = resolve(facts, change, &current)?;
    let moved: Vec<(EndpointName, PortRequest, PortRequest)> = target
        .endpoints
        .iter()
        .filter_map(|(name, new)| {
            let old = current.get(name)?;
            (old != new).then(|| (name.clone(), old, new))
        })
        .collect();

    let active_json = facts.settings.to_json();
    let proposed = target.settings.as_ref().unwrap_or(&active_json);
    let evaluation = evaluate_change(
        facts.package.settings_schema(),
        &facts.settings,
        proposed,
        lifecycle,
        facts.reload_supported,
        !moved.is_empty(),
    );
    let Some(values) = evaluation.values.as_ref().filter(|_| evaluation.is_valid()) else {
        return Err(Rejected::Fields(evaluation.field_issues));
    };
    let issues = judge_endpoints(&facts.instance, &moved, &target.endpoints, others, probe);
    if !issues.is_empty() {
        return Err(Rejected::Endpoints(issues));
    }
    if evaluation.changes.is_empty() && moved.is_empty() {
        return Ok(Planned::Unchanged);
    }
    let values_json = ConfigValuesJson::new(values.to_json().to_string())
        .map_err(|error| SupervisorError::internal(error.to_string()))?;
    let reloads = evaluate_change(
        facts.package.settings_schema(),
        &facts.settings,
        proposed,
        InstanceLifecycle::Running,
        facts.reload_supported,
        !moved.is_empty(),
    )
    .impact
        == ConfigImpact::Reload;
    Ok(Planned::Change(Box::new(ChangePlan {
        active: facts.active.number,
        values: values_json,
        endpoints: target.endpoints,
        origin: target.origin,
        source: target.source,
        changes: evaluation.changes,
        endpoint_changes: moved,
        impact: evaluation.impact,
        reloads,
    })))
}

/// The settings document, port requests, and origin a change asks for.
struct Target {
    settings: Option<serde_json::Value>,
    endpoints: EndpointRequests,
    origin: ConfigRevisionOrigin,
    source: Option<ConfigRevisionNumber>,
}

fn resolve(
    facts: &ConfigFacts,
    change: &ConfigChange,
    current: &EndpointRequests,
) -> Result<Target, Rejected> {
    match change {
        ConfigChange::Settings(document) => Ok(Target {
            settings: Some(document.clone()),
            endpoints: current.clone(),
            origin: ConfigRevisionOrigin::Applied,
            source: None,
        }),
        ConfigChange::Rollback => {
            let good = facts
                .last_known_good
                .as_ref()
                .filter(|good| good.number != facts.active.number)
                .ok_or(ConfigError::NoLastKnownGood)?;
            copy_of(good, ConfigRevisionOrigin::Rollback)
        }
        ConfigChange::Restore(number) => {
            let found = facts
                .history
                .iter()
                .find(|revision| revision.number == *number)
                .ok_or(ConfigError::RevisionNotFound { number: *number })?;
            copy_of(found, ConfigRevisionOrigin::Restored)
        }
        ConfigChange::Endpoints(requests) => {
            let mut merged: BTreeMap<EndpointName, PortRequest> = current
                .iter()
                .map(|(name, request)| (name.clone(), request))
                .collect();
            let mut issues = Vec::new();
            let mut seen: Vec<&EndpointName> = Vec::new();
            for (name, request) in requests {
                if !merged.contains_key(name) || seen.contains(&name) {
                    issues.push(EndpointIssue {
                        endpoint: name.to_string(),
                        error: CreateError::EndpointInvalid {
                            endpoint: name.to_string(),
                        },
                        suggestion: None,
                    });
                    continue;
                }
                seen.push(name);
                merged.insert(name.clone(), *request);
            }
            if !issues.is_empty() {
                return Err(Rejected::Endpoints(issues));
            }
            Ok(Target {
                settings: None,
                endpoints: EndpointRequests::new(merged),
                origin: ConfigRevisionOrigin::Endpoints,
                source: None,
            })
        }
    }
}

fn copy_of(
    revision: &stackly_domain::ConfigRevisionRecord,
    origin: ConfigRevisionOrigin,
) -> Result<Target, Rejected> {
    let document = serde_json::from_str(revision.values.as_str()).map_err(|error| {
        SupervisorError::ConfigInvalid {
            field: "settings".to_owned(),
            reason: format!("the stored settings are not JSON: {error}"),
        }
    })?;
    Ok(Target {
        settings: Some(document),
        endpoints: revision.endpoints.clone(),
        origin,
        source: Some(revision.number),
    })
}

/// Checks every changed `Fixed` port with the create checks (spec 0029,
/// AC-14). An `Auto` request needs no check.
fn judge_endpoints(
    instance: &Instance,
    moved: &[(EndpointName, PortRequest, PortRequest)],
    target: &EndpointRequests,
    others: &[Instance],
    probe: &dyn PortProbe,
) -> Vec<EndpointIssue> {
    // The endpoints as they will be: a changed one asks for its new port
    // and has no bound port, every other one stays as it is.
    let after: Vec<Endpoint> = instance
        .endpoints()
        .iter()
        .map(|endpoint| match endpoint {
            Endpoint::Ip(ip) if moved.iter().any(|(name, ..)| name == ip.name()) => {
                Endpoint::Ip(IpEndpoint::new(
                    ip.name().clone(),
                    ip.transport(),
                    ip.address(),
                    target.get(ip.name()).unwrap_or(ip.request()),
                    None,
                ))
            }
            other => other.clone(),
        })
        .collect();
    let mut issues = Vec::new();
    for (name, _, new) in moved {
        let PortRequest::Fixed(port) = new else {
            continue;
        };
        let Some(Endpoint::Ip(ip)) = instance
            .endpoints()
            .iter()
            .find(|endpoint| matches!(endpoint, Endpoint::Ip(ip) if ip.name() == name))
        else {
            continue;
        };
        let siblings: Vec<Endpoint> = after
            .iter()
            .filter(|endpoint| !matches!(endpoint, Endpoint::Ip(other) if other.name() == name))
            .cloned()
            .collect();
        let endpoint = name.to_string();
        if let Err((problem, suggestion)) =
            judge_change(probe, ip, *port, &siblings, others, ip.bound())
        {
            let error = match problem {
                PortProblem::Sibling => CreateError::EndpointInvalid {
                    endpoint: endpoint.clone(),
                },
                PortProblem::Instance(other) => CreateError::EndpointConflict {
                    endpoint: endpoint.clone(),
                    port: *port,
                    other,
                },
                PortProblem::Bind(crate::supervisor::BindOutcome::InUse) => {
                    CreateError::EndpointInUse {
                        endpoint: endpoint.clone(),
                        address: ip.address(),
                        port: *port,
                    }
                }
                PortProblem::Bind(outcome) => CreateError::EndpointUnavailable {
                    endpoint: endpoint.clone(),
                    address: ip.address(),
                    port: *port,
                    reason: outcome.as_code(),
                },
            };
            issues.push(EndpointIssue {
                endpoint,
                error,
                suggestion,
            });
        }
    }
    issues
}
