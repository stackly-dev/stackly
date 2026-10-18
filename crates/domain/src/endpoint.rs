//! Endpoints of an instance and the port rules between them (BR-004,
//! BR-005).

use std::net::IpAddr;

use crate::code::code_enum;
use crate::error::{InvariantDetail, InvariantError};
use crate::value::{EndpointName, Port, SocketName};

code_enum! {
    /// The transport of an IP endpoint.
    pub enum Transport: Transport {
        /// TCP.
        Tcp => "tcp",
        /// UDP.
        Udp => "udp",
    }
}

/// How an IP endpoint asks for its port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortRequest {
    /// Exactly this port, never a substitute (BR-005).
    Fixed(Port),
    /// Any free port, picked when the endpoint is bound.
    Auto,
}

/// An endpoint on an IP address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IpEndpoint {
    name: EndpointName,
    transport: Transport,
    address: IpAddr,
    request: PortRequest,
    bound: Option<Port>,
}

impl IpEndpoint {
    /// Creates an IP endpoint. `bound` is the port actually bound, set only
    /// after the bind and readiness succeeded (feature 10).
    #[must_use]
    pub const fn new(
        name: EndpointName,
        transport: Transport,
        address: IpAddr,
        request: PortRequest,
        bound: Option<Port>,
    ) -> Self {
        Self {
            name,
            transport,
            address,
            request,
            bound,
        }
    }

    /// Returns the endpoint's name.
    #[must_use]
    pub const fn name(&self) -> &EndpointName {
        &self.name
    }

    /// Returns the transport.
    #[must_use]
    pub const fn transport(&self) -> Transport {
        self.transport
    }

    /// Returns the address it listens on.
    #[must_use]
    pub const fn address(&self) -> IpAddr {
        self.address
    }

    /// Returns how it asks for its port.
    #[must_use]
    pub const fn request(&self) -> PortRequest {
        self.request
    }

    /// Returns the port actually bound, if any.
    #[must_use]
    pub const fn bound(&self) -> Option<Port> {
        self.bound
    }

    /// Returns the effective port: the bound port when there is one, else
    /// the fixed port. An `Auto` endpoint that is not bound has none.
    #[must_use]
    pub const fn effective_port(&self) -> Option<Port> {
        match (self.bound, self.request) {
            (Some(port), _) | (None, PortRequest::Fixed(port)) => Some(port),
            (None, PortRequest::Auto) => None,
        }
    }

    /// Returns this endpoint as if it were bound to `port`.
    fn with_port(&self, port: Port) -> Self {
        Self {
            bound: Some(port),
            ..self.clone()
        }
    }
}

/// An endpoint on a Unix socket in the instance's run folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnixEndpoint {
    name: EndpointName,
    socket: SocketName,
}

impl UnixEndpoint {
    /// Creates a Unix socket endpoint.
    #[must_use]
    pub const fn new(name: EndpointName, socket: SocketName) -> Self {
        Self { name, socket }
    }

    /// Returns the endpoint's name.
    #[must_use]
    pub const fn name(&self) -> &EndpointName {
        &self.name
    }

    /// Returns the socket's file name.
    #[must_use]
    pub const fn socket(&self) -> &SocketName {
        &self.socket
    }
}

/// One endpoint of an instance, keyed by its name within the instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// An endpoint on an IP address.
    Ip(IpEndpoint),
    /// An endpoint on a Unix socket.
    Unix(UnixEndpoint),
}

impl Endpoint {
    /// Returns the endpoint's name.
    #[must_use]
    pub const fn name(&self) -> &EndpointName {
        match self {
            Self::Ip(endpoint) => endpoint.name(),
            Self::Unix(endpoint) => endpoint.name(),
        }
    }

    /// Returns the effective port of an IP endpoint; a Unix endpoint has
    /// none.
    #[must_use]
    pub const fn effective_port(&self) -> Option<Port> {
        match self {
            Self::Ip(endpoint) => endpoint.effective_port(),
            Self::Unix(_) => None,
        }
    }
}

