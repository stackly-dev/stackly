//! The native harness of the Valkey provider (spec 0040, step 5): a test
//! root with the managed Valkey 9.1.2 runtime installed from the runtime
//! cache of `cargo xtask`, the `official/valkey` package (or a copy a test
//! changed) pinned, a daemon either in process or as the `stacklyd` binary,
//! and a small RESP client that speaks to the instance over loopback.
//! Nothing here downloads anything: an artifact that is not in the cache
//! ends the test as `BLOCKED`.
#![allow(
    dead_code,
    reason = "each scenario module uses a part of these helpers"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
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
use stackly_domain::{InstanceId, Registry, SecretRef, WriteBatch, WriteOp};
use stackly_platform::{Clock, Platform, SystemClock};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::Timestamp;
use stackly_protocol::events::EventKind;
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, InstanceReadinessState, InstanceSummary, PortRequest,
    ProviderSelector, SecretFieldInput, SettingsDocument,
};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::evidence::{Evidence, Source};
use stackly_testkit::{FakeSecretStore, ReapGuard, RuntimeCache, TestRoot};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::server::{self, DaemonConfig, DaemonHandle};
use uuid::Uuid;

use crate::common::{self, DaemonPaths, STACKLYD};
use crate::support::{Api, PATIENCE, fast};

/// The owner who can fill the runtime cache (spec 0002).
pub const OWNER: &str = "@ojiepermana";
/// The provider of the package.
pub const PROVIDER: &str = "official/valkey";
/// The artifact of `providers/valkey/versions.json` the tests install.
pub const ARTIFACT: &str = "valkey-9.1.2-r2-macos-aarch64";
/// The exact version of [`ARTIFACT`].
pub const VERSION: &str = "9.1.2";
/// The readiness user of the package.
pub const READINESS_USER: &str = "stackly-readiness";

/// The folder of the package under test.
#[must_use]
pub fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/valkey")
}

/// Reads and validates the package in `dir`.
#[must_use]
pub fn package(platform: &Platform, dir: &Path) -> ValidatedPackage {
    let files = read_package(platform.fs(), dir).expect("the package reads");
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

/// Copies the folder `from` into `to`.
pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a folder can be made");
    for entry in std::fs::read_dir(from)
        .expect("a folder can be read")
        .flatten()
    {
        let target = to.join(entry.file_name());
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a file can be copied");
        }
    }
}

/// A copy of the package in `root` with `extra` added at the end of
/// `lifecycle.start.args`, for a test that needs a Valkey option the
/// official package never sets.
#[must_use]
pub fn copy_with_start_args(root: &TestRoot, extra: &[&str]) -> PathBuf {
    let dir = root.path().join("valkey");
    copy_tree(&package_dir(), &dir);
    let manifest = dir.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest can be read");
    let anchor = "      - \"--enable-module-command\"\n      - \"no\"\n";
    let added: String = extra
        .iter()
        .map(|arg| format!("      - \"{arg}\"\n"))
        .collect();
    let changed = text.replacen(anchor, &format!("{anchor}{added}"), 1);
    assert_ne!(
        text, changed,
        "the manifest ends its start args as expected"
    );
    std::fs::write(&manifest, changed).expect("the manifest can be written");
    dir
}

fn make_private(dir: &Path) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .expect("a pinned folder can be made private");
    for entry in std::fs::read_dir(dir).expect("a pinned folder can be listed") {
        let path = entry.expect("a folder entry can be read").path();
        if path.is_dir() {
            make_private(&path);
        }
    }
}

/// An `InstanceId` from the UUID the API shows.
#[must_use]
pub fn instance_id(id: Uuid) -> InstanceId {
    id.to_string().parse().expect("an instance id")
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
    pub runtime_id: Uuid,
    pub runtime_dir: PathBuf,
    pub artifact_sha256: String,
    /// The revision of the package, as `pin_package` recorded it.
    pub revision: String,
    /// The SHA-256 of the manifest of that revision.
    pub manifest_sha256: String,
}

