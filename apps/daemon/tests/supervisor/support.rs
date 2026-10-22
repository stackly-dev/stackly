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
use sha2::{Digest, Sha256};
use stackly_core::supervisor::fixture::seed_fake;
use stackly_core::supervisor::{LogBatch, LogSink as SupervisorLogSink, SinkOutcome};
use stackly_core::{PreparedLayout, daemon_info, prepare_layout};
use stackly_domain::{
    ArtifactRecord, InstallMode, InstanceId, Registry, ResourceRevision, Runtime, RuntimeId,
    RuntimeState, Sha256Digest, StoredInstance, WriteBatch, WriteOp,
};
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, SystemClock, UserId};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::{
    CreateForm, CreateInstanceParams, CreatePreflight, DataUsage, DuplicateForm, EndpointRequest,
    GetCreateFormParams, GetDataUsageParams, GetDuplicateFormParams, GetInstanceDetailParams,
    GetInstanceParams, InstanceActionAccepted, InstanceDetail, InstancePage,
    InstanceReadinessState, InstanceSummary, ListInstancesParams, PortRequest,
    PreflightCreateParams, ProviderSelector, SecretFieldInput, SecretPlan, SecretSource,
    SettingsDocument, StartInstanceParams, StopInstanceParams,
};
use stackly_protocol::operation::{GetOperationParams, OperationDetail, OperationState};
use stackly_protocol::secret::{SecretInput, SecretMode, SecretText, SetSecretParams};
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
        Self::build(false)
    }

    /// The same, laid out as `STACKLY_ROOT` lays out a root, so the real
    /// `stackly` binary with that override finds the daemon socket.
    pub fn for_cli() -> Self {
        Self::build(true)
    }

    fn build(as_override: bool) -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let secrets = Arc::new(FakeSecretStore::new());
        let platform = if as_override {
            MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override")
        } else {
            MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version")
        }
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
        let reap = ReapGuard::new(root.path(), prepared.layout().roots().runtime());
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

    /// Removes the folder of the pinned fake package, which the seed wrote
    /// with the permissions of a test rather than the private ones `pin`
    /// writes. A create from the catalog pins it again; the registry row of
    /// the revision stays.
    pub fn unpin_fake_package(&self) {
        std::fs::remove_dir_all(self.prepared.layout().providers_dir().join("testing"))
            .expect("the pinned fake package folder is removed");
    }

    /// The folders of the instance `id` the daemon made.
    pub fn instance_paths(&self, id: Uuid) -> stackly_platform::InstancePaths {
        self.prepared
            .layout()
            .instance_paths(InstanceId::from_uuid(id).expect("an instance id the daemon made"))
    }

    /// Opens the registry of the root for a test write. No daemon may hold
    /// the root meanwhile; the registry is closed again when the caller
    /// drops it.
    pub fn open_registry(&self) -> impl Registry + use<> {
        let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
            .open(
                &self.platform,
                self.prepared.layout(),
                daemon_info().version,
            )
            .expect("the registry of the root opens")
        else {
            panic!("the registry of the root opens ready");
        };
        registry
    }

    /// [`install_fake_runtime`](stackly_core::supervisor::fixture::install_fake_runtime)
    /// for a runtime of `version` and `line` in place of the first artifact
    /// of the package, committed with the daemon stopped (spec 0025, step
    /// 4). The registry keeps one managed runtime per artifact digest, so
    /// the folder holds a stand in that differs per version instead of a
    /// copy of the dummy server; a refused start never reaches its spawn.
    /// Returns the runtime ID.
    pub fn install_fake_runtime_line(&self, version: &str, line: &str) -> Uuid {
        let layout = self.prepared.layout();
        let registry = self.open_registry();
        let first = registry
            .runtimes()
            .expect("the registry reads")
            .value
            .into_iter()
            .next()
            .expect("the root holds the seeded fake runtime");
        let version: stackly_domain::ExactVersion = version.parse().expect("an exact version");
        let bytes =
            format!("#!/bin/sh\n# stand in for the fake runtime {version}, never run\nexit 0\n");
        let digest = Sha256Digest::from_bytes(Sha256::digest(bytes.as_bytes()).into());
        let dir = layout.runtime_dir(first.software(), &version, first.target(), &digest);
        std::fs::create_dir_all(&dir).expect("the runtime folder is made");
        let executable = dir.join(stackly_core::supervisor::fixture::FAKE_EXECUTABLE);
        std::fs::write(&executable, bytes).expect("the stand in is written");
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
                .expect("the executable bit is set");
        }
        let runtime = Runtime::restore(stackly_domain::StoredRuntime {
            id: RuntimeId::generate(),
            software: first.software().clone(),
            version: version.clone(),
            line: line.parse().expect("a runtime line"),
            target: first.target(),
            install: InstallMode::Managed {
                artifact_sha256: digest,
            },
            state: RuntimeState::Installed,
            installed_at: first.installed_at(),
            last_verified_at: None,
            depends_on: std::collections::BTreeSet::new(),
            revision: ResourceRevision::INITIAL,
        })
        .expect("the runtime row is consistent");
        let artifact = ArtifactRecord::new(
            digest,
            first.software().clone(),
            version,
            first.target(),
            None,
            None,
        );
        let id = runtime.id().as_uuid();
        registry
            .commit(WriteBatch::new(vec![
                WriteOp::InsertArtifact(artifact),
                WriteOp::InsertRuntime(runtime),
            ]))
            .expect("the second runtime is committed");
        id
    }

    /// Binds instance `id` to runtime `runtime_id` in the registry, with
    /// the daemon stopped: a test only write, because a rebind to another
    /// line is exactly what the domain refuses. The revision rises by 1
    /// like any mutation, and no file of the instance changes.
    pub fn bind_runtime(&self, id: Uuid, runtime_id: Uuid) {
        let registry = self.open_registry();
        let instance_id = InstanceId::from_uuid(id).expect("an instance id the daemon made");
        let instance = registry
            .instance(instance_id)
            .expect("the registry reads")
            .value
            .expect("the instance exists");
        let expected = instance.revision();
        let bound = stackly_domain::Instance::restore(
            StoredInstance {
                id: instance.id(),
                name: instance.name().clone(),
                display_name: instance.display_name().cloned(),
                provider_revision: instance.provider_revision().clone(),
                software: instance.software().clone(),
                runtime_id: RuntimeId::from_uuid(runtime_id).expect("a runtime id"),
                data_line: instance.data_line().clone(),
                data_version: instance.data_version().clone(),
                data_initialized_at: instance.data_initialized_at(),
                endpoints: instance.endpoints().to_vec(),
                desired: instance.desired(),
                startup: instance.startup(),
                lifecycle: instance.lifecycle(),
                last_failure: instance.last_failure().cloned(),
                revision: ResourceRevision::new(expected.get() + 1)
                    .expect("the next revision is valid"),
                created_at: instance.created_at(),
                created_by: instance.created_by(),
                retained_at: instance.retained_at(),
                retained_by: instance.retained_by(),
            },
            SystemClock.now(),
        )
        .expect("the bound instance is consistent");
        registry
            .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
                instance: bound,
                expected,
            }]))
            .expect("the instance row is rewritten");
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

    /// The params of a preflight of the same draft as [`params`](Self::params),
    /// with the token as a value to be sent.
    pub fn preflight_params(&self, name: &str, extra: Value) -> PreflightCreateParams {
        let create = self.params(name, extra);
        PreflightCreateParams {
            name: create.name,
            display_name: create.display_name,
            provider: create.provider,
            runtime_id: create.runtime_id,
            endpoints: create.endpoints,
            settings: create.settings,
            secrets: vec![SecretPlan {
                field: "authToken".to_owned(),
                source: SecretSource::Value,
            }],
            start: false,
        }
    }

    /// `instance.get_create_form`.
    pub async fn form(
        &self,
        runtime_id: Uuid,
        provider: Option<&str>,
    ) -> Result<CreateForm, Box<ErrorBody>> {
        match self
            .client
            .call(Call::InstanceGetCreateForm(GetCreateFormParams {
                runtime_id,
                provider: provider.map(str::to_owned),
            }))
            .await?
        {
            Reply::InstanceGetCreateForm(form) => Ok(*form),
            other => panic!("not a create form reply: {other:?}"),
        }
    }

    /// `instance.get_duplicate_form`.
    pub async fn duplicate_form(&self, source: Uuid) -> Result<DuplicateForm, Box<ErrorBody>> {
        match self
            .client
            .call(Call::InstanceGetDuplicateForm(GetDuplicateFormParams {
                instance_id: source,
            }))
            .await?
        {
            Reply::InstanceGetDuplicateForm(form) => Ok(*form),
            other => panic!("not a duplicate form reply: {other:?}"),
        }
    }

    /// `instance.create` with the draft of a preflight and the secret
    /// inputs a client builds from the choices of the user, as the sheet
    /// and `stackly instance duplicate` do.
    pub async fn create_from_draft(
        &self,
        draft: PreflightCreateParams,
        secrets: Vec<SecretFieldInput>,
    ) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        self.create(CreateInstanceParams {
            name: draft.name,
            display_name: draft.display_name,
            provider: draft.provider,
            runtime_id: draft.runtime_id,
            endpoints: draft.endpoints,
            settings: draft.settings,
            secrets,
        })
        .await
    }

    /// `instance.preflight`.
    pub async fn preflight(
        &self,
        params: PreflightCreateParams,
    ) -> Result<CreatePreflight, Box<ErrorBody>> {
        match self.client.call(Call::InstancePreflight(params)).await? {
            Reply::InstancePreflight(preflight) => Ok(*preflight),
            other => panic!("not a preflight reply: {other:?}"),
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

    /// `instance.get_detail`.
    pub async fn detail(&self, id: Uuid) -> Result<InstanceDetail, Box<ErrorBody>> {
        match self
            .client
            .call(Call::InstanceGetDetail(GetInstanceDetailParams {
                instance_id: id,
            }))
            .await?
        {
            Reply::InstanceGetDetail(detail) => Ok(*detail),
            other => panic!("not a detail reply: {other:?}"),
        }
    }

    /// `instance.get_data_usage`; `None` is a reply without a size.
    pub async fn data_usage(
        &self,
        id: Uuid,
        refresh: bool,
        cached_only: bool,
    ) -> Result<Option<DataUsage>, Box<ErrorBody>> {
        match self
            .client
            .call(Call::InstanceGetDataUsage(GetDataUsageParams {
                instance_id: id,
                refresh,
                cached_only,
            }))
            .await?
        {
            Reply::InstanceGetDataUsage(reply) => Ok(reply.usage),
            other => panic!("not a data usage reply: {other:?}"),
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

/// One request to the dummy server on `port`: the status code and the body.
fn http_exchange(port: u16, request: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
        .expect("the dummy server accepts connections");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout is set");
    stream
        .write_all(request.as_bytes())
        .expect("the request is written");
    let mut text = String::new();
    stream.read_to_string(&mut text).expect("the reply is read");
    let status = text
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    (status, body)
}

/// `PUT /sentinel` of the dummy server on `port`; returns the status code.
pub fn put_sentinel(port: u16, value: &str) -> u16 {
    http_exchange(
        port,
        &format!(
            "PUT /sentinel HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{value}",
            value.len()
        ),
    )
    .0
}

/// `GET /sentinel` of the dummy server on `port`: the value it holds, or
/// `None` for a 404.
pub fn get_sentinel(port: u16) -> Option<String> {
    let (status, body) = http_exchange(
        port,
        "GET /sentinel HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    match status {
        200 => Some(body),
        404 => None,
        other => panic!("GET /sentinel answered {other}"),
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
pub fn shutdown_server(port: u16) {
    use std::io::{Read, Write};
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

/// A catalog that holds the fake provider, so a create without a revision
/// has the revision of the catalog to take (spec 0020).
pub fn fake_catalog() -> stackly_core::catalog::Catalog {
    use stackly_core::catalog::{Catalog, CatalogEntry, ProviderEntry, TileColor, TileHint};
    use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let files = read_package(&stackly_platform_macos::MacosFilesystem, &dir)
        .expect("the fake package reads");
    let package = ValidatedPackage::from_files(files, NameCheck::FolderName)
        .expect("the fake package validates");
    Catalog::for_tests(&[CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package),
        tile: TileHint {
            mark: "Fk".to_owned(),
            tile: TileColor::Blue,
        },
        notes: Vec::new(),
    })])
}

/// The fast config with [`fake_catalog`] as the catalog of the daemon.
pub fn fast_with_catalog() -> DaemonConfig {
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(fake_catalog()));
    config
}

/// A Keychain call of another action that waits behind its dialog.
pub struct Dialog {
    hold: stackly_testkit::SecretHold,
    waiting:
        tokio::task::JoinHandle<Result<stackly_protocol::Reply, Box<stackly_protocol::ErrorBody>>>,
}

impl Dialog {
    /// Opens the dialog: the call of another connection reaches the store
    /// and waits there.
    pub async fn open(site: &Site, reference: &str) -> Self {
        let hold = site.secrets.hold_next();
        let other = site.api().await;
        let reference = reference.to_owned();
        let waiting = tokio::spawn(async move {
            other
                .client
                .call(Call::SecretSet(SetSecretParams {
                    reference,
                    input: SecretInput::Value(SecretText::new("perf-fixture-value".to_owned())),
                    mode: SecretMode::Create,
                    expected_revision: None,
                    confirm_stored_only: false,
                }))
                .await
        });
        assert!(
            hold.wait_until_held(Duration::from_secs(10)),
            "the other action reached the Keychain and waits"
        );
        Self { hold, waiting }
    }

    /// Answers the dialog and waits for the other action to end.
    pub async fn close(self) {
        self.hold.release();
        self.waiting
            .await
            .expect("the waiting call ends")
            .expect("the other action succeeds");
    }
}
