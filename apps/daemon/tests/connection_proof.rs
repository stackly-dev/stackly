//! Start, readiness, and connection with real PostgreSQL 18 and NATS 2.15.1
//! on this Mac (spec 0021, milestone 4, build steps 12 and 13): the real
//! `stacklyd` and `stackly` binaries on a test root, the managed runtimes
//! from the runtime cache of `cargo xtask`, an instance started from the CLI
//! until it is `ready`, and then the composed connection strings used by the
//! runtime's own `psql` (TCP and Unix socket) and by a TCP client that reads
//! the `INFO` line of NATS. Every test is native: `#[ignore = "native: ..."]`,
//! run with `cargo test -p stacklyd --test connection_proof -- --ignored`. An
//! artifact that is not in the cache ends a test as `BLOCKED`, never as a
//! download.
//!
//! The `*_offline` tests are the same journeys for step 13. They change
//! nothing but a guard: they need `STACKLY_PROOF_OFFLINE=1` and refuse to run
//! unless this Mac has no default route, so the run proves that start,
//! status, `connection`, and `inspect` need no network. The user turns the
//! network off and on; no test touches it.
#![cfg(target_os = "macos")]

#[path = "common/mod.rs"]
mod common;
#[path = "nats/harness.rs"]
mod nats;
#[path = "postgresql/harness.rs"]
mod postgresql;
#[path = "supervisor/support.rs"]
mod support;

use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use stackly_protocol::instance::{
    InstanceHealthState, InstanceLifecycle, InstanceReadinessState, InstanceSummary,
};
use stackly_testkit::cargo_bin;
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

/// The environment variable that marks a run with the network off.
const OFFLINE: &str = "STACKLY_PROOF_OFFLINE";

