//! The native harness of the ClickHouse provider (spec 0043, step 6): a test
//! root with a data root whose name holds a space and Unicode, the managed
//! ClickHouse 26.8.30.8 runtime installed from the runtime cache of
//! `cargo xtask`, the `official/clickhouse` package pinned, a daemon in this
//! process, SQL through `clickhouse client` and the HTTP interface, `lsof`
//! helpers, and foreign ClickHouse servers that no daemon owns, reaped on
//! drop. Nothing here downloads anything: an artifact that is not in the
//! cache ends the test as `BLOCKED`.
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

use serde_json::Value;
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_core::supervisor::fixture::pin_package;
use stackly_core::{PreparedLayout, daemon_info, prepare_layout};
use stackly_domain::{InstanceId, Registry, WriteBatch, WriteOp};
use stackly_platform::{Clock, Platform, RootSet, SystemClock};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::Timestamp;
use stackly_protocol::events::EventKind;
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, InstanceReadinessState, InstanceSummary, PortRequest,
    ProviderSelector, SecretFieldInput, SettingsDocument,
};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::logs::{LogEntryKind, LogsReadParams};
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::{Call, Reply};
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::evidence::{Evidence, Source};
use stackly_testkit::{
    FakeSecretStore, ReapGuard, RuntimeCache, SpawnCatalog, SpawnedDaemon, TestRoot, allocate_port,
};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::server::{self, DaemonConfig, DaemonHandle};
use uuid::Uuid;

use crate::common::{self, STACKLYD};
use crate::support::{Api, fast};

/// The owner who can fill the runtime cache (spec 0002).
pub const OWNER: &str = "@ojiepermana";
/// The provider of the package.
pub const PROVIDER: &str = "official/clickhouse";
/// The artifact of `providers/clickhouse/versions.json` the tests install.
pub const ARTIFACT: &str = "clickhouse-26.8.30.8-macos-aarch64";
/// The exact version of [`ARTIFACT`].
pub const VERSION: &str = "26.8.30.8";
/// The reason of a test that finds no artifact in the cache (spec 0043,
/// *Test native*).
pub const BLOCKED_REASON: &str =
    "the managed ClickHouse 26.8.30.8 artifact is not in the runtime cache";
/// The name of the data root: a space and Unicode, so every path in
/// `config.xml` carries them.
pub const ROOT_NAME: &str = "Test Root Ünï cödé";
/// The secret field of the package that holds the default user password.
pub const PASSWORD_FIELD: &str = "defaultPassword";
/// The readiness user of the package.
pub const READINESS_USER: &str = "stackly_readiness";
/// The reload user of the package.
pub const RELOAD_USER: &str = "stackly_reload";
/// The macro that names the instance in `config.xml`.
pub const ID_MACRO: &str = "stackly_instance_id";
/// How long a binary daemon may stay in `daemon.starting`.
const PATIENCE: Duration = Duration::from_secs(60);

/// Where the daemon of a test runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Serve {
    /// In this process, through `stacklyd::server::start`, with a
    /// `FakeSecretStore`.
    InProcess,
    /// As the `stacklyd` binary with `STACKLY_ROOT` on the data root, so
    /// secrets live in the file keychain under that data root.
    Binary,
}

/// The folder of the package under test.
#[must_use]
pub fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/clickhouse")
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

/// An `InstanceId` from the UUID the API shows.
#[must_use]
pub fn instance_id(id: Uuid) -> InstanceId {
    id.to_string().parse().expect("an instance id")
}

/// A fresh fixture password.
#[must_use]
pub fn new_password() -> String {
    stackly_testkit::evidence::FixtureCredential::new()
        .value()
        .to_owned()
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
    /// The `provider.revision` of a create.
    pub revision: String,
    /// The digest of the pinned `provider.yaml`.
    pub manifest_sha256: String,
    /// The folder the test's own `clickhouse client` runs in: its `HOME`,
    /// its `TMPDIR`, and an empty client config.
    pub client_home: PathBuf,
}

impl Site {
    /// Installs ClickHouse from the runtime cache into a new root and pins
    /// the package. Notes `evidence` as `BLOCKED` and returns `None` when
    /// the cache does not hold the artifact, so the test ends there.
    pub fn open(evidence: &Evidence) -> Option<Self> {
        Self::open_with(evidence, ROOT_NAME, Serve::InProcess)
    }

