//! Trial binds that tell whether an endpoint's port is free (spec 0010,
//! *Startup transaction*, `endpoints` stage, and K2).
//!
//! A trial bind opens a socket with the options the caller picks, binds it
//! without `listen`, and closes it at once. Checking a `Fixed` port, a
//! stored `Auto` port, and every candidate uses the options the server
//! uses ([`BindOptions::like_server`]): `SO_REUSEADDR`, so a leftover
//! `TIME_WAIT` and another process's wildcard listener on the same port
//! count as free, and `IPV6_V6ONLY` for an IPv6 address. Only a listener on
//! the exact address counts as in use. Picking a candidate binds `:0`
//! without `SO_REUSEADDR` ([`PortProbe::ephemeral_port`]).

use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};
use stackly_domain::{Port, Transport};

/// How a trial bind went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindOutcome {
    /// The bind worked: the port is free for a server with the same
    /// options.
    Free,
    /// Another socket is bound to the exact address and port.
    InUse,
    /// The OS refused the bind (`EACCES` or `EPERM`).
    Denied,
    /// The address is not available on this Mac (`EADDRNOTAVAIL`), or the
    /// socket could not be opened at all.
    Unavailable,
}

impl BindOutcome {
    /// Returns the stable snake_case code, for logs and the `reason` detail
    /// of `supervisor.endpoint_unavailable`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::InUse => "in_use",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }

    /// Maps the error of a failed socket, option, or bind call.
    #[must_use]
    pub fn from_error(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::AddrInUse => Self::InUse,
            io::ErrorKind::PermissionDenied => Self::Denied,
            _ => Self::Unavailable,
        }
    }
}

impl fmt::Display for BindOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// The socket options of a trial bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindOptions {
    /// Sets `SO_REUSEADDR` before the bind.
    pub reuse_addr: bool,
    /// Sets `IPV6_V6ONLY` on an IPv6 socket; ignored for IPv4.
    pub v6only: bool,
}

impl BindOptions {
    /// The options a server binds with: `SO_REUSEADDR`, and `IPV6_V6ONLY`
    /// when `addr` is IPv6. Every check of a `Fixed` port, a stored `Auto`
    /// port, a candidate, and the early exit check uses them.
    #[must_use]
    pub const fn like_server(addr: SocketAddr) -> Self {
        Self {
            reuse_addr: true,
            v6only: addr.is_ipv6(),
        }
    }

    /// The options of the ephemeral `:0` bind that picks a candidate:
    /// no `SO_REUSEADDR`.
    #[must_use]
    pub const fn ephemeral(host: IpAddr) -> Self {
        Self {
            reuse_addr: false,
            v6only: host.is_ipv6(),
        }
    }
}

/// Trial binds on the OS's sockets, injectable so tests can answer any
/// [`BindOutcome`] (spec 0010 AC-17).
pub trait PortProbe: Send + Sync {
    /// Binds `addr` without `listen` with `options`, closes the socket, and
    /// tells how it went.
    fn try_bind(&self, transport: Transport, addr: SocketAddr, options: BindOptions)
    -> BindOutcome;

    /// Binds `host:0` with [`BindOptions::ephemeral`], reads the port the
    /// OS picked, and closes the socket. Returns `None` when the bind
    /// fails. The caller still checks the candidate with
    /// [`try_bind`](Self::try_bind) and `resolve_port`.
    fn ephemeral_port(&self, transport: Transport, host: IpAddr) -> Option<Port>;
}

/// The [`PortProbe`] over real sockets, with `socket2`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SocketPortProbe;

impl SocketPortProbe {
    /// Creates the probe.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PortProbe for SocketPortProbe {
    fn try_bind(
        &self,
        transport: Transport,
        addr: SocketAddr,
        options: BindOptions,
    ) -> BindOutcome {
        match bind(transport, addr, options) {
            Ok(_socket) => BindOutcome::Free,
            Err(error) => {
                let outcome = BindOutcome::from_error(&error);
                tracing::debug!(%addr, %transport, %outcome, %error, "trial bind failed");
                outcome
            }
        }
    }

    fn ephemeral_port(&self, transport: Transport, host: IpAddr) -> Option<Port> {
        let addr = SocketAddr::new(host, 0);
        let picked = bind(transport, addr, BindOptions::ephemeral(host))
            .and_then(|socket| socket.local_addr())
            .and_then(|local| {
                local
                    .as_socket()
                    .ok_or_else(|| io::Error::other("the socket has no IP address"))
            });
        match picked.map(|local| Port::new(local.port())) {
            Ok(Ok(port)) => Some(port),
            Ok(Err(_)) => {
                tracing::debug!(%addr, %transport, "the OS picked port 0");
                None
            }
            Err(error) => {
                tracing::debug!(%addr, %transport, %error, "ephemeral bind failed");
                None
            }
        }
    }
}

