//! What an Up reports (spec 0046 AC-10, *Kode error di wire*): its errors,
//! and the per service report the closing commit stores in
//! `environments.last_up`.

use stackly_domain::{
    DetailKey, ErrorCodeName, FailureItem, MemberAlias, NeedsSecretItem, ReasonCode, ServiceReport,
    UpPhase,
};

use crate::environment::register::EnvOpError;
use crate::operation::OperationError;
use crate::secret::{NeedsSecret, SecretError};

/// The most items `env.plan_blocked` and `env.up_incomplete` carry.
pub const UP_ITEMS_MAX: usize = 32;

const PLAN_BLOCKED: ErrorCodeName = ErrorCodeName::from_static("env.plan_blocked");
const PLAN_CHANGED: ErrorCodeName = ErrorCodeName::from_static("env.plan_changed");
const UP_INCOMPLETE: ErrorCodeName = ErrorCodeName::from_static("env.up_incomplete");

/// `env.dependency_not_ready`: the code of a `skipped` service.
pub const DEPENDENCY_NOT_READY: ErrorCodeName =
    ErrorCodeName::from_static("env.dependency_not_ready");
/// `env.service_not_ready`: an instance that runs did not become ready in
/// time.
pub const SERVICE_NOT_READY: ErrorCodeName = ErrorCodeName::from_static("env.service_not_ready");
/// `env.instance_blocked`: the instance of a service can't be started.
pub const INSTANCE_BLOCKED: ErrorCodeName = ErrorCodeName::from_static("env.instance_blocked");
/// `secret.needs_secret`, the code of a service in `needs_secret`.
pub const NEEDS_SECRET: ErrorCodeName = ErrorCodeName::from_static("secret.needs_secret");
/// `resource.busy`: a child start stayed queued too long.
pub const RESOURCE_BUSY: ErrorCodeName = ErrorCodeName::from_static("resource.busy");

/// One blocker of a plan, as `env.plan_blocked` lists it: the message is
/// the one the plan reply shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedItem {
    /// The service it is about, if any.
    pub service: Option<MemberAlias>,
    /// Its code.
    pub code: String,
    /// Its message.
    pub message: String,
}

/// Why an Up refused or failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UpError {
    /// A manifest or registry error shared with import (spec 0044).
    #[error(transparent)]
    Environment(EnvOpError),
    /// `env.plan_blocked`.
    #[error("the plan is blocked")]
    PlanBlocked {
        /// Every blocker, in plan order.
        blockers: Vec<BlockedItem>,
    },
    /// `env.plan_changed`.
    #[error("the plan changed since it was reviewed")]
    PlanChanged,
    /// `secret.needs_secret`: references that are not set (AC-2).
    #[error("a secret reference has no value")]
    NeedsSecret {
        /// Every reference, by text.
        references: Vec<NeedsSecretItem>,
    },
    /// `env.up_incomplete`: the Up ended with services that are not
    /// ready (AC-10).
    #[error("some services of the environment are not ready")]
    Incomplete {
        /// How many services are ready.
        ready: usize,
        /// How many services the Up worked on.
        total: usize,
        /// Every service that is not ready, in start order.
        services: Vec<ServiceReport>,
    },
}

fn key(text: &'static str) -> DetailKey {
    text.parse()
        .expect("the detail keys of spec 0046 are camelCase constants")
}

impl UpError {
    fn needs(references: &[NeedsSecretItem]) -> SecretError {
        SecretError::NeedsSecret {
            needs: NeedsSecret::new(references.to_vec()),
        }
    }
}