    /// [`Site::open`] with a data root named `root_name` and a daemon that
    /// runs as `serve` says.
    pub fn open_with(evidence: &Evidence, root_name: &str, serve: Serve) -> Option<Self> {
        let root = TestRoot::new().expect("a test root can be created");
        let data_root = root.path().join(root_name);
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
            evidence.blocked(BLOCKED_REASON, OWNER);
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
        let client_home = root.path().join("client home");
        std::fs::create_dir_all(client_home.join("tmp")).expect("the client home can be made");
        std::fs::write(client_home.join("client.xml"), "<config>\n</config>\n")
            .expect("the empty client config can be written");
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
            revision: revision_key.revision().to_string(),
            manifest_sha256: revision_key.manifest_sha256().to_string(),
            client_home,
        })
    }

    /// The `clickhouse` binary of the installed runtime.
    #[must_use]
    pub fn bin(&self) -> PathBuf {
        let bin = self.runtime_dir.join("clickhouse");
        assert!(bin.is_file(), "no clickhouse at {}", bin.display());
        bin
    }

    /// The paths of the instance `id`.
    #[must_use]
    pub fn instance_paths(&self, id: Uuid) -> stackly_platform::InstancePaths {
        self.prepared.layout().instance_paths(instance_id(id))
    }

    /// The `users.xml` of the instance `id`, in its secrets folder.
    #[must_use]
    pub fn users_xml(&self, id: Uuid) -> PathBuf {
        self.instance_paths(id).run.join("secrets/users.xml")
    }

    /// Notes the running server as the subject of `evidence`, with the
    /// version `SELECT version()` reported.
    pub fn subject(&self, evidence: &Evidence, server_version: &str) {
        evidence.subject(
            "clickhouse",
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

    /// Starts the `stacklyd` binary on the data root, which keeps secrets
    /// in a file keychain under it, and waits until it answers a
    /// handshake. Only for a site opened with [`Serve::Binary`].
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
                .call(Call::InstanceList(Default::default()))
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

    /// Runs `query` through the runtime's own `clickhouse client` on the
    /// native `port` as `user`, the way the test (not the daemon) does: an
    /// empty client config, a `HOME` in the test root, nothing inherited,
    /// and the fixture `password` in `CLICKHOUSE_PASSWORD` of that child.
    #[must_use]
    pub fn sql(&self, port: u16, user: &str, password: Option<&str>, query: &str) -> Sql {
        let mut command = Command::new(self.bin());
        command
            .arg("client")
            .arg("--config-file")
            .arg(self.client_home.join("client.xml"))
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--user", user, "--query", query])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.client_home)
            .env("TMPDIR", self.client_home.join("tmp"))
            .current_dir(&self.client_home)
            .stdin(Stdio::null());
        if let Some(password) = password {
            command.env("CLICKHOUSE_PASSWORD", password);
        }
        let output = command.output().expect("clickhouse client runs");
        Sql {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// [`Site::sql`] that must succeed, and its trimmed output.
    #[must_use]
    pub fn sql_ok(&self, port: u16, user: &str, password: Option<&str>, query: &str) -> String {
        let done = self.sql(port, user, password, query);
        assert!(done.ok(), "{query} as {user}: {done:?}");
        done.stdout.trim().to_owned()
    }

    /// [`Site::sql`] that must succeed, for a statement with no output.
    pub fn exec(&self, port: u16, user: &str, password: Option<&str>, query: &str) {
        let done = self.sql(port, user, password, query);
        assert!(done.ok(), "{query} as {user}: {done:?}");
    }

    /// The server version through the HTTP interface.
    #[must_use]
    pub fn version(&self, http: u16, password: &str) -> String {
        let (status, body) = http_query(http, "default", password, "SELECT version()");
        assert_eq!(status, 200, "{body}");
        body.trim().to_owned()
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

/// What one `clickhouse client` run ended with.
#[derive(Debug, Clone)]
pub struct Sql {
    /// The exit code, `None` after a signal.
    pub code: Option<i32>,
    /// What it printed on stdout.
    pub stdout: String,
    /// What it printed on stderr.
    pub stderr: String,
}

impl Sql {
    /// Whether the client exited 0.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// The params of a create of the ClickHouse package: both endpoints `Auto`
/// unless `native` names a fixed port, and `password` as the value of the
/// default user password.
#[must_use]
pub fn create_params(
    site: &Site,
    name: &str,
    password: &str,
    native: Option<u16>,
    settings: Value,
) -> CreateInstanceParams {
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
        endpoints: vec![
            EndpointRequest {
                name: "http".to_owned(),
                port: PortRequest::Auto,
            },
            EndpointRequest {
                name: "native".to_owned(),
                port: native.map_or(PortRequest::Auto, PortRequest::Fixed),
            },
        ],
        settings: SettingsDocument::new(map),
        secrets: vec![SecretFieldInput {
            field: PASSWORD_FIELD.to_owned(),
            input: SecretInput::Value(SecretText::new(password.to_owned())),
        }],
        startup: None,
        duplicate_of: None,
    }
}

/// An instance a test created.
#[derive(Debug, Clone)]
pub struct Made {
    /// The instance.
    pub id: Uuid,
    /// The password of the `default` user.
    pub password: String,
}

/// Creates an instance from `params` and waits until the create (the init)
/// succeeded.
pub async fn create_from(api: &Api, params: CreateInstanceParams) -> Made {
    let password = match &params.secrets[0].input {
        SecretInput::Value(text) => text.expose().to_owned(),
        other => panic!("the harness always gives the password: {other:?}"),
    };
    let accepted = api
        .create(params)
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
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    Made { id, password }
}

/// Creates an instance with a new fixture password, both endpoints `Auto`,
/// and `settings`.
pub async fn create_instance(site: &Site, api: &Api, name: &str, settings: Value) -> Made {
    create_from(
        api,
        create_params(site, name, &new_password(), None, settings),
    )
    .await
}

/// Stops the instance and waits until the stop succeeded.
pub async fn stop_instance(api: &Api, id: Uuid) {
    let accepted = api
        .stop_when_free(id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
}

/// Stops the instance, then the in process daemon.
pub async fn stop_and_shutdown(api: &Api, daemon: InProcess, id: Uuid) {
    stop_instance(api, id).await;
    daemon.shutdown().await;
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

/// The PID of the main process of `summary`.
#[must_use]
pub fn pid_of(summary: &InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
}

/// One request to the HTTP interface on `port`, as `user` with `password`
/// in the `X-ClickHouse-*` headers and `query` as the POST body: the status
/// code and the body.
#[must_use]
pub fn http_query(port: u16, user: &str, password: &str, query: &str) -> (u16, String) {
    http_request(
        port,
        &format!(
            "POST / HTTP/1.0\r\nHost: 127.0.0.1\r\nX-ClickHouse-User: {user}\r\nX-ClickHouse-Key: {password}\r\nContent-Length: {}\r\n\r\n{query}",
            query.len()
        ),
    )
}

/// `GET <path>` on the HTTP `port` with no credentials.
#[must_use]
pub fn http_get(port: u16, path: &str) -> (u16, String) {
    http_request(
        port,
        &format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n"),
    )
}

fn http_request(port: u16, request: &str) -> (u16, String) {
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        Duration::from_secs(5),
    )
    .expect("the HTTP port accepts");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("a read timeout can be set");
    // HTTP/1.0 keeps the reply free of chunked framing.
    stream
        .write_all(request.as_bytes())
        .expect("the request is written");
    let mut reply = Vec::new();
    stream
        .read_to_end(&mut reply)
        .expect("the reply is read to the end");
    let text = String::from_utf8_lossy(&reply);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no HTTP head in {text}"));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status in {head}"));
    (status, body.to_owned())
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

/// Asserts that `pid` listens on exactly `127.0.0.1:<http>` and
/// `127.0.0.1:<native>` over TCP and holds no UDP socket (spec 0043 AC-4).
pub fn assert_two_listeners(pid: u32, http: u16, native: u16) {
    let (listening, udp) = listeners(pid);
    let mut expected = vec![format!("127.0.0.1:{http}"), format!("127.0.0.1:{native}")];
    expected.sort();
    assert_eq!(listening, expected, "only the two endpoints listen");
    assert_eq!(udp, 0, "no UDP socket");
}

/// The PIDs of the children of `pid`.
#[must_use]
pub fn children(pid: u32) -> Vec<u32> {
    output_of("/usr/bin/pgrep", &["-P", &pid.to_string()])
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// The arguments of the process `pid`, as `ps` shows them.
#[must_use]
pub fn command_line(pid: u32) -> String {
    output_of("/bin/ps", &["-ww", "-o", "args=", "-p", &pid.to_string()])
        .trim()
        .to_owned()
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

/// Waits until `done` is true, polling every 50 ms.
pub async fn eventually(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + limit;
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "no {what} in time");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every file and folder under `folder`, as paths relative to it, sorted.
#[must_use]
pub fn entries_under(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            found.push(
                path.strip_prefix(folder)
                    .expect("under the folder")
                    .display()
                    .to_string(),
            );
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The text of every log line of the instance `id` that holds `contains`,
/// read through `instance.logs_read`.
pub async fn log_lines(api: &Api, id: Uuid, contains: &str) -> Vec<String> {
    let reply = api
        .client
        .call(Call::InstanceLogsRead(LogsReadParams {
            instance: id,
            streams: Vec::new(),
            contains: Some(contains.to_owned()),
            position: None,
            limit: Some(1000),
        }))
        .await
        .expect("the log reads");
    let Reply::InstanceLogsRead(page) = reply else {
        panic!("not a logs_read reply: {reply:?}");
    };
    page.entries
        .iter()
        .filter_map(|entry| match &entry.kind {
            LogEntryKind::Line(line) => Some(line.text.clone()),
            _ => None,
        })
        .collect()
}

/// Waits until the log of the instance `id` holds a line with `contains`,
/// and returns those lines.
pub async fn wait_log(api: &Api, id: Uuid, contains: &str, limit: Duration) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let lines = log_lines(api, id, contains).await;
        if !lines.is_empty() {
            return lines;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no log line with `{contains}` in time"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A ClickHouse server of the installed runtime that no daemon owns, on
/// `127.0.0.1` with its own ports, a `stackly_readiness` user like the
/// package's, and `macro_id` as its instance ID macro. Its process is
/// killed and waited for on drop.
pub struct Foreign {
    child: Option<Child>,
    /// The PID of the server.
    pub pid: u32,
    /// The HTTP port.
    pub http: u16,
    /// The native TCP port.
    pub native: u16,
    /// The folder of its config, users, and log.
    pub dir: PathBuf,
}

/// How to start a [`Foreign`] server.
#[derive(Debug, Clone, Default)]
pub struct ForeignOptions<'a> {
    /// The data folder, a new one in its own folder when `None`.
    pub data: Option<&'a Path>,
    /// The native port, a free one when `None`.
    pub native: Option<u16>,
    /// Whether to wait until `/ping` answers.
    pub wait: bool,
}

impl Foreign {
    /// Starts `clickhouse server` of `site` in a new folder of its test root
    /// with `macro_id` as the instance ID macro.
    pub fn start(site: &Site, macro_id: &str, options: &ForeignOptions<'_>) -> Self {
        let dir = site
            .root
            .path()
            .join(format!("foreign {}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(dir.join("tmp")).expect("the foreign folder can be made");
        let http = allocate_port().expect("a free port can be allocated");
        let native = options
            .native
            .unwrap_or_else(|| allocate_port().expect("a free port can be allocated"));
        let data = options
            .data
            .map_or_else(|| dir.join("data"), Path::to_path_buf);
        let folder = |path: &Path| xml_text(&format!("{}/", path.display()));
        let config = format!(
            "<clickhouse>
    <logger><level>information</level><console>1</console></logger>
    <macros><{ID_MACRO}>{macro_id}</{ID_MACRO}></macros>
    <disable_internal_dns_cache>1</disable_internal_dns_cache>
    <listen_host>127.0.0.1</listen_host>
    <http_port>{http}</http_port>
    <tcp_port>{native}</tcp_port>
    <path>{data}</path>
    <tmp_path>{tmp}</tmp_path>
    <user_files_path>{user_files}</user_files_path>
    <user_directories>
        <users_xml><path>{users}</path></users_xml>
        <local_directory><path>{access}</path></local_directory>
    </user_directories>
</clickhouse>
",
            data = folder(&data),
            tmp = folder(&dir.join("tmp")),
            user_files = folder(&dir.join("user_files")),
            users = xml_text(&dir.join("users.xml").display().to_string()),
            access = folder(&dir.join("access")),
        );
        std::fs::write(dir.join("config.xml"), config).expect("the foreign config is written");
        std::fs::write(
            dir.join("users.xml"),
            "<clickhouse>
    <profiles><default/><stackly_readiness><readonly>1</readonly></stackly_readiness></profiles>
    <quotas><default/></quotas>
    <users>
        <default>
            <password></password>
            <networks><ip>127.0.0.1</ip><ip>::1</ip></networks>
            <profile>default</profile>
            <quota>default</quota>
        </default>
        <stackly_readiness>
            <password></password>
            <networks><ip>127.0.0.1</ip><ip>::1</ip></networks>
            <profile>stackly_readiness</profile>
            <quota>default</quota>
            <grants><query>GRANT SELECT ON system.one</query></grants>
        </stackly_readiness>
    </users>
</clickhouse>
",
        )
        .expect("the foreign users file is written");
        let log = std::fs::File::create(dir.join("server.log")).expect("the log can be made");
        let child = Command::new(site.bin())
            .arg("server")
            .arg(format!(
                "--config-file={}",
                dir.join("config.xml").display()
            ))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", dir.join("tmp"))
            .env("TMPDIR", dir.join("tmp"))
            .env("CLICKHOUSE_WATCHDOG_ENABLE", "0")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("the foreign clickhouse server starts");
        let foreign = Self {
            pid: child.id(),
            child: Some(child),
            http,
            native,
            dir,
        };
        if options.wait {
            let deadline = Instant::now() + Duration::from_secs(60);
            while !TcpStream::connect(("127.0.0.1", http))
                .is_ok_and(|_| http_get(http, "/ping").0 == 200)
            {
                assert!(
                    Instant::now() < deadline,
                    "the foreign server did not answer /ping: {}",
                    foreign.log()
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        foreign
    }

    /// What the foreign server wrote to stderr so far.
    #[must_use]
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("server.log")).unwrap_or_default()
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

/// `text` escaped for XML element content.
#[must_use]
pub fn xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
/// has reason `probe_ok`.
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