/// Opens a socket for `transport`, sets `options`, and binds `addr`. The
/// socket closes when the caller drops it; `socket2` opens it with
/// close on exec.
fn bind(transport: Transport, addr: SocketAddr, options: BindOptions) -> io::Result<Socket> {
    let (kind, protocol) = match transport {
        Transport::Tcp => (Type::STREAM, Protocol::TCP),
        Transport::Udp => (Type::DGRAM, Protocol::UDP),
    };
    let socket = Socket::new(Domain::for_address(addr), kind, Some(protocol))?;
    if options.reuse_addr {
        socket.set_reuse_address(true)?;
    }
    if addr.is_ipv6() {
        socket.set_only_v6(options.v6only)?;
    }
    socket.bind(&addr.into())?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream, UdpSocket};

    use stackly_testkit::allocate_port;

    use super::*;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    fn probe(transport: Transport, addr: SocketAddr) -> BindOutcome {
        SocketPortProbe::new().try_bind(transport, addr, BindOptions::like_server(addr))
    }

    #[test]
    fn a_free_port_on_the_endpoint_address_is_free() {
        let addr = SocketAddr::new(LOOPBACK, allocate_port().unwrap());

        assert_eq!(probe(Transport::Tcp, addr), BindOutcome::Free);
        assert_eq!(probe(Transport::Udp, addr), BindOutcome::Free);
    }

    #[test]
    fn a_listener_on_the_exact_address_is_in_use_and_stays_alive() {
        let listener = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        assert_eq!(probe(Transport::Tcp, addr), BindOutcome::InUse);
        TcpStream::connect(addr).unwrap();
        listener.accept().unwrap();
    }

    #[test]
    fn a_udp_socket_on_the_exact_address_is_in_use() {
        let socket = UdpSocket::bind((LOOPBACK, 0)).unwrap();
        let addr = socket.local_addr().unwrap();

        assert_eq!(probe(Transport::Udp, addr), BindOutcome::InUse);
    }

    #[test]
    fn an_ipv6_loopback_listener_is_in_use_and_a_free_port_is_free() {
        let listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
        let taken = listener.local_addr().unwrap();
        let free = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), allocate_port().unwrap());

        assert_eq!(probe(Transport::Tcp, taken), BindOutcome::InUse);
        assert_eq!(probe(Transport::Tcp, free), BindOutcome::Free);
        assert!(BindOptions::like_server(taken).v6only);
    }

    // BSD socket rules: with `SO_REUSEADDR`, a bind to a specific address
    // passes next to another socket's wildcard bind on the same port.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_wildcard_listener_on_the_same_port_counts_as_free() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let addr = SocketAddr::new(LOOPBACK, port);

        assert_eq!(probe(Transport::Tcp, addr), BindOutcome::Free);
        let without_reuse = BindOptions {
            reuse_addr: false,
            v6only: false,
        };
        assert_eq!(
            SocketPortProbe::new().try_bind(Transport::Tcp, addr, without_reuse),
            BindOutcome::InUse
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_port_left_in_time_wait_counts_as_free() {
        let listener = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        // The side that closes first keeps the port in TIME_WAIT.
        drop(server_side);
        drop(client);
        drop(listener);

        assert_eq!(probe(Transport::Tcp, addr), BindOutcome::Free);
    }

    #[test]
    fn an_address_that_is_not_on_this_mac_is_unavailable() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 4222);

        assert_eq!(probe(Transport::Tcp, addr), BindOutcome::Unavailable);
        assert_eq!(probe(Transport::Udp, addr), BindOutcome::Unavailable);
        assert_eq!(
            SocketPortProbe::new().ephemeral_port(Transport::Tcp, addr.ip()),
            None
        );
    }

    #[test]
    fn os_errors_map_to_outcomes() {
        let outcomes = [
            io::ErrorKind::AddrInUse,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::AddrNotAvailable,
            io::ErrorKind::Unsupported,
        ]
        .map(|kind| BindOutcome::from_error(&io::Error::from(kind)));

        assert_eq!(
            outcomes,
            [
                BindOutcome::InUse,
                BindOutcome::Denied,
                BindOutcome::Unavailable,
                BindOutcome::Unavailable,
            ]
        );
    }

    #[test]
    fn the_ephemeral_bind_picks_a_port_the_server_options_find_free() {
        let probe = SocketPortProbe::new();

        let port = probe.ephemeral_port(Transport::Tcp, LOOPBACK).unwrap();
        let addr = SocketAddr::new(LOOPBACK, port.get());

        assert_eq!(
            probe.try_bind(Transport::Tcp, addr, BindOptions::like_server(addr)),
            BindOutcome::Free
        );
        assert!(!BindOptions::ephemeral(LOOPBACK).reuse_addr);
    }

    #[test]
    fn outcomes_have_stable_codes() {
        let codes = [
            BindOutcome::Free,
            BindOutcome::InUse,
            BindOutcome::Denied,
            BindOutcome::Unavailable,
        ]
        .map(BindOutcome::as_code);

        assert_eq!(codes, ["free", "in_use", "denied", "unavailable"]);
    }
}
