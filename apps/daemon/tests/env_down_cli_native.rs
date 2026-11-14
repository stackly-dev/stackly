//! `stackly env down` through the real `stackly` binary against the real
//! `stacklyd` binary with a real NATS runtime (spec 0047 AC-5, AC-7,
//! AC-13, AC-18, AC-19, AC-22, PRD AC-022): two environments share one NATS
//! instance, Down of the first keeps it running for the second, a stop
//! without the override is refused, and Down of the second stops it, with
//! every file of the root byte for byte the same. The runtime comes from the
//! runtime cache of `cargo xtask` and is installed into the test root before
//! the daemon starts, so nothing is downloaded: without the artifact in the
//! cache the test ends `BLOCKED`. The daemon runs with the test root as
//! `STACKLY_ROOT`, so any secret goes to the file keychain in that root,
//! never the login Keychain. Native:
//! `cargo test -p stacklyd --test env_down_cli_native -- --ignored`.
#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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

/// `shop` creates one NATS service `bus`.
const SHOP: &str = "apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop

services:
  bus:
    provider: official/nats
    version: \"2.15\"
";

/// `blog` borrows the instance `shop` created.
const BLOG: &str = "apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: blog

services:
  bus:
    provider: official/nats
    instanceRef: shop-bus
";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn layout(root: &TestRoot) -> DirectoryLayout {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
    DirectoryLayout::new(platform.directories().roots().expect("roots resolve"))
}

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

#[derive(Debug)]
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

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

/// Every file under `root` with its bytes, outside logs, `run/`, and the
/// registry database Down writes by design, and the daemon output the
/// harness captures.
fn bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let text = path.to_string_lossy().into_owned();
            let name = entry.file_name().to_string_lossy().into_owned();
            if text.contains("/logs") || text.contains("/run") || text.contains("/Logs") {
                continue;
            }
            // The registry, and the output the harness captures from the
            // daemon process (a log).
            if name.starts_with("registry")
                || name.ends_with("-wal")
                || name.ends_with("-shm")
                || name.ends_with(".stderr")
                || name.ends_with(".stdout")
            {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file()
                && let Ok(content) = std::fs::read(&path)
            {
                files.insert(path, content);
            }
        }
    }
    files
}

/// The PID and the client port of `shop-bus` from `instance get --json`.
fn bus(root: &TestRoot) -> (Option<u64>, u16, String) {
    let got = json(&stackly(
        root,
        root.path(),
        &["instance", "get", "shop-bus", "--json"],
    ));
    let result = &got["result"]["instance"];
    let pid = result["process"]["pid"].as_u64();
    let port = result["endpoints"]
        .as_array()
        .expect("endpoints")
        .iter()
        .find(|endpoint| endpoint["name"] == "client")
        .and_then(|endpoint| endpoint["port"].as_u64())
        .and_then(|port| u16::try_from(port).ok())
        .expect("a client port");
    let lifecycle = result["lifecycle"].as_str().unwrap_or_default().to_owned();
    (pid, port, lifecycle)
}

fn accepts(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(2),
    )
    .is_ok()
}