impl OperationError for UpError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::Environment(error) => error.code(),
            Self::PlanBlocked { .. } => PLAN_BLOCKED,
            Self::PlanChanged => PLAN_CHANGED,
            Self::NeedsSecret { .. } => NEEDS_SECRET,
            Self::Incomplete { .. } => UP_INCOMPLETE,
        }
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        match self {
            Self::Environment(error) => error.details(),
            Self::PlanBlocked { blockers } => vec![
                (key("count"), blockers.len().to_string()),
                (
                    key("omitted"),
                    blockers.len().saturating_sub(UP_ITEMS_MAX).to_string(),
                ),
            ],
            Self::PlanChanged => Vec::new(),
            Self::NeedsSecret { references } => Self::needs(references).details(),
            Self::Incomplete {
                ready,
                total,
                services,
            } => vec![
                (key("ready"), ready.to_string()),
                (key("total"), total.to_string()),
                (key("count"), services.len().to_string()),
                (
                    key("omitted"),
                    services.len().saturating_sub(UP_ITEMS_MAX).to_string(),
                ),
            ],
        }
    }

    fn items(&self) -> Vec<FailureItem> {
        match self {
            Self::Environment(error) => error.items(),
            Self::PlanBlocked { blockers } => blockers
                .iter()
                .take(UP_ITEMS_MAX)
                .filter_map(|blocker| {
                    let mut fields = Vec::new();
                    if let Some(service) = &blocker.service {
                        fields.push((key("service"), service.to_string()));
                    }
                    fields.push((key("code"), blocker.code.clone()));
                    fields.push((key("message"), blocker.message.clone()));
                    FailureItem::new(ReasonCode::from_static("blocker"), fields).ok()
                })
                .collect(),
            Self::PlanChanged => Vec::new(),
            Self::NeedsSecret { references } => Self::needs(references).items(),
            Self::Incomplete { services, .. } => services
                .iter()
                .take(UP_ITEMS_MAX)
                .filter_map(service_item)
                .collect(),
        }
    }
}

fn service_item(report: &ServiceReport) -> Option<FailureItem> {
    let mut fields = vec![
        (key("service"), report.alias().to_string()),
        (key("phase"), report.phase().as_code().to_owned()),
    ];
    if let Some(stage) = report.stage() {
        fields.push((key("stage"), stage.as_code().to_owned()));
    }
    if let Some(code) = report.code() {
        fields.push((key("code"), code.to_string()));
    }
    if let Some(dependency) = report.dependency() {
        fields.push((key("dependency"), dependency.to_string()));
    }
    if let Some(operation) = report.operation() {
        fields.push((key("operationId"), operation.to_string()));
    }
    FailureItem::new(ReasonCode::from_static("service"), fields).ok()
}

/// The error an Up ends with when not every service is ready: `None` when
/// they all are.
#[must_use]
pub fn incomplete(services: &[ServiceReport]) -> Option<UpError> {
    let not_ready: Vec<ServiceReport> = services
        .iter()
        .filter(|service| service.phase() != UpPhase::Ready)
        .cloned()
        .collect();
    if not_ready.is_empty() {
        return None;
    }
    Some(UpError::Incomplete {
        ready: services.len() - not_ready.len(),
        total: services.len(),
        services: not_ready,
    })
}

#[cfg(test)]
mod tests {
    use stackly_domain::UpStage;

    use super::*;

    #[test]
    fn an_incomplete_up_lists_only_the_services_that_are_not_ready() {
        let services = vec![
            ServiceReport::new("db".parse().unwrap(), UpPhase::Ready),
            ServiceReport::new("api".parse().unwrap(), UpPhase::Failed)
                .with_stage(UpStage::Start)
                .with_code(ErrorCodeName::from_static("supervisor.readiness_timeout")),
        ];

        let error = incomplete(&services).unwrap();

        assert_eq!(error.code().as_str(), "env.up_incomplete");
        let details: Vec<(String, String)> = error
            .details()
            .into_iter()
            .map(|(key, value)| (key.as_str().to_owned(), value))
            .collect();
        assert!(details.contains(&("ready".to_owned(), "1".to_owned())));
        assert!(details.contains(&("total".to_owned(), "2".to_owned())));
        let items = error.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind().as_str(), "service");
        assert_eq!(items[0].fields()[0].1, "api");
        assert_eq!(items[0].fields()[2].1, "start");
    }

    #[test]
    fn an_up_whose_services_are_all_ready_is_complete() {
        let services = vec![ServiceReport::new("db".parse().unwrap(), UpPhase::Ready)];

        assert!(incomplete(&services).is_none());
    }
}
