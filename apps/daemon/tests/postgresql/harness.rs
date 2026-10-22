//! The native harness of the PostgreSQL provider (spec 0019, step 6): a test
//! root with a real PostgreSQL runtime installed from the runtime cache of
//! `cargo xtask`, the `official/postgresql` package pinned, and a daemon
//! either in process or as the `stacklyd` binary. Nothing here downloads
//! anything: an artifact that is not in the cache ends the test as `BLOCKED`.
#![allow(
    dead_code,
    reason = "each scenario module uses a part of these helpers"
)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_core::supervisor::fixture::pin_package;
use stackly_core::{PreparedLayout, daemon_info, prepare_layout};
use stackly_domain::{
    InstanceId, LineCompatibility, Registry, RuntimeId, RuntimeLine, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, Platform, SystemClock};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::Timestamp;
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, InstanceActionAccepted, PortRequest, ProviderSelector,
    SecretFieldInput, SettingsDocument,
};
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::evidence::{Evidence, Source};
use stackly_testkit::{FakeSecretStore, ReapGuard, TestRoot, allocate_port};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::server::{self, DaemonConfig, DaemonHandle};
use uuid::Uuid;

use crate::common::{self, DaemonPaths, STACKLYD};
use crate::support::{Api, PATIENCE, fast};

/// The owner who can fill the runtime cache (spec 0002).
pub const OWNER: &str = "@ojiepermana";
/// The provider of the package.
pub const PROVIDER: &str = "official/postgresql";
/// The revision of the package.
pub const REVISION: &str = "1.0.0";
/// The role that owns the cluster.
pub const SUPERUSER: &str = "postgres";

/// One PostgreSQL artifact of `providers/postgresql/versions.json`.
#[derive(Debug, Clone, Copy)]
pub struct Pg {
    /// The artifact ID.
    pub artifact: &'static str,
    /// The line, `17` or `18`.
    pub major: &'static str,
    /// The exact version.
    pub version: &'static str,
}

/// PostgreSQL 18.
pub const PG18: Pg = Pg {
    artifact: "postgresql-18.6-r2-macos-aarch64",
    major: "18",
    version: "18.6",
};

/// PostgreSQL 17.
pub const PG17: Pg = Pg {
    artifact: "postgresql-17.11-r2-macos-aarch64",
    major: "17",
    version: "17.11",
};

/// The folder of the package under test.
#[must_use]
pub fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/postgresql")
}

/// The runtime cache of `cargo xtask` (spec 0003): where the artifact with
/// `sha256` is, if it was downloaded.
#[must_use]
pub fn cached_artifact(sha256: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let file = Path::new(&home)
        .join("Library/Caches/dev.stackly.xtask/runtime")
        .join(sha256);
    file.is_file().then_some(file)
}

/// Reads and validates the package under test.
#[must_use]
pub fn package(platform: &Platform, dir: &Path) -> ValidatedPackage {
    let files = read_package(platform.fs(), dir).expect("the package reads");
    ValidatedPackage::from_files(files, NameCheck::Skip).expect("the package validates")
}

/// The SHA-256 of the artifact `pg` in `versions.json`.
#[must_use]
pub fn artifact_sha256(pg: Pg) -> String {
    let text = std::fs::read_to_string(package_dir().join("versions.json"))
        .expect("versions.json can be read");
    let versions: serde_json::Value = serde_json::from_str(&text).expect("versions.json is JSON");
    versions["artifacts"]
        .as_array()
        .and_then(|artifacts| {
            artifacts
                .iter()
                .find(|artifact| artifact["id"] == pg.artifact)
        })
        .and_then(|artifact| artifact["sha256"].as_str())
        .unwrap_or_else(|| panic!("versions.json has {}", pg.artifact))
        .to_owned()
}

/// A platform on `root`, with a fake secret store the test can read.
fn platform_of(root: &TestRoot, secrets: &Arc<FakeSecretStore>) -> Platform {
    MacosPlatform::new(Some(root.path().into()))
        .expect("a test root is a valid root override")
        .with_secrets(Arc::clone(secrets) as Arc<dyn stackly_platform::SecretStore>)
}

/// One test's root with the runtime installed and the package pinned.
pub struct Site {
    /// Declared first, so it drops first: it stops the instance processes
    /// that name this root, then removes the runtime root.
    _reap: ReapGuard,
    pub root: TestRoot,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    pub secrets: Arc<FakeSecretStore>,
    pub pg: Pg,
    pub runtime_id: Uuid,
    pub runtime_dir: PathBuf,
    pub artifact_sha256: String,
}

impl Site {
    /// Installs `pg` from the runtime cache into a new root and pins the
    /// package. Notes `evidence` as `BLOCKED` and returns `None` when the
    /// cache does not hold the artifact, so the test ends there.
    pub fn open(evidence: &Evidence, pg: Pg) -> Option<Self> {
        Self::open_package(evidence, pg, &package_dir())
    }

