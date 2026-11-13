//! The environment a create made by Up binds its instance to (spec 0046
//! AC-7, *Lease*): the `register` batch writes the instance, its
//! `dedicated` member with its dependency conditions, and its environment
//! lease in one transaction, so no dedicated instance is ever left without
//! a member, or a member without a lease. A client's `instance.create`
//! never carries one.

use jiff::Timestamp;
use stackly_domain::{
    DependencyCondition, DetailKey, EnvironmentId, EnvironmentMember, ErrorCodeName, GuardView,
    InstanceId, Lease, LeaseHolder, LeaseId, MemberAlias, MemberDependency, Ownership,
    RegistryError, WriteOp,
};

use crate::operation::OperationError;

const ENVIRONMENT_NOT_FOUND: ErrorCodeName = ErrorCodeName::from_static("env.not_found");
const PLAN_CHANGED: ErrorCodeName = ErrorCodeName::from_static("env.plan_changed");

/// The environment, alias, and dependency conditions a create binds its
/// new instance to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentBinding {
    /// The environment the instance joins.
    pub environment: EnvironmentId,
    /// The alias of its member.
    pub alias: MemberAlias,
    /// The edges Up uses for it, with their effective timeouts.
    pub conditions: Vec<MemberDependency>,
}

/// Why the `register` guard refused a binding.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BindingFault {
    /// The environment is gone.
    #[error("environment {environment} does not exist")]
    EnvironmentGone {
        /// The environment.
        environment: EnvironmentId,
    },
    /// The alias or the instance is already a member: the environment
    /// changed since Up planned it.
    #[error("environment {environment} changed: {detail}")]
    MemberTaken {
        /// The environment.
        environment: EnvironmentId,
        /// What the environment refused.
        detail: String,
    },
}

impl OperationError for BindingFault {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::EnvironmentGone { .. } => ENVIRONMENT_NOT_FOUND,
            Self::MemberTaken { .. } => PLAN_CHANGED,
        }
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        let environment = match self {
            Self::EnvironmentGone { environment } | Self::MemberTaken { environment, .. } => {
                environment
            }
        };
        let key = "environmentId"
            .parse::<DetailKey>()
            .expect("environmentId is a camelCase detail key");
        vec![(key, environment.to_string())]
    }
}

impl EnvironmentBinding {
    /// The writes that bind `instance` inside the `register` guard, after
    /// its insert: the environment read in this transaction with the new
    /// `dedicated` member, written at the revision just read, and one
    /// `active` environment lease acquired at `now`.
    ///
    /// # Errors
    ///
    /// Returns `Ok(Err(..))` when the environment is gone or refuses the
    /// member, and `Err` when the registry cannot be read.
    pub(crate) fn guard_ops(
        &self,
        view: &dyn GuardView,
        instance: InstanceId,
        now: Timestamp,
    ) -> Result<Result<Vec<WriteOp>, BindingFault>, RegistryError> {
        let Some(mut environment) = view
            .environments()?
            .into_iter()
            .find(|environment| environment.id() == self.environment)
        else {
            return Ok(Err(BindingFault::EnvironmentGone {
                environment: self.environment,
            }));
        };
        let expected = environment.revision();
        let member = EnvironmentMember::new(self.alias.clone(), instance, Ownership::Dedicated)
            .with_conditions(self.conditions.clone());
        if let Err(error) = environment.bind_member(member) {
            return Ok(Err(BindingFault::MemberTaken {
                environment: self.environment,
                detail: error.to_string(),
            }));
        }
        let lease = Lease::new(
            LeaseId::generate(),
            instance,
            LeaseHolder::Environment(self.environment),
            None,
            now,
        )
        .map_err(|error| RegistryError::Io {
            detail: format!("an environment lease broke an invariant: {error}"),
        })?;
        Ok(Ok(vec![
            WriteOp::UpdateEnvironment {
                environment,
                expected,
            },
            WriteOp::InsertLease(lease),
        ]))
    }

    /// The JSON of the binding in a create journal's `input`.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let conditions: Vec<serde_json::Value> = self
            .conditions
            .iter()
            .map(|condition| {
                serde_json::json!({
                    "service": condition.service().as_str(),
                    "condition": condition.condition().as_code(),
                    "timeoutSeconds": condition.timeout_seconds(),
                })
            })
            .collect();
        serde_json::json!({
            "environmentId": self.environment.to_string(),
            "alias": self.alias.as_str(),
            "conditions": conditions,
        })
    }

    /// Reads the binding back from [`EnvironmentBinding::to_json`]; `None`
    /// when any part is missing or invalid.
    pub(crate) fn from_json(value: &serde_json::Value) -> Option<Self> {
        let text = |value: &serde_json::Value, key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let conditions = value
            .get("conditions")?
            .as_array()?
            .iter()
            .map(|row| {
                Some(MemberDependency::new(
                    text(row, "service")?.parse().ok()?,
                    text(row, "condition")?
                        .parse::<DependencyCondition>()
                        .ok()?,
                    u32::try_from(row.get("timeoutSeconds")?.as_u64()?).ok()?,
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            environment: text(value, "environmentId")?.parse().ok()?,
            alias: text(value, "alias")?.parse().ok()?,
            conditions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_reads_back_from_its_journal_json() {
        let binding = EnvironmentBinding {
            environment: EnvironmentId::generate(),
            alias: "db".parse().unwrap(),
            conditions: vec![MemberDependency::new(
                "cache".parse().unwrap(),
                DependencyCondition::Ready,
                600,
            )],
        };

        assert_eq!(
            EnvironmentBinding::from_json(&binding.to_json()),
            Some(binding)
        );
        assert_eq!(
            EnvironmentBinding::from_json(&serde_json::json!({ "alias": "db" })),
            None
        );
    }
}
