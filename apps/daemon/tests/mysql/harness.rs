//! The native harness of the MySQL provider (spec 0042, step 5): a test root
//! with a real MySQL runtime installed from the runtime cache of
//! `cargo xtask`, the `official/mysql` package pinned, and a daemon either in
//! process or as the `stacklyd` binary. Nothing here downloads anything: an
//! artifact that is not in the cache ends the test as `BLOCKED`.
#![allow(
    dead_code,
    reason = "each scenario module uses a part of these helpers"
)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::catalog::{Catalog, CatalogEntry, ProviderEntry, TileColor, TileHint};
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
use stackly_testkit::{
    FakeFetcher, FakeSecretStore, ReapGuard, RuntimeCache, TestRoot, allocate_port,
};
use stacklyd::catalog::CatalogOverride;
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::server::{self, DaemonConfig, DaemonHandle};
use uuid::Uuid;

use crate::common::{self, DaemonPaths, STACKLYD};
use crate::support::{Api, PATIENCE, fast};

/// The owner who can fill the runtime cache (spec 0002).
pub const OWNER: &str = "@ojiepermana";
/// The provider of the package.
pub const PROVIDER: &str = "official/mysql";
/// The revision of the package.
pub const REVISION: &str = "1.0.0";
/// The account that owns the server.
pub const ROOT: &str = "root";
/// The account readiness signs in as.
pub const READINESS: &str = "stackly_readiness";
/// The secret field of the root password.
pub const PASSWORD_FIELD: &str = "rootPassword";

/// One MySQL artifact of `providers/mysql/versions.json`.
#[derive(Debug, Clone, Copy)]
pub struct My {
    /// The artifact ID.
    pub artifact: &'static str,
    /// The line, `8.4` or `9.7`.
    pub line: &'static str,
    /// The exact version.
    pub version: &'static str,
}

/// MySQL 8.4.
pub const MY84: My = My {
    artifact: "mysql-8.4.11-macos-aarch64",
    line: "8.4",
    version: "8.4.11",
};

/// MySQL 9.7.
pub const MY97: My = My {
    artifact: "mysql-9.7.2-macos-aarch64",
    line: "9.7",
    version: "9.7.2",
};

/// The folder of the package under test.
#[must_use]
pub fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/mysql")
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

/// The SHA-256 of the artifact `my` in `versions.json`.
#[must_use]
pub fn artifact_sha256(my: My) -> String {
    let text = std::fs::read_to_string(package_dir().join("versions.json"))
        .expect("versions.json can be read");
    let versions: serde_json::Value = serde_json::from_str(&text).expect("versions.json is JSON");
    versions["artifacts"]
        .as_array()
        .and_then(|artifacts| {
            artifacts
                .iter()
                .find(|artifact| artifact["id"] == my.artifact)
        })
        .and_then(|artifact| artifact["sha256"].as_str())
        .unwrap_or_else(|| panic!("versions.json has {}", my.artifact))
        .to_owned()
}

/// The size in bytes of the artifact `my` in `versions.json`.
#[must_use]
pub fn artifact_size(my: My) -> u64 {
    let text = std::fs::read_to_string(package_dir().join("versions.json"))
        .expect("versions.json can be read");
    let versions: serde_json::Value = serde_json::from_str(&text).expect("versions.json is JSON");
    versions["artifacts"]
        .as_array()
        .and_then(|artifacts| {
            artifacts
                .iter()
                .find(|artifact| artifact["id"] == my.artifact)
        })
        .and_then(|artifact| artifact["size"].as_u64())
        .unwrap_or_else(|| panic!("versions.json has a size for {}", my.artifact))
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
    pub my: My,
    pub runtime_id: Uuid,
    pub runtime_dir: PathBuf,
    pub artifact_sha256: String,
    /// The revision of the package, as `pin_package` recorded it.
    pub revision: String,
    /// The SHA-256 of the manifest of that revision.
    pub manifest_sha256: String,
}

impl Site {
    /// The `BLOCKED` reason of a test whose artifact `my` is not in the
    /// runtime cache (spec 0042 AC-16).
    #[must_use]
    pub fn blocked_reason(my: My) -> String {
        format!("artifact {} is not in the runtime cache", my.artifact)
    }

