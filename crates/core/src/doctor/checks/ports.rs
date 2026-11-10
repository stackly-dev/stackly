//! `endpoints`: every IP endpoint with a fixed or assigned port, of every
//! instance that is not `running`, `starting`, or `stopping` and that no
//! active operation holds, goes through the change checks of spec 0029
//! (`judge_change`, AC-14) with its own port and no port of its own bound.
//! A refused port names who holds it, as far as this user can see, and the
//! first free port after it. The doctor never changes a port.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};
use stackly_domain::{
    DesiredState, Endpoint, Instance, InstanceId, InstanceLifecycle, IpEndpoint, OperationResource,
    OperationState, Port, Registry,
};
use stackly_platform::ListenerOwner;

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{
    Action, Finding, FindingFacts, InstanceRef, PortOwnerFacts, PortProblemKind,
};
use crate::instance::create::{PortProblem, judge_change};

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let supervisor = context.needs_supervisor()?;
    let registry = context.registry()?;
    let probe = Arc::clone(&supervisor.deps().ports);
    let everyone = context.all_instances()?;
    let held = held_by_operations(registry.as_ref())?;
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for instance in context.instances()? {
        if matches!(
            instance.lifecycle(),
            InstanceLifecycle::Running | InstanceLifecycle::Starting | InstanceLifecycle::Stopping
        ) || held.contains(&instance.id())
        {
            continue;
        }
        let others: Vec<Instance> = everyone
            .iter()
            .filter(|other| other.id() != instance.id())
            .cloned()
            .collect();
        for endpoint in instance.endpoints() {
            if stopped(stop) {
                return Ok(CheckFindings::checked(findings, checked));
            }
            let Endpoint::Ip(ip) = endpoint else {
                continue;
            };
            let Some(port) = ip.effective_port() else {
                continue;
            };
            checked += 1;
            let siblings: Vec<Endpoint> = instance
                .endpoints()
                .iter()
                .filter(|sibling| sibling.name() != ip.name())
                .cloned()
                .collect();
            let Err((problem, suggestion)) =
                judge_change(probe.as_ref(), ip, port, &siblings, &others, None)
            else {
                continue;
            };
            findings.push(finding(
                context,
                registry.as_ref(),
                &instance,
                &everyone,
                ip,
                port,
                problem,
                suggestion,
            ));
        }
    }
    Ok(CheckFindings::checked(findings, checked))
}

/// The instances an active operation holds: queued, running, or asked to
/// cancel. One that needs recovery holds nothing that a probe races.
fn held_by_operations(registry: &dyn Registry) -> Result<BTreeSet<InstanceId>, CheckFailure> {
    Ok(registry
        .unfinished_operations()
        .map_err(|error| CheckFailure::registry(&error))?
        .value
        .iter()
        .filter(|(operation, _)| operation.state() != OperationState::NeedsRecovery)
        .filter_map(|(operation, _)| match operation.resource() {
            OperationResource::Instance(id) => Some(id),
            _ => None,
        })
        .collect())
}

#[expect(clippy::too_many_arguments, reason = "one finding needs every fact")]
fn finding(
    context: &CheckContext,
    registry: &dyn Registry,
    instance: &Instance,
    everyone: &[Instance],
    ip: &IpEndpoint,
    port: Port,
    problem: PortProblem,
    suggestion: Option<Port>,
) -> Finding {
    let severity = if instance.desired() == DesiredState::Running
        || instance.lifecycle() == InstanceLifecycle::Failed
    {
        DoctorSeverity::Error
    } else {
        DoctorSeverity::Warning
    };
    let problem = match problem {
        PortProblem::Sibling => PortProblemKind::Sibling,
        PortProblem::Instance(id) => everyone
            .iter()
            .find(|other| other.id() == id)
            .map_or(PortProblemKind::Sibling, |other| {
                PortProblemKind::Instance(InstanceRef::of(other))
            }),
        PortProblem::Bind(outcome) => PortProblemKind::Bind(outcome),
    };
    let owner = owner(context, registry, everyone, ip, port);
    let reference = InstanceRef::of(instance);
    let mut finding = Finding::new(
        DoctorFindingCode::PortInUse,
        severity,
        FindingFacts::Port {
            endpoint: ip.name().clone(),
            transport: ip.transport(),
            address: ip.address(),
            port,
            problem,
            suggestion,
            owner,
        },
    )
    .about(reference)
    .subject(ip.name().to_string());
    if let Some(suggested) = suggestion {
        finding = finding.action(Action::ChangePort {
            instance: instance.id(),
            endpoint: ip.name().clone(),
            port: suggested,
        });
    }
    finding
}

/// Who holds the port: the first listener of this user, named as another
/// instance when it is that instance's recorded process with the same
/// start time.
fn owner(
    context: &CheckContext,
    registry: &dyn Registry,
    everyone: &[Instance],
    ip: &IpEndpoint,
    port: Port,
) -> PortOwnerFacts {
    let owners = context
        .sources
        .platform
        .listeners()
        .owners(ip.transport(), ip.address(), port.get())
        .unwrap_or_default();
    let Some(owner) = owners.into_iter().next() else {
        return PortOwnerFacts::Unknown;
    };
    match recorded_instance(registry, everyone, &owner) {
        Some(instance) => PortOwnerFacts::StacklyInstance { owner, instance },
        None => PortOwnerFacts::Process(owner),
    }
}

fn recorded_instance(
    registry: &dyn Registry,
    everyone: &[Instance],
    owner: &ListenerOwner,
) -> Option<InstanceRef> {
    let start = owner.start?;
    everyone.iter().find_map(|instance| {
        let observation = registry
            .latest_process_observation(instance.id())
            .ok()?
            .value?;
        (observation.identity.pid == owner.pid && observation.identity.start == start)
            .then(|| InstanceRef::of(instance))
    })
}
