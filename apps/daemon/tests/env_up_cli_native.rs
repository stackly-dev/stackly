//! `stackly env up` through the real `stackly` binary against the real
//! `stacklyd` binary with real NATS runtimes (spec 0046 AC-17, AC-18,
//! AC-25; spec 0044 AC-15; spec 0045 `--locked` and export). The runtime
//! comes from the runtime cache of `cargo xtask` and is installed into the
//! test root before the daemon starts, so nothing is downloaded: without
//! the artifact in the cache the test ends `BLOCKED`. The daemon runs with
//! the test root as `STACKLY_ROOT`, so its secrets go to the file keychain
//! in that root, never the login Keychain. Native:
//! `cargo test -p stacklyd --test env_up_cli_native -- --ignored`.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_core::provider::{ArtifactStatus, NameCheck, ValidatedPackage, read_package};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_core::{daemon_info, prepare_layout};
use stackly_ipc::{Client, ConnectPolicy};
use stackly_platform::DirectoryLayout;
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::evidence::{Kind, Source, evidence};
use stackly_testkit::{ReapGuard, RuntimeCache, SpawnCatalog, SpawnedDaemon, TestRoot, cargo_bin};

/// Who can fill the runtime cache (spec 0002).
const OWNER: &str = "user";

/// Two NATS services on `auto` ports; `events` waits for `bus`.
const MANIFEST: &str = "apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop

services:
  bus:
    provider: official/nats
    version: \"2.15\"
  events:
    provider: official/nats
    version: \"2.15\"
    dependsOn:
      - {service: bus}
";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The socket, the runtime root, and the data root of `root` as
/// `STACKLY_ROOT`.
fn layout(root: &TestRoot) -> DirectoryLayout {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
    DirectoryLayout::new(platform.directories().roots().expect("roots resolve"))
}

/// Builds the catalog of `providers/` into a folder of `root`.
fn built_catalog(root: &TestRoot) -> PathBuf {
    let dir = root.path().join("catalog");
    let output = Command::new(cargo_bin("xtask", "xtask"))
        .current_dir(repo())
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

/// Whether a client completes a handshake on `socket` right now.
fn handshakes(socket: &Path) -> bool {
    let socket = socket.to_path_buf();
    let uid = std::fs::metadata(env!("CARGO_MANIFEST_DIR"))
        .map(|metadata| std::os::unix::fs::MetadataExt::uid(&metadata))
        .expect("the package folder can be read");
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime starts")
            .block_on(async {
                let client = ClientInfo {
                    kind: ClientKind::Cli,
                    version: "0.1.0".to_owned(),
                    pid: Some(std::process::id()),
                    interactive: Some(false),
                };
                let connect = Client::connect(&socket, ConnectPolicy::same_user(uid), client);
                tokio::time::timeout(Duration::from_secs(5), connect)
                    .await
                    .is_ok_and(|connected| connected.is_ok())
            })
    })
    .join()
    .unwrap_or(false)
}

/// Starts the `stacklyd` binary with the catalog folder `catalog`.
fn start(root: &TestRoot, catalog: &Path) -> SpawnedDaemon {
    let layout = layout(root);
    SpawnedDaemon::start_with_catalog(
        Path::new(env!("CARGO_BIN_EXE_stacklyd")),
        root.path(),
        &layout.daemon_socket(),
        layout.roots().runtime(),
        SpawnCatalog::Dir(catalog.to_path_buf()),
        |daemon| daemon.stderr().contains("registry_state") && handshakes(daemon.socket()),
    )
    .expect("stacklyd starts")
}

/// What one run of `stackly` left.
#[derive(Debug)]
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs the `stackly` binary in `cwd` for `root`, with a clean
/// environment.
fn stackly(root: &TestRoot, cwd: &Path, args: &[&str]) -> Run {
    let output = Command::new(cargo_bin("stackly-cli", "stackly"))
        .current_dir(cwd)
        .args(args)
        .env("STACKLY_ROOT", root.path())
        .env("TZ", "UTC")
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("STACKLY_NO_INPUT")
        .env("TERM", "dumb")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("stackly runs");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("stdout is utf-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is utf-8"),
    }
}