    /// Installs `my` from the runtime cache into a new root and pins the
    /// package. Notes `evidence` as `BLOCKED` and returns `None` when the
    /// cache does not hold the artifact, so the test ends there.
    pub fn open(evidence: &Evidence, my: My) -> Option<Self> {
        Self::open_package(evidence, my, &package_dir())
    }

    /// [`Site::open`] with the package in `dir`, a copy of the package under
    /// test that a test changed.
    pub fn open_package(evidence: &Evidence, my: My, dir: &Path) -> Option<Self> {
        let root = TestRoot::new().expect("a test root can be created");
        let secrets = Arc::new(FakeSecretStore::new());
        let platform = platform_of(&root, &secrets);
        let package = package(&platform, dir);
        let sha256 = artifact_sha256(my);
        let Some(cache_file) = cached_artifact(&sha256) else {
            evidence.blocked(&Self::blocked_reason(my), OWNER);
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
            my.artifact,
            &cache_file,
        )
        .expect("the cached artifact installs, exec check included");
        let revision =
            pin_package(&platform, prepared.layout(), dir).expect("the package can be pinned");
        // A daemon with a catalog pins the same revision again and refuses
        // folders that are not private; the fixture pin makes them 0755.
        make_private(&prepared.layout().providers_dir());
        let revision_key = revision.revision.key().clone();
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
            my,
            runtime_id: runtime.id().as_uuid(),
            runtime_dir,
            artifact_sha256: sha256,
            revision: revision_key.revision().to_string(),
            manifest_sha256: revision_key.manifest_sha256().to_string(),
        })
    }

    /// Installs `other` from the runtime cache into the registry of this
    /// root, next to the runtime of the site. The daemon must be stopped.
    /// Notes `evidence` as `BLOCKED` and returns `None` when the cache does
    /// not hold the artifact.
    pub fn install_other(
        &self,
        evidence: &Evidence,
        other: My,
    ) -> Option<(RuntimeId, RuntimeLine)> {
        let Some(cache_file) = cached_artifact(&artifact_sha256(other)) else {
            evidence.blocked(&Self::blocked_reason(other), OWNER);
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

    /// The runtime and line of the site itself.
    #[must_use]
    pub fn own_runtime(&self) -> (RuntimeId, RuntimeLine) {
        (
            self.runtime_id
                .to_string()
                .parse::<RuntimeId>()
                .expect("a runtime id"),
            self.my.line.parse().expect("a runtime line"),
        )
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
            "mysql",
            server_version,
            Some(&self.artifact_sha256),
            Source::Managed,
        );
    }

    /// Starts a daemon in this process, with the supervisor constants of
    /// the tests and `config` adjusted by `tweak`.
    pub async fn in_process(&self, tweak: impl FnOnce(&mut DaemonConfig)) -> InProcess {
        self.in_process_on(self.platform.clone(), tweak).await
    }

    /// [`Site::in_process`] for a daemon that cannot reach the network and
    /// whose catalog holds the real `official/mysql` package of the
    /// repository, so `runtime.install` finds the artifacts of its
    /// `versions.json` (spec 0025, *Lingkungan test native*). The fetcher
    /// serves nothing: an install that is not answered by a staged download
    /// fails the test.
    ///
    /// The catalog is preloaded because `providers/` holds only the source
    /// of the catalog, `catalog.toml`, and the daemon reads the
    /// `catalog.json` that `cargo xtask catalog build` writes.
    pub async fn in_process_offline(&self, tweak: impl FnOnce(&mut DaemonConfig)) -> InProcess {
        let platform = self
            .platform
            .clone()
            .with_fetcher(Arc::new(FakeFetcher::new()));
        let entry = CatalogEntry::Provider(ProviderEntry {
            package: Arc::new(package(&self.platform, &package_dir())),
            tile: TileHint {
                mark: "My".to_owned(),
                tile: TileColor::Teal,
            },
            notes: Vec::new(),
            origin: stackly_core::catalog::EntryOrigin::Catalog,
        });
        self.in_process_on(platform, |config| {
            config.catalog = Some(CatalogOverride::Preloaded(Catalog::for_tests(&[entry])));
            tweak(config);
        })
        .await
    }

    /// Copies the cached archive of `my` into the download cache of the
    /// daemon, where `runtime.install` finds a finished download (spec 0018
    /// AC-6). Notes `evidence` as `BLOCKED` and returns `false` when the
    /// runtime cache does not hold the artifact, intact.
    pub fn stage_download(&self, evidence: &Evidence, my: My) -> bool {
        let staged = RuntimeCache::open().and_then(|cache| {
            cache
                .copy_to(
                    &artifact_sha256(my),
                    artifact_size(my),
                    &self.prepared.layout().downloads_dir(),
                )
                .expect("the cached artifact can be copied")
        });
        match staged {
            Some(file) => {
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
                    .expect("the staged download can be made private");
                true
            }
            None => {
                evidence.blocked(&Self::blocked_reason(my), OWNER);
                false
            }
        }
    }

    async fn in_process_on(
        &self,
        platform: Platform,
        tweak: impl FnOnce(&mut DaemonConfig),
    ) -> InProcess {
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
        let deps = server::DaemonDeps::new(platform, self.prepared.clone(), facts, Arc::new(log))
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

    /// `mysql` of the runtime against the instance `id`.
    #[must_use]
    pub fn mysql(&self, id: Uuid, port: u16) -> Mysql {
        self.mysql_in(&self.runtime_dir, id, port)
    }

    /// `mysql` of the runtime installed in `runtime_dir` against the
    /// instance `id`, for an instance that does not run on the runtime of
    /// the site.
    #[must_use]
    pub fn mysql_in(&self, runtime_dir: &Path, id: Uuid, port: u16) -> Mysql {
        Mysql {
            bin: runtime_dir.join("bin").join("mysql"),
            socket: self.run_dir(id).join("mysql.sock"),
            port,
            home: self.root.path().to_path_buf(),
        }
    }
}

/// Makes `dir` and every folder under it private (0700), the way the pin of
/// the daemon makes them.
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
    /// When the first handshake to this daemon succeeded, on the monotonic
    /// clock of the test.
    pub first_handshake: Instant,
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
            first_handshake: Instant::now(),
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
        daemon.first_handshake = Instant::now();
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

/// How `mysql` reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// The private Unix socket of the instance.
    Socket,
    /// TCP on the loopback address.
    Tcp,
}

