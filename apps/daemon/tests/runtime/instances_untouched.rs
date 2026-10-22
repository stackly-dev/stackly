//! Installing a runtime leaves the instances that exist alone (spec 0025
//! AC-1, fixture part, REQ-043, BR-001): a daemon in process on a real
//! registry with two instances of the fake provider, one `running` and
//! `ready` and one `stopped`, and a `runtime.install` of another line of the
//! same software, served by the fake fetcher, once failing at the Verify
//! digest stage and once succeeding.
//!
//! The instances run as real processes of the dummy server; only the
//! download and the version check of the install are fake.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::json;
use stackly_core::catalog::Catalog;
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::runtime::samples::TarBuilder;
use stackly_core::supervisor::fixture::seed_fake;
use stackly_core::{PreparedLayout, daemon_info, prepare_layout};
use stackly_domain::{InstanceId, Pid};
use stackly_ipc::{Client, ConnectPolicy};
use stackly_platform::{
    CapturedOutput, ChildRole, Clock, ExitListener, ExitStatus, FileIdentity, HelperHandle,
    HoldsFile, LockOutcome, LockState, MainHandle, Platform, ProcessError, ProcessFacts,
    ProcessSupervision, ProviderSignal, SignalTarget, SpawnRequest, SpoolReader, SpoolWriter,
    SuspendedChild, SystemClock, UserId,
};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::codes::runtime as runtime_codes;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, EndpointStatus, GetInstanceParams, InstanceDesired,
    InstanceLifecycle, InstanceReadinessState, InstanceRestart, InstanceSummary,
    ListInstancesParams, PortRequest, ProviderPin, ProviderSelector, SecretFieldInput,
    SettingsDocument, StartInstanceParams, StopInstanceParams,
};
use stackly_protocol::operation::{GetOperationParams, OperationDetail, OperationState};
use stackly_protocol::runtime::{
    InstallOutcome, InstallRuntimeParams, ListRuntimesParams, RuntimeSummary,
};
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::{Call, Reply, Timestamp};
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{
    FakePlatform, FakeProcessSupervision, FakeSecretStore, ReapGuard, TestRoot, allocate_port,
    fake_server,
};
use stacklyd::catalog::CatalogOverride;
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonConfig, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use crate::support::{call_with_key, catalog};

/// How long the test waits for something that should happen.
const PATIENCE: Duration = Duration::from_secs(30);
/// Where the artifact of the second line is served from.
const URL: &str = "https://downloads.example.test/fake-0.2.0.tar.gz";
/// The provider of the fake package.
const PROVIDER: &str = "testing/fake";
/// The revision of the fake package.
const REVISION: &str = "0.3.0";
/// The auth token the instances are created with.
const TOKEN: &str = "fixture-token-for-the-fake-provider";

/// Spawns the version checks of an install as fake processes and every
/// other process, the instances included, as real ones.
///
/// Only the methods an install reaches for a check helper are routed by the
/// ID of the process; the rest belong to the real adapter.
struct CheckFaking {
    real: Platform,
    checks: FakeProcessSupervision,
    check_pids: Mutex<HashSet<Pid>>,
}

impl CheckFaking {
    fn is_check(&self, pid: Pid) -> bool {
        self.check_pids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&pid)
    }

    fn real(&self) -> &dyn ProcessSupervision {
        self.real.processes()
    }
}

impl ProcessSupervision for CheckFaking {
    fn spawn_main(&self, request: SpawnRequest<'_>) -> Result<SuspendedChild, ProcessError> {
        self.real().spawn_main(request)
    }

    fn resume(&self, child: SuspendedChild) -> Result<MainHandle, ProcessError> {
        self.real().resume(child)
    }

    fn discard(&self, child: SuspendedChild) -> Result<Option<ExitStatus>, ProcessError> {
        self.real().discard(child)
    }

