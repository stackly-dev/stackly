//! Helpers of the supervisor tests: a test root with the fake runtime and
//! the fake provider installed, a daemon started in process, and a typed
//! client of the instance methods.
#![allow(
    dead_code,
    reason = "each scenario module uses a part of these helpers"
)]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use stackly_core::supervisor::fixture::seed_fake;
use stackly_core::supervisor::{LogBatch, LogSink as SupervisorLogSink, SinkOutcome};
use stackly_core::{PreparedLayout, daemon_info, prepare_layout};
use stackly_domain::InstanceId;
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, SystemClock, UserId};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, GetInstanceParams, InstanceActionAccepted, InstancePage,
    InstanceReadinessState, InstanceSummary, ListInstancesParams, PortRequest, ProviderSelector,
    SecretFieldInput, SettingsDocument, StartInstanceParams, StopInstanceParams,
};
use stackly_protocol::operation::{GetOperationParams, OperationDetail, OperationState};
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::{Call, ErrorBody, Reply, Timestamp};
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::{FakeSecretStore, ReapGuard, TestRoot, fake_server};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonConfig, DaemonDeps, DaemonHandle};
use uuid::Uuid;

/// How long a test waits for something that should happen.
pub const PATIENCE: Duration = Duration::from_secs(30);
/// The provider of the fake package and its revision.
pub const PROVIDER: &str = "testing/fake";
/// The revision of the fake package.
pub const REVISION: &str = "0.3.0";
/// The auth token every test gives `authToken`.
pub const TOKEN: &str = "fixture-token-for-the-fake-provider";

/// A daemon config whose supervisor constants take milliseconds.
#[must_use]
pub fn fast() -> DaemonConfig {
    let mut config = DaemonConfig::default();
    let supervisor = &mut config.supervisor;
    supervisor.helper_term_grace = Duration::from_millis(200);
    supervisor.readiness_refresh_interval = Duration::from_millis(200);
    supervisor.restart_base_delay = Duration::from_millis(50);
    supervisor.restart_max_delay = Duration::from_millis(400);
    supervisor.spool_drain_interval = Duration::from_millis(50);
    config
}

/// One test's root: the platform, the layout, the fake secret store, the
/// daemon lock, and the fake runtime and package already in the registry.
pub struct Site {
    /// Declared first, so it drops first: it stops the instance processes
    /// that name this root, then removes the runtime root.
    _reap: ReapGuard,
    pub root: TestRoot,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    pub secrets: Arc<FakeSecretStore>,
    pub runtime_id: Uuid,
    _lock: DaemonLock,
}