/// What a `mysql` run printed.
#[derive(Debug)]
pub struct Ran {
    /// Whether `mysql` exited 0.
    pub ok: bool,
    /// Standard output, trimmed.
    pub stdout: String,
    /// Standard error, trimmed, without the warning `MYSQL_PWD` prints.
    pub stderr: String,
}

/// The `mysql` of the runtime, pointed at one instance. It runs with
/// `--no-defaults` and a closed environment, never with a password unless
/// the call gives one, which goes through `MYSQL_PWD`, never argv.
#[derive(Debug, Clone)]
pub struct Mysql {
    bin: PathBuf,
    socket: PathBuf,
    port: u16,
    home: PathBuf,
}

impl Mysql {
    /// Runs `sql` as `user`, with `password` when given.
    #[must_use]
    pub fn run(&self, via: Via, user: &str, password: Option<&str>, sql: &str) -> Ran {
        let mut args = vec!["--no-defaults".to_owned()];
        match via {
            Via::Socket => {
                args.push("--protocol=socket".to_owned());
                args.push(format!("--socket={}", self.socket.display()));
            }
            Via::Tcp => {
                args.push("--protocol=tcp".to_owned());
                args.push("--host=127.0.0.1".to_owned());
                args.push(format!("--port={}", self.port));
            }
        }
        args.extend([
            format!("--user={user}"),
            "--connect-timeout=5".to_owned(),
            "--batch".to_owned(),
            "--skip-column-names".to_owned(),
            format!("--execute={sql}"),
        ]);
        self.exec(&args, password)
    }

    /// Runs `sql` as `root` with `password`, over the socket.
    #[must_use]
    pub fn admin(&self, password: &str, sql: &str) -> Ran {
        self.run(Via::Socket, ROOT, Some(password), sql)
    }