    fn spawn_helper(&self, request: SpawnRequest<'_>) -> Result<HelperHandle, ProcessError> {
        if request.role != ChildRole::RuntimeCheck {
            return self.real().spawn_helper(request);
        }
        let handle = self.checks.spawn_helper(request)?;
        self.check_pids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(handle.pid());
        Ok(handle)
    }

    fn inspect(&self, pid: Pid) -> Result<Option<ProcessFacts>, ProcessError> {
        self.real().inspect(pid)
    }

    fn holds_file(&self, pid: Pid, file: FileIdentity) -> Result<HoldsFile, ProcessError> {
        self.real().holds_file(pid, file)
    }

    fn signal(&self, target: &SignalTarget, signal: ProviderSignal) -> Result<(), ProcessError> {
        self.real().signal(target, signal)
    }

    fn continue_stopped(&self, target: &SignalTarget) -> Result<(), ProcessError> {
        self.real().continue_stopped(target)
    }

    fn terminate_helper_group(
        &self,
        helper: &HelperHandle,
        force: bool,
    ) -> Result<(), ProcessError> {
        if self.is_check(helper.pid()) {
            self.checks.terminate_helper_group(helper, force)
        } else {
            self.real().terminate_helper_group(helper, force)
        }
    }

    fn group_members(&self, group: Pid) -> Result<Vec<Pid>, ProcessError> {
        if self.is_check(group) {
            self.checks.group_members(group)
        } else {
            self.real().group_members(group)
        }
    }

    fn boot_id(&self) -> Result<stackly_domain::BootId, ProcessError> {
        self.real().boot_id()
    }

    fn watch_exit(&self, pid: Pid, listener: Arc<dyn ExitListener>) -> Result<(), ProcessError> {
        if self.is_check(pid) {
            self.checks.watch_exit(pid, listener)
        } else {
            self.real().watch_exit(pid, listener)
        }
    }

    fn lock_instance(&self, path: &Path) -> Result<LockOutcome, ProcessError> {
        self.real().lock_instance(path)
    }

    fn probe_lock(&self, path: &Path) -> Result<LockState, ProcessError> {
        self.real().probe_lock(path)
    }

    fn create_spool(&self, path: &Path) -> Result<SpoolWriter, ProcessError> {
        self.real().create_spool(path)
    }

    fn open_spool(&self, path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError> {
        self.real().open_spool(path)
    }
}

/// One test's root: the dummy server seeded as the first runtime, a fake
/// download, and the real process supervision for everything but the
/// version checks.
struct Site {
    /// Declared first, so it drops first: it stops the instance processes
    /// that name this root.
    _reap: ReapGuard,
    _root: TestRoot,
    fake: FakePlatform,
    platform: Platform,
    prepared: PreparedLayout,
    runtime_id: Uuid,
    _lock: DaemonLock,
}

impl Site {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let base =
            MacosPlatform::with_roots(root.roots())
                .expect("this Mac reports its OS version")
                .with_secrets(
                    Arc::new(FakeSecretStore::new()) as Arc<dyn stackly_platform::SecretStore>
                )
                .with_fetcher(Arc::new(fake.fetcher().clone()));
        let routed = CheckFaking {
            real: base.clone(),
            checks: fake.processes().clone(),
            check_pids: Mutex::new(HashSet::new()),
        };
        let platform = base.with_processes(Arc::new(routed));
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
        let seeded = seed_fake(
            &platform,
            prepared.layout(),
            &registry,
            &fake_server::binary(),
            &package_dir(),
        )
        .expect("the fake runtime and package are seeded");
        crate::support::make_pinned_folders_private(&prepared.layout().providers_dir());
        // The registry closes here, so the daemon opens it alone.
        drop(registry);
        let reap = ReapGuard::new(root.path(), root.roots().runtime());
        Self {
            _reap: reap,
            _root: root,
            fake,
            platform,
            prepared,
            runtime_id: seeded.runtime_id.as_uuid(),
            _lock: lock,
        }
    }

