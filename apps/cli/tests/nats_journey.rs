//! The NATS journey from the CLI against a real `stacklyd` and a real NATS
//! 2.15.1 (spec 0023, milestone 4, build step 10, AC-13): `runtime install`
//! from the download cache, `instance create` with the defaults and with
//! JetStream off, `instance start`, `instance inspect <name> --json`, and
//! `instance stop`. The artifact comes from the runtime cache of `cargo
//! xtask` and is copied into the download cache of the test root, so the
//! install is a cache hit and nothing is downloaded; an artifact that is not
//! in the cache ends the test as `BLOCKED`.
//!
//! `instance start`, `stop`, and `get` take a full instance ID; `inspect`
//! and `connection` take a name or an ID. The test follows that.
//!
//! Native: `#[ignore]`, run with `cargo test -p stackly-cli --test
//! nats_journey -- --ignored`.
#![cfg(target_os = "macos")]

#[path = "common/mod.rs"]
mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_platform::DirectoryLayout;
use stackly_platform_macos::MacosPlatform;
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};
use stackly_testkit::{ReapGuard, RuntimeCache, SpawnCatalog, TestRoot, cargo_bin};

use crate::common::stacklyd::{closed_connections, handshakes};
use crate::common::{Run, envelope, paths, run};

/// The owner who can fill the runtime cache (spec 0002).
const OWNER: &str = "@ojiepermana";
/// The artifact of `providers/nats/versions.json` the journey installs.
const ARTIFACT: &str = "nats-2.15.1-macos-aarch64";
/// How long a wait of the CLI may take.
const WAIT: &str = "60s";

/// The `field` of [`ARTIFACT`] in `versions.json` of the NATS package.
fn artifact_field(field: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats/versions.json");
    let text = std::fs::read_to_string(&path).expect("versions.json can be read");
    let versions: Value = serde_json::from_str(&text).expect("versions.json is JSON");
    versions["artifacts"]
        .as_array()
        .and_then(|artifacts| artifacts.iter().find(|artifact| artifact["id"] == ARTIFACT))
        .map(|artifact| artifact[field].clone())
        .unwrap_or_else(|| panic!("versions.json has {ARTIFACT}"))
}

