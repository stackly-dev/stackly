//! A ready registry in a `TestRoot`, a side connection to its file, and
//! valid aggregates to write into it.

use std::collections::BTreeSet;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use jiff::Timestamp;
use rusqlite::Connection;
use stackly_domain::{
    ArtifactRecord, AuditEventId, BatchError, Capability, CapabilitySet, Committed, DesiredState,
    Endpoint, Instance, InstanceId, InstanceLifecycle, IpEndpoint, LineCompatibility, NewInstance,
    NewOperation, NewRuntime, Operation, OperationId, OperationKind, OperationResource,
    PortRequest, ProviderRevision, ProviderRevisionKey, ProviderRevisionRecord, Registry,
    ResourceRevision, Runtime, Sha256Digest, StartupPolicy, StoredInstance, StoredOperation,
    Target, Transport, UnixEndpoint, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, DirectoryLayout};
use stackly_storage::{OpenOptions, OpenSummary, RegistryOpen, SqliteRegistry};
use stackly_testkit::{
    BackupSeed, FakeClock, FakePlatform, RegistrySeed, TestRoot, seeded_observation,
};

/// The app version the registry is opened with.
const APP_VERSION: &str = "0.1.0-test";

/// A registry opened in its own test root with the fake platform, so its
/// clock is a [`FakeClock`] the test moves.
pub struct Harness {
    // Declared first, so it closes before the root goes away.
    pub registry: SqliteRegistry,
    pub summary: OpenSummary,
    pub layout: DirectoryLayout,
    pub fake: FakePlatform,
    /// Kept for its drop, which removes the test folders.
    _root: TestRoot,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_options(&OpenOptions::new())
    }

    /// A registry opened with `options`, for example a short busy timeout.
    pub fn with_options(options: &OpenOptions) -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let layout = DirectoryLayout::new(root.roots());
        let mut builder = DirBuilder::new();
        builder.mode(0o700).recursive(true);
        builder
            .create(layout.registry_dir())
            .expect("the registry folder can be created");
        let open = options
            .open(&fake.platform(), &layout, APP_VERSION)
            .expect("opening a registry in a clean root succeeds");
        let (registry, summary) = match open {
            RegistryOpen::Ready { registry, summary } => (registry, summary),
            RegistryOpen::Recovery(report) => panic!("expected a ready registry, got {report:#?}"),
        };
        Self {
            registry,
            summary,
            layout,
            fake,
            _root: root,
        }
    }

    pub fn clock(&self) -> &FakeClock {
        self.fake.clock()
    }

    /// The fake clock's time.
    pub fn now(&self) -> Timestamp {
        self.fake.clock().now()
    }

    /// A second connection to the registry file, as the conformance seeding
    /// and raw checks use it.
    pub fn side(&self) -> Connection {
        let conn = Connection::open(self.layout.registry_db()).expect("the registry file opens");
        conn.busy_timeout(Duration::from_secs(5))
            .expect("a busy timeout can be set");
        conn
    }

    /// A [`RegistrySeed`] that writes through a second connection to the
    /// registry file, the way the conformance suite seeds tables no
    /// `WriteOp` covers yet.
    pub fn seeder(&self) -> SideSeed {
        SideSeed {
            conn: Mutex::new(self.side()),
        }
    }

    /// Commits `ops` and expects success.
    pub fn commit(&self, ops: Vec<WriteOp>) -> Committed {
        self.registry
            .commit(WriteBatch::new(ops))
            .expect("the batch commits")
    }

    /// Commits `ops` and expects a failure.
    pub fn reject(&self, ops: Vec<WriteOp>) -> BatchError {
        self.registry
            .commit(WriteBatch::new(ops))
            .expect_err("the batch is rejected")
    }

    /// Records a provider revision, an artifact, and a managed runtime of
    /// PostgreSQL.
    pub fn base(&self) -> Base {
        self.base_of("postgresql")
    }

    /// Records a provider revision, an artifact, and a managed runtime of
    /// `software`.
    pub fn base_of(&self, software: &str) -> Base {
        let base = Base::new(software);
        self.commit(base.ops());
        base
    }

    /// A new instance on `base`, created now.
    pub fn instance(&self, base: &Base, name: &str) -> Instance {
        let created = Instance::new(NewInstance {
            id: InstanceId::generate(),
            name: name.parse().expect("a valid instance name"),
            display_name: Some("Main database".parse().expect("a valid display name")),
            provider_revision: base.provider.revision.key().clone(),
            software: base.software(),
            runtime_id: base.runtime.id(),
            data_line: "17".parse().expect("a valid line"),
            data_version: "17.6".parse().expect("a valid version"),
            endpoints: unbound(endpoints()),
            startup: StartupPolicy::Manual,
            created_at: self.now(),
            created_by: OperationId::generate(),
        })
        .expect("the fixture endpoints are valid for a new instance");
        // `Instance::new` refuses a bound endpoint, so the fixture that
        // stores bound ports restores the instance instead.
        let mut stored = stored(&created);
        stored.endpoints = endpoints();
        Instance::restore(stored, self.now()).expect("the fixture record is consistent")
    }

    /// A new operation on `resource`, created now.
    pub fn operation(&self, resource: OperationResource) -> Operation {
        Operation::new(NewOperation {
            id: OperationId::generate(),
            kind: OperationKind::CreateInstance,
            resource,
            idempotency_key: None,
            created_at: self.now(),
        })
    }

    /// The text of one column of one row, read through the side
    /// connection.
    pub fn raw(&self, sql: &str, id: &str) -> Option<String> {
        self.side()
            .query_row(sql, [id], |row| row.get(0))
            .expect("the raw query runs")
    }
}

