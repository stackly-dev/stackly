//! The native harness of the NATS provider (spec 0023, step 7): a test root
//! with a data root whose name holds a space and Unicode, a real NATS
//! runtime installed from the runtime cache of `cargo xtask`, the
//! `official/nats` package pinned, and a daemon either in process or as the
//! `stacklyd` binary. Nothing here downloads anything: an artifact that is
//! not in the cache ends the test as `BLOCKED`.
#![allow(
    dead_code,
    reason = "each scenario module uses a part of these helpers"
)]

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_core::supervisor::fixture::pin_package;
use stackly_core::{PreparedLayout, daemon_info, prepare_layout};
use stackly_domain::{Registry, WriteBatch, WriteOp};
use stackly_platform::{Clock, Platform, RootSet, SystemClock};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::Timestamp;
use stackly_protocol::events::EventKind;
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, InstanceReadinessState, InstanceSummary, PortRequest,
    ProviderSelector, SettingsDocument,
};
use stackly_protocol::json::DecodedFrame;
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::evidence::{Evidence, Source};
use stackly_testkit::{
    FakeSecretStore, ReapGuard, RuntimeCache, SpawnCatalog, SpawnedDaemon, TestRoot, allocate_port,
};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::server::{self, DaemonConfig, DaemonHandle};
use uuid::Uuid;

use crate::common::{self, STACKLYD};
use crate::support::{Api, PATIENCE, fast};

/// The owner who can fill the runtime cache (spec 0002).
pub const OWNER: &str = "@ojiepermana";
/// The provider of the package.
pub const PROVIDER: &str = "official/nats";
/// The artifact of `providers/nats/versions.json` the tests install.
pub const ARTIFACT: &str = "nats-2.15.1-macos-aarch64";
/// The name of the data root: a space and Unicode, so `store_dir` in
/// `nats.conf` has to quote them.
pub const ROOT_NAME: &str = "Test Root Ünï cödé";

/// Where the daemon of a test runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Serve {
    /// In this process, through `stacklyd::server::start`.
    InProcess,
    /// As the `stacklyd` binary, with `STACKLY_ROOT` on the data root.
    Binary,
}

/// The folder of the package under test.
#[must_use]
pub fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats")
}

/// Reads and validates the package under test.
#[must_use]
pub fn package(platform: &Platform) -> ValidatedPackage {
    let files = read_package(platform.fs(), &package_dir()).expect("the package reads");
    ValidatedPackage::from_files(files, NameCheck::Skip).expect("the package validates")
}

/// The `field` of [`ARTIFACT`] in `versions.json`.
fn artifact_field(field: &str) -> Value {
    let text = std::fs::read_to_string(package_dir().join("versions.json"))
        .expect("versions.json can be read");
    let versions: Value = serde_json::from_str(&text).expect("versions.json is JSON");
    versions["artifacts"]
        .as_array()
        .and_then(|artifacts| artifacts.iter().find(|artifact| artifact["id"] == ARTIFACT))
        .map(|artifact| artifact[field].clone())
        .unwrap_or_else(|| panic!("versions.json has {ARTIFACT}"))
}

/// The SHA-256 of [`ARTIFACT`] in `versions.json`.
#[must_use]
pub fn artifact_sha256() -> String {
    artifact_field("sha256")
        .as_str()
        .expect("an artifact has a sha256")
        .to_owned()
}

/// The size in bytes of [`ARTIFACT`] in `versions.json`.
#[must_use]
pub fn artifact_size() -> u64 {
    artifact_field("size")
        .as_u64()
        .expect("an artifact has a size")
}

/// One test's root with the runtime installed and the package pinned.
pub struct Site {
    /// Declared first, so it drops first: it stops the processes that name
    /// this root, then removes the runtime root.
    _reap: ReapGuard,
    pub root: TestRoot,
    /// `<test root>/Test Root Ünï cödé`, the data root of the daemon.
    pub data_root: PathBuf,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    pub secrets: Arc<FakeSecretStore>,
    pub runtime_id: Uuid,
    pub runtime_dir: PathBuf,
    pub artifact_sha256: String,
    /// The `provider.revision` of a create, from the record `pin_package`
    /// returned (spec 0010, K1).
    pub revision: String,
    /// The key of the pinned revision.
    pub revision_key: stackly_domain::ProviderRevisionKey,
    /// The digest of the pinned `provider.yaml`.
    pub manifest_sha256: String,
}