/// Builds the catalog of `providers/` into a folder of `root`.
fn built_catalog(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("catalog");
    let output = Command::new(cargo_bin("xtask", "xtask"))
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .args(["catalog", "build", "--out"])
        .arg(&dir)
        .output()
        .expect("xtask runs");
    assert!(
        output.status.success(),
        "the catalog build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}

/// Starts `stacklyd` with `catalog` and waits until it is ready.
fn start_daemon(root: &TestRoot, catalog: SpawnCatalog) -> stackly_testkit::SpawnedDaemon {
    let paths = paths(root);
    let closed_before = closed_connections(&paths.log);
    let daemon = stackly_testkit::SpawnedDaemon::start_with_catalog(
        &cargo_bin("stacklyd", "stacklyd"),
        root.path(),
        &paths.socket,
        &paths.runtime,
        catalog,
        |daemon| daemon.stderr().contains("registry_state") && handshakes(daemon.socket()),
    )
    .expect("stacklyd starts");
    let deadline = Instant::now() + Duration::from_secs(5);
    while closed_connections(&paths.log) <= closed_before {
        assert!(
            Instant::now() < deadline,
            "stacklyd did not log the close of the ready probe: {}",
            daemon.stderr()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon
}

/// Puts the cached artifact where the install looks for a download, so the
/// install is a cache hit.
fn fill_download_cache(root: &TestRoot, cache_file: &Path, sha256: &str) {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
    let layout = DirectoryLayout::new(platform.directories().roots().expect("roots resolve"));
    let downloads = layout.downloads_dir();
    assert!(
        downloads.is_dir(),
        "the daemon prepared {}",
        downloads.display()
    );
    let entry = downloads.join(sha256);
    std::fs::copy(cache_file, &entry).expect("the artifact can be copied");
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o600))
        .expect("the entry can be made private");
}

/// `stackly <args> --json` that has to exit 0; returns the `result`.
fn ok_json(root: &Path, args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.push("--json");
    let ran: Run = run(root, &all);
    assert_eq!(ran.code, Some(0), "stackly {args:?}: {ran:?}");
    envelope(&ran)["result"].clone()
}

/// Installs the NATS runtime and returns its ID.
fn install_runtime(root: &Path) -> String {
    let result = ok_json(
        root,
        &[
            "runtime",
            "install",
            "official/nats@2.15.1",
            "--wait",
            "--timeout",
            WAIT,
        ],
    );
    result["runtimeId"]
        .as_str()
        .unwrap_or_else(|| panic!("the install names the runtime: {result}"))
        .to_owned()
}

/// Creates an instance of `official/nats` with `extra` flags and returns its
/// ID.
fn create(root: &Path, name: &str, runtime: &str, extra: &[&str]) -> String {
    let mut args = vec![
        "instance",
        "create",
        name,
        "--runtime",
        runtime,
        "--provider",
        "official/nats",
        "--wait",
        "--timeout",
        WAIT,
    ];
    args.extend_from_slice(extra);
    let result = ok_json(root, &args);
    result["instance"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("the create names the instance: {result}"))
        .to_owned()
}

/// `inspect <name> --json` until the readiness is `ready`, so the test sees
/// what the daemon shows after `start --wait`.
fn inspect_ready(root: &Path, name: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let result = ok_json(root, &["instance", "inspect", name]);
        let instance = result["instance"].clone();
        if instance["readiness"]["state"] == "ready" {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "{name} stayed {}: {instance}",
            instance["readiness"]
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The port of the endpoint `name` in an `instance` JSON.
fn port_of(instance: &Value, name: &str) -> u16 {
    let port = instance["endpoints"]
        .as_array()
        .and_then(|endpoints| endpoints.iter().find(|endpoint| endpoint["name"] == name))
        .and_then(|endpoint| endpoint["port"].as_u64())
        .unwrap_or_else(|| panic!("the instance has a port for {name}: {instance}"));
    u16::try_from(port).expect("a port fits in 16 bits")
}

/// Connects to `port` on loopback and returns the JSON of the `INFO` line
/// the server sends first.
fn read_info(port: u16) -> Value {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
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

/// Starts, inspects, and stops one instance. Returns the INFO of the server
/// while it ran and the version it reported.
fn run_instance(root: &Path, id: &str, name: &str) -> Value {
    let before = ok_json(root, &["instance", "inspect", name]);
    assert_eq!(before["instance"]["lifecycle"], "stopped", "{before}");
    assert_eq!(before["instance"]["provider"]["name"], "official/nats");

    let started = run(
        root,
        &["instance", "start", id, "--wait", "--timeout", WAIT],
    );
    assert_eq!(started.code, Some(0), "{started:?}");

    let inspected = inspect_ready(root, name);
    let instance = &inspected["instance"];
    assert_eq!(instance["name"], name);
    assert_eq!(instance["id"], id);
    assert_eq!(instance["lifecycle"], "running", "{instance}");
    assert_eq!(instance["readiness"]["state"], "ready", "{instance}");
    assert_eq!(instance["readiness"]["reason"], "probe_ok", "{instance}");
    let client = port_of(instance, "client");
    let monitoring = port_of(instance, "monitoring");
    assert_ne!(client, monitoring);
    let info = read_info(client);

    let stopped = run(root, &["instance", "stop", id, "--wait", "--timeout", WAIT]);
    assert_eq!(stopped.code, Some(0), "{stopped:?}");
    let after = ok_json(root, &["instance", "inspect", name]);
    assert_eq!(after["instance"]["lifecycle"], "stopped", "{after}");
    assert_ne!(
        after["instance"]["readiness"]["state"], "ready",
        "a stopped instance is not ready: {after}"
    );
    info
}

fn note_subject(ev: &Evidence, version: &str, sha256: &str) {
    ev.subject("nats", version, Some(sha256), Source::Managed);
}

/// Spec 0023 AC-13, the CLI part.
#[test]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
fn nats_created_with_defaults_and_with_jetstream_off_starts_inspects_and_stops_from_the_cli() {
    let ev = evidence!(
        covers = ["REQ-029", "REQ-036", "AC-029"],
        kind = Kind::Native,
        expected = "against a real stacklyd and NATS 2.15.1, stackly instance create for official/nats with the defaults and with --set jetstream=false, instance start, instance inspect <name> --json with readiness ready and reason probe_ok, and instance stop all exit 0; the server named after the instance answers INFO with JetStream on for the defaults and off for the second instance, and a stopped instance is not ready",
    );
    let sha256 = artifact_field("sha256")
        .as_str()
        .expect("an artifact has a sha256")
        .to_owned();
    let size = artifact_field("size")
        .as_u64()
        .expect("an artifact has a size");
    let Some(cache_file) = RuntimeCache::open().and_then(|cache| cache.entry(&sha256, size)) else {
        ev.blocked(
            &format!("artifact {ARTIFACT} is not in the runtime cache"),
            OWNER,
        );
        return;
    };

    let root = TestRoot::new().expect("a test root");
    let catalog = built_catalog(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root, SpawnCatalog::Dir(catalog));
    fill_download_cache(&root, &cache_file, &sha256);
    let runtime = install_runtime(root.path());

    // The defaults: JetStream on, both endpoints Auto.
    let plain = create(root.path(), "bus", &runtime, &[]);
    let info = run_instance(root.path(), &plain, "bus");
    assert_eq!(info["server_name"], "bus", "{info}");
    assert_eq!(info["jetstream"], true, "{info}");
    let version = info["version"]
        .as_str()
        .expect("INFO has a version")
        .to_owned();
    assert_eq!(version, "2.15.1");
    note_subject(&ev, &version, &sha256);

    // JetStream off: the server must say so, and the readiness still passes.
    let light = create(
        root.path(),
        "light",
        &runtime,
        &["--set", "jetstream=false"],
    );
    let info = run_instance(root.path(), &light, "light");
    assert_eq!(info["server_name"], "light", "{info}");
    assert_ne!(
        info["jetstream"], true,
        "JetStream was switched off in the settings: {info}"
    );

    drop(daemon);
    ev.pass_with(&format!(
        "NATS {version} was created from the CLI with the defaults and with JetStream off, started until ready, inspected by name, and stopped; INFO showed JetStream on for the first and off for the second"
    ));
}
