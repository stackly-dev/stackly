//! The harness every supervisor test shares: the supervisor and the
//! operation engine on `FakePlatform`, with the fake provider package pinned
//! and a copy of the dummy server as the runtime.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use stackly_core::operation::testing::RecordingEvents;
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BusyPolicy, EngineDeps, EngineLimits, OperationDetailView,
    OperationEngine, OperationWork, Origin,
};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::secret::{
    PutRequest, PutTarget, Redactor, SecretCaller, SecretChange, SecretDeps, SecretEvents,
    SecretFieldRule, SecretFieldRules, SecretInput, SecretService,
};
use stackly_core::supervisor::{
    BindOptions, BindOutcome, InstanceStatus, LogBatch, LogSink, PortProbe, SinkOutcome, SinkSet,
    SocketPortProbe, Supervisor, SupervisorDeps, SupervisorEvents,
};
use stackly_core::{PreparedLayout, create_instance_layout, daemon_info, prepare_layout};
use stackly_domain::{
    BatchError, Committed, ConfigRevisionRecord, ConfigValuesJson, DesiredState, Endpoint,
    Instance, InstanceId, InstanceLifecycle, InstanceManifest, IpEndpoint, JournalPayload,
    ManifestRuntime, NewInstanceManifest, OperationId, OperationKind, OperationResource,
    OperationState, Port, PortRequest, ProviderRevisionKey, Registry, ResourceRevision,
    RuntimeState, SecretFieldName, SecretRef, SecretUser, Transport, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, Platform, ProcessError, ProcessSupervision};
use stackly_testkit::{
    FakePlatform, FakeProcessSupervision, FakeRegistry, InstanceBuilder, ProcessOp, RuntimeBuilder,
    TestRoot, allocate_port, fake_server,
};

pub const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
pub const TOKEN: &str = "fixture-token-value";

/// Shrinks the constants of the supervisor so probes, pauses, and graces
/// take milliseconds.
pub fn fast_config(config: &mut stackly_core::supervisor::SupervisorConfig) {
    config.helper_term_grace = Duration::from_millis(100);
    config.readiness_refresh_interval = Duration::from_millis(100);
    config.restart_base_delay = Duration::from_millis(20);
    config.restart_max_delay = Duration::from_millis(200);
}

/// What the supervisor reported.
#[derive(Default)]
pub struct Recorded {
    pub lifecycles: Mutex<Vec<(InstanceId, InstanceLifecycle, ResourceRevision)>>,
    pub statuses: Mutex<Vec<(InstanceId, InstanceStatus, ResourceRevision)>>,
    pub degraded: Mutex<Vec<bool>>,
    pub reconciling: Mutex<Vec<stackly_core::supervisor::ReconcilePhase>>,
}

impl Recorded {
    pub fn lifecycle_names(&self, id: InstanceId) -> Vec<&'static str> {
        self.lifecycles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(instance, _, _)| *instance == id)
            .map(|(_, lifecycle, _)| lifecycle.as_code())
            .collect()
    }

    pub fn statuses_of(&self, id: InstanceId) -> Vec<InstanceStatus> {
        self.statuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(instance, _, _)| *instance == id)
            .map(|(_, status, _)| status.clone())
            .collect()
    }
}

impl SupervisorEvents for Recorded {
    fn instance_changed(&self, instance: &Instance) {
        self.lifecycles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((instance.id(), instance.lifecycle(), instance.revision()));
    }

    fn status_changed(
        &self,
        instance: InstanceId,
        status: &InstanceStatus,
        revision: ResourceRevision,
    ) {
        self.statuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((instance, status.clone(), revision));
    }

    fn registry_degraded(&self, degraded: bool, _detail: &str) {
        self.degraded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(degraded);
    }

    fn reconciling(&self, phase: &stackly_core::supervisor::ReconcilePhase) {
        self.reconciling
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(*phase);
    }
}

/// A log sink that records every batch and can be told to be busy.
#[derive(Default)]
pub struct LogRecorder {
    batches: Mutex<Vec<(InstanceId, LogBatch)>>,
    busy: AtomicBool,
}

impl LogRecorder {
    /// Makes the sink answer `Busy` (or accept again).
    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::SeqCst);
    }

    /// Every batch of `id`, oldest first.
    pub fn batches_of(&self, id: InstanceId) -> Vec<LogBatch> {
        self.batches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(instance, _)| *instance == id)
            .map(|(_, batch)| batch.clone())
            .collect()
    }

    /// The text of every line of `id`, oldest first, with its flags.
    pub fn lines_of(&self, id: InstanceId) -> Vec<String> {
        self.batches_of(id)
            .into_iter()
            .flat_map(|batch| batch.lines)
            .map(|line| line.text)
            .collect()
    }
}

impl LogSink for LogRecorder {
    fn accept(&self, instance: InstanceId, batch: LogBatch) -> SinkOutcome {
        if self.busy.load(Ordering::SeqCst) {
            return SinkOutcome::Busy;
        }
        self.batches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((instance, batch));
        SinkOutcome::Accepted
    }
}