/// A provider revision, its artifact, and a managed runtime of it.
pub struct Base {
    pub provider: ProviderRevisionRecord,
    pub artifact: ArtifactRecord,
    pub runtime: Runtime,
}

impl Base {
    pub fn new(software: &str) -> Self {
        let provider = provider_revision(software);
        let artifact = artifact(software);
        let runtime = managed_runtime(&artifact);
        Self {
            provider,
            artifact,
            runtime,
        }
    }

    pub fn software(&self) -> stackly_domain::SoftwareName {
        self.artifact.software.clone()
    }

    pub fn ops(&self) -> Vec<WriteOp> {
        vec![
            WriteOp::InsertProviderRevision(self.provider.clone()),
            WriteOp::InsertArtifact(self.artifact.clone()),
            WriteOp::InsertRuntime(self.runtime.clone()),
        ]
    }
}

/// A digest no other call returns.
pub fn digest() -> Sha256Digest {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("{:064x}", NEXT.fetch_add(1, Ordering::Relaxed))
        .parse()
        .expect("64 hex characters are a digest")
}

pub fn target() -> Target {
    Target::new(stackly_domain::Os::Macos, stackly_domain::Arch::Aarch64)
}

pub fn provider_revision(software: &str) -> ProviderRevisionRecord {
    let line = |text: &str| text.parse().expect("a valid line");
    let key = ProviderRevisionKey::new(
        format!("official/{software}")
            .parse()
            .expect("a valid provider name"),
        "2026.10.1".parse().expect("a valid revision label"),
        digest(),
    );
    let revision = ProviderRevision::new(
        key,
        software.parse().expect("a valid software name"),
        CapabilitySet::new([Capability::Tls, Capability::Reload, Capability::ColdBackup]),
        LineCompatibility::default()
            .allow(line("17"), line("19"))
            .allow(line("17"), line("18"))
            .allow(line("16"), line("17")),
    );
    ProviderRevisionRecord::new(revision, Some("stackly/v1".into()), None, None)
}

pub fn artifact(software: &str) -> ArtifactRecord {
    ArtifactRecord::new(
        digest(),
        software.parse().expect("a valid software name"),
        "17.6".parse().expect("a valid version"),
        target(),
        Some("14.0".into()),
        None,
    )
}

pub fn managed_runtime(artifact: &ArtifactRecord) -> Runtime {
    Runtime::new(NewRuntime {
        id: stackly_domain::RuntimeId::generate(),
        software: artifact.software.clone(),
        version: artifact.version.clone(),
        line: "17".parse().expect("a valid line"),
        target: artifact.target,
        install: stackly_domain::InstallMode::Managed {
            artifact_sha256: artifact.sha256,
        },
        depends_on: BTreeSet::new(),
    })
}

/// Three endpoints, deliberately not in name order: a fixed TCP port, an
/// automatic UDP port that was bound, and a socket.
pub fn endpoints() -> Vec<Endpoint> {
    let port = |number| stackly_domain::Port::new(number).expect("a valid port");
    vec![
        Endpoint::Ip(IpEndpoint::new(
            "sql".parse().expect("a valid endpoint name"),
            Transport::Tcp,
            "127.0.0.1".parse().expect("a valid address"),
            PortRequest::Fixed(port(5432)),
            Some(port(5432)),
        )),
        Endpoint::Ip(IpEndpoint::new(
            "metrics".parse().expect("a valid endpoint name"),
            Transport::Udp,
            "::1".parse().expect("a valid address"),
            PortRequest::Auto,
            Some(port(49_152)),
        )),
        Endpoint::Unix(UnixEndpoint::new(
            "admin".parse().expect("a valid endpoint name"),
            ".s.PGSQL.5432".parse().expect("a valid socket name"),
        )),
    ]
}

/// `endpoints` as a new instance gets them: nothing bound yet.
fn unbound(endpoints: Vec<Endpoint>) -> Vec<Endpoint> {
    endpoints
        .into_iter()
        .map(|endpoint| match endpoint {
            Endpoint::Ip(ip) => Endpoint::Ip(IpEndpoint::new(
                ip.name().clone(),
                ip.transport(),
                ip.address(),
                ip.request(),
                None,
            )),
            unix @ Endpoint::Unix(_) => unix,
        })
        .collect()
}

