//! Which processes of this user hold a port (spec 0034, *Pemilik port*).
//!
//! `proc_listpids(PROC_UID_ONLY)` lists the processes of the effective
//! user, `PROC_PIDLISTFDS` their descriptors, and
//! `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` each socket: a TCP socket in
//! `LISTEN` or a bound UDP socket on the port, with an address that serves
//! the one asked about, makes its process an owner. Nothing here reads the
//! memory or the arguments of a process, spawns, or signals; the kernel
//! refuses the descriptors of another user's process, so only this user's
//! processes are ever seen.

use std::io;
use std::mem::{self, size_of};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr;
use std::time::Instant;

use libc::{c_int, c_void};
use stackly_domain::{Pid, Transport};
use stackly_platform::{
    LISTENER_SCAN_DEADLINE, LISTENER_SCAN_MAX_PIDS, ListenerOwner, ProcessError, listener_serves,
};

use super::inspect::{self, BsdInfo, Listing};
use super::sys;
use crate::identity::current_uid;

/// The processes of this user that hold a TCP listener or a bound UDP
/// socket on `port` with an address that serves `address`, in PID order and
/// each once. It reads at most [`LISTENER_SCAN_MAX_PIDS`] processes for at
/// most [`LISTENER_SCAN_DEADLINE`], and skips a process whose sockets
/// cannot be read or that exits during the scan.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when the processes of the user cannot be
/// listed.
pub(super) fn owners(
    transport: Transport,
    address: IpAddr,
    port: u16,
) -> Result<Vec<ListenerOwner>, ProcessError> {
    if port == 0 {
        return Ok(Vec::new());
    }
    let deadline = Instant::now() + LISTENER_SCAN_DEADLINE;
    let mut owners = Vec::new();
    for pid in user_pids()? {
        if Instant::now() >= deadline {
            break;
        }
        if holds_port(pid, transport, address, port)
            && let Some(owner) = owner(pid)
        {
            owners.push(owner);
        }
    }
    owners.sort_by_key(|owner| owner.pid);
    owners.dedup_by_key(|owner| owner.pid);
    Ok(owners)
}

/// One IP socket as the kernel describes it: what a bind to a port would
/// meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoundSocket {
    transport: Transport,
    /// The local port, in host byte order.
    port: u16,
    /// The IPv4 address it serves, when it serves IPv4.
    v4: Option<Ipv4Addr>,
    /// The IPv6 address it serves, when it serves IPv6.
    v6: Option<Ipv6Addr>,
}

impl BoundSocket {
    /// Reads a socket the kernel described: a TCP socket in `LISTEN` or a
    /// UDP socket with a local port, of either IP family. Any other socket
    /// answers `None`.
    fn decode(info: &sys::socket_info) -> Option<Self> {
        if info.soi_family != libc::AF_INET && info.soi_family != libc::AF_INET6 {
            return None;
        }
        let (transport, inet) = match (info.soi_protocol, info.soi_kind) {
            (libc::IPPROTO_TCP, sys::SOCKINFO_TCP) => {
                // SAFETY: `soi_kind` SOCKINFO_TCP says the kernel filled
                // `pri_tcp`, which is plain integers.
                let tcp = unsafe { info.soi_proto.pri_tcp };
                if tcp.tcpsi_state != sys::TSI_S_LISTEN {
                    return None;
                }
                (Transport::Tcp, tcp.tcpsi_ini)
            }
            (libc::IPPROTO_UDP, sys::SOCKINFO_IN) => {
                // SAFETY: `soi_kind` SOCKINFO_IN says the kernel filled
                // `pri_in`, which is plain integers.
                (Transport::Udp, unsafe { info.soi_proto.pri_in })
            }
            _ => return None,
        };
        // `insi_lport` holds the u_short port in network byte order.
        let port = u16::from_be(u16::try_from(inet.insi_lport).ok()?);
        if port == 0 {
            return None;
        }
        let v4 = (inet.insi_vflag & sys::INI_IPV4 != 0).then(|| {
            // SAFETY: both members of the union are 16 plain bytes, so
            // either reading is a valid value; `INI_IPV4` says the IPv4
            // form is the one the kernel means.
            let addr = unsafe { inet.insi_laddr.ina_46 }.i46a_addr4;
            // `s_addr` holds the address in network byte order, so its
            // bytes in memory are the address in order.
            Ipv4Addr::from(addr.s_addr.to_ne_bytes())
        });
        let v6 = (inet.insi_vflag & sys::INI_IPV6 != 0).then(|| {
            // SAFETY: as above; `INI_IPV6` says the IPv6 form is meant.
            Ipv6Addr::from(unsafe { inet.insi_laddr.ina_6 }.s6_addr)
        });
        Some(Self {
            transport,
            port,
            v4,
            v6,
        })
    }

    /// True when this socket holds `port` for `transport` at an address
    /// that serves `address`.
    fn holds(&self, transport: Transport, address: IpAddr, port: u16) -> bool {
        self.transport == transport
            && self.port == port
            && (self
                .v4
                .is_some_and(|local| listener_serves(IpAddr::V4(local), address))
                || self
                    .v6
                    .is_some_and(|local| listener_serves(IpAddr::V6(local), address)))
    }
}

