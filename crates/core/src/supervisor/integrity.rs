//! The two read only checks reconciliation adds after `classify` (spec
//! 0030, *Pemeriksaan rekonsiliasi tambahan*): whether the data of an
//! initialized instance is still there, and whether the runtime still has
//! the executable the instance runs.
//!
//! Neither check writes, creates a folder, or runs a process. A check that
//! cannot answer (the package cannot be read, the settings are invalid)
//! says nothing is missing: the start that follows reports that problem
//! with its own code.

use stackly_domain::{Endpoint, EndpointName, Instance, InstanceLifecycle, Port};

use super::Supervisor;
use super::error::SupervisorError;
use super::persist::blocking;
use super::prepare::{self, Checks};
use super::steps::verify_executable;

/// What a finding of reconciliation is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FindingKind {
    /// The data folder is gone, empty, or has no data marker.
    DataMissing,
    /// The runtime executable is gone from the runtime's install folder.
    RuntimeMissing,
}

/// A problem reconciliation found on one instance and left in place
/// (`daemon.reconciling` carries these as `findings`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReconcileFinding {
    /// The instance.
    pub instance_id: stackly_domain::InstanceId,
    /// The instance name.
    pub name: String,
    /// What is wrong.
    pub kind: FindingKind,
}

/// Whether the checks apply to `instance` at all: it is not retained, and
/// its data was initialized, so an absent folder means it was lost and not
/// that it was never made.
pub(crate) fn applies(instance: &Instance) -> bool {
    !matches!(
        instance.lifecycle(),
        InstanceLifecycle::Uninitialized
            | InstanceLifecycle::Initializing
            | InstanceLifecycle::Retained
    ) && instance.data_initialized_at().is_some()
}

/// Whether the data of `instance` is gone (see [`prepare::data_missing`]).
pub(crate) async fn data_is_missing(sup: &Supervisor, instance: &Instance) -> bool {
    let deps = sup.deps().clone();
    let instance = instance.clone();
    blocking(move || {
        let package = prepare::load_package(&deps.platform, &deps.layout, &instance).ok()?;
        let paths = deps.layout.instance_paths(instance.id());
        prepare::data_missing(deps.platform.fs(), &package, &paths)
    })
    .await
    .ok()
    .flatten()
    .is_some()
}

/// Whether the runtime of `instance` has lost the executable its run plan
/// names: the runtime record or folder is gone, or the program fails the
/// checks of a start (`verify_executable`) when looked at in the runtime's
/// install folder.
pub(crate) async fn runtime_is_missing(sup: &Supervisor, instance: &Instance) -> bool {
    let prepared = match prepare::prepare(sup, instance.id(), Checks::Plan).await {
        Ok(prepared) => prepared,
        Err(SupervisorError::RuntimeUnavailable { .. }) => return true,
        Err(error) => {
            tracing::debug!(instance = %instance.id(), %error, "the runtime check did not run");
            return false;
        }
    };
    let deps = sup.deps().clone();
    blocking(move || {
        // Only the program matters, so an endpoint that has no port yet
        // gets a stand in the plan can render.
        let ports: std::collections::BTreeMap<EndpointName, Port> = prepared
            .instance
            .endpoints()
            .iter()
            .filter_map(|endpoint| match endpoint {
                Endpoint::Ip(ip) if ip.effective_port().is_none() => {
                    Port::new(49152).ok().map(|port| (ip.name().clone(), port))
                }
                _ => None,
            })
            .collect();
        let plan = prepared.package.run_plan(&prepared.context(&ports)).ok()?;
        Some(matches!(
            verify_executable(&deps, &prepared, &plan.spawn.program),
            Err(SupervisorError::ExecutableInvalid { .. })
        ))
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}