    /// Starts a daemon whose catalog offers `catalog`.
    async fn start(&self, catalog: Catalog) -> DaemonHandle {
        let log = LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON)
            .expect("the log writer starts");
        let facts = StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            true,
            std::process::id(),
        );
        let mut config = DaemonConfig::default();
        config.catalog = Some(CatalogOverride::Preloaded(catalog));
        let supervisor = &mut config.supervisor;
        supervisor.helper_term_grace = Duration::from_millis(200);
        supervisor.readiness_refresh_interval = Duration::from_millis(200);
        supervisor.spool_drain_interval = Duration::from_millis(50);
        let handle = server::start(
            DaemonDeps::new(
                self.platform.clone(),
                self.prepared.clone(),
                facts,
                Arc::new(log),
            )
            .with_config(config),
        )
        .await
        .expect("the server starts");
        handle.started().await;
        handle
    }

    /// Ends every version check the install spawns, as a binary that prints
    /// `output`.
    fn answer_checks(&self, output: &'static str) -> tokio::task::JoinHandle<()> {
        let fake = self.fake.clone();
        tokio::spawn(async move {
            let mut answered = 0;
            loop {
                let spawns: Vec<_> = fake
                    .processes()
                    .spawns()
                    .into_iter()
                    .filter(|spawn| spawn.role == ChildRole::RuntimeCheck)
                    .collect();
                for spawn in spawns.iter().skip(answered) {
                    fake.processes().exit_with_output(
                        spawn.pid,
                        Some(ExitStatus::Exited(0)),
                        CapturedOutput {
                            stdout: output.as_bytes().to_vec(),
                            stderr: Vec::new(),
                        },
                    );
                }
                answered = spawns.len();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    }

    fn uid(&self) -> u32 {
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

    async fn client(&self) -> Client {
        Client::connect(
            &self.prepared.layout().daemon_socket(),
            ConnectPolicy::same_user(self.uid()),
            ClientInfo {
                kind: ClientKind::Cli,
                version: "0.1.0".to_owned(),
                pid: Some(std::process::id()),
                interactive: Some(true),
            },
        )
        .await
        .expect("the client connects")
        .0
    }

    /// The folders an instance owns: its tree and its log folder.
    fn folders(&self, id: Uuid) -> [PathBuf; 2] {
        let id = InstanceId::from_uuid(id).expect("an instance id");
        let layout = self.prepared.layout();
        [
            layout.instances_dir().join(id.to_string()),
            layout.instance_log_dir(id),
        ]
    }
}

fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake")
}

/// `bytes` of the dummy server inside a tar.gz under `pkg/`.
fn archive() -> Vec<u8> {
    let server = std::fs::read(fake_server::binary()).expect("the dummy server can be read");
    TarBuilder::new()
        .dir("pkg", 0o755)
        .file("pkg/stackly-fake-server", 0o755, &server)
        .finish_gz()
}

/// `providers/fake` with an artifact of line `0.2` that describes
/// `archive`, besides the fixture artifact the folder offers.
fn package(archive: &[u8]) -> ValidatedPackage {
    let dir = package_dir();
    let mut files: BTreeMap<String, Vec<u8>> = [
        "provider.yaml",
        "schemas/settings.schema.json",
        "templates/fake.json.tmpl",
    ]
    .into_iter()
    .map(|path| {
        (
            path.to_owned(),
            std::fs::read(dir.join(path)).expect("package file"),
        )
    })
    .collect();
    let versions = format!(
        r#"{{ "schema": "stackly.provider-versions/v1", "software": "fake", "artifacts": [
        {{ "id": "fake-0.2.0-macos-aarch64", "line": "0.2", "version": "0.2.0", "os": "macos",
           "arch": "aarch64", "status": "available", "minOs": "11.0", "format": "tar_gz",
           "urls": ["{URL}"], "sha256": "{}", "size": {}, "archiveRoot": "pkg",
           "signingPolicy": "keep_or_adhoc", "license": "MIT OR Apache-2.0",
           "licenseUrl": "https://example.test/LICENSE", "executables": ["stackly-fake-server"],
           "stable": true,
           "dependencies": [
             {{ "name": "libSystem.B.dylib", "kind": "system",
                "installName": "/usr/lib/libSystem.B.dylib" }},
             {{ "name": "libiconv.2.dylib", "kind": "system",
                "installName": "/usr/lib/libiconv.2.dylib" }} ] }} ] }}"#,
        crate::support::digest(archive),
        archive.len()
    );
    files.insert("versions.json".to_owned(), versions.into_bytes());
    ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::FolderName)
        .unwrap_or_else(|error| panic!("the test package validates: {:?}", error.issues))
}

