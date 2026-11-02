//! The guard of the commit that activates a config revision (spec 0029,
//! *Operasi*): it runs inside the registry transaction, so the rules that
//! come from the registry hold at the moment of the write.

use stackly_domain::{
    ConfigRevisionRecord, Endpoint, GuardView, InstanceFilter, InstanceId, PortRequest,
    RegistryError, WriteOp, apply_allowed,
};

use super::error::SupervisorError;
use crate::instance::config::{CONFIG_HISTORY_KEEP, ChangePlan, ConfigError, refusal_error};
use crate::instance::create::{CreateError, fixed_port_owner};

/// Why the guard writes nothing.
#[derive(Debug)]
pub(super) enum GuardFailure {
    /// A rule refused the change.
    Refused(ConfigError),
    /// The guard could not read the registry.
    Registry(RegistryError),
}

impl From<RegistryError> for GuardFailure {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

/// Reads the instance and the other instances through `view`, checks the
/// lifecycle and the fixed ports of `plan` against them, and returns the
/// ops of one activation: the instance with its new port requests and a
/// revision raised by 1, the new revision `record`, and the prune.
pub(super) fn activate_ops(
    view: &dyn GuardView,
    id: InstanceId,
    plan: &ChangePlan,
    record: ConfigRevisionRecord,
) -> Result<Vec<WriteOp>, GuardFailure> {
    let instances = view.instances(InstanceFilter {
        include_retained: false,
    })?;
    let Some(current) = instances.iter().find(|other| other.id() == id) else {
        return Err(GuardFailure::Refused(SupervisorError::NotFound(id).into()));
    };
    if let Err(refusal) = apply_allowed(current.lifecycle()) {
        return Err(GuardFailure::Refused(
            refusal_error(refusal, current.lifecycle()).into(),
        ));
    }
    let others: Vec<_> = instances
        .iter()
        .filter(|other| other.id() != id)
        .cloned()
        .collect();
    for (name, _, new) in &plan.endpoint_changes {
        let PortRequest::Fixed(port) = new else {
            continue;
        };
        let Some(Endpoint::Ip(ip)) = current
            .endpoints()
            .iter()
            .find(|endpoint| matches!(endpoint, Endpoint::Ip(ip) if ip.name() == name))
        else {
            continue;
        };
        if let Some(other) = fixed_port_owner(&others, ip, *port) {
            return Err(GuardFailure::Refused(
                CreateError::EndpointConflict {
                    endpoint: name.to_string(),
                    port: *port,
                    other,
                }
                .into(),
            ));
        }
    }
    let expected = current.revision();
    let mut next = current.clone();
    next.set_port_requests(&plan.endpoints).map_err(|_| {
        GuardFailure::Refused(
            CreateError::EndpointInvalid {
                endpoint: String::new(),
            }
            .into(),
        )
    })?;
    Ok(vec![
        WriteOp::ActivateConfigRevision {
            instance: next,
            expected,
            record: Box::new(record),
        },
        WriteOp::PruneConfigRevisions {
            instance: id,
            keep: CONFIG_HISTORY_KEEP,
        },
    ])
}
