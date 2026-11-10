//! The port checks of create (spec 0020, *Pemeriksaan port*): steps 1 to 3
//! are pure and run in `evaluate`; steps 4 to 6 open trial sockets and run in
//! `gather`, which also searches the suggestion.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use stackly_domain::{
    Endpoint, EndpointName, Instance, InstanceId, IpEndpoint, Port, PortRequest, Transport,
    endpoints_conflict,
};

use super::draft::{CreateDraft, PORT_SUGGESTION_ATTEMPTS, PORT_SUGGESTION_MIN, PortRequestDraft};
use crate::provider::{EndpointPurpose, EndpointTransport, ValidatedPackage};
use crate::supervisor::{BindOptions, BindOutcome, PortProbe};

/// One TCP or UDP endpoint of the manifest, where create can bind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Slot {
    pub(super) name: EndpointName,
    pub(super) transport: Transport,
    pub(super) host: IpAddr,
    pub(super) purpose: EndpointPurpose,
    pub(super) default_port: Option<Port>,
}

impl Slot {
    /// The slot an IP endpoint of an instance occupies.
    pub(super) fn of(endpoint: &IpEndpoint) -> Self {
        Self {
            name: endpoint.name().clone(),
            transport: endpoint.transport(),
            host: endpoint.address(),
            purpose: EndpointPurpose::Client,
            default_port: None,
        }
    }

    /// The endpoint as a new instance would hold it with `request`.
    pub(super) fn endpoint(&self, request: PortRequest) -> Endpoint {
        Endpoint::Ip(IpEndpoint::new(
            self.name.clone(),
            self.transport,
            self.host,
            request,
            None,
        ))
    }
}

const fn purpose_rank(purpose: EndpointPurpose) -> u8 {
    match purpose {
        EndpointPurpose::Client => 0,
        EndpointPurpose::Admin => 1,
        _ => 2,
    }
}

/// The IP endpoints of the package, in form order: purpose `client`,
/// `admin`, `monitoring`, then name.
pub(super) fn slots(package: &ValidatedPackage) -> Vec<Slot> {
    let mut slots: Vec<Slot> = package
        .endpoints()
        .filter_map(|spec| {
            let transport = match spec.transport() {
                EndpointTransport::Tcp => Transport::Tcp,
                EndpointTransport::Udp => Transport::Udp,
                EndpointTransport::Unix => return None,
            };
            Some(Slot {
                name: spec.name().clone(),
                transport,
                host: spec
                    .default_host()
                    .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                purpose: spec.purpose(),
                default_port: spec.default_port(),
            })
        })
        .collect();
    slots.sort_by(|a, b| {
        purpose_rank(a.purpose)
            .cmp(&purpose_rank(b.purpose))
            .then_with(|| a.name.cmp(&b.name))
    });
    slots
}

/// The request the draft makes for the first mention of `name`.
pub(super) fn request_for(draft: &CreateDraft, name: &EndpointName) -> Option<PortRequestDraft> {
    draft
        .endpoints
        .iter()
        .find(|(requested, _)| requested == name.as_str())
        .map(|(_, request)| *request)
}

/// The fixed port the draft asks for `slot`, if it is a valid one.
fn fixed_port(draft: &CreateDraft, slot: &Slot) -> Option<Port> {
    match request_for(draft, &slot.name)? {
        PortRequestDraft::Fixed(port) => Port::new(port).ok(),
        PortRequestDraft::Auto => None,
    }
}

/// Step 2: whether the endpoint at `port` conflicts with another endpoint of
/// the draft that asks for a valid fixed port.
pub(super) fn conflicts_in_draft(
    draft: &CreateDraft,
    slots: &[Slot],
    slot: &Slot,
    port: Port,
) -> bool {
    let candidate = slot.endpoint(PortRequest::Fixed(port));
    slots
        .iter()
        .filter(|other| other.name != slot.name)
        .filter_map(|other| {
            fixed_port(draft, other).map(|port| other.endpoint(PortRequest::Fixed(port)))
        })
        .any(|other| endpoints_conflict(&candidate, &other))
}

/// Step 3: the first instance that is not retained whose effective or fixed
/// port conflicts with the endpoint at `port`.
pub(super) fn conflicts_with_instances(
    instances: &[Instance],
    slot: &Slot,
    port: Port,
) -> Option<InstanceId> {
    let candidate = slot.endpoint(PortRequest::Fixed(port));
    instances
        .iter()
        .find(|instance| {
            instance
                .endpoints()
                .iter()
                .any(|theirs| endpoints_conflict(&candidate, theirs))
        })
        .map(Instance::id)
}