/// Every stored field of `instance`, for building variants of it.
pub fn stored(instance: &Instance) -> StoredInstance {
    StoredInstance {
        id: instance.id(),
        name: instance.name().clone(),
        display_name: instance.display_name().cloned(),
        provider_revision: instance.provider_revision().clone(),
        software: instance.software().clone(),
        runtime_id: instance.runtime_id(),
        data_line: instance.data_line().clone(),
        data_version: instance.data_version().clone(),
        data_initialized_at: instance.data_initialized_at(),
        endpoints: instance.endpoints().to_vec(),
        desired: instance.desired(),
        startup: instance.startup(),
        lifecycle: instance.lifecycle(),
        last_failure: instance.last_failure().cloned(),
        revision: instance.revision(),
        created_at: instance.created_at(),
        created_by: instance.created_by(),
        retained_at: instance.retained_at(),
        retained_by: instance.retained_by(),
    }
}

/// `instance` as the registry reads it back: readiness and health
/// `unknown`, reason `daemon_restarted`, observed at `loaded_at`.
pub fn restored(instance: &Instance, loaded_at: Timestamp) -> Instance {
    Instance::restore(stored(instance), loaded_at).expect("a written instance restores")
}

/// `instance` retained at `at`, one revision later, as an update writes
/// it.
pub fn retained(instance: &Instance, at: Timestamp) -> Instance {
    let mut stored = stored(instance);
    stored.revision = instance.revision().next();
    stored.lifecycle = InstanceLifecycle::Retained;
    stored.desired = DesiredState::Stopped;
    stored.retained_at = Some(at);
    stored.retained_by = Some(OperationId::generate());
    Instance::restore(stored, at).expect("a retained instance with its marker restores")
}

/// Every stored field of `operation`, for building variants of it.
pub fn stored_operation(operation: &Operation) -> StoredOperation {
    StoredOperation {
        id: operation.id(),
        kind: operation.kind(),
        resource: operation.resource(),
        idempotency_key: operation.idempotency_key().cloned(),
        state: operation.state(),
        started_at: operation.started_at(),
        stage: operation.stage().cloned(),
        progress: operation.progress(),
        failure: operation.failure().cloned(),
        recovery_hint: operation.recovery_hint(),
        created_at: operation.created_at(),
        updated_at: operation.updated_at(),
        revision: operation.revision(),
    }
}

/// `operation` in its current state at revision 1, as an insert takes it.
pub fn as_new(operation: &Operation) -> Operation {
    Operation::restore(StoredOperation {
        revision: ResourceRevision::INITIAL,
        ..stored_operation(operation)
    })
    .expect("an operation that was valid stays valid at revision 1")
}

/// Seeds rows through its own connection to the registry file. Seeding
/// never raises the generation, and foreign keys still apply.
pub struct SideSeed {
    conn: Mutex<Connection>,
}

impl SideSeed {
    fn run(&self, what: &str, write: impl FnOnce(&Connection) -> rusqlite::Result<()>) {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        conn.execute_batch("BEGIN IMMEDIATE")
            .and_then(|()| write(&conn))
            .and_then(|()| conn.execute_batch("COMMIT"))
            .unwrap_or_else(|err| panic!("seeding {what} failed: {err}"));
    }
}

impl RegistrySeed for SideSeed {
    fn seed_backup(&self, backup: BackupSeed) {
        self.run("a backup", |conn| {
            conn.execute(
                "INSERT INTO backups (id, instance_id, runtime_id, runtime_identity, method, \
                 scope, consistency, path, created_at) VALUES (?1, ?2, ?3, \
                 'postgresql 17.6 macos aarch64', 'cold', 'instance', 'stopped', \
                 '/Volumes/backups/stackly', ?4)",
                (
                    backup.id.to_string(),
                    backup.instance_id.map(|id| id.to_string()),
                    backup.runtime_id.map(|id| id.to_string()),
                    format!("{:.9}", backup.created_at),
                ),
            )
            .map(drop)
        });
    }

    fn seed_audit_events(&self, count: u32, at: Timestamp) {
        self.run("audit events", |conn| {
            for _ in 0..count {
                conn.execute(
                    "INSERT INTO audit_events (id, actor, action, result, safe_details, \
                     occurred_at) VALUES (?1, 'daemon', 'test.seed', 'succeeded', '{}', ?2)",
                    (AuditEventId::generate().to_string(), format!("{at:.9}")),
                )?;
            }
            Ok(())
        });
    }

    fn seed_observations(&self, instance: InstanceId, count: u32, at: Timestamp) {
        let seeded = seeded_observation(instance, at);
        let identity = &seeded.identity;
        let executable = identity
            .executable
            .to_str()
            .expect("the seeded executable is UTF-8")
            .to_owned();
        self.run("process observations", |conn| {
            for _ in 0..count {
                conn.execute(
                    "INSERT INTO process_observations (instance_id, pid, start_identity, \
                     executable_identity, boot_id, session_id, observed_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    (
                        instance.to_string(),
                        identity.pid.get(),
                        identity.start.to_string(),
                        &executable,
                        identity.boot.to_string(),
                        identity.session.to_string(),
                        format!("{at:.9}"),
                    ),
                )?;
            }
            Ok(())
        });
    }
}

/// `path` with `suffix` appended to its file name, for `-wal` and `-shm`.
pub fn suffixed(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}
