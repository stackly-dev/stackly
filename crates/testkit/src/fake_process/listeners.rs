//! The listener lookup of [`FakeProcessSupervision`] (spec 0034, *Pemilik
//! port*): sockets a test makes its processes hold, and the
//! [`ListenerLookup`] answer the adapter would give for them.

use std::net::IpAddr;

use stackly_domain::{Pid, ProcessStart, Transport};
use stackly_platform::{ListenerLookup, ListenerOwner, ProcessError, listener_serves};

use super::{FakeProcessSupervision, ProcessCall, ProcessOp};

/// A TCP listener or bound UDP socket that one process of the fake holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FakeSocket {
    pid: Pid,
    /// The start time of the process that opened it, so a reused PID does
    /// not inherit it.
    start: ProcessStart,
    transport: Transport,
    address: IpAddr,
    port: u16,
}

impl FakeProcessSupervision {
    /// Makes the live process `pid` hold a TCP listener, or a bound UDP
    /// socket, on `address` and `port`. The socket belongs to the process
    /// as it is now: once it exits, or its facts get another start time
    /// (a reused PID), it holds the socket no more. A process of another
    /// user can hold one too, but [`owners`](ListenerLookup::owners) never
    /// sees it, like the adapter.
    ///
    /// # Panics
    ///
    /// Panics when no live process has `pid`.
    pub fn listen(&self, pid: Pid, transport: Transport, address: IpAddr, port: u16) {
        let mut state = self.lock_state();
        let start = state
            .processes
            .get(&pid)
            .filter(|process| process.is_alive())
            .map(|process| process.facts.start)
            .unwrap_or_else(|| panic!("a socket needs a live process with PID {pid}"));
        let socket = FakeSocket {
            pid,
            start,
            transport,
            address,
            port,
        };
        if !state.sockets.contains(&socket) {
            state.sockets.push(socket);
        }
    }

    /// Closes the socket `pid` holds for `transport` on `address` and
    /// `port`, if it holds one.
    pub fn close_socket(&self, pid: Pid, transport: Transport, address: IpAddr, port: u16) {
        self.lock_state().sockets.retain(|socket| {
            (socket.pid, socket.transport, socket.address, socket.port)
                != (pid, transport, address, port)
        });
    }
}