    /// The output of the one row `sql` returns as `root`, over the socket.
    #[must_use]
    pub fn scalar(&self, password: &str, sql: &str) -> String {
        let ran = self.admin(password, sql);
        assert!(ran.ok, "{sql}: {}", ran.stderr);
        ran.stdout
    }

    /// The path of the socket of the instance.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    fn exec(&self, args: &[String], password: Option<&str>) -> Ran {
        let mut command = Command::new(&self.bin);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home);
        if let Some(password) = password {
            command.env("MYSQL_PWD", password);
        }
        let output = command.output().expect("mysql runs");
        let stderr: Vec<&str> = std::str::from_utf8(&output.stderr)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.contains("[Warning]"))
            .collect();
        Ran {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            stderr: stderr.join("\n").trim().to_owned(),
        }
    }
}

/// A free loopback port for an instance.
#[must_use]
pub fn free_port() -> u16 {
    allocate_port().expect("a free port can be allocated")
}

/// The params of a `create` of the MySQL package.
#[must_use]
pub fn create_params(
    site: &Site,
    name: &str,
    port: PortRequest,
    password: &str,
    settings: serde_json::Value,
) -> CreateInstanceParams {
    create_params_on(site.runtime_id, name, port, password, settings)
}

/// [`create_params`] for the runtime `runtime_id`, which does not have to be
/// the runtime of the site.
#[must_use]
pub fn create_params_on(
    runtime_id: Uuid,
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
        runtime_id,
        endpoints: vec![EndpointRequest {
            name: "client".to_owned(),
            port,
        }],
        settings: SettingsDocument::new(map),
        secrets: vec![SecretFieldInput {
            field: PASSWORD_FIELD.to_owned(),
            input: SecretInput::Value(SecretText::new(password.to_owned())),
        }],
        startup: None,
        duplicate_of: None,
    }
}