fn json(run: &Run) -> Value {
    serde_json::from_str(run.stdout.trim()).unwrap_or_else(|error| {
        panic!("one JSON envelope: {error}: {run:?}");
    })
}

/// The bound port of each endpoint of each service in `env status --json`,
/// as `<alias> <endpoint> <port>`.
fn bound_ports(status: &Value) -> Vec<String> {
    let mut ports = Vec::new();
    for service in status["result"]["services"].as_array().expect("services") {
        let alias = service["alias"].as_str().expect("an alias");
        for endpoint in service["instance"]["endpoints"]
            .as_array()
            .expect("the instance's endpoints")
        {
            assert_eq!(endpoint["bound"], true, "{endpoint}");
            let port = endpoint["port"].as_u64().expect("a bound port");
            ports.push(format!(
                "{alias} {} {port}",
                endpoint["name"].as_str().expect("a name")
            ));
        }
    }
    ports
}

// covers: AC-17, AC-18, AC-25 (spec 0046); AC-15 (spec 0044)
#[test]
#[ignore = "native: brings up two NATS runtimes from the xtask runtime cache through the binaries"]
fn stackly_env_up_brings_two_nats_services_up_through_the_real_binaries() {
    let ev = evidence!(
        covers = ["REQ-021", "REQ-028", "AC-021"],
        kind = Kind::Native,
        expected = "with NATS installed from the xtask runtime cache and no download, stackly env up ./shop --wait against the stacklyd binary exits 0 with 2 of 2 ready. and real ports, env status shows Ready 2 of 2 ready with every endpoint bound on a real port that accepts a TCP connection, instance list names the environment shop for both instances, env export prints those ports and no reference or root path, env up --locked --wait after env lock exits 0 and leaves stackly.lock as it was, and the secrets live in the file keychain of the test root",
    );
    let root = TestRoot::new().expect("a test root");
    let layout = layout(&root);
    let _reap = ReapGuard::new(root.path(), layout.roots().runtime());
    let catalog = built_catalog(&root);
    let project = root.path().join("work/shop");
    std::fs::create_dir_all(&project).expect("the project folder can be made");
    std::fs::write(project.join("stackly.yaml"), MANIFEST).expect("the manifest writes");

    // The plan names the artifact; install it from the cache, daemon off.
    let daemon = start(&root, &catalog);
    let planned = stackly(&root, &project, &["env", "plan", "--json"]);
    daemon.stop().expect("stacklyd stops");
    assert_eq!(planned.code, Some(0), "{planned:?}");
    let plan = json(&planned);
    let runtime = &plan["result"]["items"][0]["runtime"];
    let artifact_id = runtime["artifactId"].as_str().expect("an artifact id");
    let sha256 = runtime["artifactSha256"]
        .as_str()
        .expect("an artifact digest");
    let version = runtime["version"].as_str().expect("a version");
    assert_eq!(
        plan["result"]["items"][1]["runtime"]["artifactSha256"], sha256,
        "both services take one artifact"
    );
    let files = read_package(
        &stackly_platform_macos::MacosFilesystem,
        &repo().join("providers/nats"),
    )
    .expect("the package reads");
    let package =
        ValidatedPackage::from_files(files, NameCheck::FolderName).expect("the package validates");
    let size = package
        .versions()
        .artifacts()
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .and_then(|artifact| match &artifact.status {
            ArtifactStatus::Available { size, .. } => Some(*size),
            _ => None,
        })
        .unwrap_or_else(|| panic!("versions.json has {artifact_id} available"));
    let Some(file) = RuntimeCache::open().and_then(|cache| cache.entry(sha256, size)) else {
        ev.blocked(
            &format!("artifact {artifact_id} is not in the xtask runtime cache"),
            OWNER,
        );
        return;
    };
    ev.subject("nats", version, Some(sha256), Source::Managed);
    {
        let platform = MacosPlatform::new(Some(root.path().into())).expect("a valid override");
        let prepared = prepare_layout(&platform).expect("the layout prepares");
        let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
            .open(&platform, prepared.layout(), daemon_info().version)
            .expect("the registry opens")
        else {
            panic!("the registry opens ready");
        };
        install_cached_artifact(
            &platform,
            &prepared,
            &registry,
            &package,
            artifact_id,
            &file,
        )
        .unwrap_or_else(|error| panic!("{artifact_id} installs from the cache: {error:?}"));
    }

    let _daemon = start(&root, &catalog);
    let started = Instant::now();
    let up = stackly(&root, root.path(), &["env", "up", "./work/shop", "--wait"]);
    assert_eq!(up.code, Some(0), "{up:?}");
    assert!(up.stdout.ends_with("2 of 2 ready.\n"), "{}", up.stdout);
    assert!(
        up.stderr.contains("ready shop-bus 127.0.0.1:"),
        "{}",
        up.stderr
    );
    assert!(started.elapsed() < Duration::from_secs(300));

    let status_text = stackly(&root, &project, &["env", "status"]);
    assert_eq!(status_text.code, Some(0), "{status_text:?}");
    let head = status_text.stdout.lines().next().unwrap_or_default();
    assert!(
        head.starts_with("shop  Ready  2 of 2 ready  "),
        "{}",
        status_text.stdout
    );
    assert!(
        status_text
            .stdout
            .contains("SERVICE  INSTANCE  VERSION  OWNERSHIP  STATUS  ENDPOINTS")
            || status_text.stdout.contains("ENDPOINTS"),
        "{}",
        status_text.stdout
    );
    let status = json(&stackly(&root, &project, &["env", "status", "--json"]));
    let ports = bound_ports(&status);
    assert_eq!(ports.len(), 4, "client and monitoring of each: {ports:?}");
    for line in &ports {
        let port: u16 = line
            .rsplit(' ')
            .next()
            .and_then(|port| port.parse().ok())
            .expect("a port");
        assert_ne!(port, 0);
        assert!(
            std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
            "{line} accepts a connection"
        );
        let shown = format!("127.0.0.1:{port}");
        assert!(
            status_text.stdout.contains(&shown),
            "{shown} in {}",
            status_text.stdout
        );
    }

    let list = stackly(&root, &project, &["instance", "list"]);
    assert_eq!(list.code, Some(0), "{list:?}");
    assert!(list.stdout.contains("ENVIRONMENT"), "{}", list.stdout);
    let members: Vec<&str> = list
        .stdout
        .lines()
        .filter(|line| line.starts_with("shop-"))
        .collect();
    assert_eq!(members.len(), 2, "{}", list.stdout);
    assert!(
        members.iter().all(|line| line.contains(" shop")),
        "{}",
        list.stdout
    );

    let export = stackly(&root, &project, &["env", "export", "shop"]);
    assert_eq!(export.code, Some(0), "{export:?}");
    for line in &ports {
        let port = line.rsplit(' ').next().expect("a port");
        assert!(export.stdout.contains(port), "{port} in {}", export.stdout);
    }
    assert!(!export.stdout.contains("local:"), "{}", export.stdout);
    assert!(
        !export.stdout.contains(&root.path().display().to_string()),
        "{}",
        export.stdout
    );

    let lock = stackly(&root, &project, &["env", "lock"]);
    assert_eq!(lock.code, Some(0), "{lock:?}");
    let lock_bytes = std::fs::read(project.join("stackly.lock")).expect("the lock reads");
    let locked = stackly(&root, &project, &["env", "up", "--locked", "--wait"]);
    assert_eq!(locked.code, Some(0), "{locked:?}");
    assert!(
        locked.stdout.ends_with("2 of 2 ready.\n"),
        "{}",
        locked.stdout
    );
    assert_eq!(
        std::fs::read(project.join("stackly.lock")).expect("the lock reads"),
        lock_bytes,
        "Up never writes stackly.lock"
    );

    // The secrets went to the file keychain of this root.
    assert!(
        layout.keychain_dir().join("stackly.keychain-db").is_file(),
        "the file keychain of the root exists"
    );
    ev.pass();
}