impl ListenerLookup for FakeProcessSupervision {
    /// Lists the live processes of the fake's user that hold a socket for
    /// `transport` on `port` at an address that serves `address`, in PID
    /// order and each once, after the call is logged as
    /// [`ProcessOp::ListenerOwners`] and any fault for it applied.
    fn owners(
        &self,
        transport: Transport,
        address: IpAddr,
        port: u16,
    ) -> Result<Vec<ListenerOwner>, ProcessError> {
        self.call(ProcessCall::new(ProcessOp::ListenerOwners), |state| {
            if port == 0 {
                return Ok(Vec::new());
            }
            let mut owners: Vec<ListenerOwner> = state
                .sockets
                .iter()
                .filter(|socket| {
                    socket.transport == transport
                        && socket.port == port
                        && listener_serves(socket.address, address)
                })
                .filter_map(|socket| {
                    state.processes.get(&socket.pid).filter(|process| {
                        process.is_alive()
                            && process.facts.start == socket.start
                            && process.facts.uid == state.user
                    })
                })
                .map(|process| {
                    ListenerOwner::new(
                        process.facts.pid,
                        Some(process.facts.executable.clone()),
                        Some(process.facts.start),
                    )
                })
                .collect();
            owners.sort_by_key(|owner| owner.pid);
            owners.dedup_by_key(|owner| owner.pid);
            Ok(owners)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use stackly_platform::{ProcessFacts, UserId};

    use super::*;
    use crate::TestRoot;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn pid(value: u32) -> Pid {
        Pid::new(value).unwrap()
    }

    fn process(fake: &FakeProcessSupervision, value: u32, uid: UserId) -> Pid {
        let id = pid(value);
        fake.insert(ProcessFacts {
            pid: id,
            parent: None,
            group: id,
            session: id,
            uid,
            start: ProcessStart::new(1_767_225_600 + u64::from(value), 0).unwrap(),
            executable: PathBuf::from(format!("/opt/test/bin/listener-{value}")),
            stopped: false,
        });
        id
    }

    #[test]
    fn a_listening_process_of_the_user_is_the_owner_with_its_facts() {
        let root = TestRoot::new().unwrap();
        let fake = FakeProcessSupervision::new(&root);
        let owner = process(&fake, 600, fake.user());
        fake.listen(owner, Transport::Tcp, ip("0.0.0.0"), 5432);

        let owners = fake.owners(Transport::Tcp, ip("127.0.0.1"), 5432).unwrap();

        assert_eq!(
            owners,
            [ListenerOwner::new(
                owner,
                Some(PathBuf::from("/opt/test/bin/listener-600")),
                Some(ProcessStart::new(1_767_226_200, 0).unwrap()),
            )]
        );
        assert_eq!(fake.count(ProcessOp::ListenerOwners), 1);
    }

    #[test]
    fn another_transport_port_or_address_has_no_owner() {
        let root = TestRoot::new().unwrap();
        let fake = FakeProcessSupervision::new(&root);
        let owner = process(&fake, 601, fake.user());
        fake.listen(owner, Transport::Udp, ip("127.0.0.1"), 8125);

        assert_eq!(
            fake.owners(Transport::Tcp, ip("127.0.0.1"), 8125).unwrap(),
            []
        );
        assert_eq!(
            fake.owners(Transport::Udp, ip("127.0.0.1"), 8126).unwrap(),
            []
        );
        assert_eq!(
            fake.owners(Transport::Udp, ip("127.0.0.2"), 8125).unwrap(),
            []
        );
        assert_eq!(fake.owners(Transport::Udp, ip("127.0.0.1"), 0).unwrap(), []);
    }

    #[test]
    fn a_process_of_another_user_is_never_an_owner() {
        let root = TestRoot::new().unwrap();
        let fake = FakeProcessSupervision::new(&root);
        let foreign = process(&fake, 602, UserId::Unix(0));
        fake.listen(foreign, Transport::Tcp, ip("127.0.0.1"), 80);

        assert_eq!(
            fake.owners(Transport::Tcp, ip("127.0.0.1"), 80).unwrap(),
            []
        );
    }

    #[test]
    fn a_socket_goes_with_its_process_and_never_passes_to_a_reused_pid() {
        let root = TestRoot::new().unwrap();
        let fake = FakeProcessSupervision::new(&root);
        let first = process(&fake, 603, fake.user());
        let second = process(&fake, 604, fake.user());
        fake.listen(first, Transport::Tcp, ip("127.0.0.1"), 6379);
        fake.listen(second, Transport::Tcp, ip("::"), 6379);
        fake.listen(second, Transport::Tcp, ip("0.0.0.0"), 6379);

        let both = fake.owners(Transport::Tcp, ip("0.0.0.0"), 6379).unwrap();
        assert_eq!(
            both.iter().map(|owner| owner.pid).collect::<Vec<_>>(),
            [first, second],
            "each owner once, in PID order"
        );

        fake.update_facts(first, |facts| {
            facts.start = ProcessStart::new(1_767_300_000, 0).unwrap();
        });
        fake.exit(second, None);
        assert_eq!(
            fake.owners(Transport::Tcp, ip("127.0.0.1"), 6379).unwrap(),
            []
        );
    }

    #[test]
    fn a_closed_socket_has_no_owner_and_a_fault_is_returned_after_the_log() {
        let root = TestRoot::new().unwrap();
        let fake = FakeProcessSupervision::new(&root);
        let owner = process(&fake, 605, fake.user());
        fake.listen(owner, Transport::Tcp, ip("127.0.0.1"), 3306);
        fake.close_socket(owner, Transport::Tcp, ip("127.0.0.1"), 3306);
        assert_eq!(
            fake.owners(Transport::Tcp, ip("127.0.0.1"), 3306).unwrap(),
            []
        );

        let error = ProcessError::Io {
            op: "proc_listpids PROC_UID_ONLY",
            path: None,
            kind: std::io::ErrorKind::Other,
            message: "injected".to_owned(),
        };
        fake.fail_next(ProcessOp::ListenerOwners, error.clone());

        assert_eq!(
            fake.owners(Transport::Tcp, ip("127.0.0.1"), 3306),
            Err(error)
        );
        assert_eq!(fake.count(ProcessOp::ListenerOwners), 2);
    }

    #[test]
    #[should_panic(expected = "a socket needs a live process")]
    fn listening_needs_a_live_process() {
        let root = TestRoot::new().unwrap();
        let fake = FakeProcessSupervision::new(&root);

        fake.listen(pid(606), Transport::Tcp, ip("127.0.0.1"), 5432);
    }
}
