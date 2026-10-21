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
        expected = "on this Mac a listener on 127.0.0.1:p is endpoint_in_use, one on 0.0.0.0:p, [::]:p dual stack or IPv6 only, or [::1]:p is a caution, and a free port and UDP on the same port are ok",
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
    assert_eq!(
        detail,
        "Also in use on all addresses of this Mac; clients may reach that server"
    );
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

    // TCP [::]:p IPv6 only.
    if let Some(six_only) = listen(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        Some(true),
    ) {
        let port = six_only
            .local_addr()
            .expect("an address")
            .as_socket()
            .expect("an inet address")
            .port();
        let (status, detail, error) = judge(&site, port);
        assert_eq!((status, error), (CheckStatus::Caution, None), "[::] v6only");
        assert!(detail.starts_with("Also in use on [::1]:"), "{detail}");
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

/// A TCP connection between two loopback sockets whose server side closes
/// first, so the server address `127.0.0.1:port` or `[::1]:port` keeps the
/// `TIME_WAIT`. Returns the server port.
fn time_wait_on(host: IpAddr) -> Option<u16> {
    let server = TcpListener::bind((host, 0)).ok()?;
    let port = server.local_addr().ok()?.port();
    let mut client = TcpStream::connect((host, port)).ok()?;
    let (accepted, _) = server.accept().ok()?;
    drop(accepted);
    let mut rest = Vec::new();
    let _ = client.read_to_end(&mut rest);
    drop(client);
    drop(server);
    Some(port)
}

#[test]
#[ignore = "native: TCP leftovers on this Mac's loopback after a server stops"]
fn a_time_wait_leftover_on_the_exact_address_is_not_a_conflict() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Native,
        expected = "after a server on 127.0.0.1:p or [::1]:p closes a connection and stops, a connection left open on the port, or an outgoing connection from port p, the port is ok, and next to a dual stack [::]:p listener it is still a caution",
    );
    let site = Site::new();

    // TIME_WAIT on 127.0.0.1:p.
    let port = time_wait_on(V4).expect("a loopback connection");
    let (status, detail, error) = judge(&site, port);
    assert_eq!(
        (status, error.as_deref()),
        (CheckStatus::Ok, None),
        "{detail}"
    );

    // TIME_WAIT on [::1]:p, for an endpoint on 127.0.0.1.
    if let Some(port) = time_wait_on(IpAddr::V6(Ipv6Addr::LOCALHOST)) {
        let (status, detail, error) = judge(&site, port);
        assert_eq!(
            (status, error.as_deref()),
            (CheckStatus::Ok, None),
            "{detail}"
        );
    }

    // A connection left open after its listener stopped.
    let listener = TcpListener::bind((V4, 0)).expect("a loopback listener");
    let port = listener.local_addr().expect("an address").port();
    let client = TcpStream::connect((V4, port)).expect("a client connects");
    let (accepted, _) = listener.accept().expect("the server accepts");
    drop(listener);
    let (status, detail, error) = judge(&site, port);
    assert_eq!(
        (status, error.as_deref()),
        (CheckStatus::Ok, None),
        "{detail}"
    );
    drop((client, accepted));

    // An outgoing connection whose local port is p.
    let other = TcpListener::bind((V4, 0)).expect("a loopback listener");
    let port = allocate_port().expect("a free port");
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).expect("a socket");
    socket
        .bind(&SocketAddr::new(V4, port).into())
        .expect("the client binds its local port");
    socket
        .connect(&other.local_addr().expect("an address").into())
        .expect("the client connects");
    let (status, detail, error) = judge(&site, port);
    assert_eq!(
        (status, error.as_deref()),
        (CheckStatus::Ok, None),
        "{detail}"
    );
    drop((socket, other));

    // TIME_WAIT next to a dual stack listener: the listener is still found.
    let port = time_wait_on(V4).expect("a loopback connection");
    let dual = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).expect("a socket");
    dual.set_only_v6(false).expect("dual stack");
    dual.set_reuse_address(true).expect("reuse the port");
    dual.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port).into())
        .expect("a dual stack listener binds next to the TIME_WAIT");
    dual.listen(1).expect("the dual stack socket listens");
    let (status, detail, error) = judge(&site, port);
    assert_eq!((status, error), (CheckStatus::Caution, None), "{detail}");
    assert_eq!(
        detail,
        "Also in use on all addresses of this Mac; clients may reach that server"
    );
    ev.pass();
}
