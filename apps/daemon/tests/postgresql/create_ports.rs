//! The `endpoint.client` row of `instance.preflight` for PostgreSQL against
//! real listeners on this Mac (spec 0020 AC-4, the table *Hasil yang
//! diharapkan*): the daemon, the real `official/postgresql` package, and
//! sockets this test holds. Every case judges `127.0.0.1:p`.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener, TcpStream, UdpSocket};

use stackly_protocol::instance::{PortRequest, PreflightStatus};
use stackly_testkit::evidence::{Kind, evidence};

use crate::create::{check, preflight_draft};
use crate::harness::{PG18, Site, free_port};
use crate::support::Api;

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

/// What the `endpoint.client` row says about `127.0.0.1:port`: the status,
/// the detail, and the code of the error behind an error row.
async fn judge(site: &Site, api: &Api, port: u16) -> (PreflightStatus, String, Option<String>) {
    let preflight = api
        .preflight(preflight_draft(site, "ports", PortRequest::Fixed(port)))
        .await
        .expect("the preflight is answered");
    let row = check(&preflight, "endpoint.client");
    (
        row.status,
        row.detail.clone(),
        row.error.as_ref().map(|error| error.code().to_owned()),
    )
}

/// A TCP connection between two loopback sockets whose server side closes
/// first, so the server address keeps the `TIME_WAIT`. Returns the server
/// port.
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

const ALL_ADDRESSES: &str =
    "Also in use on all addresses of this Mac; clients may reach that server";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache and real listeners on this Mac"]
async fn the_postgresql_endpoint_row_matches_the_table_against_real_listeners() {
    let ev = evidence!(
        covers = ["AC-056", "VAL-006"],
        kind = Kind::Native,
        expected = "through the daemon, for PostgreSQL on 127.0.0.1:p, a listener on 127.0.0.1:p is supervisor.endpoint_in_use, one on 0.0.0.0:p or [::]:p dual stack is a caution about all addresses, one on [::1]:p is a caution about [::1], a free port, a TIME_WAIT leftover on 127.0.0.1:p or [::1]:p, a connection left open, and UDP on the same port are ok, and a TIME_WAIT leftover next to a dual stack listener is still the caution",
    );
    let Some(site) = Site::open(&ev, PG18) else {
        return;
    };
    site.subject(&ev, PG18.version);
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    // Free.
    let (status, _, error) = judge(&site, &api, free_port()).await;
    assert_eq!((status, error), (PreflightStatus::Ok, None), "free");

    // TCP 127.0.0.1:p (LISTEN).
    let held = TcpListener::bind((V4, 0)).expect("a loopback listener");
    let port = held.local_addr().expect("an address").port();
    let (status, detail, error) = judge(&site, &api, port).await;
    assert_eq!(status, PreflightStatus::Error, "{detail}");
    assert_eq!(error.as_deref(), Some("supervisor.endpoint_in_use"));
    assert_eq!(detail, "In use by another process");
    drop(held);

    // TCP 0.0.0.0:p.
    let any = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("a wildcard listener");
    let port = any.local_addr().expect("an address").port();
    let (status, detail, error) = judge(&site, &api, port).await;
    assert_eq!((status, error), (PreflightStatus::Caution, None), "0.0.0.0");
    assert_eq!(detail, ALL_ADDRESSES);
    drop(any);

    // TCP [::]:p dual stack (the default of macOS, and of std).
    if let Ok(dual) = TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0)) {
        let port = dual.local_addr().expect("an address").port();
        let (status, detail, error) = judge(&site, &api, port).await;
        assert_eq!(
            (status, error),
            (PreflightStatus::Caution, None),
            "[::] dual"
        );
        assert_eq!(detail, ALL_ADDRESSES);
    }

    // TCP [::1]:p.
    if let Ok(six) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) {
        let port = six.local_addr().expect("an address").port();
        let (status, detail, error) = judge(&site, &api, port).await;
        assert_eq!((status, error), (PreflightStatus::Caution, None), "[::1]");
        assert!(detail.starts_with("Also in use on [::1]:"), "{detail}");
    }

    // UDP on the same port.
    let udp = UdpSocket::bind((V4, 0)).expect("a udp socket");
    let port = udp.local_addr().expect("an address").port();
    let (status, detail, error) = judge(&site, &api, port).await;
    assert_eq!(
        (status, error),
        (PreflightStatus::Ok, None),
        "udp: {detail}"
    );
    drop(udp);

    // TIME_WAIT on 127.0.0.1:p.
    let port = time_wait_on(V4).expect("a loopback connection");
    let (status, detail, error) = judge(&site, &api, port).await;
    assert_eq!((status, error), (PreflightStatus::Ok, None), "{detail}");

    // TIME_WAIT on [::1]:p, for an endpoint on 127.0.0.1.
    if let Some(port) = time_wait_on(V6) {
        let (status, detail, error) = judge(&site, &api, port).await;
        assert_eq!((status, error), (PreflightStatus::Ok, None), "{detail}");
    }

    // A connection left open after its listener stopped.
    let listener = TcpListener::bind((V4, 0)).expect("a loopback listener");
    let port = listener.local_addr().expect("an address").port();
    let client = TcpStream::connect((V4, port)).expect("a client connects");
    let (accepted, _) = listener.accept().expect("the server accepts");
    drop(listener);
    let (status, detail, error) = judge(&site, &api, port).await;
    assert_eq!((status, error), (PreflightStatus::Ok, None), "{detail}");
    drop((client, accepted));

    // TIME_WAIT next to a dual stack listener: the listener is still found.
    // The leftover holds 127.0.0.1:p, and std binds with SO_REUSEADDR, so
    // the listener gets the same port.
    let port = time_wait_on(V4).expect("a loopback connection");
    let dual = TcpListener::bind((Ipv6Addr::UNSPECIFIED, port))
        .expect("a dual stack listener binds next to the TIME_WAIT");
    let (status, detail, error) = judge(&site, &api, port).await;
    assert_eq!(
        (status, error),
        (PreflightStatus::Caution, None),
        "{detail}"
    );
    assert_eq!(detail, ALL_ADDRESSES);
    drop(dual);

    drop(api);
    daemon.shutdown().await;
    ev.pass_with(
        "on this Mac the endpoint.client row of PostgreSQL gave the table result for every listener kind: 127.0.0.1 error, 0.0.0.0, [::] dual stack and [::1] caution, TIME_WAIT on 127.0.0.1 and [::1], an open connection, and UDP ok, and TIME_WAIT next to a dual stack listener caution",
    );
}