/// Trial binds that answer what a test scripts for a port, and the real
/// socket answer for every other port.
#[derive(Default)]
pub struct ScriptedPorts {
    scripted: Mutex<HashMap<u16, BindOutcome>>,
    real: SocketPortProbe,
}

impl ScriptedPorts {
    pub fn answer(&self, port: u16, outcome: BindOutcome) {
        self.scripted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(port, outcome);
    }
}

impl PortProbe for ScriptedPorts {
    fn try_bind(
        &self,
        transport: Transport,
        addr: SocketAddr,
        options: BindOptions,
    ) -> BindOutcome {
        let scripted = self
            .scripted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&addr.port())
            .copied();
        scripted.unwrap_or_else(|| self.real.try_bind(transport, addr, options))
    }

    fn ephemeral_port(&self, transport: Transport, host: IpAddr) -> Option<Port> {
        self.real.ephemeral_port(transport, host)
    }
}

struct Rules;

impl SecretFieldRules for Rules {
    fn rules(&self, _instance: &Instance, _field: &SecretFieldName) -> Option<SecretFieldRule> {
        Some(SecretFieldRule::new(1, Some(256), None))
    }
}

struct NoSecretEvents;

impl SecretEvents for NoSecretEvents {
    fn changed(&self, _change: &SecretChange) {}
}

/// A registry that refuses a commit holding an op a test names, and
/// otherwise delegates to the fake.
pub struct FaultyRegistry {
    pub inner: Arc<FakeRegistry>,
    refuse_observation: Mutex<bool>,
    /// Errors the next commits answer with, one each, oldest first.
    commit_faults: Mutex<Vec<stackly_domain::RegistryError>>,
    /// Holds every `instances` read until the test opens it.
    hold: Mutex<Option<Arc<Hold>>>,
    /// Errors the next `instance` reads answer with, one each, oldest first.
    read_faults: Mutex<Vec<stackly_domain::RegistryError>>,
    /// Holds the next commit that records a crash until the test opens it.
    crash_hold: Mutex<Option<Arc<Hold>>>,
    /// Holds the next commit that moves an operation of a kind to a stage.
    stage_hold: Mutex<Option<(stackly_domain::OperationKind, &'static str, Arc<Hold>)>>,
}

/// A closed door in front of the registry's `instances` read, which the
/// `endpoints` stage makes before the `journal`.
pub struct Hold {
    entered: AtomicBool,
    open: Mutex<bool>,
    opened: std::sync::Condvar,
}

impl Hold {
    /// Whether a read is waiting at the door.
    pub fn entered(&self) -> bool {
        self.entered.load(Ordering::SeqCst)
    }

    /// Opens the door for the waiting read and every later one.
    pub fn release(&self) {
        *self.open.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.opened.notify_all();
    }

    fn pass(&self) {
        self.entered.store(true, Ordering::SeqCst);
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        while !*open {
            open = self
                .opened
                .wait(open)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl FaultyRegistry {
    /// Closes a door in front of the `instances` read: the `endpoints`
    /// stage waits there, before the `journal`, until the hold is released.
    pub fn hold_instances(&self) -> Arc<Hold> {
        let hold = Arc::new(Hold {
            entered: AtomicBool::new(false),
            open: Mutex::new(false),
            opened: std::sync::Condvar::new(),
        });
        *self.hold.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&hold));
        hold
    }

    /// Makes the next commits fail with `errors`, one error per commit.
    pub fn fail_commits(&self, errors: Vec<stackly_domain::RegistryError>) {
        *self
            .commit_faults
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = errors;
    }

    /// Makes the next `instance` reads fail with `errors`, one error per
    /// read.
    pub fn fail_instance_reads(&self, errors: Vec<stackly_domain::RegistryError>) {
        *self
            .read_faults
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = errors;
    }

    /// Closes a door in front of the next commit that records a crash (an
    /// instance update that ends `recovering` or `failed`): the commit has
    /// read its aggregate and decided, and waits there until the hold is
    /// released, so a test can land a stop in between.
    pub fn hold_crash_commit(&self) -> Arc<Hold> {
        let hold = Arc::new(Hold {
            entered: AtomicBool::new(false),
            open: Mutex::new(false),
            opened: std::sync::Condvar::new(),
        });
        *self
            .crash_hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&hold));
        hold
    }

    /// Closes a door in front of the next commit that moves an operation of
    /// `kind` to `stage`: the operation has not run the stage yet, and
    /// waits there until the hold is released.
    pub fn hold_stage(
        &self,
        kind: stackly_domain::OperationKind,
        stage: &'static str,
    ) -> Arc<Hold> {
        let hold = Arc::new(Hold {
            entered: AtomicBool::new(false),
            open: Mutex::new(false),
            opened: std::sync::Condvar::new(),
        });
        *self
            .stage_hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((kind, stage, Arc::clone(&hold)));
        hold
    }

    pub fn refuse_process_observations(&self, refuse: bool) {
        *self
            .refuse_observation
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = refuse;
    }
}

macro_rules! delegate {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> $ret { self.inner.$name($($arg),*) })*
    };
}