/// The processes of the effective user, at most
/// [`LISTENER_SCAN_MAX_PIDS`] of them, through
/// `proc_listpids(PROC_UID_ONLY)`, which answers a number of bytes.
fn user_pids() -> Result<Vec<Pid>, ProcessError> {
    let uid = current_uid();
    // SAFETY: a null buffer of size 0 only asks for the size needed.
    let needed = unsafe { libc::proc_listpids(sys::PROC_UID_ONLY, uid, ptr::null_mut(), 0) };
    if needed < 0 {
        return Err(io_error("proc_listpids PROC_UID_ONLY"));
    }
    // Room for a few more processes than were there a moment ago.
    let count = (usize::try_from(needed).unwrap_or(0) / size_of::<libc::pid_t>() + 16)
        .min(LISTENER_SCAN_MAX_PIDS);
    let mut pids: Vec<libc::pid_t> = vec![0; count];
    let bytes = c_int::try_from(count * size_of::<libc::pid_t>()).unwrap_or(c_int::MAX);
    // SAFETY: the buffer holds `bytes` bytes of pid_t values.
    let filled = unsafe {
        libc::proc_listpids(
            sys::PROC_UID_ONLY,
            uid,
            pids.as_mut_ptr().cast::<c_void>(),
            bytes,
        )
    };
    if filled < 0 {
        return Err(io_error("proc_listpids PROC_UID_ONLY"));
    }
    pids.truncate(usize::try_from(filled).unwrap_or(0) / size_of::<libc::pid_t>());
    Ok(pids
        .into_iter()
        .filter_map(|pid| u32::try_from(pid).ok().and_then(|pid| Pid::new(pid).ok()))
        .collect())
}

/// True when one of the sockets of `pid` holds the port. A process whose
/// descriptors cannot be listed, or that is gone, holds nothing here.
fn holds_port(pid: Pid, transport: Transport, address: IpAddr, port: u16) -> bool {
    let Ok(fds) = inspect::list_fds(pid) else {
        return false;
    };
    let socket = u32::try_from(libc::PROX_FDTYPE_SOCKET).unwrap_or(u32::MAX);
    fds.iter()
        .filter(|fd| fd.proc_fdtype == socket)
        .any(|fd| match socket_info(pid, fd.proc_fd) {
            Ok(Some(info)) => BoundSocket::decode(&info)
                .is_some_and(|bound| bound.holds(transport, address, port)),
            Ok(None) | Err(_) => false,
        })
}

/// What the kernel says about one socket descriptor, or `None` when the
/// descriptor closed in the meantime.
fn socket_info(pid: Pid, fd: i32) -> Result<Option<sys::socket_info>, Listing> {
    // SAFETY: the struct is plain integers, for which all zero bytes are a
    // valid value.
    let mut info: sys::socket_fdinfo = unsafe { mem::zeroed() };
    let size = c_int::try_from(size_of::<sys::socket_fdinfo>()).unwrap_or(c_int::MAX);
    // SAFETY: `info` is writable for `size` bytes.
    let filled = unsafe {
        libc::proc_pidfdinfo(
            pid.as_i32(),
            fd,
            sys::PROC_PIDFDSOCKETINFO,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    if filled == size {
        return Ok(Some(info.psi));
    }
    if filled > 0 {
        return Err(Listing::Failed(ProcessError::Io {
            op: "proc_pidfdinfo PROC_PIDFDSOCKETINFO",
            path: None,
            kind: io::ErrorKind::InvalidData,
            message: format!("the kernel answered {filled} bytes, not {size}"),
        }));
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EBADF) => Ok(None),
        _ => Err(inspect::listing_error(
            "proc_pidfdinfo PROC_PIDFDSOCKETINFO",
        )),
    }
}

/// The owner a socket's process makes, or `None` when the process exited
/// in the meantime. A start time or executable that cannot be read is left
/// out, never guessed.
fn owner(pid: Pid) -> Option<ListenerOwner> {
    let start = match inspect::bsd_info(pid) {
        Ok(BsdInfo::Gone) => return None,
        Ok(BsdInfo::Info(info)) if info.pbi_status == libc::SZOMB => return None,
        Ok(BsdInfo::Info(info)) => {
            inspect::start_time(info.pbi_start_tvsec, info.pbi_start_tvusec).ok()
        }
        Ok(BsdInfo::Denied) | Err(_) => None,
    };
    let executable = inspect::executable(pid).ok().flatten();
    Some(ListenerOwner::new(pid, executable, start))
}

fn io_error(op: &'static str) -> ProcessError {
    ProcessError::io(op, None, &io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn tcp(port: u16, v4: Option<&str>, v6: Option<&str>) -> BoundSocket {
        BoundSocket {
            transport: Transport::Tcp,
            port,
            v4: v4.map(|text| text.parse().unwrap()),
            v6: v6.map(|text| text.parse().unwrap()),
        }
    }

    #[test]
    fn a_socket_holds_only_its_own_transport_and_port() {
        let socket = tcp(5432, Some("127.0.0.1"), None);

        assert!(socket.holds(Transport::Tcp, ip("127.0.0.1"), 5432));
        assert!(!socket.holds(Transport::Udp, ip("127.0.0.1"), 5432));
        assert!(!socket.holds(Transport::Tcp, ip("127.0.0.1"), 5433));
    }

    #[test]
    fn a_dual_stack_wildcard_socket_holds_the_port_for_both_families() {
        let socket = tcp(8080, Some("0.0.0.0"), Some("::"));

        assert!(socket.holds(Transport::Tcp, ip("127.0.0.1"), 8080));
        assert!(socket.holds(Transport::Tcp, ip("::1"), 8080));
    }

    #[test]
    fn an_ipv6_only_socket_does_not_hold_an_ipv4_address() {
        let socket = tcp(8080, None, Some("::"));

        assert!(!socket.holds(Transport::Tcp, ip("127.0.0.1"), 8080));
        assert!(socket.holds(Transport::Tcp, ip("::1"), 8080));
    }

    #[test]
    fn a_port_of_zero_has_no_owners() {
        assert_eq!(owners(Transport::Tcp, ip("127.0.0.1"), 0).unwrap(), []);
    }
}