impl Site {
    /// A root with the fake runtime and the fake provider seeded.
    pub fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let secrets = Arc::new(FakeSecretStore::new());
        let platform = MacosPlatform::with_roots(root.roots())
            .with_secrets(Arc::clone(&secrets) as Arc<dyn stackly_platform::SecretStore>);
        let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            "0.1.0",
        );
        let lock = DaemonLock::acquire(&prepared.layout().daemon_lock(), owner)
            .expect("the test root's lock is free");
        let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
            .open(&platform, prepared.layout(), daemon_info().version)
            .expect("a fresh registry opens")
        else {
            panic!("a fresh registry opens ready");
        };
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
        let seeded = seed_fake(
            &platform,
            prepared.layout(),
            &registry,
            &fake_server::binary(),
            &package,
        )
        .expect("the fake runtime and package are seeded");
        // The registry closes here, so the daemon opens it alone.
        drop(registry);
        let reap = ReapGuard::new(root.path(), root.roots().runtime());
        Self {
            _reap: reap,
            root,
            platform,
            prepared,
            secrets,
            runtime_id: seeded.runtime_id.as_uuid(),
            _lock: lock,
        }
    }

    pub fn socket(&self) -> PathBuf {
        self.prepared.layout().daemon_socket()
    }

    pub fn uid(&self) -> u32 {
        match self
            .platform
            .identity()
            .current_user()
            .expect("the current user can be read")
        {
            UserId::Unix(uid) => uid,
            other => panic!("not a unix user: {other:?}"),
        }
    }

    /// The deps of a daemon on this site with `config`.
    pub fn deps(&self, config: DaemonConfig) -> DaemonDeps {
        let log = LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON)
            .expect("the log writer starts");
        let facts = StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            true,
            std::process::id(),
        );
        DaemonDeps::new(
            self.platform.clone(),
            self.prepared.clone(),
            facts,
            Arc::new(log),
        )
        .with_config(config)
    }

    /// Starts a daemon with `config` and waits for its start slot.
    pub async fn start_with(&self, config: DaemonConfig) -> DaemonHandle {
        let handle = server::start(self.deps(config))
            .await
            .expect("the server starts");
        handle.started().await;
        handle
    }

    /// Starts a daemon with the fast supervisor constants.
    pub async fn start(&self) -> DaemonHandle {
        self.start_with(fast()).await
    }

    /// A typed client of this site's uid, as an interactive CLI.
    pub async fn api(&self) -> Api {
        self.api_try().await.expect("the client connects")
    }

    /// The same, or `None` while the daemon does not accept a client.
    pub async fn api_try(&self) -> Option<Api> {
        let (client, _) = Client::connect(
            &self.socket(),
            ConnectPolicy::same_user(self.uid()),
            ClientInfo {
                kind: ClientKind::Cli,
                version: "0.1.0".to_owned(),
                pid: Some(std::process::id()),
                interactive: Some(true),
            },
        )
        .await
        .ok()?;
        Some(Api {
            client,
            runtime_id: self.runtime_id,
        })
    }
}

/// The settings of an instance of the fake provider, besides its token.
#[must_use]
pub fn settings(extra: Value) -> SettingsDocument {
    let map = match extra {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => panic!("settings must be an object, not {other}"),
    };
    SettingsDocument::new(map)
}

/// A typed client of the instance methods.
pub struct Api {
    pub client: Client,
    pub runtime_id: Uuid,
}

/// Installs the fake runtime and the fake package in the registry of a
/// root the `stacklyd` binary will serve with `STACKLY_ROOT`, and returns
/// the ID of the runtime. The registry is closed again when this returns.
pub fn seed_root(root: &TestRoot) -> Uuid {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid root override");
    let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
    let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
        .open(&platform, prepared.layout(), daemon_info().version)
        .expect("a fresh registry opens")
    else {
        panic!("a fresh registry opens ready");
    };
    let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let seeded = seed_fake(
        &platform,
        prepared.layout(),
        &registry,
        &fake_server::binary(),
        &package,
    )
    .expect("the fake runtime and package are seeded");
    drop(registry);
    seeded.runtime_id.as_uuid()
}

/// A guard for a root a `stacklyd` binary serves with `STACKLY_ROOT`: on
/// drop it stops every process whose argv names the root (the instances a
/// stopped daemon leaves running) and removes the root's runtime root. It
/// matches only paths under this root's own canonical path, never a name
/// pattern. Declare it right after the root, before the daemons.
pub fn reap_guard(root: &TestRoot) -> ReapGuard {
    let platform =
        MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid root override");
    let runtime = platform
        .directories()
        .roots()
        .expect("the macOS platform resolved its roots")
        .runtime()
        .to_path_buf();
    ReapGuard::new(root.path(), &runtime)
}

impl Api {
    /// A client from a connection the caller made.
    pub const fn new(client: Client, runtime_id: Uuid) -> Self {
        Self { client, runtime_id }
    }
}

impl Api {
    /// The params of a create on the fake provider with the test token.
    pub fn params(&self, name: &str, extra: Value) -> CreateInstanceParams {
        CreateInstanceParams {
            name: name.to_owned(),
            display_name: None,
            provider: ProviderSelector {
                name: PROVIDER.to_owned(),
                revision: Some(REVISION.to_owned()),
            },
            runtime_id: self.runtime_id,
            endpoints: Vec::new(),
            settings: settings(extra),
            secrets: vec![SecretFieldInput {
                field: "authToken".to_owned(),
                input: SecretInput::Value(SecretText::new(TOKEN.to_owned())),
            }],
        }
    }