impl Registry for FaultyRegistry {
    delegate! {
        provider_revision(key: &ProviderRevisionKey) -> Result<stackly_domain::Versioned<Option<stackly_domain::ProviderRevisionRecord>>, stackly_domain::RegistryError>;
        artifact(sha256: &stackly_domain::Sha256Digest) -> Result<stackly_domain::Versioned<Option<stackly_domain::ArtifactRecord>>, stackly_domain::RegistryError>;
        runtime(id: stackly_domain::RuntimeId) -> Result<stackly_domain::Versioned<Option<stackly_domain::Runtime>>, stackly_domain::RegistryError>;
        runtimes() -> Result<stackly_domain::Versioned<Vec<stackly_domain::Runtime>>, stackly_domain::RegistryError>;
        runtime_references(id: stackly_domain::RuntimeId) -> Result<stackly_domain::Versioned<stackly_domain::RuntimeReferences>, stackly_domain::RegistryError>;
        instance_by_name(name: &stackly_domain::InstanceName) -> Result<stackly_domain::Versioned<Option<Instance>>, stackly_domain::RegistryError>;
        known_instance_ids() -> Result<stackly_domain::Versioned<Vec<InstanceId>>, stackly_domain::RegistryError>;
        latest_process_observation(instance: InstanceId) -> Result<stackly_domain::Versioned<Option<stackly_domain::ProcessObservation>>, stackly_domain::RegistryError>;
        active_config_revision(instance: InstanceId) -> Result<stackly_domain::Versioned<Option<ConfigRevisionRecord>>, stackly_domain::RegistryError>;
        instance_paths(id: InstanceId) -> Result<stackly_domain::Versioned<Option<Vec<stackly_domain::InstancePathEntry>>>, stackly_domain::RegistryError>;
        environment(id: stackly_domain::EnvironmentId) -> Result<stackly_domain::Versioned<Option<stackly_domain::Environment>>, stackly_domain::RegistryError>;
        environments() -> Result<stackly_domain::Versioned<Vec<stackly_domain::Environment>>, stackly_domain::RegistryError>;
        leases(instance: InstanceId, filter: stackly_domain::LeaseFilter) -> Result<stackly_domain::Versioned<Vec<stackly_domain::Lease>>, stackly_domain::RegistryError>;
        operation(id: OperationId) -> Result<stackly_domain::Versioned<Option<stackly_domain::Operation>>, stackly_domain::RegistryError>;
        operations(query: stackly_domain::OperationQuery) -> Result<stackly_domain::Versioned<stackly_domain::OperationPage>, stackly_domain::RegistryError>;
        operation_journal(id: OperationId) -> Result<stackly_domain::Versioned<Option<stackly_domain::JournalView>>, stackly_domain::RegistryError>;
        operation_journals(ids: &[OperationId]) -> Result<stackly_domain::Versioned<Vec<stackly_domain::OperationJournal>>, stackly_domain::RegistryError>;
        operation_by_idempotency_key(key: &stackly_domain::IdempotencyKey) -> Result<stackly_domain::Versioned<Option<(stackly_domain::Operation, stackly_domain::OperationJournal)>>, stackly_domain::RegistryError>;
        unfinished_operations() -> Result<stackly_domain::Versioned<Vec<(stackly_domain::Operation, stackly_domain::JournalView)>>, stackly_domain::RegistryError>;
        secret_reference(reference: &SecretRef) -> Result<stackly_domain::Versioned<Option<stackly_domain::SecretReferenceView>>, stackly_domain::RegistryError>;
        secret_references(filter: stackly_domain::SecretReferenceFilter) -> Result<stackly_domain::Versioned<Vec<stackly_domain::SecretReferenceView>>, stackly_domain::RegistryError>;
        secret_uses_by(user: SecretUser) -> Result<stackly_domain::Versioned<Vec<stackly_domain::SecretUse>>, stackly_domain::RegistryError>;
        audit_events(query: stackly_domain::AuditQuery) -> Result<stackly_domain::Versioned<stackly_domain::AuditPage>, stackly_domain::RegistryError>;
        prune(policy: &stackly_domain::RetentionPolicy, now: jiff::Timestamp) -> Result<stackly_domain::PruneReport, stackly_domain::RegistryError>;
    }

    fn instance(
        &self,
        id: InstanceId,
    ) -> Result<stackly_domain::Versioned<Option<Instance>>, stackly_domain::RegistryError> {
        {
            let mut faults = self
                .read_faults
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !faults.is_empty() {
                return Err(faults.remove(0));
            }
        }
        self.inner.instance(id)
    }

    fn instances(
        &self,
        filter: stackly_domain::InstanceFilter,
    ) -> Result<stackly_domain::Versioned<Vec<Instance>>, stackly_domain::RegistryError> {
        let hold = self
            .hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(hold) = hold {
            hold.pass();
        }
        self.inner.instances(filter)
    }