impl Site {
    /// Installs NATS from the runtime cache into a new root and pins the
    /// package. Notes `evidence` as `BLOCKED` and returns `None` when the
    /// cache does not hold the artifact, so the test ends there.
    pub fn open(evidence: &Evidence, serve: Serve) -> Option<Self> {
        let root = TestRoot::new().expect("a test root can be created");
        let data_root = root.path().join(ROOT_NAME);
        // A root override must exist; the daemon creates everything below it.
        std::fs::create_dir(&data_root).expect("the data root can be created");
        std::fs::set_permissions(&data_root, std::fs::Permissions::from_mode(0o700))
            .expect("the data root can be made private");
        let secrets = Arc::new(FakeSecretStore::new());
        let platform = match serve {
            Serve::InProcess => MacosPlatform::with_roots(
                RootSet::under_override(&data_root, root.run_dir())
                    .expect("a test root is a valid root override"),
            ),
            Serve::Binary => MacosPlatform::new(Some(data_root.clone().into())),
        }
        .expect("a test root is a valid root override")
        .with_secrets(Arc::clone(&secrets) as Arc<dyn stackly_platform::SecretStore>);
        let package = package(&platform);
        let sha256 = artifact_sha256();
        let Some(cache_file) =
            RuntimeCache::open().and_then(|cache| cache.entry(&sha256, artifact_size()))
        else {
            evidence.blocked(
                &format!("artifact {ARTIFACT} is not in the runtime cache"),
                OWNER,
            );
            return None;
        };
        let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
        let reap = ReapGuard::new(
            root.path(),
            platform
                .directories()
                .roots()
                .expect("the macOS platform resolved its roots")
                .runtime(),
        );
        let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
            .open(&platform, prepared.layout(), daemon_info().version)
            .expect("a fresh registry opens")
        else {
            panic!("a fresh registry opens ready");
        };
        let (runtime, record) = install_cached_artifact(
            &platform,
            &prepared,
            &registry,
            &package,
            ARTIFACT,
            &cache_file,
        )
        .expect("the cached artifact installs, exec check included");
        let pinned = pin_package(&platform, prepared.layout(), &package_dir())
            .expect("the package can be pinned");
        let revision_key = pinned.revision.key().clone();
        let revision = revision_key.revision().to_string();
        let manifest_sha256 = revision_key.manifest_sha256().to_string();
        registry
            .commit(WriteBatch::new(vec![WriteOp::InsertProviderRevision(
                pinned,
            )]))
            .expect("the provider revision can be committed");
        // The registry closes here, so the daemon opens it alone.
        drop(registry);
        let runtime_dir = prepared.layout().runtime_dir(
            runtime.software(),
            runtime.version(),
            runtime.target(),
            &record.sha256,
        );
        Some(Self {
            _reap: reap,
            root,
            data_root,
            platform,
            prepared,
            secrets,
            runtime_id: runtime.id().as_uuid(),
            runtime_dir,
            artifact_sha256: sha256,
            revision,
            revision_key,
            manifest_sha256,
        })
    }

    /// The `nats-server` of the installed runtime.
    #[must_use]
    pub fn server_bin(&self) -> PathBuf {
        let bin = self.runtime_dir.join("nats-server");
        assert!(bin.is_file(), "no nats-server at {}", bin.display());
        bin
    }

    /// The paths of the instance `id`.
    #[must_use]
    pub fn instance_paths(&self, id: Uuid) -> stackly_platform::InstancePaths {
        self.prepared.layout().instance_paths(
            id.to_string()
                .parse()
                .expect("the daemon shows an instance id"),
        )
    }

    /// Notes the running server as the subject of `evidence`, with the
    /// version `GET /varz` reported.
    pub fn subject(&self, evidence: &Evidence, server_version: &str) {
        evidence.subject(
            "nats",
            server_version,
            Some(&self.artifact_sha256),
            Source::Managed,
        );
    }