    /// A request for a fixed port on the `http` endpoint.
    pub fn http_port(port: u16) -> EndpointRequest {
        EndpointRequest {
            name: "http".to_owned(),
            port: PortRequest::Fixed(port),
        }
    }

    /// Creates with a fresh key.
    pub async fn create(
        &self,
        params: CreateInstanceParams,
    ) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        self.create_with_key(params, &key()).await
    }

    /// Creates with `key`.
    pub async fn create_with_key(
        &self,
        params: CreateInstanceParams,
        key: &str,
    ) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(
                Call::InstanceCreate(params),
                CallOptions::default().with_idempotency_key(key),
            )
            .await?
        {
            Reply::InstanceCreate(accepted) => Ok(*accepted),
            other => panic!("not a create reply: {other:?}"),
        }
    }

    /// Creates an instance and waits until its operation succeeded;
    /// returns its ID.
    pub async fn create_ready(&self, name: &str, extra: Value) -> Uuid {
        let accepted = self
            .create(self.params(name, extra))
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
        let detail = self
            .wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
        assert_eq!(detail.operation.state, OperationState::Succeeded);
        id
    }

    /// `instance.start` with a fresh key.
    pub async fn start(&self, id: Uuid) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        self.start_with_key(id, &key(), None).await
    }

    /// `instance.start` with `key` and an expected revision.
    pub async fn start_with_key(
        &self,
        id: Uuid,
        key: &str,
        expected_revision: Option<stackly_protocol::Revision>,
    ) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(
                Call::InstanceStart(StartInstanceParams {
                    instance_id: id,
                    expected_revision,
                }),
                CallOptions::default().with_idempotency_key(key),
            )
            .await?
        {
            Reply::InstanceStart(accepted) => Ok(*accepted),
            other => panic!("not a start reply: {other:?}"),
        }
    }

    /// `instance.stop` with a fresh key.
    pub async fn stop(&self, id: Uuid) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(
                Call::InstanceStop(StopInstanceParams {
                    instance_id: id,
                    expected_revision: None,
                    override_leases: false,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceStop(accepted) => Ok(*accepted),
            other => panic!("not a stop reply: {other:?}"),
        }
    }

    /// `instance.stop` that waits out `resource.busy`: the start operation
    /// the daemon began after reconciliation may still hold the instance
    /// for a moment after the instance reads ready.
    pub async fn stop_when_free(&self, id: Uuid) -> InstanceActionAccepted {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            match self.stop(id).await {
                Ok(accepted) => return accepted,
                Err(error) if error.code() == "resource.busy" => {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "the instance stayed busy"
                    );
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => panic!("the stop is refused: {error:?}"),
            }
        }
    }

    /// `instance.get`.
    pub async fn get(&self, id: Uuid) -> Result<InstanceSummary, Box<ErrorBody>> {
        match self
            .client
            .call(Call::InstanceGet(GetInstanceParams { instance_id: id }))
            .await?
        {
            Reply::InstanceGet(summary) => Ok(*summary),
            other => panic!("not a get reply: {other:?}"),
        }
    }

    /// `instance.list`.
    pub async fn list(&self, params: ListInstancesParams) -> InstancePage {
        match self.client.call(Call::InstanceList(params)).await {
            Ok(Reply::InstanceList(page)) => *page,
            other => panic!("instance.list: {other:?}"),
        }
    }

    /// `operation.get`.
    pub async fn operation(&self, id: Uuid) -> OperationDetail {
        match self
            .client
            .call(Call::OperationGet(GetOperationParams { operation_id: id }))
            .await
        {
            Ok(Reply::OperationGet(detail)) => *detail,
            other => panic!("operation.get of {id}: {other:?}"),
        }
    }

    /// Waits until the operation is in `state`.
    pub async fn wait_state(&self, id: Uuid, state: OperationState) -> OperationDetail {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let detail = self.operation(id).await;
            if detail.operation.state == state {
                return detail;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "operation {id} did not reach {state:?}: {detail:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Waits until the operation ended in any final state.
    pub async fn wait_final(&self, id: Uuid) -> OperationDetail {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let detail = self.operation(id).await;
            if matches!(
                detail.operation.state,
                OperationState::Succeeded
                    | OperationState::Failed
                    | OperationState::Cancelled
                    | OperationState::NeedsRecovery
            ) {
                return detail;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "operation {id} did not end: {detail:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Waits until the instance matches `ready`.
    pub async fn wait_instance(
        &self,
        id: Uuid,
        what: &str,
        ready: impl Fn(&InstanceSummary) -> bool,
    ) -> InstanceSummary {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let summary = self.get(id).await.expect("the instance can be read");
            if ready(&summary) {
                return summary;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "instance {id} did not get {what}: {summary:#?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Starts the instance and waits until it is `running` and `ready`.
    pub async fn start_ready(&self, id: Uuid) -> InstanceSummary {
        let accepted = self
            .start(id)
            .await
            .expect("the start is accepted")
            .accepted
            .expect("a start from stopped begins an operation");
        self.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
        self.wait_instance(id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await
    }

    /// The code of the error of a failed operation.
    pub fn failure_code(detail: &OperationDetail) -> String {
        detail
            .operation
            .failure
            .as_ref()
            .expect("the failed operation carries its error")
            .error
            .code()
            .to_owned()
    }
}

/// A fresh idempotency key.
pub fn key() -> String {
    Uuid::now_v7().to_string()
}

/// The PIDs of the dummy server processes that serve `data`, found by the
/// argv they run with.
pub fn serving(data: &Path) -> Vec<u32> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,command="])
        .output()
        .expect("ps runs");
    let needle = format!("--data={}", data.display());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(" serve ") && line.contains(&needle))
        .filter_map(|line| line.split_whitespace().next()?.parse().ok())
        .collect()
}

/// Whether a process with `pid` exists.
pub fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Tells the dummy server on `port` to exit, as `POST /shutdown` does.
///
/// It waits a moment first, so a stop accepted just before has read the
/// instance and its process before the process leaves: a stop that finds
/// the lifecycle `stopping` but no process fails `identity_unproven`, and
/// the exit of the process can land in that window (a race of the stop
/// operation, not of this helper).
pub fn shutdown_server(port: u16) {
    use std::io::{Read, Write};
    std::thread::sleep(Duration::from_millis(300));
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
        .expect("the dummy server accepts connections");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout is set");
    write!(
        stream,
        "POST /shutdown HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is written");
    let mut text = String::new();
    let _ = stream.read_to_string(&mut text);
}

/// A log sink that records every batch it is given (spec 0010 K5).
#[derive(Default)]
pub struct LogRecorder {
    batches: std::sync::Mutex<Vec<(InstanceId, LogBatch)>>,
}

impl LogRecorder {
    /// The number of lines recorded for the instance `id`.
    pub fn lines_of(&self, id: Uuid) -> usize {
        self.batches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(instance, _)| instance.as_uuid() == id)
            .map(|(_, batch)| batch.lines.len())
            .sum()
    }

    /// The text of every line recorded for the instance `id`.
    pub fn text_of(&self, id: Uuid) -> Vec<String> {
        self.batches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(instance, _)| instance.as_uuid() == id)
            .flat_map(|(_, batch)| batch.lines.iter().map(|line| line.text.clone()))
            .collect()
    }
}

impl SupervisorLogSink for LogRecorder {
    fn accept(&self, instance: InstanceId, batch: LogBatch) -> SinkOutcome {
        self.batches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((instance, batch));
        SinkOutcome::Accepted
    }
}