/// What the trial binds of steps 4 to 6 said about one fixed port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortFacts {
    /// Step 4: the exact address, with `SO_REUSEADDR`.
    pub exact: BindOutcome,
    /// Step 5: the wildcard of the same family, with `SO_REUSEADDR` like
    /// the server; run only when step 4 was free.
    pub wildcard: Option<BindOutcome>,
    /// Step 6: the first result that is not free of the loopback and then
    /// the wildcard of the other family (the wildcard is bound only when the
    /// loopback was free), or free; run only when step 4 was free and the
    /// endpoint is on loopback.
    pub other_loopback: Option<BindOutcome>,
    /// The first free port after the requested one that passes steps 2 to
    /// 6 without error or caution, only searched when step 3 or 4 failed.
    pub suggestion: Option<Port>,
}

impl PortFacts {
    /// Whether steps 4 to 6 found nothing to report.
    pub(crate) const fn clean(&self) -> bool {
        matches!(self.exact, BindOutcome::Free)
            && matches!(self.wildcard, None | Some(BindOutcome::Free))
            && matches!(self.other_loopback, None | Some(BindOutcome::Free))
    }
}

/// Runs steps 4 to 6 for `slot` at `port`.
fn probe(probe: &dyn PortProbe, slot: &Slot, port: Port) -> PortFacts {
    let exact_addr = SocketAddr::new(slot.host, port.get());
    let exact = probe.try_bind(
        slot.transport,
        exact_addr,
        BindOptions::like_server(exact_addr),
    );
    if exact != BindOutcome::Free {
        return PortFacts {
            exact,
            wildcard: None,
            other_loopback: None,
            suggestion: None,
        };
    }
    let wildcard_host = match slot.host {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    let wildcard_addr = SocketAddr::new(wildcard_host, port.get());
    let wildcard = probe.try_bind(
        slot.transport,
        wildcard_addr,
        BindOptions::like_server(wildcard_addr),
    );
    let other_loopback = slot.host.is_loopback().then(|| {
        let (other_host, other_wildcard) = match slot.host {
            IpAddr::V4(_) => (
                IpAddr::V6(Ipv6Addr::LOCALHOST),
                IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            ),
            IpAddr::V6(_) => (
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            ),
        };
        let other_addr = SocketAddr::new(other_host, port.get());
        let loopback = probe.try_bind(
            slot.transport,
            other_addr,
            BindOptions::like_server(other_addr),
        );
        if loopback != BindOutcome::Free {
            return loopback;
        }
        let other_wildcard_addr = SocketAddr::new(other_wildcard, port.get());
        probe.try_bind(
            slot.transport,
            other_wildcard_addr,
            BindOptions::like_server(other_wildcard_addr),
        )
    });
    PortFacts {
        exact,
        wildcard: Some(wildcard),
        other_loopback,
        suggestion: None,
    }
}

/// Searches the first port from `max(port + 1, 1024)` up to 65535, at most
/// [`PORT_SUGGESTION_ATTEMPTS`] candidates, that passes steps 2 to 6 without
/// error or caution (AC-5).
fn suggest(
    port_probe: &dyn PortProbe,
    draft: &CreateDraft,
    slots: &[Slot],
    slot: &Slot,
    instances: &[Instance],
    port: u16,
) -> Option<Port> {
    let start = (u32::from(port) + 1).max(PORT_SUGGESTION_MIN);
    (start..=u32::from(u16::MAX))
        .take(PORT_SUGGESTION_ATTEMPTS)
        .filter_map(|candidate| {
            u16::try_from(candidate)
                .ok()
                .and_then(|n| Port::new(n).ok())
        })
        .find(|&candidate| {
            !conflicts_in_draft(draft, slots, slot, candidate)
                && conflicts_with_instances(instances, slot, candidate).is_none()
                && probe(port_probe, slot, candidate).clean()
        })
}

/// Runs steps 4 to 6 for every endpoint whose request is a valid fixed port
/// that steps 1 to 3 let through, and searches the suggestion for every
/// fixed port that fails step 3 or step 4.
pub(super) fn gather_ports(
    port_probe: &dyn PortProbe,
    draft: &CreateDraft,
    slots: &[Slot],
    instances: &[Instance],
) -> BTreeMap<EndpointName, PortFacts> {
    let mut facts = BTreeMap::new();
    for slot in slots {
        let Some(port) = fixed_port(draft, slot) else {
            continue;
        };
        if conflicts_in_draft(draft, slots, slot, port) {
            continue;
        }
        let blocked_by_instance = conflicts_with_instances(instances, slot, port).is_some();
        let mut found = if blocked_by_instance {
            PortFacts {
                exact: BindOutcome::Free,
                wildcard: None,
                other_loopback: None,
                suggestion: None,
            }
        } else {
            probe(port_probe, slot, port)
        };
        let failed = blocked_by_instance || found.exact != BindOutcome::Free;
        if failed {
            found.suggestion = suggest(port_probe, draft, slots, slot, instances, port.get());
        }
        facts.insert(slot.name.clone(), found);
    }
    facts
}

/// Why a changed fixed port is refused (spec 0029, AC-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortProblem {
    /// Another endpoint of the same instance has the port (step 2).
    Sibling,
    /// Another instance has the port (step 3).
    Instance(InstanceId),
    /// The trial bind of the exact address was not free (step 4).
    Bind(BindOutcome),
}