impl Site {
    /// Installs Valkey from the runtime cache into a new root and pins the
    /// package. Notes `evidence` as `BLOCKED` and returns `None` when the
    /// cache does not hold the artifact, so the test ends there.
    pub fn open(evidence: &Evidence) -> Option<Self> {
        Self::open_package(evidence, &package_dir())
    }

    /// [`Site::open`] with the package in `dir`, a copy of the package under
    /// test that a test changed.
    pub fn open_package(evidence: &Evidence, dir: &Path) -> Option<Self> {
        let root = TestRoot::new().expect("a test root can be created");
        let secrets = Arc::new(FakeSecretStore::new());
        let platform = MacosPlatform::new(Some(root.path().into()))
            .expect("a test root is a valid root override")
            .with_secrets(Arc::clone(&secrets) as Arc<dyn stackly_platform::SecretStore>);
        let package = package(&platform, dir);
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
        let pinned =
            pin_package(&platform, prepared.layout(), dir).expect("the package can be pinned");
        make_private(&prepared.layout().providers_dir());
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
        Some(Self {
            _reap: reap,
            root,
            platform,
            prepared,
            secrets,
            runtime_id: runtime.id().as_uuid(),
            runtime_dir,
            artifact_sha256: sha256,
            revision: revision_key.revision().to_string(),
            manifest_sha256: revision_key.manifest_sha256().to_string(),
        })
    }

    /// The paths the binary daemon computes for this root.
    #[must_use]
    pub fn paths(&self) -> DaemonPaths {
        common::daemon_paths(&self.root)
    }

    /// The `bin/<name>` of the installed runtime.
    #[must_use]
    pub fn bin(&self, name: &str) -> PathBuf {
        let bin = self.runtime_dir.join("bin").join(name);
        assert!(bin.is_file(), "no {name} at {}", bin.display());
        bin
    }

    /// The paths of the instance `id`.
    #[must_use]
    pub fn instance_paths(&self, id: Uuid) -> stackly_platform::InstancePaths {
        self.prepared.layout().instance_paths(instance_id(id))
    }

    /// The `users.conf` of the instance `id`, in its secrets folder.
    #[must_use]
    pub fn users_conf(&self, id: Uuid) -> PathBuf {
        self.instance_paths(id).run.join("secrets/users.conf")
    }

    /// Notes the running server as the subject of `evidence`, with the
    /// version `INFO server` reported.
    pub fn subject(&self, evidence: &Evidence, server_version: &str) {
        evidence.subject(
            "valkey",
            server_version,
            Some(&self.artifact_sha256),
            Source::Managed,
        );
    }