/// An instance made by [`create_instance`].
#[derive(Debug, Clone)]
pub struct Made {
    /// The instance.
    pub id: Uuid,
    /// The port of the `client` endpoint.
    pub port: u16,
    /// The password of `root`.
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

/// [`create_instance`] on the runtime `runtime_id`, with a new fixture
/// password: the create of an instance beside the ones of the site runtime.
pub async fn create_on(api: &Api, runtime_id: Uuid, name: &str) -> Made {
    let password = new_password();
    let port = free_port();
    let accepted = api
        .create(create_params_on(
            runtime_id,
            name,
            PortRequest::Fixed(port),
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
    Made { id, port, password }
}

/// [`create_on`] with the smallest settings the package takes, so several
/// servers of one test stay light.
pub async fn create_small_on(api: &Api, runtime_id: Uuid, name: &str) -> Made {
    let password = new_password();
    let port = free_port();
    let accepted = api
        .create(create_params_on(
            runtime_id,
            name,
            PortRequest::Fixed(port),
            &password,
            json!({ "innodbBufferPoolSizeMb": 64, "maxConnections": 20 }),
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
    Made { id, port, password }
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

/// SHA-256 and mode of every regular file under `root`, by path relative to
/// it. A folder that does not exist counts as an empty tree, because the log
/// folder of an instance may not exist yet.
#[must_use]
pub fn tree_digest(root: &Path) -> BTreeMap<String, (String, u32)> {
    use sha2::{Digest, Sha256};
    let mut digest = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("a folder entry can be read").path();
            let meta = std::fs::symlink_metadata(&path).expect("an entry has metadata");
            if meta.is_dir() {
                pending.push(path);
            } else if meta.is_file() {
                let bytes = std::fs::read(&path).expect("a file of the tree can be read");
                let relative = path
                    .strip_prefix(root)
                    .expect("the file is under the root")
                    .display()
                    .to_string();
                let hash: String = Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                digest.insert(relative, (hash, meta.permissions().mode() & 0o7777));
            }
        }
    }
    digest
}

/// A runtime installed by [`install_runtime`].
#[derive(Debug, Clone)]
pub struct Installed {
    /// The runtime ID, from the result of the install operation.
    pub id: Uuid,
    /// The folder of the runtime, from `runtime.get`.
    pub dir: PathBuf,
}

/// How long a native install may take: the archive is hashed, unpacked, and
/// checked with `codesign` on a busy Mac.
const INSTALL_PATIENCE: Duration = Duration::from_secs(300);

/// Installs `my` through `runtime.install` from the download cache staged by
/// [`Site::stage_download`] and waits until the operation succeeded.
pub async fn install_runtime(api: &Api, my: My) -> Installed {
    use stackly_protocol::operation::{GetOperationParams, OperationState};
    use stackly_protocol::runtime::{GetRuntimeParams, InstallOutcome, InstallRuntimeParams};
    use stackly_protocol::{Call, Reply};

    let mut options = stackly_ipc::CallOptions::default();
    options.idempotency_key = Some(format!("install-{}", my.artifact));
    let installed = api
        .client
        .call_detailed(
            Call::RuntimeInstall(InstallRuntimeParams {
                provider: PROVIDER.to_owned(),
                selector: my.line.to_owned(),
                expected_sha256: None,
            }),
            options,
        )
        .await;
    let Ok(Reply::RuntimeInstall(reply)) = installed else {
        panic!("runtime.install is not answered: {installed:?}");
    };
    assert_eq!(reply.resolved.artifact_id, my.artifact);
    let InstallOutcome::Started(accepted) = reply.outcome else {
        panic!("the runtime is not installed yet, so the install starts");
    };
    let deadline = tokio::time::Instant::now() + INSTALL_PATIENCE;
    let done = loop {
        let Ok(Reply::OperationGet(detail)) = api
            .client
            .call(Call::OperationGet(GetOperationParams {
                operation_id: accepted.operation.id,
            }))
            .await
        else {
            panic!("the install operation can be read");
        };
        if matches!(
            detail.operation.state,
            OperationState::Succeeded
                | OperationState::Failed
                | OperationState::Cancelled
                | OperationState::NeedsRecovery
        ) {
            break detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the install did not end: {:?}",
            detail.operation
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(
        done.operation.state,
        OperationState::Succeeded,
        "the install ended as {:?}",
        done.operation
    );
    let id = done
        .operation
        .result
        .as_ref()
        .and_then(|result| result.resources.first())
        .map(|resource| resource.id.parse::<Uuid>().expect("a runtime id"))
        .expect("the install result names the runtime");
    let Ok(Reply::RuntimeGet(detail)) = api
        .client
        .call(Call::RuntimeGet(GetRuntimeParams { runtime_id: id }))
        .await
    else {
        panic!("the runtime can be read");
    };
    Installed {
        id,
        dir: PathBuf::from(&detail.path),
    }
}

/// The runtime ID and line of `installed`, as `Site::rebind_instance` takes
/// them.
#[must_use]
pub fn runtime_pair(installed: &Installed, my: My) -> (RuntimeId, RuntimeLine) {
    (
        installed
            .id
            .to_string()
            .parse::<RuntimeId>()
            .expect("a runtime id"),
        my.line.parse().expect("a runtime line"),
    )
}

/// Copies the folder `from` into `to`, the way a test makes a changed copy
/// of the package.
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

/// A copy of the package in `root` whose `provider.yaml` has `from` replaced
/// by `to`.
#[must_use]
pub fn package_copy_with(root: &TestRoot, from: &str, to: &str) -> PathBuf {
    let dir = root.path().join("mysql");
    copy_tree(&package_dir(), &dir);
    let manifest = dir.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest can be read");
    let changed = text.replace(from, to);
    assert_ne!(text, changed, "the manifest holds {from}");
    std::fs::write(&manifest, changed).expect("the manifest can be written");
    dir
}

/// Creates an instance that is expected to fail and waits for the failed
/// operation: returns the instance it named and the operation.
pub async fn failed_create(
    site: &Site,
    api: &Api,
    name: &str,
    password: &str,
) -> (Uuid, stackly_protocol::operation::OperationDetail) {
    let accepted = api
        .create(create_params(
            site,
            name,
            PortRequest::Auto,
            password,
            json!({}),
        ))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id: Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    let detail = api
        .wait_state(
            accepted.operation.id,
            stackly_protocol::operation::OperationState::Failed,
        )
        .await;
    (id, detail)
}

/// The PIDs of the `mysqld` processes whose argv names `data` as their data
/// folder.
#[must_use]
pub fn mysqld_serving(data: &Path) -> Vec<u32> {
    let needle = format!("--datadir={}", data.display());
    output_of("/bin/ps", &["-axww", "-o", "pid=,command="])
        .lines()
        .filter(|line| line.contains("bin/mysqld") && line.contains(&needle))
        .filter_map(|line| line.split_whitespace().next()?.parse().ok())
        .collect()
}

/// The `(LISTEN)` lines and the Unix socket paths `lsof` shows for `pid`.
#[must_use]
pub fn listeners(pid: u32) -> (Vec<String>, Vec<String>) {
    let pid = pid.to_string();
    // `-i` and `-U` are ANDed by `-a`, so each kind is listed on its own.
    let tcp = output_of("/usr/sbin/lsof", &["-a", "-p", &pid, "-i", "-n", "-P"]);
    let unix = output_of("/usr/sbin/lsof", &["-a", "-p", &pid, "-U", "-n", "-P"]);
    let listening = tcp
        .lines()
        .filter(|line| line.contains("(LISTEN)"))
        .map(str::to_owned)
        .collect();
    let sockets = unix
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .filter(|name| name.starts_with('/'))
        .map(str::to_owned)
        .collect();
    (listening, sockets)
}

/// The value of the variable `name` as `root` reads it.
#[must_use]
pub fn variable(mysql: &Mysql, password: &str, name: &str) -> String {
    mysql.scalar(password, &format!("SELECT @@{name}"))
}

/// The text of every line of the durable log of `id` (spec 0027), oldest
/// first, read through `instance.logs_read`.
pub async fn log_lines(api: &Api, id: Uuid) -> Vec<String> {
    use stackly_protocol::logs::{LogAnchor, LogEntryKind, LogPosition, LogsReadParams};
    use stackly_protocol::{Call, Reply};

    let mut params = LogsReadParams {
        instance: id,
        streams: Vec::new(),
        contains: None,
        position: None,
        limit: Some(1000),
    };
    let mut lines = Vec::new();
    loop {
        let reply = api
            .client
            .call(Call::InstanceLogsRead(params.clone()))
            .await
            .expect("the log reads");
        let Reply::InstanceLogsRead(page) = reply else {
            panic!("not a logs_read reply: {reply:?}");
        };
        let mut texts: Vec<String> = page
            .entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                LogEntryKind::Line(line) => Some(line.text.clone()),
                _ => None,
            })
            .collect();
        texts.append(&mut lines);
        lines = texts;
        match page.earlier {
            Some(cursor) => params.position = Some(LogPosition::Before(LogAnchor { cursor })),
            None => return lines,
        }
    }
}

/// Creates an instance whose root password the daemon generates, and
/// returns it with the generated value, read from the fake secret store.
pub async fn create_generated(site: &Site, api: &Api, name: &str) -> Made {
    let port = free_port();
    let mut params = create_params(site, name, PortRequest::Fixed(port), "unused", json!({}));
    params.secrets = vec![SecretFieldInput {
        field: PASSWORD_FIELD.to_owned(),
        input: SecretInput::Generate,
    }];
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
    let done = api
        .wait_state(
            accepted.operation.id,
            stackly_protocol::operation::OperationState::Succeeded,
        )
        .await;
    assert_eq!(
        done.operation.state,
        stackly_protocol::operation::OperationState::Succeeded,
        "{done:?}"
    );
    let reference: stackly_domain::SecretRef =
        format!("local:instance/{}/root-password", instance_id(id))
            .parse()
            .expect("a secret reference");
    let password = site
        .secrets
        .stored_value(&reference)
        .expect("the generated password is in the secret store");
    Made { id, port, password }
}

/// Whether `data` holds a `binlog.*` file.
pub fn binlog_files(data: &std::path::Path) -> bool {
    std::fs::read_dir(data).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("binlog."))
    })
}