/// What `stackly` printed.
#[derive(Debug)]
struct Cli {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `stackly <args>` on the data root `root`, in text mode, the way a
/// person at a terminal would (no prompt, no color).
fn stackly(root: &Path, args: &[&str]) -> Cli {
    let output = Command::new(cargo_bin("stackly-cli", "stackly"))
        .args(args)
        .env("STACKLY_ROOT", root)
        .env("STACKLY_NO_INPUT", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .expect("stackly runs");
    Cli {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// `stackly <args>` that has to exit 0; returns its standard output without
/// the final newline.
fn stackly_ok(root: &Path, args: &[&str]) -> String {
    let ran = stackly(root, args);
    assert_eq!(ran.code, Some(0), "stackly {args:?}: {ran:?}");
    ran.stdout.trim_end_matches('\n').to_owned()
}

/// Refuses to go on unless the run claims to be offline and this Mac has no
/// default route, so an online run never counts as the offline evidence.
fn require_network_off() {
    assert!(
        std::env::var_os(OFFLINE).is_some(),
        "set {OFFLINE}=1 after the user turned the network of this Mac off"
    );
    let route = Command::new("/sbin/route")
        .args(["-n", "get", "default"])
        .output()
        .expect("route runs");
    assert!(
        !route.status.success(),
        "{OFFLINE} is set but this Mac still has a default route; turn Wi-Fi and Ethernet off first"
    );
}

/// The suffix the evidence `actual` gets when the run was offline.
fn offline_note() -> &'static str {
    if std::env::var_os(OFFLINE).is_some() {
        " with the network of this Mac off (no default route)"
    } else {
        ""
    }
}

/// Starts `name` from the CLI, waits for the operation, then for `ready`.
async fn start_from_cli(root: &Path, api: &support::Api, id: Uuid) -> InstanceSummary {
    let before = api.get(id).await.expect("the instance can be read");
    assert_eq!(before.lifecycle, InstanceLifecycle::Stopped);
    let started = stackly(root, &["instance", "start", &id.to_string(), "--wait"]);
    assert_eq!(started.code, Some(0), "{started:?}");
    let ready = api
        .wait_instance(id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(ready.lifecycle, InstanceLifecycle::Running);
    assert_eq!(ready.readiness.reason, "probe_ok");
    api.wait_instance(id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await;
    // `instance get` is the status command of the CLI.
    let status = stackly_ok(root, &["instance", "get", &id.to_string()]);
    assert!(
        status.to_lowercase().contains("ready"),
        "instance get shows the readiness: {status}"
    );
    ready
}

/// The masked string of the connection `entry` in the detail the GUI Summary
/// card reads.
async fn summary_masked(api: &support::Api, id: Uuid, entry: &str) -> String {
    let detail = api.detail(id).await.expect("the detail can be read");
    detail
        .connections
        .iter()
        .find(|connection| connection.name == entry)
        .unwrap_or_else(|| panic!("the detail has a {entry} connection"))
        .masked
        .clone()
        .unwrap_or_else(|| panic!("the {entry} connection is available"))
}

/// `text` with every byte outside the unreserved set of RFC 3986 written as
/// `%XX`, the way the connection template encodes a value.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Replaces the password in `full` by the four stars `masked` shows.
fn mask_password(full: &str, password: &str) -> String {
    let encoded = percent_encode(password);
    assert!(
        full.contains(&encoded),
        "the revealed string holds the encoded password"
    );
    full.replacen(&encoded, "****", 1)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_started_from_the_cli_connects_with_the_composed_strings() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016", "AC-027"],
        kind = Kind::Native,
        expected = "a PostgreSQL 18 instance made through the daemon starts from stackly instance start until it is ready; the uri string from stackly instance connection --show-password runs SELECT 1 through the psql of the runtime over TCP, the socket string runs it over the Unix socket, the masked string of the CLI is the masked string of the detail the Summary tab reads, and the password is on stdout only with --show-password",
    );
    postgresql_journey(&ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache and the network of this Mac turned off by the user (spec 0021 AC-10, step 13)"]
async fn postgresql_18_offline_start_status_connection_and_inspect() {
    let ev = evidence!(
        covers = ["REQ-046", "AC-046"],
        kind = Kind::Native,
        expected = "with the network of this Mac off, a PostgreSQL 18 instance starts from the CLI until it is ready, instance get shows it, and connection, connection --show-password, and inspect give the same strings as online, and psql runs SELECT 1 with them",
    );
    require_network_off();
    postgresql_journey(&ev).await;
}

async fn postgresql_journey(ev: &Evidence) {
    let Some(site) = postgresql::Site::open(ev, postgresql::PG18) else {
        return;
    };
    let daemon = site.binary(&[]);
    let api = site.binary_api(&daemon).await;
    let name = "orders";
    let made = postgresql::create_instance(&site, &api, name).await;
    let root = site.root.path().to_path_buf();

    let ready = start_from_cli(&root, &api, made.id).await;
    let port = nats::port_of(&ready, "client");
    assert_eq!(port, made.port);

    // Masked strings: the CLI and the detail of the Summary tab agree.
    let masked_uri = stackly_ok(&root, &["instance", "connection", name]);
    assert_eq!(
        masked_uri,
        format!("postgresql://postgres:****@127.0.0.1:{port}/postgres")
    );
    assert_eq!(masked_uri, summary_masked(&api, made.id, "uri").await);
    let masked_socket = stackly_ok(
        &root,
        &["instance", "connection", name, "--format", "socket"],
    );
    assert_eq!(masked_socket, summary_masked(&api, made.id, "socket").await);
    let run_dir = site.run_dir(made.id);
    assert!(
        masked_socket.starts_with("postgresql://postgres:****@/postgres?host="),
        "{masked_socket}"
    );
    assert!(
        masked_socket.ends_with(&format!("&port={port}")),
        "{masked_socket}"
    );
    let all = stackly_ok(&root, &["instance", "connection", name, "--all"]);
    assert!(
        all.contains(&masked_uri) && all.contains(&masked_socket),
        "{all}"
    );

    // The default never carries the password; the flag does.
    let plain = stackly(&root, &["instance", "connection", name]);
    assert!(!plain.stdout.contains(&made.password) && !plain.stderr.contains(&made.password));
    let uri = stackly_ok(&root, &["instance", "connection", name, "--show-password"]);
    let socket = stackly_ok(
        &root,
        &[
            "instance",
            "connection",
            name,
            "--format",
            "socket",
            "--show-password",
        ],
    );
    assert_eq!(mask_password(&uri, &made.password), masked_uri);
    assert_eq!(mask_password(&socket, &made.password), masked_socket);

    // The inspect output shows the same masked strings and no password.
    let inspected = stackly(&root, &["instance", "inspect", name]);
    assert_eq!(inspected.code, Some(0), "{inspected:?}");
    assert!(
        inspected.stdout.contains(&masked_uri),
        "{}",
        inspected.stdout
    );
    assert!(
        !inspected.stdout.contains(&made.password) && !inspected.stderr.contains(&made.password)
    );

    // The runtime's own psql accepts both strings.
    let over_tcp = psql_uri(&site, &uri);
    assert!(over_tcp.0, "psql over TCP: {}", over_tcp.2);
    assert_eq!(over_tcp.1, "1");
    let over_socket = psql_uri(&site, &socket);
    assert!(over_socket.0, "psql over the socket: {}", over_socket.2);
    assert_eq!(over_socket.1, "1");
    assert!(
        socket.contains(&format!(
            "host={}&",
            percent_encode(&run_dir.display().to_string())
        )),
        "the socket string names the run folder {}: {socket}",
        run_dir.display()
    );
    // The masked string is not a working string: its password is wrong.
    let masked_run = psql_uri(&site, &masked_uri);
    assert!(!masked_run.0, "the masked string must not log in");

    let version = server_version(&site, &made.password, &run_dir, port);
    site.subject(ev, &version);
    let stopped = stackly(&root, &["instance", "stop", &made.id.to_string(), "--wait"]);
    assert_eq!(stopped.code, Some(0), "{stopped:?}");
    drop(daemon);
    ev.pass_with(&format!(
        "PostgreSQL {version} started from the CLI, was ready, and ran SELECT 1 through psql with the uri string over TCP and the socket string over the Unix socket{}",
        offline_note()
    ));
}

/// `psql` of the runtime with `uri` as the whole connection string. Returns
/// whether it exited 0, its standard output, and its standard error.
fn psql_uri(site: &postgresql::Site, uri: &str) -> (bool, String, String) {
    let output = Command::new(site.bin("psql"))
        .args([
            "--no-psqlrc",
            "--no-password",
            "--tuples-only",
            "--no-align",
            "--set=ON_ERROR_STOP=1",
        ])
        .arg(format!("--dbname={uri}"))
        .arg("--command=SELECT 1")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", site.root.path())
        .env("PGCONNECT_TIMEOUT", "5")
        .output()
        .expect("psql runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    )
}

/// `SHOW server_version` over the Unix socket of the instance.
fn server_version(site: &postgresql::Site, password: &str, run_dir: &Path, port: u16) -> String {
    let ran = Command::new(site.bin("psql"))
        .args([
            "--no-psqlrc",
            "--no-password",
            "--tuples-only",
            "--no-align",
        ])
        .arg(format!("--host={}", run_dir.display()))
        .arg(format!("--port={port}"))
        .args(["--username=postgres", "--dbname=postgres"])
        .arg("--command=SHOW server_version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", site.root.path())
        .env("PGPASSWORD", password)
        .output()
        .expect("psql runs");
    assert!(ran.status.success(), "SHOW server_version failed");
    String::from_utf8_lossy(&ran.stdout).trim().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_started_from_the_cli_answers_its_connection_string_with_info() {
    let ev = evidence!(
        covers = ["AC-027"],
        kind = Kind::Native,
        expected = "a NATS 2.15.1 instance with both endpoints on Auto starts from stackly instance start until it is ready; the uri string of stackly instance connection is the same as the masked string of the detail the Summary tab reads, and a TCP client that connects to its address receives the INFO line of the server",
    );
    nats_journey(&ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache and the network of this Mac turned off by the user (spec 0021 AC-10, step 13)"]
async fn nats_offline_start_status_connection_and_inspect() {
    let ev = evidence!(
        covers = ["REQ-046", "AC-046"],
        kind = Kind::Native,
        expected = "with the network of this Mac off, a NATS 2.15.1 instance starts from the CLI until it is ready, instance get shows it, connection and inspect give the same strings as online, and the server answers INFO on the uri string",
    );
    require_network_off();
    nats_journey(&ev).await;
}

async fn nats_journey(ev: &Evidence) {
    let Some(site) = nats::Site::open(ev, nats::Serve::Binary) else {
        return;
    };
    let daemon = site.spawn();
    let api = site.binary_api(&daemon).await;
    let name = "bus";
    let id = nats::create_instance(&site, &api, name, json!({})).await;
    let root = site.data_root.clone();

    let ready = start_from_cli(&root, &api, id).await;
    let client = nats::port_of(&ready, "client");
    let monitoring = nats::port_of(&ready, "monitoring");

    let uri = stackly_ok(&root, &["instance", "connection", name]);
    assert_eq!(uri, format!("nats://127.0.0.1:{client}"));
    assert_eq!(uri, summary_masked(&api, id, "uri").await);
    let shown = stackly_ok(&root, &["instance", "connection", name, "--show-password"]);
    assert_eq!(shown, uri, "an entry without a secret needs no reveal");
    let monitor_url = stackly_ok(
        &root,
        &["instance", "connection", name, "--format", "monitoring"],
    );
    assert_eq!(monitor_url, format!("http://127.0.0.1:{monitoring}"));
    let all = stackly_ok(&root, &["instance", "connection", name, "--all"]);
    assert!(all.contains(&uri) && all.contains(&monitor_url), "{all}");
    let inspected = stackly(&root, &["instance", "inspect", name]);
    assert_eq!(inspected.code, Some(0), "{inspected:?}");
    assert!(inspected.stdout.contains(&uri), "{}", inspected.stdout);

    // A client that follows the string: connect and read the INFO line.
    let address = uri
        .strip_prefix("nats://")
        .expect("the uri string is a nats URL");
    let info = read_info(address);
    assert_eq!(info["server_name"], name, "{info}");
    assert_eq!(info["port"], client, "{info}");
    let version = info["version"]
        .as_str()
        .expect("INFO has a version")
        .to_owned();
    assert_eq!(version, "2.15.1");
    site.subject(ev, &version);

    let stopped = stackly(&root, &["instance", "stop", &id.to_string(), "--wait"]);
    assert_eq!(stopped.code, Some(0), "{stopped:?}");
    drop(daemon);
    ev.pass_with(&format!(
        "NATS {version} started from the CLI, was ready, and sent its INFO line to a client that connected to {address}{}",
        offline_note()
    ));
}

/// Connects to `address` (`127.0.0.1:port`) and returns the JSON of the
/// `INFO` line the server sends first.
fn read_info(address: &str) -> Value {
    let socket: SocketAddr = address.parse().expect("the address is host:port");
    assert_eq!(socket.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    let mut stream = TcpStream::connect_timeout(&socket, Duration::from_secs(5))
        .expect("the NATS port accepts a connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout can be set");
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .expect("the server sends INFO first");
    let _ = stream.write_all(b"QUIT\r\n");
    let body = line
        .strip_prefix("INFO ")
        .unwrap_or_else(|| panic!("the first line is not INFO: {line:?}"));
    serde_json::from_str(body.trim()).expect("INFO carries JSON")
}