    /// Starts a daemon in this process, with the supervisor constants of
    /// the tests and `config` adjusted by `tweak`.
    pub async fn in_process(&self, tweak: impl FnOnce(&mut DaemonConfig)) -> InProcess {
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            "0.1.0",
        );
        let lock = DaemonLock::acquire(&self.prepared.layout().daemon_lock(), owner)
            .expect("the test root's lock is free");
        let mut config = fast();
        tweak(&mut config);
        let log = stacklyd::log_sink::LogSink::start(
            Box::new(std::io::sink()),
            stacklyd::log_sink::LogSinkConfig::DAEMON,
        )
        .expect("the log writer starts");
        let facts = stacklyd::map::StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            true,
            std::process::id(),
        );
        let deps = server::DaemonDeps::new(
            self.platform.clone(),
            self.prepared.clone(),
            facts,
            Arc::new(log),
        )
        .with_config(config);
        let handle = server::start(deps).await.expect("the server starts");
        handle.started().await;
        InProcess {
            handle,
            _lock: lock,
        }
    }

    /// A typed client of the in process daemon.
    pub async fn api(&self) -> Api {
        let (client, _) = stackly_ipc::Client::connect(
            &self.prepared.layout().daemon_socket(),
            stackly_ipc::ConnectPolicy::same_user(common::uid()),
            stackly_protocol::handshake::ClientInfo {
                kind: stackly_protocol::handshake::ClientKind::Cli,
                version: "0.1.0".to_owned(),
                pid: Some(std::process::id()),
                interactive: Some(true),
            },
        )
        .await
        .expect("the client connects");
        Api::new(client, self.runtime_id)
    }

    /// Starts the `stacklyd` binary on the data root with the default
    /// supervisor constants (a probe every 10 seconds), and waits until it
    /// answers a handshake. Only one daemon runs at a time, so a test that
    /// restarts after `SIGKILL` calls this again.
    pub fn spawn(&self) -> SpawnedDaemon {
        let socket = self.prepared.layout().daemon_socket();
        let runtime_root = self
            .platform
            .directories()
            .roots()
            .expect("the macOS platform resolved its roots")
            .runtime()
            .to_path_buf();
        SpawnedDaemon::start_with_catalog(
            Path::new(STACKLYD),
            &self.data_root,
            &socket,
            &runtime_root,
            SpawnCatalog::Unset,
            |daemon| {
                let stderr = daemon.stderr();
                common::logged(&stderr, "registry_state") && common::handshakes(daemon.socket())
            },
        )
        .expect("stacklyd starts")
    }

    /// A typed client of the binary daemon, once its start slot ended.
    pub async fn binary_api(&self, daemon: &SpawnedDaemon) -> Api {
        let (client, _) = common::client(daemon.socket()).await;
        let api = Api::new(client, self.runtime_id);
        // The socket answers a handshake while the supervisor still
        // reconciles, and the instance methods say `daemon.starting` until
        // it is done.
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            match api
                .client
                .call(stackly_protocol::Call::InstanceList(Default::default()))
                .await
            {
                Ok(_) => break,
                Err(error) if error.code() == "daemon.starting" => {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "the daemon stayed in starting"
                    );
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => panic!("instance.list: {error:?}"),
            }
        }
        api
    }
}

/// A daemon running in this process.
pub struct InProcess {
    pub handle: DaemonHandle,
    _lock: DaemonLock,
}

impl InProcess {
    /// Stops the daemon the way `daemon.shutdown` does. Instance processes
    /// keep running, as they do after any daemon stop.
    pub async fn shutdown(self) {
        self.handle
            .shutdown(
                stackly_protocol::events::ShutdownReason::Requested,
                std::future::pending(),
            )
            .await;
    }
}

