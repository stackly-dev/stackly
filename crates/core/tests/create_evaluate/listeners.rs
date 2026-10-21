//! The port rows against real listeners on this Mac (spec 0020 AC-4, the
//! table *Hasil yang diharapkan*): the probe is the real `SocketPortProbe`,
//! the rows are the ones preflight shows.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};

use socket2::{Domain, Protocol, Socket, Type};
use stackly_core::instance::create::CheckStatus;
use stackly_core::supervisor::SocketPortProbe;
use stackly_evidence::{Kind, evidence};
use stackly_testkit::allocate_port;

use crate::common::{Site, fixed, row};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// The status and detail of the `http` row for `127.0.0.1:port`.
fn judge(site: &Site, port: u16) -> (CheckStatus, String, Option<String>) {
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", port)];
    let evaluation = site.evaluate_with_probe(&SocketPortProbe::new(), &draft);
    let http = row(&evaluation, "endpoint.http");
    (
        http.status,
        http.detail.clone(),
        evaluation
            .first_error()
            .map(|error| error.code().to_owned()),
    )
}

/// Binds `addr` and listens, like a server would, with the options the OS
/// default gives it.
fn listen(addr: SocketAddr, v6only: Option<bool>) -> Option<Socket> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP)).ok()?;
    if let Some(v6only) = v6only {
        socket.set_only_v6(v6only).ok()?;
    }
    socket.bind(&addr.into()).ok()?;
    socket.listen(1).ok()?;
    Some(socket)
}

#[test]
fn each_listener_gets_the_result_of_the_expected_table() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Fixture,
        expected = "on this Mac a listener on 127.0.0.1:p is endpoint_in_use, one on 0.0.0.0:p, [::]:p dual stack, or [::1]:p is a caution, and a free port and UDP on the same port are ok",
    );
    let site = Site::new();

    // Free.
    let port = allocate_port().expect("a free port");
    let (status, _, error) = judge(&site, port);
    assert_eq!((status, error), (CheckStatus::Ok, None), "free");

    // TCP 127.0.0.1:p (LISTEN).
    let held = TcpListener::bind((V4, 0)).expect("a loopback listener");
    let port = held.local_addr().expect("an address").port();
    let (status, detail, error) = judge(&site, port);
    assert_eq!(status, CheckStatus::Error);
    assert_eq!(error.as_deref(), Some("supervisor.endpoint_in_use"));
    assert_eq!(detail, "In use by another process");
    drop(held);

    // TCP 0.0.0.0:p.
    let any = listen(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0), None)
        .expect("a wildcard listener");
    let port = any
        .local_addr()
        .expect("an address")
        .as_socket()
        .expect("an inet address")
        .port();
    let (status, detail, error) = judge(&site, port);
    assert_eq!((status, error), (CheckStatus::Caution, None), "0.0.0.0");
    assert_eq!(detail, "In use on another address of this Mac");
    drop(any);

    // TCP [::]:p dual stack.
    if let Some(dual) = listen(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        Some(false),
    ) {
        let port = dual
            .local_addr()
            .expect("an address")
            .as_socket()
            .expect("an inet address")
            .port();
        let (status, _, error) = judge(&site, port);
        assert_eq!((status, error), (CheckStatus::Caution, None), "[::] dual");
    }

    // TCP [::1]:p.
    if let Ok(six) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) {
        let port = six.local_addr().expect("an address").port();
        let (status, detail, error) = judge(&site, port);
        assert_eq!((status, error), (CheckStatus::Caution, None), "[::1]");
        assert!(detail.starts_with("Also in use on [::1]:"), "{detail}");
    }

    // UDP on the same port.
    let udp = UdpSocket::bind((V4, 0)).expect("a udp socket");
    let port = udp.local_addr().expect("an address").port();
    let (status, _, error) = judge(&site, port);
    assert_eq!((status, error), (CheckStatus::Ok, None), "udp");
    drop(udp);
    ev.pass();
}

#[test]
#[ignore = "native: spec 0020 expects ok for a TIME_WAIT leftover, but this Mac answers caution (wildcard bind without SO_REUSEADDR); the table goes back to /architect"]
fn a_time_wait_leftover_on_the_exact_address_is_not_a_conflict() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Native,
        expected = "after a server on 127.0.0.1:p closes a connection and stops, the leftover TIME_WAIT socket does not make the port in use",
    );
    let site = Site::new();
    let server = TcpListener::bind((V4, 0)).expect("a loopback listener");
    let port = server.local_addr().expect("an address").port();
    let client = TcpStream::connect((V4, port)).expect("a client connects");
    let (accepted, _) = server.accept().expect("the server accepts");
    // The server side closes first, so its address keeps the TIME_WAIT.
    drop(accepted);
    let mut client = client;
    let mut rest = Vec::new();
    let _ = client.read_to_end(&mut rest);
    drop(client);
    drop(server);

    let (status, detail, error) = judge(&site, port);

    assert_eq!(
        (status, error.as_deref()),
        (CheckStatus::Ok, None),
        "{detail}"
    );
    ev.pass();
}