// covers: AC-5, AC-7, AC-13, AC-18, AC-19, AC-22 (spec 0047), PRD AC-022
#[test]
#[ignore = "native: runs one NATS runtime from the xtask runtime cache through the binaries"]
fn stackly_env_down_keeps_a_shared_nats_instance_for_the_other_environment_then_stops_it() {
    let ev = evidence!(
        covers = ["AC-022", "REQ-022", "REQ-028", "BR-014", "BR-015", "BR-006"],
        kind = Kind::Native,
        expected = "with NATS installed from the xtask runtime cache and no download, shop creates shop-bus and blog borrows it through the real binaries; stackly env down shop --wait exits 0 writing keep shop-bus: used by blog and Down. 0 stopped, 1 kept running., the same PID keeps accepting TCP connections, env status shows lease: none for shop and lease: held on 1 instance for blog; stackly instance stop shop-bus exits 3 with shop-bus is used by blog (bus); stackly env down blog --wait exits 0 writing stopped shop-bus and the port refuses connections; every file of the root outside logs, run, and the registry keeps its bytes; env up shop --wait brings it back ready",
    );
    let root = TestRoot::new().expect("a test root");
    let layout = layout(&root);
    let _reap = ReapGuard::new(root.path(), layout.roots().runtime());
    let catalog = built_catalog(&root);
    let work = root.path().join("work");
    for (name, text) in [("shop", SHOP), ("blog", BLOG)] {
        let dir = work.join(name);
        std::fs::create_dir_all(&dir).expect("the project folder can be made");
        std::fs::write(dir.join("stackly.yaml"), text).expect("the manifest writes");
    }

    let daemon = start(&root, &catalog);
    let planned = stackly(&root, &work.join("shop"), &["env", "plan", "--json"]);
    daemon.stop().expect("stacklyd stops");
    assert_eq!(planned.code, Some(0), "{planned:?}");
    let plan = json(&planned);
    let runtime = &plan["result"]["items"][0]["runtime"];
    let artifact_id = runtime["artifactId"].as_str().expect("an artifact id");
    let sha256 = runtime["artifactSha256"]
        .as_str()
        .expect("an artifact digest");
    let version = runtime["version"].as_str().expect("a version");
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
    for name in ["shop", "blog"] {
        let up = stackly(&root, &work.join(name), &["env", "up", "--wait"]);
        assert_eq!(up.code, Some(0), "{name}: {up:?}");
        assert!(up.stdout.ends_with("1 of 1 ready.\n"), "{}", up.stdout);
    }
    let (pid, port, _) = bus(&root);
    let pid = pid.expect("shop-bus runs");
    assert!(accepts(port), "NATS accepts a connection on {port}");
    let files_before = bytes(root.path());

    // Down of shop keeps the instance blog borrows.
    let shop_down = stackly(&root, root.path(), &["env", "down", "shop", "--wait"]);
    assert_eq!(shop_down.code, Some(0), "{shop_down:?}");
    assert!(
        shop_down.stderr.contains("keep shop-bus: used by blog\n"),
        "{}",
        shop_down.stderr
    );
    assert_eq!(shop_down.stdout, "Down. 0 stopped, 1 kept running.\n");
    let (kept_pid, kept_port, lifecycle) = bus(&root);
    assert_eq!(kept_pid, Some(pid), "the same PID");
    assert_eq!(kept_port, port);
    assert_eq!(lifecycle, "running");
    assert!(accepts(port), "NATS still answers after Down of shop");
    let shop_status = stackly(&root, root.path(), &["env", "status", "shop"]);
    assert_eq!(shop_status.code, Some(0), "{shop_status:?}");
    assert_eq!(shop_status.stdout.lines().nth(1), Some("lease: none"));
    let blog_status = stackly(&root, root.path(), &["env", "status", "blog"]);
    let blog_lease = blog_status
        .stdout
        .lines()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    assert!(
        blog_lease.starts_with("lease: held on 1 instance since "),
        "{}",
        blog_status.stdout
    );

    // A stop without the override is refused, naming blog.
    let refused = stackly(&root, root.path(), &["instance", "stop", "shop-bus"]);
    assert_eq!(refused.code, Some(3), "{refused:?}");
    assert!(
        refused
            .stderr
            .starts_with("shop-bus is used by blog (bus)\n"),
        "{}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains("--override-leases")
            && refused.stderr.contains("stackly env down blog"),
        "{}",
        refused.stderr
    );
    assert!(accepts(port), "the refused stop changed nothing");

    // Down of blog, the last holder, stops it.
    let blog_down = stackly(
        &root,
        root.path(),
        &["env", "down", "blog", "--wait", "--json"],
    );
    assert_eq!(blog_down.code, Some(0), "{blog_down:?}");
    let envelope = json(&blog_down);
    assert_eq!(envelope["result"]["operation"]["state"], "succeeded");
    let stopped_ids = envelope["result"]["preview"]["stop"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    assert_eq!(stopped_ids, 1, "{envelope}");
    let (gone_pid, _, lifecycle) = bus(&root);
    assert_eq!(gone_pid, None, "no process");
    assert_eq!(lifecycle, "stopped");
    assert!(!accepts(port), "the port refuses connections");

    let files_after = bytes(root.path());
    let mut changed = Vec::new();
    for (path, content) in &files_before {
        match files_after.get(path) {
            Some(now) if now == content => {}
            Some(_) => changed.push(format!("{} changed", path.display())),
            None => changed.push(format!("{} is gone", path.display())),
        }
    }
    assert!(
        changed.is_empty(),
        "Down deletes or changes nothing: {changed:?}"
    );

    // Up again brings the same instance back ready.
    let again = stackly(&root, &work.join("shop"), &["env", "up", "--wait"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    let (back_pid, back_port, lifecycle) = bus(&root);
    assert!(back_pid.is_some());
    assert_eq!(lifecycle, "running");
    assert!(accepts(back_port), "NATS answers again");
    let list = stackly(&root, root.path(), &["instance", "list"]);
    assert_eq!(
        list.stdout
            .lines()
            .filter(|line| line.starts_with("shop-bus"))
            .count(),
        1,
        "one instance: {}",
        list.stdout
    );
    ev.pass_with(&format!(
        "nats {version} from the cache; env down shop --wait exited 0 with keep shop-bus: used by blog and Down. 0 stopped, 1 kept running.; PID {pid} kept answering on port {port}; env status shop read lease: none and blog read {blog_lease}; instance stop shop-bus exited 3 with shop-bus is used by blog (bus); env down blog --wait --json exited 0 succeeded and the port refused connections; {} files kept their bytes; env up shop --wait brought it back on port {back_port}",
        files_before.len()
    ));
}