/// Runs the create checks of spec 0020 for one endpoint of an existing
/// instance whose request changes to `Fixed(port)` (spec 0029, AC-14):
/// `siblings` are the other endpoints of the instance as they will be, and
/// `others` the other instances that are not retained. `own_bound` is the
/// port the endpoint is bound to now: the same port skips the trial bind,
/// because the instance's own process holds it, and is still checked
/// against every other endpoint and instance. A failure carries the first
/// free port after `port` that passes the same checks.
pub(crate) fn judge_change(
    port_probe: &dyn PortProbe,
    endpoint: &IpEndpoint,
    port: Port,
    siblings: &[Endpoint],
    others: &[Instance],
    own_bound: Option<Port>,
) -> Result<(), (PortProblem, Option<Port>)> {
    let slot = Slot::of(endpoint);
    let problem =
        |candidate: Port| problem_at(port_probe, &slot, candidate, siblings, others, own_bound);
    let Some(failure) = problem(port) else {
        return Ok(());
    };
    Err((failure, first_free_after(port_probe, &slot, port, &problem)))
}

/// The port a start that found `port` unusable for `endpoint` suggests
/// (spec 0033, AC-4): the search of [`judge_change`] with nothing bound,
/// the first port after `port` that no endpoint of `siblings` (the other
/// endpoints of the instance) and no instance of `others` (the other
/// instances that are not retained) has, and whose trial binds of steps 4
/// to 6 are clean. It only opens trial sockets, which close at once, and
/// writes nothing. `None` when no candidate passes.
pub(crate) fn suggest_for_start(
    port_probe: &dyn PortProbe,
    endpoint: &IpEndpoint,
    port: Port,
    siblings: &[Endpoint],
    others: &[Instance],
) -> Option<Port> {
    let slot = Slot::of(endpoint);
    let problem =
        |candidate: Port| problem_at(port_probe, &slot, candidate, siblings, others, None);
    first_free_after(port_probe, &slot, port, &problem)
}

/// What steps 2 to 4 of spec 0020 find for `slot` at `candidate`, for an
/// endpoint of an existing instance; `own_bound` skips the trial bind of
/// the port the instance's own process holds.
fn problem_at(
    port_probe: &dyn PortProbe,
    slot: &Slot,
    candidate: Port,
    siblings: &[Endpoint],
    others: &[Instance],
    own_bound: Option<Port>,
) -> Option<PortProblem> {
    let moved = slot.endpoint(PortRequest::Fixed(candidate));
    if siblings
        .iter()
        .any(|sibling| endpoints_conflict(&moved, sibling))
    {
        return Some(PortProblem::Sibling);
    }
    if let Some(other) = conflicts_with_instances(others, slot, candidate) {
        return Some(PortProblem::Instance(other));
    }
    if own_bound == Some(candidate) {
        return None;
    }
    let found = probe(port_probe, slot, candidate);
    (found.exact != BindOutcome::Free).then_some(PortProblem::Bind(found.exact))
}

/// The first port from `max(port + 1, 1024)` up to 65535, at most
/// [`PORT_SUGGESTION_ATTEMPTS`] candidates, that `problem` lets through
/// and whose trial binds of steps 4 to 6 are clean.
fn first_free_after(
    port_probe: &dyn PortProbe,
    slot: &Slot,
    port: Port,
    problem: &dyn Fn(Port) -> Option<PortProblem>,
) -> Option<Port> {
    let start = (u32::from(port.get()) + 1).max(PORT_SUGGESTION_MIN);
    (start..=u32::from(u16::MAX))
        .take(PORT_SUGGESTION_ATTEMPTS)
        .filter_map(|candidate| {
            u16::try_from(candidate)
                .ok()
                .and_then(|n| Port::new(n).ok())
        })
        .find(|&candidate| {
            problem(candidate).is_none() && probe(port_probe, slot, candidate).clean()
        })
}

/// The first of `others` whose effective or fixed port conflicts with
/// `endpoint` asking for `Fixed(port)` (spec 0029, AC-14: the check the
/// guarded commit runs again).
pub(crate) fn fixed_port_owner(
    others: &[Instance],
    endpoint: &IpEndpoint,
    port: Port,
) -> Option<InstanceId> {
    conflicts_with_instances(others, &Slot::of(endpoint), port)
}