/// Returns whether two endpoints conflict (BR-004).
///
/// Two IP endpoints conflict when they use the same transport, the same
/// effective port, and overlapping addresses: the same address, `0.0.0.0`
/// with any IPv4 address, `::` with any address, and `::ffff:a.b.c.d` the
/// same as `a.b.c.d`. TCP and UDP never conflict with each other, and an
/// endpoint without an effective port conflicts with nothing. A Unix
/// endpoint never conflicts with an IP endpoint. Two Unix endpoints
/// conflict when they name the same socket; sockets live in the run folder
/// of their own instance, so that only matters within one instance.
///
/// ```
/// use std::net::{IpAddr, Ipv4Addr};
///
/// use stackly_domain::{Endpoint, IpEndpoint, Port, PortRequest, Transport, endpoints_conflict};
///
/// let tcp = |address: IpAddr| {
///     Endpoint::Ip(IpEndpoint::new(
///         "main".parse().expect("valid name"),
///         Transport::Tcp,
///         address,
///         PortRequest::Fixed(Port::new(5432).expect("valid port")),
///         None,
///     ))
/// };
/// let any = tcp(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
/// let local = tcp(IpAddr::V4(Ipv4Addr::LOCALHOST));
/// assert!(endpoints_conflict(&any, &local));
/// ```
#[must_use]
pub fn endpoints_conflict(a: &Endpoint, b: &Endpoint) -> bool {
    match (a, b) {
        (Endpoint::Ip(a), Endpoint::Ip(b)) => ip_endpoints_conflict(a, b),
        (Endpoint::Unix(a), Endpoint::Unix(b)) => a.socket == b.socket,
        (Endpoint::Ip(_), Endpoint::Unix(_)) | (Endpoint::Unix(_), Endpoint::Ip(_)) => false,
    }
}