    fn commit(&self, batch: WriteBatch) -> Result<Committed, BatchError> {
        let records_crash = batch.ops.iter().any(|op| {
            matches!(
                op,
                WriteOp::UpdateInstance { instance, .. }
                    if matches!(
                        instance.lifecycle(),
                        InstanceLifecycle::Recovering | InstanceLifecycle::Failed
                    )
            )
        });
        let held_stage = {
            let mut door = self
                .stage_hold
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let hit = door.as_ref().is_some_and(|(kind, stage, _)| {
                batch.ops.iter().any(|op| {
                    matches!(
                        op,
                        WriteOp::UpdateOperation { operation, .. }
                            if operation.kind() == *kind
                                && operation.stage().is_some_and(|name| name.as_str() == *stage)
                    )
                })
            });
            if hit { door.take() } else { None }
        };
        if let Some((_, _, hold)) = held_stage {
            hold.pass();
        }
        if records_crash {
            let hold = self
                .crash_hold
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(hold) = hold {
                hold.pass();
            }
        }
        {
            let mut faults = self
                .commit_faults
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !faults.is_empty() {
                return Err(BatchError::batch(faults.remove(0)));
            }
        }
        let refuse = *self
            .refuse_observation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if refuse
            && batch
                .ops
                .iter()
                .any(|op| matches!(op, WriteOp::RecordProcessObservation(_)))
        {
            return Err(BatchError::batch(stackly_domain::RegistryError::NoSpace));
        }
        self.inner.commit(batch)
    }
}

/// How a process planted by [`Rig::plant`] looks.
#[derive(Clone, Copy)]
pub struct Plant {
    /// It is held back before its first instruction.
    pub stopped: bool,
    /// It holds the instance lock.
    pub holds_lock: bool,
}

impl Plant {
    /// A running process that holds the lock: the one an instance's main
    /// process is.
    pub const fn live() -> Self {
        Self {
            stopped: false,
            holds_lock: true,
        }
    }
}

/// What an instance of the fake provider is made of.
#[derive(Clone)]
pub struct Spec {
    pub lifecycle: InstanceLifecycle,
    pub desired: DesiredState,
    pub request: Option<u16>,
    pub settings: serde_json::Value,
    pub token: Option<&'static str>,
    pub marker: &'static str,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            lifecycle: InstanceLifecycle::Stopped,
            desired: DesiredState::Stopped,
            request: None,
            settings: serde_json::json!({}),
            token: Some(TOKEN),
            marker: "1.0\n",
        }
    }
}

/// The supervisor and the engine on fakes, in their own test root.
pub struct Rig {
    pub root: TestRoot,
    pub fake: FakePlatform,
    pub platform: Arc<Platform>,
    pub layout: Arc<PreparedLayout>,
    pub registry: Arc<FaultyRegistry>,
    pub secrets: Arc<SecretService>,
    pub events: Arc<Recorded>,
    pub ports: Arc<ScriptedPorts>,
    pub logs: Arc<LogRecorder>,
    pub sup: Supervisor,
    pub deps: SupervisorDeps,
    pub engine: OperationEngine,
    pub package: ValidatedPackage,
    pub runtime: stackly_domain::Runtime,
    pub servers: Mutex<Vec<Child>>,
}

impl Rig {
    pub async fn new() -> Self {
        Self::build(|_| {}, |text| text.to_owned()).await
    }

    /// A rig whose provider.yaml went through `patch` before it was pinned.
    pub async fn patched(patch: impl Fn(&str) -> String) -> Self {
        Self::build(|_| {}, patch).await
    }

    pub async fn with_config(
        tweak: impl FnOnce(&mut stackly_core::supervisor::SupervisorConfig),
    ) -> Self {
        Self::build(tweak, |text| text.to_owned()).await
    }

    /// A rig of a provider that cannot reload: the capability, the reload
    /// section, its timeout, and the schema's `apply: reload` are gone.
    pub async fn without_reload() -> Self {
        Self::build_with(
            |_| {},
            |text| {
                let text = replace(text, "  reload: true", "  reload: false");
                let text = replace(
                    &text,
                    "  reload:\n    render: [fakeConfig]\n    action: signal\n    signal: hup\n    verify: readiness\n",
                    "",
                );
                replace(&text, "  reloadSeconds: 10\n", "")
            },
            |schema| replace(schema, "\"apply\": \"reload\"", "\"apply\": \"restart\""),
        )
        .await
    }

    pub async fn build(
        tweak: impl FnOnce(&mut stackly_core::supervisor::SupervisorConfig),
        patch: impl Fn(&str) -> String,
    ) -> Self {
        Self::build_with(tweak, patch, |text| text.to_owned()).await
    }