/// The params of an install of line `0.2` of the fake software.
fn install_params() -> InstallRuntimeParams {
    InstallRuntimeParams {
        provider: PROVIDER.to_owned(),
        selector: "0.2".to_owned(),
        expected_sha256: None,
    }
}

/// The fields of `instance.get` the spec names, plus the identity of the
/// process.
#[derive(Debug, Clone, PartialEq)]
struct Facts {
    runtime_id: Uuid,
    provider: ProviderPin,
    lifecycle: InstanceLifecycle,
    desired: InstanceDesired,
    readiness: InstanceReadinessState,
    endpoints: Vec<EndpointStatus>,
    process: Option<(u32, Timestamp)>,
    restart: InstanceRestart,
}

impl Facts {
    fn of(summary: &InstanceSummary) -> Self {
        Self {
            runtime_id: summary.runtime_id,
            provider: summary.provider.clone(),
            lifecycle: summary.lifecycle,
            desired: summary.desired,
            readiness: summary.readiness.state,
            endpoints: summary.endpoints.clone(),
            process: summary
                .process
                .as_ref()
                .map(|process| (process.pid, process.started_at)),
            restart: summary.restart.clone(),
        }
    }
}

/// SHA-256 and mode of every regular file under `root`, by relative path.
fn tree_digest(root: &Path) -> BTreeMap<String, (String, u32)> {
    use sha2::{Digest, Sha256};
    let mut digest = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("a folder of the tree can be read") {
            let path = entry.expect("a folder entry").path();
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

/// The digests of `folders`, taken again until two reads 200 ms apart agree,
/// so the log writer of a process that just stopped is done.
async fn settled(folders: &[PathBuf; 2]) -> [BTreeMap<String, (String, u32)>; 2] {
    let read = || folders.each_ref().map(|folder| tree_digest(folder));
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let mut last = read();
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let now = read();
        if now == last {
            return now;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the folders of the stopped instance kept changing"
        );
        last = now;
    }
}

/// A typed client of the instance and runtime methods of the test.
struct Api<'a> {
    client: &'a Client,
    runtime_id: Uuid,
}