fn ip_endpoints_conflict(a: &IpEndpoint, b: &IpEndpoint) -> bool {
    let same_port = match (a.effective_port(), b.effective_port()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    a.transport == b.transport && same_port && addresses_overlap(a.address, b.address)
}

/// Whether a listener on `a` and one on `b` can claim the same traffic.
fn addresses_overlap(a: IpAddr, b: IpAddr) -> bool {
    let (a, b) = (a.to_canonical(), b.to_canonical());
    let v6_any = |address: IpAddr| matches!(address, IpAddr::V6(v6) if v6.is_unspecified());
    let v4_any = |address: IpAddr| matches!(address, IpAddr::V4(v4) if v4.is_unspecified());
    a == b || v6_any(a) || v6_any(b) || (a.is_ipv4() && b.is_ipv4() && (v4_any(a) || v4_any(b)))
}

/// Picks the port for an IP endpoint (BR-005).
///
/// A `Fixed` port that conflicts with the effective port of one of
/// `others` is an error, and no other port is ever substituted. An `Auto`
/// endpoint takes `candidate`, the free port the caller found (features 10
/// and 20). `others` are the other endpoints to check against, not this
/// one.
///
/// # Errors
///
/// Returns an [`InvariantError`] for BR-005:
/// [`InvariantDetail::FixedPortConflict`] for a conflicting fixed port, and
/// [`InvariantDetail::AutoPortUnavailable`] when an `Auto` endpoint has no
/// candidate or the candidate conflicts.
pub fn resolve_port(
    endpoint: &IpEndpoint,
    candidate: Option<Port>,
    others: &[Endpoint],
) -> Result<Port, InvariantError> {
    let conflicts = |port: Port| {
        let probe = Endpoint::Ip(endpoint.with_port(port));
        others.iter().any(|other| endpoints_conflict(&probe, other))
    };
    match endpoint.request {
        PortRequest::Fixed(port) if conflicts(port) => {
            Err(InvariantDetail::FixedPortConflict { port }.into())
        }
        PortRequest::Fixed(port) => Ok(port),
        PortRequest::Auto => match candidate {
            Some(port) if !conflicts(port) => Ok(port),
            _ => Err(InvariantDetail::AutoPortUnavailable { candidate }.into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;
    use crate::rule::Rule;

    fn port(number: u16) -> Port {
        Port::new(number).unwrap()
    }

    fn ip(transport: Transport, address: &str, request: PortRequest) -> IpEndpoint {
        IpEndpoint::new(
            "main".parse().unwrap(),
            transport,
            address.parse().unwrap(),
            request,
            None,
        )
    }

    fn tcp(address: &str, number: u16) -> Endpoint {
        Endpoint::Ip(ip(
            Transport::Tcp,
            address,
            PortRequest::Fixed(port(number)),
        ))
    }

    fn unix(socket: &str) -> Endpoint {
        Endpoint::Unix(UnixEndpoint::new(
            "socket".parse().unwrap(),
            socket.parse().unwrap(),
        ))
    }

    #[test]
    fn effective_port_prefers_the_bound_port_and_auto_without_bind_has_none() {
        let auto = ip(Transport::Tcp, "127.0.0.1", PortRequest::Auto);
        let fixed = ip(Transport::Tcp, "127.0.0.1", PortRequest::Fixed(port(5432)));

        assert_eq!(auto.effective_port(), None);
        assert_eq!(
            auto.with_port(port(6000)).effective_port(),
            Some(port(6000))
        );
        assert_eq!(fixed.effective_port(), Some(port(5432)));
        assert_eq!(unix(".s.PGSQL.5432").effective_port(), None);
    }

    #[test]
    fn overlapping_addresses_cover_wildcards_and_mapped_ipv4() {
        let overlap = |a: &str, b: &str| {
            addresses_overlap(a.parse().unwrap(), b.parse().unwrap())
                && addresses_overlap(b.parse().unwrap(), a.parse().unwrap())
        };

        assert!(overlap("127.0.0.1", "127.0.0.1"));
        assert!(overlap("0.0.0.0", "192.168.1.5"));
        assert!(overlap("::", "127.0.0.1"));
        assert!(overlap("::", "::1"));
        assert!(overlap("::ffff:127.0.0.1", "127.0.0.1"));
        assert!(overlap("::ffff:0.0.0.0", "10.0.0.1"));
        assert!(!overlap("127.0.0.1", "127.0.0.2"));
        assert!(!overlap("0.0.0.0", "::1"));
        assert!(!overlap("::1", "fe80::1"));
        assert_eq!(
            IpAddr::V6(Ipv6Addr::UNSPECIFIED).to_canonical(),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        );
        assert!(Ipv4Addr::UNSPECIFIED.is_unspecified());
    }

    #[test]
    fn unix_endpoints_conflict_only_with_the_same_socket_name() {
        assert!(endpoints_conflict(&unix("a.sock"), &unix("a.sock")));
        assert!(!endpoints_conflict(&unix("a.sock"), &unix("b.sock")));
        assert!(!endpoints_conflict(&unix("a.sock"), &tcp("0.0.0.0", 5432)));
    }

    #[test]
    fn auto_port_takes_a_free_candidate_and_reports_a_missing_or_taken_one() {
        let auto = ip(Transport::Tcp, "127.0.0.1", PortRequest::Auto);
        let others = [tcp("0.0.0.0", 6000)];

        assert_eq!(
            resolve_port(&auto, Some(port(6001)), &others),
            Ok(port(6001))
        );
        for candidate in [None, Some(port(6000))] {
            let err = resolve_port(&auto, candidate, &others).unwrap_err();
            assert_eq!(
                err.detail(),
                &InvariantDetail::AutoPortUnavailable { candidate }
            );
            assert_eq!(err.rule(), Some(Rule::Br005));
        }
    }

    #[test]
    fn a_free_fixed_port_ignores_the_candidate() {
        let fixed = ip(Transport::Tcp, "127.0.0.1", PortRequest::Fixed(port(5432)));
        let others = [tcp("127.0.0.1", 5433), unix(".s.PGSQL.5432")];

        assert_eq!(
            resolve_port(&fixed, Some(port(7000)), &others),
            Ok(port(5432))
        );
    }
}