/// The params of a create of the NATS package, both endpoints `Auto`.
#[must_use]
pub fn create_params(site: &Site, name: &str, settings: Value) -> CreateInstanceParams {
    let map = match settings {
        Value::Object(map) => map,
        Value::Null => serde_json::Map::new(),
        other => panic!("settings must be an object, not {other}"),
    };
    CreateInstanceParams {
        name: name.to_owned(),
        display_name: None,
        provider: ProviderSelector {
            name: PROVIDER.to_owned(),
            revision: Some(site.revision.clone()),
        },
        runtime_id: site.runtime_id,
        endpoints: ["client", "monitoring"]
            .into_iter()
            .map(|endpoint| EndpointRequest {
                name: endpoint.to_owned(),
                port: PortRequest::Auto,
            })
            .collect(),
        settings: SettingsDocument::new(map),
        secrets: Vec::new(),
    }
}

/// Creates an instance and waits until the create (the init) succeeded.
/// Returns the instance ID.
pub async fn create_instance(site: &Site, api: &Api, name: &str, settings: Value) -> Uuid {
    let accepted = api
        .create(create_params(site, name, settings))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    let detail = api
        .wait_state(
            accepted.operation.id,
            stackly_protocol::operation::OperationState::Succeeded,
        )
        .await;
    assert_eq!(
        detail.operation.state,
        stackly_protocol::operation::OperationState::Succeeded,
        "{detail:?}"
    );
    id
}

/// Stops the instance and waits until the stop succeeded.
pub async fn stop_instance(api: &Api, id: Uuid) {
    let accepted = api
        .stop_when_free(id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
}

/// The effective port of the endpoint `name` of `summary`.
#[must_use]
pub fn port_of(summary: &InstanceSummary, name: &str) -> u16 {
    summary
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == name)
        .and_then(|endpoint| endpoint.port)
        .unwrap_or_else(|| panic!("the instance has a port for endpoint {name}: {summary:#?}"))
}

/// `GET /varz` on a monitoring port, over plain HTTP from the test.
#[must_use]
pub fn varz(port: u16) -> Value {
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        Duration::from_secs(5),
    )
    .expect("the monitoring port accepts");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout can be set");
    // HTTP/1.0 keeps the reply free of chunked framing.
    stream
        .write_all(b"GET /varz HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .expect("the request is written");
    let mut reply = Vec::new();
    stream
        .read_to_end(&mut reply)
        .expect("the reply is read to the end");
    let text = String::from_utf8_lossy(&reply);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no HTTP head in {text}"));
    assert!(
        head.starts_with("HTTP/1.0 200") || head.starts_with("HTTP/1.1 200"),
        "{head}"
    );
    serde_json::from_str(body).unwrap_or_else(|error| panic!("/varz is not JSON: {error}"))
}

/// Whether `kill(pid, 0)` succeeds, that is, the process exists.
#[must_use]
pub fn alive(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .expect("kill runs")
        .success()
}