    pub async fn build_with(
        tweak: impl FnOnce(&mut stackly_core::supervisor::SupervisorConfig),
        patch: impl Fn(&str) -> String,
        schema_patch: impl Fn(&str) -> String,
    ) -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let platform = Arc::new(fake.platform());
        let layout = Arc::new(prepare_layout(&platform).expect("a fresh test root prepares"));
        let inner = Arc::new(FakeRegistry::new(
            Arc::new(fake.clock().clone()),
            fake.clock().now(),
        ));
        let registry = Arc::new(FaultyRegistry {
            inner,
            refuse_observation: Mutex::new(false),
            commit_faults: Mutex::new(Vec::new()),
            hold: Mutex::new(None),
            read_faults: Mutex::new(Vec::new()),
            crash_hold: Mutex::new(None),
            stage_hold: Mutex::new(None),
        });
        let redactor = Arc::new(Redactor::new());
        let secrets = Arc::new(SecretService::new(SecretDeps {
            registry: Arc::clone(&registry) as Arc<dyn Registry>,
            platform: Arc::clone(&platform),
            redactor: Arc::clone(&redactor),
            fields: Arc::new(Rules),
            events: Arc::new(NoSecretEvents),
        }));
        let events = Arc::new(Recorded::default());
        let ports = Arc::new(ScriptedPorts::default());
        let mut deps = SupervisorDeps::new(
            Arc::clone(&registry) as Arc<dyn Registry>,
            Arc::clone(&platform),
            Arc::clone(&layout),
            Arc::clone(&secrets),
            Arc::clone(&redactor),
        );
        deps.ports = Arc::clone(&ports) as Arc<dyn PortProbe>;
        deps.events = Arc::clone(&events) as Arc<dyn SupervisorEvents>;
        let logs = Arc::new(LogRecorder::default());
        deps.sinks = Arc::new(SinkSet::new(vec![
            Arc::clone(&deps.tail) as Arc<dyn LogSink>,
            Arc::clone(&logs) as Arc<dyn LogSink>,
        ]));
        fast_config(&mut deps.config);
        tweak(&mut deps.config);
        let sup = Supervisor::new(deps.clone());
        let engine = OperationEngine::new(
            EngineDeps {
                registry: Arc::clone(&registry) as Arc<dyn Registry>,
                platform: Arc::clone(&platform),
                layout: Arc::clone(&layout),
                events: Arc::new(RecordingEvents::new()),
                redactor: Arc::clone(&redactor) as Arc<dyn stackly_core::operation::Redact>,
            },
            EngineLimits::default()
                .with_intervals(Duration::from_millis(100), Duration::from_millis(20)),
        );
        engine.recover().await.expect("an empty registry recovers");
        engine.start();
        sup.attach_operations(engine.clone());

