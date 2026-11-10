//! The listener lookup port (spec 0034, *Pemilik port*): which processes
//! of the current user hold a TCP listener or a bound UDP socket on a
//! port, so the doctor can name the owner of a port conflict.
//!
//! An adapter only sees processes of the current user and never reads the
//! memory or the arguments of a process, so a port that root or another
//! user holds has no owner here.

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use stackly_domain::{Os, Pid, ProcessStart, Transport};

use crate::capability::PlatformCapability;
use crate::process::ProcessError;

/// The most processes one lookup reads the sockets of.
pub const LISTENER_SCAN_MAX_PIDS: usize = 4096;

/// How long one lookup reads processes before it answers with the owners
/// it found so far.
pub const LISTENER_SCAN_DEADLINE: Duration = Duration::from_secs(2);

/// A process of the current user that holds a socket on the port a
/// [`ListenerLookup`] was asked about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ListenerOwner {
    /// The process ID.
    pub pid: Pid,
    /// The path of its executable, or `None` when it could not be read.
    pub executable: Option<PathBuf>,
    /// When the process started, or `None` when it could not be read. A
    /// caller compares it with a recorded identity before it calls the
    /// process an instance of Stackly.
    pub start: Option<ProcessStart>,
}

impl ListenerOwner {
    /// Creates an owner from what the adapter read.
    #[must_use]
    pub const fn new(pid: Pid, executable: Option<PathBuf>, start: Option<ProcessStart>) -> Self {
        Self {
            pid,
            executable,
            start,
        }
    }
}

/// Finds the processes of the current user that hold a port (spec 0034,
/// *Pemilik port*).
///
/// Every method is sync and object safe, and safe to call from any thread;
/// core calls it through `spawn_blocking`. It never signals, spawns, or
/// locks anything.
pub trait ListenerLookup: Send + Sync {
    /// Lists the processes of the current user that hold a TCP socket in
    /// `LISTEN`, or a UDP socket bound, on `port` with an address that
    /// [`listener_serves`] `address`, in PID order and each once.
    ///
    /// A port of 0 has no owners. An adapter reads at most
    /// [`LISTENER_SCAN_MAX_PIDS`] processes for at most
    /// [`LISTENER_SCAN_DEADLINE`] and then answers with what it found; a
    /// process whose sockets cannot be read is skipped. An empty list
    /// means no process of this user was seen holding the port, not that
    /// the port is free.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the processes of the user cannot
    /// be listed, and [`ProcessError::NotSupported`] on an OS without an
    /// adapter.
    fn owners(
        &self,
        transport: Transport,
        address: IpAddr,
        port: u16,
    ) -> Result<Vec<ListenerOwner>, ProcessError>;
}

/// True when a socket bound to `local` takes what is sent to `query`, or
/// stands in the way of a bind to it: the same address, or either one the
/// unspecified address of its family. An IPv4 address mapped into IPv6
/// counts as the IPv4 address, and the two families never meet otherwise;
/// an adapter passes each family a dual stack socket serves on its own.
///
/// ```
/// use std::net::IpAddr;
/// use stackly_platform::listener_serves;
///
/// let ip = |text: &str| text.parse::<IpAddr>().unwrap();
/// assert!(listener_serves(ip("0.0.0.0"), ip("127.0.0.1")));
/// assert!(listener_serves(ip("127.0.0.1"), ip("0.0.0.0")));
/// assert!(!listener_serves(ip("127.0.0.2"), ip("127.0.0.1")));
/// assert!(!listener_serves(ip("::"), ip("127.0.0.1")));
/// ```
#[must_use]
pub fn listener_serves(local: IpAddr, query: IpAddr) -> bool {
    match (local.to_canonical(), query.to_canonical()) {
        (IpAddr::V4(local), IpAddr::V4(query)) => {
            local == query || local.is_unspecified() || query.is_unspecified()
        }
        (IpAddr::V6(local), IpAddr::V6(query)) => {
            local == query || local.is_unspecified() || query.is_unspecified()
        }
        _ => false,
    }
}

/// The listener lookup of an OS without an adapter: every lookup answers
/// [`ProcessError::NotSupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedListenerLookup {
    os: Os,
}

impl UnsupportedListenerLookup {
    /// Creates the port for `os`.
    #[must_use]
    pub const fn new(os: Os) -> Self {
        Self { os }
    }
}

impl ListenerLookup for UnsupportedListenerLookup {
    fn owners(
        &self,
        _transport: Transport,
        _address: IpAddr,
        _port: u16,
    ) -> Result<Vec<ListenerOwner>, ProcessError> {
        Err(ProcessError::NotSupported {
            capability: PlatformCapability::ProcessSupervision,
            os: self.os,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn a_listener_serves_its_own_address_and_any_address_under_a_wildcard() {
        assert!(listener_serves(ip("127.0.0.1"), ip("127.0.0.1")));
        assert!(listener_serves(ip("0.0.0.0"), ip("127.0.0.1")));
        assert!(listener_serves(ip("0.0.0.0"), ip("192.168.1.20")));
        assert!(listener_serves(ip("::1"), ip("::1")));
        assert!(listener_serves(ip("::"), ip("::1")));
    }

    #[test]
    fn a_wildcard_query_meets_every_listener_of_its_family() {
        assert!(listener_serves(ip("127.0.0.1"), ip("0.0.0.0")));
        assert!(listener_serves(ip("::1"), ip("::")));
        assert!(!listener_serves(ip("::1"), ip("0.0.0.0")));
        assert!(!listener_serves(ip("127.0.0.1"), ip("::")));
    }

    #[test]
    fn different_addresses_of_one_family_do_not_meet() {
        assert!(!listener_serves(ip("127.0.0.2"), ip("127.0.0.1")));
        assert!(!listener_serves(ip("::2"), ip("::1")));
    }

    #[test]
    fn an_ipv4_mapped_address_counts_as_its_ipv4_address() {
        assert!(listener_serves(ip("::ffff:127.0.0.1"), ip("127.0.0.1")));
        assert!(listener_serves(ip("127.0.0.1"), ip("::ffff:127.0.0.1")));
        assert!(listener_serves(ip("0.0.0.0"), ip("::ffff:10.0.0.1")));
        assert!(!listener_serves(ip("::ffff:127.0.0.2"), ip("127.0.0.1")));
    }

    #[test]
    fn the_unsupported_lookup_answers_not_supported_for_its_os() {
        let lookup = UnsupportedListenerLookup::new(Os::Windows);

        let error = lookup
            .owners(Transport::Tcp, ip("127.0.0.1"), 5432)
            .unwrap_err();

        assert_eq!(
            error,
            ProcessError::NotSupported {
                capability: PlatformCapability::ProcessSupervision,
                os: Os::Windows,
            }
        );
    }
}