    /// Opens the registry of this root. The daemon must be stopped.
    pub fn open_registry(&self) -> impl Registry + use<> {
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

    /// Starts the `stacklyd` binary on this root with `env` added to its
    /// environment, and waits until it answers a handshake. Its stderr goes
    /// to a file of the test root, which [`BinaryDaemon::stderr`] reads.
    pub fn binary(&self, env: &[(&str, &str)]) -> BinaryDaemon {
        BinaryDaemon::start(self, env)
    }

    /// A typed client of the binary daemon, once its start slot ended.
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

    /// The value stored for the secret field `field` of the instance `id`.
    #[must_use]
    pub fn stored_secret(&self, id: Uuid, field: &str) -> String {
        let reference: SecretRef = format!("local:instance/{id}/{field}")
            .parse()
            .expect("a secret reference");
        self.secrets
            .stored_value(&reference)
            .unwrap_or_else(|| panic!("the store holds {reference}"))
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

/// The `stacklyd` binary on the root of a site.
pub struct BinaryDaemon {
    child: Option<Child>,
    pub socket: PathBuf,
    /// The file its stderr goes to.
    pub stderr_file: PathBuf,
}

impl BinaryDaemon {
    fn start(site: &Site, env: &[(&str, &str)]) -> Self {
        let paths = site.paths();
        let stderr_file = site
            .root
            .path()
            .join(format!("stacklyd-{}.stderr", Uuid::now_v7().simple()));
        let stderr = std::fs::File::create(&stderr_file).expect("the stderr file can be created");
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
            stderr_file,
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

    /// What the daemon wrote to stderr so far, its JSON log.
    #[must_use]
    pub fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr_file).unwrap_or_default()
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

/// A fresh fixture password.
#[must_use]
pub fn new_password() -> String {
    stackly_testkit::evidence::FixtureCredential::new()
        .value()
        .to_owned()
}

/// The params of a create of the Valkey package with the `client` endpoint
/// on `Auto`: the password `password`, or a generated one for `None`.
#[must_use]
pub fn create_params(
    site: &Site,
    name: &str,
    password: Option<&str>,
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
        endpoints: vec![EndpointRequest {
            name: "client".to_owned(),
            port: PortRequest::Auto,
        }],
        settings: SettingsDocument::new(map),
        secrets: vec![SecretFieldInput {
            field: "password".to_owned(),
            input: password.map_or(SecretInput::Generate, |value| {
                SecretInput::Value(SecretText::new(value.to_owned()))
            }),
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

/// Creates an instance and waits until the create succeeded: with
/// `password`, or with a password the daemon generates for `None`, read
/// back from the fake secret store.
pub async fn create_with(
    site: &Site,
    api: &Api,
    name: &str,
    password: Option<&str>,
    settings: Value,
) -> Made {
    let accepted = api
        .create(create_params(site, name, password, settings))
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
    let password = password.map_or_else(|| site.stored_secret(id, "password"), str::to_owned);
    Made { id, password }
}

/// Creates an instance with a new fixture password and `settings`.
pub async fn create_instance(site: &Site, api: &Api, name: &str, settings: Value) -> Made {
    create_with(site, api, name, Some(&new_password()), settings).await
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

/// The effective port of the `client` endpoint of `summary`.
#[must_use]
pub fn port_of(summary: &InstanceSummary) -> u16 {
    summary
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "client")
        .and_then(|endpoint| endpoint.port)
        .unwrap_or_else(|| panic!("the instance has a client port: {summary:#?}"))
}

/// The PID of the main process of `summary`.
#[must_use]
pub fn pid_of(summary: &InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
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

/// The sockets of the process `pid` that `lsof` shows: every TCP
/// `host:port` with `(LISTEN)`, sorted, the number of UDP sockets, and the
/// number of Unix sockets.
#[must_use]
pub fn sockets(pid: u32) -> (Vec<String>, usize, usize) {
    let pid = pid.to_string();
    let shown = output_of("/usr/sbin/lsof", &["-nP", "-a", "-p", &pid, "-i"]);
    let unix_shown = output_of("/usr/sbin/lsof", &["-nP", "-a", "-p", &pid, "-U"]);
    let mut listening: Vec<String> = shown
        .lines()
        .filter(|line| line.ends_with("(LISTEN)"))
        .filter_map(|line| line.split_whitespace().rev().nth(1).map(str::to_owned))
        .collect();
    listening.sort();
    let udp = shown.lines().filter(|line| line.contains(" UDP ")).count();
    let unix = unix_shown
        .lines()
        .filter(|line| line.contains(" unix "))
        .count();
    (listening, udp, unix)
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

/// Sends `signal` (`KILL`, `STOP`, ...) to `pid`.
pub fn signal(pid: u32, signal: &str) {
    let status = Command::new("/bin/kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "kill -{signal} {pid} failed");
}

/// Waits until `done` is true, polling every 50 ms.
pub async fn eventually(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + limit;
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "no {what} in time");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every regular file under `folder`, as paths relative to it, sorted.
#[must_use]
pub fn files_under(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
            } else {
                found.push(
                    path.strip_prefix(folder)
                        .expect("under the folder")
                        .display()
                        .to_string(),
                );
            }
        }
    }
    found.sort();
    found
}

/// One RESP reply of Valkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// `+OK`.
    Simple(String),
    /// `-ERR ...`, the whole message.
    Error(String),
    /// `:1`.
    Integer(i64),
    /// `$3 abc`, or `$-1` as `None`.
    Bulk(Option<String>),
    /// `*2 ...`, or `*-1` as `None`.
    Array(Option<Vec<Reply>>),
}

impl Reply {
    /// The text of a simple or bulk reply.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Simple(text) | Self::Bulk(Some(text)) => Some(text),
            _ => None,
        }
    }

    /// The message of an error reply.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Error(text) => Some(text),
            _ => None,
        }
    }

    /// The value `CONFIG GET <name>` returned: the second element.
    #[must_use]
    pub fn config_value(&self) -> Option<&str> {
        match self {
            Self::Array(Some(items)) => items.get(1).and_then(Self::text),
            _ => None,
        }
    }
}

/// One RESP connection to an instance on loopback.
pub struct Valkey {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Valkey {
    /// Connects to `127.0.0.1:<port>`.
    #[must_use]
    pub fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("the client port accepts");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("a read timeout can be set");
        let writer = stream.try_clone().expect("the stream can be cloned");
        Self {
            reader: BufReader::new(stream),
            writer,
        }
    }

    /// Connects and signs in as `default` with `password`.
    #[must_use]
    pub fn signed_in(port: u16, password: &str) -> Self {
        let mut connection = Self::connect(port);
        let reply = connection.cmd(&["AUTH", password]);
        assert_eq!(reply, Reply::Simple("OK".to_owned()), "AUTH default");
        connection
    }

    /// Sends one command and reads its reply.
    pub fn cmd(&mut self, args: &[&str]) -> Reply {
        self.send(args);
        self.read()
    }

    /// Sends one command without reading its reply.
    pub fn send(&mut self, args: &[&str]) {
        let mut frame = format!("*{}\r\n", args.len()).into_bytes();
        for arg in args {
            frame.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
            frame.extend_from_slice(arg.as_bytes());
            frame.extend_from_slice(b"\r\n");
        }
        self.writer.write_all(&frame).expect("the command is sent");
    }

    /// Reads one reply.
    pub fn read(&mut self) -> Reply {
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .expect("a reply line is read");
        assert!(read > 0, "the server closed the connection");
        let line = line.trim_end_matches("\r\n");
        let (kind, rest) = line.split_at(1);
        match kind {
            "+" => Reply::Simple(rest.to_owned()),
            "-" => Reply::Error(rest.to_owned()),
            ":" => Reply::Integer(rest.parse().expect("an integer reply")),
            "$" => {
                let len: i64 = rest.parse().expect("a bulk length");
                if len < 0 {
                    return Reply::Bulk(None);
                }
                let len = usize::try_from(len).expect("a bulk length fits");
                let mut bytes = vec![0; len + 2];
                self.reader
                    .read_exact(&mut bytes)
                    .expect("the bulk is read");
                bytes.truncate(len);
                Reply::Bulk(Some(String::from_utf8_lossy(&bytes).into_owned()))
            }
            "*" => {
                let len: i64 = rest.parse().expect("an array length");
                if len < 0 {
                    return Reply::Array(None);
                }
                Reply::Array(Some((0..len).map(|_| self.read()).collect()))
            }
            other => panic!("unknown RESP reply {other}{rest}"),
        }
    }

    /// The `INFO server` field `name`.
    pub fn info_server(&mut self, name: &str) -> String {
        let reply = self.cmd(&["INFO", "server"]);
        let text = reply.text().expect("INFO answers text").to_owned();
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{name}:")))
            .map(|value| value.trim().to_owned())
            .unwrap_or_else(|| panic!("INFO server has no {name}: {text}"))
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