        let package = install_package(&layout, &patch, &schema_patch);
        let runtime = install_runtime(&layout, &package);
        let mut ops = vec![WriteOp::InsertProviderRevision(
            stackly_domain::ProviderRevisionRecord::new(
                package.provider_revision().clone(),
                None,
                None,
                None,
            ),
        )];
        if let stackly_domain::InstallMode::Managed { artifact_sha256 } = runtime.install() {
            ops.push(WriteOp::InsertArtifact(
                stackly_domain::ArtifactRecord::new(
                    *artifact_sha256,
                    runtime.software().clone(),
                    runtime.version().clone(),
                    runtime.target(),
                    None,
                    None,
                ),
            ));
        }
        ops.push(WriteOp::InsertRuntime(runtime.clone()));
        registry
            .commit(WriteBatch::new(ops))
            .expect("the provider and the runtime can be stored");
        Self {
            root,
            fake,
            platform,
            layout,
            registry,
            secrets,
            events,
            ports,
            logs,
            sup,
            deps,
            engine,
            package,
            runtime,
            servers: Mutex::new(Vec::new()),
        }
    }

    /// A new supervisor on the same registry, platform, and engine, as a
    /// daemon that started again: it knows no actor, and the processes of
    /// the fake platform are still there.
    pub fn reborn(&self) -> Supervisor {
        let sup = Supervisor::new(self.deps.clone());
        sup.attach_operations(self.engine.clone());
        sup
    }

    /// Like [`reborn`](Self::reborn), but the new daemon also lost what the
    /// old one knew about secret values: its redactor and secret service
    /// are new, over the same registry and secret store.
    pub fn reborn_forgetting_secrets(&self) -> (Supervisor, Arc<Redactor>) {
        let redactor = Arc::new(Redactor::new());
        let mut deps = self.deps.clone();
        deps.secrets = Arc::new(SecretService::new(SecretDeps {
            registry: Arc::clone(&self.registry) as Arc<dyn Registry>,
            platform: Arc::clone(&self.platform),
            redactor: Arc::clone(&redactor),
            fields: Arc::new(Rules),
            events: Arc::new(NoSecretEvents),
        }));
        deps.redactor = Arc::clone(&redactor);
        let sup = Supervisor::new(deps);
        sup.attach_operations(self.engine.clone());
        (sup, redactor)
    }

    /// Plants a process the fake did not spawn as the last recorded process
    /// of `id`: the facts of a live process, the observation in the
    /// registry, and, when `plant.holds_lock`, the instance lock.
    pub fn plant(&self, id: InstanceId, plant: Plant) -> stackly_domain::ProcessIdentity {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(60_000);
        let raw = NEXT.fetch_add(1, Ordering::SeqCst);
        let pid = stackly_domain::Pid::new(raw).expect("a planted PID is not 0");
        let executable = PathBuf::from("/planted/stackly-fake-server");
        let start = stackly_domain::ProcessStart::new(1_760_000_000, raw % 1_000_000)
            .expect("a planted start time");
        let boot = self
            .processes()
            .boot_id()
            .expect("the fake answers its boot ID");
        let identity = stackly_domain::ProcessIdentity {
            pid,
            start,
            executable: executable.clone(),
            session: pid,
            boot,
        };
        self.processes().insert(stackly_platform::ProcessFacts {
            pid,
            parent: stackly_domain::Pid::new(1).ok(),
            group: pid,
            session: pid,
            uid: self.processes().user(),
            start,
            executable,
            stopped: plant.stopped,
        });
        let lock = self.take_lock(id);
        if plant.holds_lock {
            self.processes().hand_lock(pid, lock);
        }
        self.registry
            .commit(WriteBatch::new(vec![WriteOp::RecordProcessObservation(
                stackly_domain::ProcessObservation {
                    instance: id,
                    identity: identity.clone(),
                    observed_at: self.fake_now(),
                },
            )]))
            .expect("the observation is stored");
        identity
    }

    /// Waits until nobody holds the instance lock of `id`.
    ///
    /// The executor drops its handle when an action ends, but a real child
    /// another test of this process forks at that moment (the dummy
    /// server) shares the open lock file until it has exec'd, so the lock
    /// can read `Held` for a moment longer under load. A handle the
    /// executor leaked would stay held and fail here after the wait.
    pub async fn wait_lock_free(&self, id: InstanceId) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let state = self
                .processes()
                .probe_lock(&self.paths(id).lock)
                .expect("the lock can be probed");
            if state == stackly_platform::LockState::Free {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the instance lock of {id} was never released"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Takes the instance lock of `id` and returns it; dropping it frees
    /// the lock.
    pub fn take_lock(&self, id: InstanceId) -> stackly_platform::InstanceLock {
        match self
            .processes()
            .lock_instance(&self.paths(id).lock)
            .expect("the fake locks")
        {
            stackly_platform::LockOutcome::Acquired(lock) => lock,
            stackly_platform::LockOutcome::Held => panic!("the lock of {id} is held already"),
        }
    }

    pub fn processes(&self) -> &FakeProcessSupervision {
        self.fake.processes()
    }

    pub fn key(&self) -> &ProviderRevisionKey {
        self.package.key()
    }

    /// Stores an instance of the fake provider with its folders, marker,
    /// config revision, and secret, and returns its ID.
    pub fn instance(&self, spec: &Spec) -> InstanceId {
        let request = spec.request.map_or(PortRequest::Auto, |port| {
            PortRequest::Fixed(Port::new(port).expect("a test port is not 0"))
        });
        let endpoint = Endpoint::Ip(IpEndpoint::new(
            "http".parse().expect("an endpoint name"),
            Transport::Tcp,
            LOOPBACK,
            request,
            None,
        ));
        let builder = InstanceBuilder::new(&self.runtime, self.key())
            .endpoints(vec![endpoint])
            .lifecycle(spec.lifecycle)
            .desired(spec.desired);
        let id = builder.id();
        let instance = builder.build();
        let now = self.fake.clock().now();

        let manifest = InstanceManifest::new(NewInstanceManifest {
            instance_id: id,
            name: instance.name().clone(),
            display_name: None,
            software: self.runtime.software().clone(),
            provider: self.key().clone(),
            runtime: ManifestRuntime::managed(
                self.runtime.id(),
                self.runtime.version().clone(),
                self.runtime.line().clone(),
                self.runtime.target(),
                match self.runtime.install() {
                    stackly_domain::InstallMode::Managed { artifact_sha256 } => *artifact_sha256,
                    stackly_domain::InstallMode::External { .. } => {
                        unreachable!("the rig installs a managed runtime")
                    }
                },
            ),
            created_at: now,
            created_by_operation: OperationId::generate(),
            stackly_version: daemon_info().version.to_owned(),
        });
        let paths = create_instance_layout(&self.platform, &self.layout, &manifest, &[])
            .expect("the instance folders can be made");
        fs::write(paths.data.join("FAKE_VERSION"), spec.marker).expect("the marker is written");

        let reference: SecretRef = format!("local:test/{}", instance.id().short_key())
            .parse()
            .expect("a test secret reference");
        let mut values = match spec.settings.clone() {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        values.insert(
            "authToken".to_owned(),
            serde_json::Value::String(reference.to_string()),
        );
        if let Some(token) = spec.token {
            self.secrets
                .put(
                    PutRequest::new(
                        PutTarget::Existing(reference.clone()),
                        SecretInput::Value(
                            stackly_platform::SecretValue::new(token.to_owned())
                                .expect("a valid test token"),
                        ),
                        stackly_platform::WriteMode::Upsert,
                    ),
                    &SecretCaller::client("cli".parse().expect("a client code"), 501, true),
                )
                .expect("the test token is stored");
        } else {
            // The reference exists without a value.
            self.secrets
                .put(
                    PutRequest::new(
                        PutTarget::Existing(reference.clone()),
                        SecretInput::Value(
                            stackly_platform::SecretValue::new("gone".to_owned())
                                .expect("a valid test token"),
                        ),
                        stackly_platform::WriteMode::Upsert,
                    ),
                    &SecretCaller::client("cli".parse().expect("a client code"), 501, true),
                )
                .expect("the placeholder is stored");
            self.fake.secrets().remove(&reference);
        }
        let revision = ConfigRevisionRecord::first(
            stackly_domain::ConfigRevisionId::generate(),
            id,
            ConfigValuesJson::new(serde_json::Value::Object(values).to_string())
                .expect("the settings fit"),
            None,
            now,
        );
        let uses: BTreeMap<SecretFieldName, SecretRef> =
            [("authToken".parse().expect("a field name"), reference)]
                .into_iter()
                .collect();
        self.registry
            .commit(WriteBatch::new(vec![
                WriteOp::InsertInstance(instance),
                WriteOp::InsertConfigRevision(revision),
                WriteOp::ReplaceSecretUses {
                    user: SecretUser::Instance(id),
                    uses,
                },
            ]))
            .expect("the instance can be stored");
        id
    }

    pub fn instance_row(&self, id: InstanceId) -> Instance {
        self.registry
            .instance(id)
            .expect("the registry reads")
            .value
            .expect("the instance exists")
    }

    pub fn fake_now(&self) -> jiff::Timestamp {
        self.fake.clock().now()
    }

    pub fn paths(&self, id: InstanceId) -> stackly_platform::InstancePaths {
        self.layout.layout().instance_paths(id)
    }

    /// A user start request, interactive.
    pub fn user() -> SecretCaller {
        SecretCaller::client("cli".parse().expect("a client code"), 501, true)
    }

    pub async fn begin(
        &self,
        kind: OperationKind,
        id: InstanceId,
        work: Box<dyn OperationWork>,
    ) -> OperationId {
        let outcome = self
            .engine
            .begin(BeginRequest {
                kind,
                resource: OperationResource::Instance(id),
                creates_resource: false,
                extra_locks: Vec::new(),
                busy: BusyPolicy::Queue,
                expected_revision: None,
                origin: Origin::Background,
                input: JournalPayload::empty(),
                work,
                precheck: None,
            })
            .await
            .expect("the engine accepts the operation");
        match outcome {
            BeginOutcome::Created(view) => view.operation.id(),
            other => panic!("expected a created operation, got {other:?}"),
        }
    }

    pub async fn start(&self, id: InstanceId) -> OperationId {
        let work = self
            .sup
            .start_work(stackly_core::supervisor::StartRequest::user(
                id,
                Self::user(),
            ));
        self.begin(OperationKind::StartInstance, id, work).await
    }

    pub async fn stop(&self, id: InstanceId) -> OperationId {
        let work = self
            .sup
            .stop_work(stackly_core::supervisor::StopRequest::new(id));
        self.begin(OperationKind::StopInstance, id, work).await
    }

    pub async fn reload(&self, id: InstanceId) -> OperationId {
        let work = self
            .sup
            .reload_work(stackly_core::supervisor::ReloadRequest::new(
                id,
                Self::user(),
            ));
        self.begin(OperationKind::ReloadInstance, id, work).await
    }

    pub async fn wait_state(&self, id: OperationId, state: OperationState) -> OperationDetailView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(detail) = self.engine.get(id).await.expect("the registry reads")
                && detail.view.operation.state() == state
            {
                return detail;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "operation {id} did not reach {state:?}: {:?}",
                self.engine.get(id).await
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Waits for the operation to end in any final state.
    pub async fn wait_final(&self, id: OperationId) -> OperationDetailView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(detail) = self.engine.get(id).await.expect("the registry reads")
                && detail.view.operation.state().is_final()
            {
                return detail;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "operation {id} did not end: {:?}",
                self.engine.get(id).await
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Every operation of `kind` on `id`, newest first, with its journal
    /// input.
    pub async fn operations_of(
        &self,
        id: InstanceId,
        kind: OperationKind,
    ) -> Vec<stackly_core::operation::OperationView> {
        self.engine
            .list(stackly_domain::OperationQuery {
                active_only: false,
                resource: Some(OperationResource::Instance(id)),
                limit: 100,
                before: None,
            })
            .await
            .expect("the operations list")
            .items
            .into_iter()
            .filter(|view| view.operation.kind() == kind)
            .collect()
    }

    /// The code of a failed operation's error.
    pub fn failure_code(detail: &OperationDetailView) -> String {
        detail
            .view
            .journal
            .failure_error()
            .expect("the failed operation stored its error")
            .code()
            .to_string()
    }

    /// Waits until the fake has spawned `count` main processes and the last
    /// one is no longer suspended, and returns it.
    pub async fn main_spawn(&self, count: usize) -> stackly_testkit::FakeSpawn {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let mains: Vec<_> = self
                .processes()
                .spawns()
                .into_iter()
                .filter(|spawn| spawn.role == stackly_platform::ChildRole::Main)
                .collect();
            // A main process is handed out once it left the suspended state:
            // resumed, or discarded.
            if let Some(spawn) = mains.get(count - 1)
                && self.processes().process_state(spawn.pid)
                    != Some(stackly_testkit::FakeProcessState::Suspended)
            {
                return spawn.clone();
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the fake never spawned main process {count}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Waits for the first helper spawn with `role` after `already`.
    pub async fn helper_spawn(
        &self,
        role: stackly_platform::ChildRole,
        already: usize,
    ) -> stackly_testkit::FakeSpawn {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let found: Vec<_> = self
                .processes()
                .spawns()
                .into_iter()
                .filter(|spawn| spawn.role == role)
                .collect();
            if found.len() > already {
                return found[already].clone();
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the fake never spawned a {role:?} helper"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Runs the real dummy server with exactly the arguments the executor
    /// gave the fake main process, and waits until it accepts connections.
    pub fn serve(&self, spawn: &stackly_testkit::FakeSpawn) -> u16 {
        let port = spawn
            .args
            .iter()
            .find_map(|arg| arg.strip_prefix("--port="))
            .and_then(|port| port.parse::<u16>().ok())
            .expect("the serve command carries its port");
        let child = Command::new(fake_server::binary())
            .args(&spawn.args)
            .current_dir(&spawn.cwd)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the dummy server starts");
        self.servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(child);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(20) {
            if TcpStream::connect((LOOPBACK, port)).is_ok() {
                return port;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the dummy server did not accept connections on {port}");
    }

    /// Stops the dummy servers started by [`serve`](Self::serve).
    pub fn kill_servers(&self) {
        for mut child in self
            .servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Makes the next `op` call fail with a spawn error.
    pub fn fail_spawn(&self) {
        self.processes().fail_next(
            ProcessOp::SpawnMain,
            ProcessError::SpawnFailed {
                kind: std::io::ErrorKind::PermissionDenied,
                errno: 13,
            },
        );
    }

    /// Returns a TCP port that is free now.
    pub fn free_port() -> u16 {
        allocate_port().expect("a free loopback port")
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.kill_servers();
    }
}

/// Pins the fake provider package, with `patch` applied to provider.yaml,
/// under the layout's provider folder.
fn install_package(
    layout: &PreparedLayout,
    patch: &dyn Fn(&str) -> String,
    schema_patch: &dyn Fn(&str) -> String,
) -> ValidatedPackage {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let mut map: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for name in [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
    ] {
        map.insert(
            name.to_owned(),
            fs::read(source.join(name)).expect("a package file reads"),
        );
    }
    for entry in fs::read_dir(source.join("templates")).expect("the templates folder reads") {
        let entry = entry.expect("a template entry reads");
        map.insert(
            format!("templates/{}", entry.file_name().to_string_lossy()),
            fs::read(entry.path()).expect("a template reads"),
        );
    }
    let yaml = String::from_utf8(map["provider.yaml"].clone()).expect("the manifest is UTF-8");
    map.insert("provider.yaml".to_owned(), patch(&yaml).into_bytes());
    let schema = String::from_utf8(map["schemas/settings.schema.json"].clone())
        .expect("the schema is UTF-8");
    map.insert(
        "schemas/settings.schema.json".to_owned(),
        schema_patch(&schema).into_bytes(),
    );
    let package = ValidatedPackage::from_files(PackageFiles::new("fake", map), NameCheck::Skip)
        .expect("the patched package is valid");
    let dir = layout.layout().provider_revision_dir(package.key());
    for (path, bytes) in package.files().iter() {
        let target = dir.join(path);
        fs::create_dir_all(target.parent().expect("a package file has a parent"))
            .expect("the package folder is made");
        fs::write(&target, bytes).expect("a package file is written");
    }
    package
}

/// Installs a copy of the dummy server as the runtime `fake 1.0.0`.
fn install_runtime(layout: &PreparedLayout, package: &ValidatedPackage) -> stackly_domain::Runtime {
    let built = RuntimeBuilder::new()
        .software(package.software().as_str())
        .version("1.0.0", "1.0")
        .state(RuntimeState::Installed)
        .installed_at(
            "2026-01-01T00:00:00Z"
                .parse()
                .expect("a valid test timestamp"),
        )
        .build();
    let stackly_domain::InstallMode::Managed { artifact_sha256 } = built.install() else {
        unreachable!("the builder makes a managed runtime");
    };
    let dir = layout.layout().runtime_dir(
        built.software(),
        built.version(),
        built.target(),
        artifact_sha256,
    );
    fs::create_dir_all(&dir).expect("the runtime folder is made");
    let executable = dir.join("stackly-fake-server");
    fs::copy(fake_server::binary(), &executable).expect("the dummy server is copied");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .expect("the executable bit is set");
    built
}

/// Replaces `from` with `to` in a provider.yaml text, which must hold it.
pub fn replace(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "provider.yaml has no {from:?}");
    text.replacen(from, to, 1)
}

/// Where the rendered config of an instance goes.
pub fn config_file(paths: &stackly_platform::InstancePaths) -> PathBuf {
    paths.config.join("fake.json")
}
