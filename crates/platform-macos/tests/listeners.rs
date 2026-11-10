//! The `ListenerLookup` of `MacosProcessSupervision` against real sockets
//! (spec 0034 build step 2, AC-5): TCP listeners and UDP sockets this test
//! binds, and `/usr/bin/nc` as another process of the user that holds a
//! port. Every test takes its ports from `allocate_port`, and a guard kills
//! and reaps any `nc` it started.
//!
//! The tests run one at a time: between its fork and its exec, a child the
//! `nc` test spawns is a copy of this process that holds every socket of
//! the other tests, and the lookup rightly lists it as an owner too.
#![cfg(target_os = "macos")]

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use stackly_domain::{Pid, Transport};
use stackly_platform::{ListenerLookup, ListenerOwner, ProcessSupervision, UserIdentity};
use stackly_platform_macos::{MacosIdentity, MacosPlatform, MacosProcessSupervision};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{TestRoot, allocate_port};

/// How long a test waits for `nc` to bind. Generous on purpose: a passing
/// run never comes close.
const PATIENCE: Duration = Duration::from_secs(30);
/// The netcat that ships with macOS.
const NETCAT: &str = "/usr/bin/nc";

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const LOOPBACK_V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);
const ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Held by every test for its whole run, so no fork of one test holds the
/// sockets of another (see the module doc).
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Kills and reaps a child `nc` when the test ends, pass or fail.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn me() -> Pid {
    Pid::new(std::process::id()).expect("this process has a PID above 0")
}

fn pids(owners: &[ListenerOwner]) -> Vec<Pid> {
    owners.iter().map(|owner| owner.pid).collect()
}

fn owners(transport: Transport, address: IpAddr, port: u16) -> Vec<ListenerOwner> {
    MacosProcessSupervision::new()
        .owners(transport, address, port)
        .expect("the processes of this user can be listed")
}

/// Starts `nc` listening on loopback `port`, over UDP when `udp` is true,
/// and waits until the lookup sees it.
fn netcat(udp: bool, port: u16) -> (Reaped, Pid) {
    let mut command = Command::new(NETCAT);
    if udp {
        command.arg("-u");
    }
    let child = command
        .args(["-l", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("nc ships with macOS");
    let pid = Pid::new(child.id()).expect("a spawned child has a PID above 0");
    let guard = Reaped(child);
    let transport = if udp { Transport::Udp } else { Transport::Tcp };
    let deadline = Instant::now() + PATIENCE;
    while !pids(&owners(transport, LOOPBACK, port)).contains(&pid) {
        assert!(Instant::now() < deadline, "nc never bound port {port}");
        thread::sleep(Duration::from_millis(20));
    }
    (guard, pid)
}

#[test]
fn a_tcp_listener_of_this_process_is_its_owner_with_its_executable_and_start() {
    let _serial = serial();
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "a TCP listener this test binds on 127.0.0.1 makes this process the only owner for TCP on that port at 127.0.0.1 and at 0.0.0.0, with the executable from proc_pidpath and the start time inspect reads; UDP on that port and TCP at 127.0.0.2 have no owner",
    );
    let port = allocate_port().unwrap();
    let _listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
    let adapter = MacosProcessSupervision::new();
    let facts = adapter.inspect(me()).unwrap().unwrap();

    let found = owners(Transport::Tcp, LOOPBACK, port);

    assert_eq!(
        found,
        [ListenerOwner::new(
            me(),
            Some(facts.executable.clone()),
            Some(facts.start)
        )]
    );
    assert_eq!(
        found[0].executable.as_deref().map(canonical),
        Some(canonical(&std::env::current_exe().unwrap()))
    );
    assert_eq!(pids(&owners(Transport::Tcp, ANY, port)), [me()]);
    assert_eq!(owners(Transport::Udp, LOOPBACK, port), []);
    assert_eq!(
        owners(
            Transport::Tcp,
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)),
            port
        ),
        []
    );
    ev.pass();
}

#[test]
fn a_bound_udp_socket_holds_its_port_for_udp_only() {
    let _serial = serial();
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "a UDP socket bound on 127.0.0.1 makes this process the owner for UDP on that port, also at 0.0.0.0, and never for TCP; once the socket closes the port has no owner",
    );
    let port = allocate_port().unwrap();
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).unwrap();

    assert_eq!(pids(&owners(Transport::Udp, LOOPBACK, port)), [me()]);
    assert_eq!(pids(&owners(Transport::Udp, ANY, port)), [me()]);
    assert_eq!(owners(Transport::Tcp, LOOPBACK, port), []);

    drop(socket);
    assert_eq!(owners(Transport::Udp, LOOPBACK, port), []);
    ev.pass();
}