    /// [`Site::open`] with the package in `dir`, a copy of the package under
    /// test that a test changed.
    pub fn open_package(evidence: &Evidence, pg: Pg, dir: &Path) -> Option<Self> {
        let root = TestRoot::new().expect("a test root can be created");
        let secrets = Arc::new(FakeSecretStore::new());
        let platform = platform_of(&root, &secrets);
        let package = package(&platform, dir);
        let sha256 = artifact_sha256(pg);
        let Some(cache_file) = cached_artifact(&sha256) else {
            evidence.blocked(
                &format!("artifact {} is not in the runtime cache", pg.artifact),
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
            pg.artifact,
            &cache_file,
        )
        .expect("the cached artifact installs, exec check included");
        let revision =
            pin_package(&platform, prepared.layout(), dir).expect("the package can be pinned");
        registry
            .commit(WriteBatch::new(vec![WriteOp::InsertProviderRevision(
                revision,
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
            platform,
            prepared,
            secrets,
            pg,
            runtime_id: runtime.id().as_uuid(),
            runtime_dir,
            artifact_sha256: sha256,
        })
    }

    /// Installs `other` from the runtime cache into the registry of this
    /// root, next to the runtime of the site. The daemon must be stopped.
    /// Notes `evidence` as `BLOCKED` and returns `None` when the cache does
    /// not hold the artifact.
    pub fn install_other(
        &self,
        evidence: &Evidence,
        other: Pg,
    ) -> Option<(RuntimeId, RuntimeLine)> {
        let Some(cache_file) = cached_artifact(&artifact_sha256(other)) else {
            evidence.blocked(
                &format!("artifact {} is not in the runtime cache", other.artifact),
                OWNER,
            );
            return None;
        };
        let registry = self.open_registry();
        let package = package(&self.platform, &package_dir());
        let (runtime, _) = install_cached_artifact(
            &self.platform,
            &self.prepared,
            &registry,
            &package,
            other.artifact,
            &cache_file,
        )
        .expect("the cached artifact installs, exec check included");
        Some((runtime.id(), runtime.line().clone()))
    }

    /// Binds the stopped instance `id` to `runtime` by writing the registry
    /// the way no contract method does, so a test can start data of one
    /// major on a runtime of another. The daemon must be stopped.
    pub fn rebind_instance(&self, id: Uuid, runtime: &(RuntimeId, RuntimeLine)) {
        let registry = self.open_registry();
        let mut instance = registry
            .instance(instance_id(id))
            .expect("the instance can be read")
            .value
            .expect("the instance exists");
        let expected = instance.revision();
        let software = instance.software().clone();
        let compat =
            LineCompatibility::default().allow(instance.data_line().clone(), runtime.1.clone());
        instance
            .rebind(runtime.0, &software, &runtime.1, &compat)
            .expect("the stopped instance can be rebound");
        registry
            .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
                instance,
                expected,
            }]))
            .expect("the rebound instance can be committed");
    }

    /// Opens the registry of this root. The daemon must be stopped.
    fn open_registry(&self) -> impl Registry + use<> {
        let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
            .open(
                &self.platform,
                self.prepared.layout(),
                daemon_info().version,
            )
            .expect("the registry opens")
        else {
            panic!("the registry opens ready");
        };
        registry
    }

    /// The paths the binary daemon computes for this root.
    #[must_use]
    pub fn paths(&self) -> DaemonPaths {
        common::daemon_paths(&self.root)
    }

    /// The uid of the test.
    #[must_use]
    pub fn uid(&self) -> u32 {
        common::uid()
    }

    /// The `bin/<name>` of the installed runtime.
    #[must_use]
    pub fn bin(&self, name: &str) -> PathBuf {
        self.runtime_dir.join("bin").join(name)
    }

    /// The paths of the instance `id`.
    #[must_use]
    pub fn instance_paths(&self, id: Uuid) -> stackly_platform::InstancePaths {
        self.prepared.layout().instance_paths(instance_id(id))
    }

    /// The folder of the socket of the instance `id`.
    #[must_use]
    pub fn run_dir(&self, id: Uuid) -> PathBuf {
        self.instance_paths(id).run
    }

    /// Notes the running server as the subject of `evidence`, with the
    /// version it reports.
    pub fn subject(&self, evidence: &Evidence, server_version: &str) {
        evidence.subject(
            "postgresql",
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
        // PostgreSQL answers its first query in about a second; a probe every
        // 200 ms keeps the waits short without changing what is checked.
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
            stackly_ipc::ConnectPolicy::same_user(self.uid()),
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

    /// Starts the `stacklyd` binary on this root with `env` added to its
    /// environment, and waits until it answers a handshake.
    pub fn binary(&self, env: &[(&str, &str)]) -> BinaryDaemon {
        BinaryDaemon::start(self, env)
    }

    /// A typed client of the binary daemon.
    pub async fn binary_api(&self, daemon: &BinaryDaemon) -> Api {
        let (client, _) = common::client(&daemon.socket).await;
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

    /// `psql` of the runtime against the instance `id`.
    #[must_use]
    pub fn psql(&self, id: Uuid, port: u16) -> Psql {
        Psql {
            bin: self.bin("psql"),
            socket_dir: self.run_dir(id),
            port,
            home: self.root.path().to_path_buf(),
        }
    }
}

/// An `InstanceId` from the UUID the API shows.
#[must_use]
pub fn instance_id(id: Uuid) -> InstanceId {
    id.to_string().parse().expect("an instance id")
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

/// The `stacklyd` binary on a test root.
pub struct BinaryDaemon {
    child: Option<Child>,
    pub socket: PathBuf,
}

impl BinaryDaemon {
    fn start(site: &Site, env: &[(&str, &str)]) -> Self {
        let paths = site.paths();
        let stderr = std::fs::File::create(
            site.root
                .path()
                .join(format!("stacklyd-{}.stderr", Uuid::now_v7().simple())),
        )
        .expect("the stderr file can be created");
        let mut command = Command::new(STACKLYD);
        command
            .env("STACKLY_ROOT", site.root.path())
            .env("STACKLY_LOG", "debug");
        for (name, value) in env {
            command.env(name, value);
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("stacklyd starts");
        let mut daemon = Self {
            child: Some(child),
            socket: paths.socket,
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        while !common::handshakes(&daemon.socket) {
            assert!(
                Instant::now() < deadline,
                "stacklyd did not answer a handshake"
            );
            assert!(
                daemon
                    .child
                    .as_mut()
                    .and_then(|child| child.try_wait().ok().flatten())
                    .is_none(),
                "stacklyd exited before it was ready"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        daemon
    }

    /// The process ID of the daemon.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    /// Sends `SIGKILL`, so the daemon cleans up nothing.
    pub fn kill(mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill().expect("the daemon can be killed");
            child.wait().expect("the daemon can be waited for");
        }
    }

    /// Sends `SIGTERM` and waits for the daemon to exit.
    pub fn stop(mut self) {
        self.terminate();
    }

    fn terminate(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(30);
        while child.try_wait().ok().flatten().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for BinaryDaemon {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// How `psql` reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// The private Unix socket of the instance.
    Socket,
    /// TCP on the loopback address.
    Tcp,
}

/// What a `psql` run printed.
#[derive(Debug)]
pub struct Ran {
    /// Whether `psql` exited 0.
    pub ok: bool,
    /// Standard output, trimmed.
    pub stdout: String,
    /// Standard error, trimmed.
    pub stderr: String,
}

/// The `psql` of the runtime, pointed at one instance. It runs with a
/// closed environment, as a person at a terminal would, never with a
/// password unless the call gives one.
#[derive(Debug, Clone)]
pub struct Psql {
    bin: PathBuf,
    socket_dir: PathBuf,
    port: u16,
    home: PathBuf,
}

impl Psql {
    /// Runs `sql` as `user` on database `db`.
    #[must_use]
    pub fn run(&self, via: Via, user: &str, password: Option<&str>, db: &str, sql: &str) -> Ran {
        let host = match via {
            Via::Socket => self.socket_dir.display().to_string(),
            Via::Tcp => "127.0.0.1".to_owned(),
        };
        self.exec(
            &[
                "--no-psqlrc",
                "--no-password",
                "--tuples-only",
                "--no-align",
                "--set=ON_ERROR_STOP=1",
                &format!("--host={host}"),
                &format!("--port={}", self.port),
                &format!("--username={user}"),
                &format!("--dbname={db}"),
                &format!("--command={sql}"),
            ],
            password,
        )
    }

    /// Runs `sql` as the superuser with `password`, over the socket.
    #[must_use]
    pub fn admin(&self, password: &str, sql: &str) -> Ran {
        self.run(Via::Socket, SUPERUSER, Some(password), "postgres", sql)
    }

    /// Runs `sql` with a `key=value` connection string.
    #[must_use]
    pub fn conninfo(&self, conninfo: &str, sql: &str) -> Ran {
        self.exec(
            &[
                "--no-psqlrc",
                "--no-password",
                "--tuples-only",
                "--no-align",
                "--set=ON_ERROR_STOP=1",
                &format!("--dbname={conninfo}"),
                &format!("--command={sql}"),
            ],
            None,
        )
    }

    /// The first column of the one row `sql` returns as the superuser.
    #[must_use]
    pub fn scalar(&self, password: &str, sql: &str) -> String {
        let ran = self.admin(password, sql);
        assert!(ran.ok, "{sql}: {}", ran.stderr);
        ran.stdout
    }

    /// The conninfo prefix of this instance over the socket.
    #[must_use]
    pub fn socket_conninfo(&self) -> String {
        format!("host={} port={}", self.socket_dir.display(), self.port)
    }

    fn exec(&self, args: &[&str], password: Option<&str>) -> Ran {
        let mut command = Command::new(&self.bin);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("PGCONNECT_TIMEOUT", "5");
        if let Some(password) = password {
            command.env("PGPASSWORD", password);
        }
        let output = command.output().expect("psql runs");
        Ran {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        }
    }
}

/// A free loopback port for an instance.
#[must_use]
pub fn free_port() -> u16 {
    allocate_port().expect("a free port can be allocated")
}

/// The params of a `create` of the PostgreSQL package.
#[must_use]
pub fn create_params(
    site: &Site,
    name: &str,
    port: PortRequest,
    password: &str,
    settings: serde_json::Value,
) -> CreateInstanceParams {
    let map = match settings {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => serde_json::Map::new(),
        other => panic!("settings must be an object, not {other}"),
    };
    CreateInstanceParams {
        name: name.to_owned(),
        display_name: None,
        provider: ProviderSelector {
            name: PROVIDER.to_owned(),
            revision: Some(REVISION.to_owned()),
        },
        runtime_id: site.runtime_id,
        endpoints: vec![EndpointRequest {
            name: "client".to_owned(),
            port,
        }],
        settings: SettingsDocument::new(map),
        secrets: vec![SecretFieldInput {
            field: "superuserPassword".to_owned(),
            input: SecretInput::Value(SecretText::new(password.to_owned())),
        }],
    }
}

/// An instance made by [`create_instance`].
#[derive(Debug, Clone)]
pub struct Made {
    /// The instance.
    pub id: Uuid,
    /// The port of the `client` endpoint.
    pub port: u16,
    /// The password of the superuser.
    pub password: String,
}

/// Creates an instance on a free port with a new fixture password and
/// waits until the create succeeded.
pub async fn create_instance(site: &Site, api: &Api, name: &str) -> Made {
    let password = stackly_testkit::evidence::FixtureCredential::new()
        .value()
        .to_owned();
    create_with(site, api, name, &password, json!({})).await
}

/// [`create_instance`] with a chosen password and settings.
pub async fn create_with(
    site: &Site,
    api: &Api,
    name: &str,
    password: &str,
    settings: serde_json::Value,
) -> Made {
    let port = free_port();
    let accepted = start_create(site, api, name, port, password, settings).await;
    let operation = accepted
        .accepted
        .expect("a create starts an operation")
        .operation;
    let id = operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    let detail = api
        .wait_state(
            operation.id,
            stackly_protocol::operation::OperationState::Succeeded,
        )
        .await;
    assert_eq!(
        detail.operation.state,
        stackly_protocol::operation::OperationState::Succeeded,
        "{detail:?}"
    );
    Made {
        id,
        port,
        password: password.to_owned(),
    }
}

/// Sends the create and returns the acceptance, without waiting.
pub async fn start_create(
    site: &Site,
    api: &Api,
    name: &str,
    port: u16,
    password: &str,
    settings: serde_json::Value,
) -> InstanceActionAccepted {
    api.create(create_params(
        site,
        name,
        PortRequest::Fixed(port),
        password,
        settings,
    ))
    .await
    .expect("the create is accepted")
}

/// How long a test waits for the server to do something slow.
pub const SLOW: Duration = PATIENCE;

/// Waits until `done` is true, polling every 50 ms.
pub async fn eventually(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + limit;
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "no {what} in time");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A fresh fixture password.
#[must_use]
pub fn new_password() -> String {
    stackly_testkit::evidence::FixtureCredential::new()
        .value()
        .to_owned()
}

/// Creates an instance whose `client` port the daemon picks, and waits until
/// the create succeeded. Returns the instance and its password.
pub async fn create_auto(site: &Site, api: &Api, name: &str) -> (Uuid, String) {
    let password = new_password();
    let accepted = api
        .create(create_params(
            site,
            name,
            PortRequest::Auto,
            &password,
            json!({}),
        ))
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
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    (id, password)
}

/// Stops the instance and the daemon, so nothing outlives the test.
pub async fn stop_and_shutdown(api: &Api, daemon: InProcess, id: Uuid) {
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
    daemon.shutdown().await;
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

/// Every regular file under `dir` whose bytes hold `needle`.
#[must_use]
pub fn files_holding(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file()
                && std::fs::read(entry.path())
                    .is_ok_and(|bytes| bytes.windows(needle.len()).any(|w| w == needle.as_bytes()))
            {
                found.push(entry.path());
            }
        }
    }
    found
}