/// Sends `signal` (`STOP`, `CONT`, ...) to `pid`.
pub fn signal(pid: u32, signal: &str) {
    let status = Command::new("/bin/kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "kill -{signal} {pid} failed");
}

/// Sends `SIGCONT` to a process on drop, so a test that stopped a server
/// never leaves it stopped, also when an assertion failed. Declare it after
/// the daemons, so it drops before them.
pub struct ContGuard(pub u32);

impl Drop for ContGuard {
    fn drop(&mut self) {
        let _ = Command::new("/bin/kill")
            .args(["-CONT", &self.0.to_string()])
            .status();
    }
}

/// Waits until `done` is true, polling every 50 ms.
pub async fn eventually(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + limit;
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "no {what} in time");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Runs `command` with `args` and returns its standard output.
#[must_use]
pub fn output_of(command: &str, args: &[&str]) -> String {
    let output = Command::new(command)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{command} runs: {error}"));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The listeners of the process `pid`: every `host:port` that `lsof` shows
/// with `(LISTEN)`, sorted, and the number of UDP sockets it holds.
#[must_use]
pub fn listeners(pid: u32) -> (Vec<String>, usize) {
    let shown = output_of(
        "/usr/sbin/lsof",
        &["-nP", "-a", "-p", &pid.to_string(), "-i"],
    );
    let mut listening: Vec<String> = shown
        .lines()
        .filter(|line| line.ends_with("(LISTEN)"))
        .filter_map(|line| line.split_whitespace().rev().nth(1).map(str::to_owned))
        .collect();
    listening.sort();
    let udp = shown.lines().filter(|line| line.contains(" UDP ")).count();
    (listening, udp)
}

/// A `nats-server` of the installed runtime that no daemon owns, bound to
/// `127.0.0.1` on a free port. Its process is killed and waited for on drop.
pub struct Foreign {
    child: Option<Child>,
    /// The client port.
    pub port: u16,
}

impl Foreign {
    /// Starts `bin` as `-a 127.0.0.1 -p <port> -n <name>` plus `extra`, no
    /// monitoring, and waits until the port accepts a connection.
    pub fn start(bin: &Path, name: &str, extra: &[&str]) -> Self {
        let port = allocate_port().expect("a free port can be allocated");
        let child = Command::new(bin)
            .args(["-a", "127.0.0.1", "-p", &port.to_string(), "-n", name])
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nats-server starts");
        let foreign = Self {
            child: Some(child),
            port,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(
                Instant::now() < deadline,
                "the foreign nats-server did not listen"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        foreign
    }
}

impl Drop for Foreign {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The checks `nats_compatibility` proved, in the order they passed. A name
/// is added only after its assertions passed.
#[derive(Debug, Default)]
pub struct Checks(Vec<&'static str>);

impl Checks {
    /// Notes that `name` passed.
    pub fn passed(&mut self, name: &'static str) {
        assert!(!self.0.contains(&name), "check {name} passed twice");
        self.0.push(name);
    }

    /// Writes `compatibility.json` into the artifact folder of `evidence`
    /// (spec 0018, *Compatibility record*): the final `provider.yaml` of the
    /// run, the checks in alphabetical order, and what is not proven yet.
    pub fn write(&self, evidence: &Evidence, site: &Site) {
        let mut checks = self.0.clone();
        checks.sort_unstable();
        let file = json!({
            "schema": "stackly.compatibility-evidence/v1",
            "providerRevision": {
                "id": PROVIDER,
                "revision": site.revision,
                "manifestSha256": site.manifest_sha256,
            },
            "checks": checks,
            "knownLimitations": [
                "client_auth_not_configured",
                "jetstream_persistence_not_covered",
                "multi_instance_not_covered",
            ],
        });
        let path = evidence.artifact_dir().join("compatibility.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&file).expect("the file is JSON"),
        )
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    }
}

/// One readiness observation of an `instance.health_changed` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    /// The `statusSeq` of the event.
    pub seq: u64,
    /// The readiness state.
    pub state: InstanceReadinessState,
    /// The reason code.
    pub reason: String,
    /// The detail, if the probe gave one.
    pub detail: Option<String>,
}

/// Reads `events` until 500 ms pass without one and returns the readiness
/// observations of the instance `id` in the order of their `statusSeq`.
pub async fn drain_readiness(events: &mut stackly_ipc::EventStream, id: Uuid) -> Vec<Observed> {
    let mut seen = Vec::new();
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_millis(500), events.next()).await
    {
        let DecodedFrame::Event(event) = frame else {
            continue;
        };
        if let EventKind::InstanceHealthChanged(changed) = event.kind
            && changed.instance_id == id
        {
            seen.push(Observed {
                seq: changed.status_seq.get(),
                state: changed.readiness.state,
                reason: changed.readiness.reason.clone(),
                detail: changed.readiness.detail.clone(),
            });
        }
    }
    assert!(
        seen.windows(2).all(|pair| pair[0].seq < pair[1].seq),
        "statusSeq only goes up: {seen:?}"
    );
    seen
}

/// Asserts that the first `ready` of `seen` comes after a `not_ready` and
/// has reason `probe_ok`: the readiness the daemon showed while the server
/// started was honest.
pub fn assert_not_ready_before_ready(seen: &[Observed]) {
    let ready = seen
        .iter()
        .position(|observed| observed.state == InstanceReadinessState::Ready)
        .unwrap_or_else(|| panic!("no ready observation in {seen:#?}"));
    assert!(
        seen[..ready]
            .iter()
            .any(|observed| observed.state == InstanceReadinessState::NotReady),
        "ready without a not_ready before it: {seen:#?}"
    );
    assert_eq!(seen[ready].reason, "probe_ok", "{seen:#?}");
}