impl Api<'_> {
    async fn wait_state(&self, id: Uuid, state: OperationState) -> OperationDetail {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let Reply::OperationGet(detail) = self
                .client
                .call(Call::OperationGet(GetOperationParams { operation_id: id }))
                .await
                .expect("the operation can be read")
            else {
                panic!("expected an operation.get reply");
            };
            if detail.operation.state == state {
                return *detail;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "operation {id} did not reach {state:?}: {:?}",
                detail.operation
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn get(&self, id: Uuid) -> InstanceSummary {
        let Reply::InstanceGet(summary) = self
            .client
            .call(Call::InstanceGet(GetInstanceParams { instance_id: id }))
            .await
            .expect("the instance can be read")
        else {
            panic!("expected an instance.get reply");
        };
        *summary
    }

    async fn wait_instance(
        &self,
        id: Uuid,
        what: &str,
        ready: impl Fn(&InstanceSummary) -> bool,
    ) -> InstanceSummary {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let summary = self.get(id).await;
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

    /// Creates an instance of the fake provider on a free fixed port.
    async fn create(&self, name: &str) -> Uuid {
        let port = allocate_port().expect("a loopback port is free");
        let params = CreateInstanceParams {
            name: name.to_owned(),
            display_name: None,
            provider: ProviderSelector {
                name: PROVIDER.to_owned(),
                revision: Some(REVISION.to_owned()),
            },
            runtime_id: self.runtime_id,
            endpoints: vec![EndpointRequest {
                name: "http".to_owned(),
                port: PortRequest::Fixed(port),
            }],
            settings: SettingsDocument::new(match json!({}) {
                serde_json::Value::Object(map) => map,
                _ => unreachable!("an empty object literal is an object"),
            }),
            secrets: vec![SecretFieldInput {
                field: "authToken".to_owned(),
                input: SecretInput::Value(SecretText::new(TOKEN.to_owned())),
            }],
        };
        let Reply::InstanceCreate(accepted) = call_with_key(
            self.client,
            Call::InstanceCreate(params),
            &Uuid::now_v7().to_string(),
        )
        .await
        .expect("the create is accepted") else {
            panic!("expected an instance.create reply");
        };
        let accepted = accepted.accepted.expect("a create starts an operation");
        let id = accepted
            .operation
            .resource
            .as_ref()
            .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
            .expect("the operation names the instance");
        self.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
        id
    }

    /// Starts the instance and waits until it is `ready`.
    async fn start_ready(&self, id: Uuid) -> InstanceSummary {
        let Reply::InstanceStart(accepted) = call_with_key(
            self.client,
            Call::InstanceStart(StartInstanceParams {
                instance_id: id,
                expected_revision: None,
            }),
            &Uuid::now_v7().to_string(),
        )
        .await
        .expect("the start is accepted") else {
            panic!("expected an instance.start reply");
        };
        let accepted = accepted.accepted.expect("a start begins an operation");
        self.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
        self.wait_instance(id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await
    }

    /// Stops the instance and waits until it is `stopped`.
    async fn stop(&self, id: Uuid) -> InstanceSummary {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        let accepted = loop {
            match call_with_key(
                self.client,
                Call::InstanceStop(StopInstanceParams {
                    instance_id: id,
                    expected_revision: None,
                    override_leases: false,
                }),
                &Uuid::now_v7().to_string(),
            )
            .await
            {
                Ok(Reply::InstanceStop(accepted)) => break accepted,
                // The start operation may hold the instance for a moment
                // after it reads ready.
                Err(error) if error.code() == "resource.busy" => {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "the instance stayed busy"
                    );
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                other => panic!("the stop is not accepted: {other:?}"),
            }
        };
        let accepted = accepted.accepted.expect("a stop begins an operation");
        self.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
        self.wait_instance(id, "stopped", |summary| {
            summary.lifecycle == InstanceLifecycle::Stopped && summary.process.is_none()
        })
        .await
    }

    async fn instance_ids(&self) -> Vec<Uuid> {
        let Reply::InstanceList(page) = self
            .client
            .call(Call::InstanceList(ListInstancesParams::default()))
            .await
            .expect("instances can be listed")
        else {
            panic!("expected an instance.list reply");
        };
        let mut ids: Vec<Uuid> = page.items.iter().map(|item| item.id).collect();
        ids.sort();
        ids
    }

    async fn runtimes(&self) -> Vec<RuntimeSummary> {
        let Reply::RuntimeList(page) = self
            .client
            .call(Call::RuntimeList(ListRuntimesParams::default()))
            .await
            .expect("runtimes can be listed")
        else {
            panic!("expected a runtime.list reply");
        };
        page.items
    }

    /// An install of line `0.2` under `key`; returns the operation.
    async fn install(&self, key: &str, end: OperationState) -> OperationDetail {
        let Reply::RuntimeInstall(reply) =
            call_with_key(self.client, Call::RuntimeInstall(install_params()), key)
                .await
                .expect("the install starts")
        else {
            panic!("expected a runtime.install reply");
        };
        let InstallOutcome::Started(accepted) = reply.outcome else {
            panic!("expected a started outcome");
        };
        self.wait_state(accepted.operation.id, end).await
    }
}

/// What a reader of the registry sees of the two instances.
#[derive(Debug, PartialEq)]
struct Snapshot {
    ids: Vec<Uuid>,
    running: Facts,
    stopped: Facts,
    stopped_trees: [BTreeMap<String, (String, u32)>; 2],
}

// covers: AC-1
#[tokio::test]
async fn runtime_install_leaves_existing_instances_untouched() {
    let ev = evidence!(
        covers = ["REQ-043", "BR-001"],
        kind = Kind::Fixture,
        expected = "an install of another line of the same software, once failing at the digest check and once succeeding, changes nothing of a running instance (same fields, PID, start time, and restart count, still ready) or of a stopped one (same fields and the same bytes and modes under its folders), and the instance list stays the same",
    );
    let site = Site::new();
    let archive = archive();
    site.fake.fetcher().serve(URL, archive.clone());
    let answering = site.answer_checks("stackly-fake-server 0.2.0\n");
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = site.client().await;
    let api = Api {
        client: &client,
        runtime_id: site.runtime_id,
    };

    let running_id = api.create("steady").await;
    let stopped_id = api.create("resting").await;
    api.start_ready(running_id).await;
    api.start_ready(stopped_id).await;
    api.stop(stopped_id).await;
    let stopped_folders = site.folders(stopped_id);

    let snapshot = || async {
        Snapshot {
            ids: api.instance_ids().await,
            running: Facts::of(&api.get(running_id).await),
            stopped: Facts::of(&api.get(stopped_id).await),
            stopped_trees: settled(&stopped_folders).await,
        }
    };
    let before = snapshot().await;
    assert_eq!(before.ids.len(), 2);
    assert_eq!(before.running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(before.running.readiness, InstanceReadinessState::Ready);
    assert!(before.running.process.is_some());
    assert!(before.running.endpoints.iter().all(|e| e.bound));
    assert_eq!(before.stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(before.stopped.process.is_none());
    // The dummy server writes no log files here, so the log folder digest
    // is empty; it still catches a file appearing under it.
    assert!(
        !before.stopped_trees[0].is_empty(),
        "the stopped instance has files: {:?}",
        before.stopped_trees
    );
    let first_runtimes = api.runtimes().await;
    assert_eq!(first_runtimes.len(), 1);

    // An install that fails at the Verify digest stage.
    let mut tampered = archive.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    site.fake.fetcher().serve(URL, tampered);
    let failed = api
        .install("install-fake-0001", OperationState::Failed)
        .await;
    assert_eq!(
        failed.operation.failure.expect("a failure").error.code(),
        runtime_codes::DIGEST_MISMATCH.code
    );
    assert_eq!(api.runtimes().await, first_runtimes);
    assert_eq!(snapshot().await, before, "after the failed install");

    // The same install with the right bytes.
    site.fake.fetcher().serve(URL, archive.clone());
    let done = api
        .install("install-fake-0002", OperationState::Succeeded)
        .await;
    answering.abort();
    let installed = done.operation.result.expect("a result").resources;
    assert_eq!(installed.len(), 1);
    let runtimes = api.runtimes().await;
    assert_eq!(runtimes.len(), 2);
    assert!(
        runtimes.iter().any(|runtime| runtime.version == "0.2.0"),
        "the new line is installed: {runtimes:?}"
    );
    assert!(
        first_runtimes
            .iter()
            .all(|runtime| runtimes.contains(runtime)),
        "the runtime the instances run on is unchanged"
    );
    assert_eq!(snapshot().await, before, "after the install");

    api.stop(running_id).await;
    ev.pass();
}