#[test]
fn a_connected_tcp_socket_that_does_not_listen_holds_no_port() {
    let _serial = serial();
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "after the listener closes, the accepted and the client TCP sockets stay established on the listener port and the client port, and neither port has an owner, because only a socket in LISTEN counts for TCP",
    );
    let port = allocate_port().unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
    let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let (accepted, _) = listener.accept().unwrap();
    let client_port = client.local_addr().unwrap().port();
    assert_eq!(pids(&owners(Transport::Tcp, LOOPBACK, port)), [me()]);

    drop(listener);

    assert_eq!(accepted.local_addr().unwrap().port(), port);
    assert_eq!(owners(Transport::Tcp, LOOPBACK, port), []);
    assert_eq!(owners(Transport::Tcp, LOOPBACK, client_port), []);
    drop((client, accepted));
    ev.pass();
}

#[test]
fn ipv6_listeners_hold_their_family_and_a_dual_stack_wildcard_holds_both() {
    let _serial = serial();
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "a TCP listener on [::1] owns the port for ::1 but not for 127.0.0.1, and a TCP listener on [::] (dual stack, net.inet6.ip6.v6only 0) owns it for both ::1 and 127.0.0.1",
    );
    let only_v6 = allocate_port().unwrap();
    let _v6 = TcpListener::bind((Ipv6Addr::LOCALHOST, only_v6)).unwrap();
    assert_eq!(pids(&owners(Transport::Tcp, LOOPBACK_V6, only_v6)), [me()]);
    assert_eq!(owners(Transport::Tcp, LOOPBACK, only_v6), []);

    let dual = allocate_port().unwrap();
    let _any = TcpListener::bind((Ipv6Addr::UNSPECIFIED, dual)).unwrap();
    assert_eq!(pids(&owners(Transport::Tcp, LOOPBACK_V6, dual)), [me()]);
    assert_eq!(pids(&owners(Transport::Tcp, LOOPBACK, dual)), [me()]);
    ev.pass();
}

#[test]
fn another_process_of_this_user_is_found_by_its_pid_executable_and_start() {
    let _serial = serial();
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "/usr/bin/nc listening on TCP and bound on UDP on loopback ports is the one owner of each, with its PID, /usr/bin/nc as the executable, and the start time inspect reads for it; after it is killed and reaped the port has no owner",
    );
    let adapter = MacosProcessSupervision::new();
    let tcp_port = allocate_port().unwrap();
    let udp_port = allocate_port().unwrap();
    let (tcp_guard, tcp_pid) = netcat(false, tcp_port);
    let (_udp_guard, udp_pid) = netcat(true, udp_port);

    for (transport, port, pid) in [
        (Transport::Tcp, tcp_port, tcp_pid),
        (Transport::Udp, udp_port, udp_pid),
    ] {
        let facts = adapter.inspect(pid).unwrap().unwrap();
        assert_eq!(facts.executable, canonical(Path::new(NETCAT)));
        assert_eq!(
            owners(transport, LOOPBACK, port),
            [ListenerOwner::new(
                pid,
                Some(facts.executable),
                Some(facts.start)
            )]
        );
    }

    drop(tcp_guard);
    assert_eq!(owners(Transport::Tcp, LOOPBACK, tcp_port), []);
    ev.pass();
}

#[test]
fn every_owner_is_a_process_of_this_user_and_port_zero_has_none() {
    let _serial = serial();
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "the lookup for UDP 5353, which mDNSResponder holds as another user, lists only processes of the current user, and port 0 has no owner",
    );
    let adapter = MacosProcessSupervision::new();
    let ours = MacosIdentity.current_user().unwrap();

    for owner in owners(Transport::Udp, ANY, 5353) {
        if let Some(facts) = adapter.inspect(owner.pid).unwrap() {
            assert_eq!(facts.uid, ours, "{owner:?} is a process of this user");
        }
    }
    assert_eq!(owners(Transport::Tcp, LOOPBACK, 0), []);
    ev.pass();
}

#[test]
fn the_macos_platform_carries_the_listener_lookup() {
    let _serial = serial();
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "Platform::listeners of MacosPlatform answers the owner of a TCP listener this test binds, so core reaches the adapter through the platform bundle",
    );
    let root = TestRoot::new().unwrap();
    let platform = MacosPlatform::with_roots(root.roots()).unwrap();
    let port = allocate_port().unwrap();
    let _listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();

    let found = platform
        .listeners()
        .owners(Transport::Tcp, LOOPBACK, port)
        .unwrap();

    assert_eq!(pids(&found), [me()]);
    ev.pass();
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).expect("the path exists")
}
