//! A fake daemon for Up (spec 0046): a real engine on a fake registry, and
//! ports whose children are real engine operations that move a table of
//! instance states instead of processes.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_core::environment::children::{ChildOrigin, ChildStart};
use stackly_core::environment::up::{
    BlockedItem, InstanceNow, PlanRequest, PlannedUp, UpBoard, UpChildren, UpError, UpReads,
};
use stackly_core::environment::{PlanDeps, PlanInputs, plan, validate};
use stackly_core::instance::create::{CreateDeps, CreateDraft, EnvironmentBinding};
use stackly_core::operation::testing::RecordingEvents;
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BoxFuture, BusyPolicy, EngineDeps, EngineLimits, OperationContext,
    OperationEngine, OperationError, OperationOutcome, OperationWork, Origin, StepSpec, WorkError,
    WorkOutcome, WorkResult,
};
use stackly_core::secret::Redactor;
use stackly_core::supervisor::fixture::install_fake_runtime;
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{
    DetailKey, Environment, EnvironmentMember, ErrorCodeName, FailureError, FailureItem,
    GuardDecision, Instance, InstanceId, InstanceLifecycle, JournalPayload, Lease, LeaseHolder,
    LeaseId, OperationKind, OperationResource, Ownership, ProviderName, ProviderRevisionKey,
    ProviderRevisionRecord, ReasonCode, Registry, ResourceRef, Runtime, RuntimeId, Sha256Digest,
    WriteBatch, WriteOp,
};
use stackly_platform::{Clock, Platform};
use stackly_testkit::{FakePlatform, FakeRegistry, InstanceBuilder, TestRoot};

use crate::common::{catalog, package};
use crate::site::Probe;

/// How a scripted child ends.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// It succeeds after `delay`.
    Ok(Duration),
    /// It fails with `code` after `delay`.
    Fail(Duration, &'static str),
    /// It fails with `secret.needs_secret` for `reference`.
    NeedsSecret(&'static str),
    /// The port refuses to start it, with `code`.
    Refuse(&'static str),
}

/// A scripted failure.
#[derive(Debug, thiserror::Error)]
#[error("scripted child failed with {code}")]
struct ChildError {
    code: ErrorCodeName,
    items: Vec<FailureItem>,
}

impl OperationError for ChildError {
    fn code(&self) -> ErrorCodeName {
        self.code.clone()
    }
    fn retryable(&self) -> bool {
        false
    }
    fn details(&self) -> Vec<(DetailKey, String)> {
        Vec::new()
    }
    fn items(&self) -> Vec<FailureItem> {
        self.items.clone()
    }
}

fn needs_items(reference: &str) -> Vec<FailureItem> {
    vec![
        FailureItem::new(
            ReasonCode::from_static("reference"),
            vec![
                (
                    "reference".parse().expect("the test fixture holds"),
                    reference.to_owned(),
                ),
                (
                    "reason".parse().expect("the test fixture holds"),
                    "missing".to_owned(),
                ),
                (
                    "generateAllowed".parse().expect("the test fixture holds"),
                    "true".to_owned(),
                ),
            ],
        )
        .expect("the test fixture holds"),
    ]
}

type Job = Box<dyn FnOnce(OperationContext) -> BoxFuture<'static, WorkResult> + Send>;

/// A work from a closure.
struct FnWork(Job);

impl OperationWork for FnWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        (self.0)(ctx)
    }
}

/// What the fake knows of instances.
#[derive(Default)]
pub struct Table {
    states: Mutex<HashMap<InstanceId, InstanceNow>>,
    /// Every event, in order: `install <sha>`, `create <name>`, `start
    /// <name>`.
    pub log: Mutex<Vec<String>>,
    names: Mutex<HashMap<InstanceId, String>>,
}

impl Table {
    pub fn set(&self, id: InstanceId, lifecycle: InstanceLifecycle, ready: bool) {
        self.states
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id,
                InstanceNow {
                    lifecycle,
                    ready,
                    refuses_start: false,
                },
            );
    }

    pub fn get(&self, id: InstanceId) -> Option<InstanceNow> {
        self.states
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .copied()
    }

    fn name(&self, id: InstanceId) -> String {
        self.names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn log(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn push(&self, line: String) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }
}

/// The scripts of the children, by instance name or artifact.
#[derive(Default)]
pub struct Scripts {
    pub starts: Mutex<HashMap<String, Outcome>>,
    pub creates: Mutex<HashMap<String, Outcome>>,
    pub installs: Mutex<HashMap<Sha256Digest, Outcome>>,
}

/// The fake daemon.
pub struct Rig {
    pub root: TestRoot,
    pub fake: FakePlatform,
    pub platform: Arc<Platform>,
    pub prepared: Arc<PreparedLayout>,
    pub registry: Arc<FakeRegistry>,
    pub engine: OperationEngine,
    pub runtime: Runtime,
    pub key: ProviderRevisionKey,
    pub table: Arc<Table>,
    pub scripts: Arc<Scripts>,
    pub board: Arc<UpBoard>,
    pub probe: Arc<Probe>,
}

impl Rig {
    pub async fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = Arc::new(fake.platform());
        let prepared = Arc::new(prepare_layout(&platform).expect("the fake roots prepare"));
        let registry = Arc::new(FakeRegistry::new(
            Arc::new(fake.clock().clone()),
            fake.clock().now(),
        ));
        let fake_package = package("fake");
        let server = root.path().join("stand-in-server");
        fs::write(&server, b"#!/bin/sh\nexit 0\n").expect("the stand in writes");
        let (runtime, artifact) = install_fake_runtime(
            prepared.layout(),
            &fake_package,
            &server,
            fake.clock().now(),
        )
        .unwrap_or_else(|error| panic!("the runtime installs: {error}"));
        registry
            .commit(WriteBatch::new(vec![
                WriteOp::InsertArtifact(artifact),
                WriteOp::InsertRuntime(runtime.clone()),
                WriteOp::InsertProviderRevision(ProviderRevisionRecord::new(
                    fake_package.provider_revision().clone(),
                    None,
                    None,
                    None,
                )),
            ]))
            .expect("the runtime commits");
        let engine = OperationEngine::new(
            EngineDeps {
                registry: Arc::clone(&registry) as Arc<dyn Registry>,
                platform: Arc::clone(&platform),
                layout: Arc::clone(&prepared),
                events: Arc::new(RecordingEvents::new()),
                redactor: Arc::new(Redactor::new()) as Arc<dyn stackly_core::operation::Redact>,
            },
            EngineLimits::default()
                .with_intervals(Duration::from_millis(100), Duration::from_millis(20)),
        );
        engine.recover().await.expect("an empty registry recovers");
        engine.start();
        Self {
            root,
            fake,
            platform,
            prepared,
            registry,
            engine,
            runtime,
            key: fake_package.key().clone(),
            table: Arc::new(Table::default()),
            scripts: Arc::new(Scripts::default()),
            board: Arc::new(UpBoard::new()),
            probe: Arc::new(Probe::default()),
        }
    }

    /// Commits a fake instance named `name` in `lifecycle`, ready when it
    /// runs.
    pub fn instance(&self, name: &str, lifecycle: InstanceLifecycle) -> Instance {
        let instance = InstanceBuilder::new(&self.runtime, &self.key)
            .name(name)
            .lifecycle(lifecycle)
            .build();
        self.registry
            .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
                instance.clone(),
            )]))
            .expect("the instance commits");
        self.table.set(
            instance.id(),
            lifecycle,
            lifecycle == InstanceLifecycle::Running,
        );
        self.table
            .names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(instance.id(), name.to_owned());
        instance
    }

    pub fn children(&self) -> Arc<FakeChildren> {
        Arc::new(FakeChildren {
            engine: self.engine.clone(),
            table: Arc::clone(&self.table),
            scripts: Arc::clone(&self.scripts),
            runtime: self.runtime.clone(),
            key: self.key.clone(),
        })
    }

    pub fn environment(&self, id: stackly_domain::EnvironmentId) -> Option<Environment> {
        self.registry
            .environments()
            .expect("the registry reads")
            .value
            .into_iter()
            .find(|environment| environment.id() == id)
    }
}

/// The child ports.
pub struct FakeChildren {
    engine: OperationEngine,
    table: Arc<Table>,
    scripts: Arc<Scripts>,
    runtime: Runtime,
    key: ProviderRevisionKey,
}

fn refusal(code: &'static str) -> FailureError {
    FailureError::new(
        ErrorCodeName::from_static(code),
        "the child was refused",
        Vec::new(),
    )
    .expect("the test fixture holds")
}

impl FakeChildren {
    async fn begin(
        &self,
        parent: &ChildOrigin,
        kind: OperationKind,
        resource: OperationResource,
        creates: bool,
        busy: BusyPolicy,
        job: Job,
    ) -> Result<ChildStart<InstanceId>, FailureError> {
        let request = BeginRequest {
            kind,
            resource,
            creates_resource: creates,
            extra_locks: Vec::new(),
            busy,
            expected_revision: None,
            origin: Origin::Child(parent.operation.clone()),
            input: JournalPayload::empty(),
            work: Box::new(FnWork(job)),
            precheck: None,
        };
        match self.engine.begin(request).await {
            Ok(BeginOutcome::Created(view)) => Ok(ChildStart::Started {
                operation: view.operation.id(),
            }),
            Ok(other) => panic!("a fake child begins: {other:?}"),
            Err(stackly_core::operation::BeginError::RestartPending) => {
                Err(refusal("daemon.restart_pending"))
            }
            Err(error) => panic!("a fake child begins: {error:?}"),
        }
    }
}

/// The work of a scripted child: waits `delay`, then runs `on_ok` and
/// succeeds with `resources`, or fails.
fn scripted(
    outcome: Outcome,
    on_ok: Box<dyn FnOnce() + Send>,
    on_fail: Box<dyn FnOnce() + Send>,
    resources: Vec<ResourceRef>,
    commit: Vec<WriteOp>,
) -> Job {
    Box::new(move |mut ctx: OperationContext| {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "run".parse().expect("the test fixture holds"),
            ))
            .await?;
            let (delay, failure) = match &outcome {
                Outcome::Ok(delay) => (*delay, None),
                Outcome::Fail(delay, code) => (*delay, Some((*code, Vec::new()))),
                Outcome::NeedsSecret(reference) => (
                    Duration::ZERO,
                    Some(("secret.needs_secret", needs_items(reference))),
                ),
                Outcome::Refuse(_) => (Duration::ZERO, None),
            };
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = ctx.stopped() => {
                    if let Err(stop) = ctx.check_cancel() {
                        on_fail();
                        return Err(stop);
                    }
                }
            }
            if let Some((code, items)) = failure {
                on_fail();
                return Err(WorkError::Failed(Box::new(ChildError {
                    code: ErrorCodeName::from_static(code),
                    items,
                })));
            }
            if !commit.is_empty() {
                ctx.commit(stackly_core::operation::FinalBatch {
                    expected_generation: None,
                    ops: commit,
                })
                .await?;
            }
            on_ok();
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome {
                    resources,
                    partial: Vec::new(),
                },
                finalize: None,
            })
        })
    })
}

impl UpChildren for FakeChildren {
    fn install(
        &self,
        parent: &ChildOrigin,
        _provider: &ProviderName,
        artifact: Sha256Digest,
    ) -> BoxFuture<'_, Result<ChildStart<RuntimeId>, FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            self.table.push(format!("install {artifact}"));
            let outcome = self
                .scripts
                .installs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&artifact)
                .cloned()
                .unwrap_or(Outcome::Ok(Duration::from_millis(50)));
            if let Outcome::Refuse(code) = outcome {
                return Err(refusal(code));
            }
            let runtime = self.runtime.id();
            let job = scripted(
                outcome,
                Box::new(|| {}),
                Box::new(|| {}),
                vec![ResourceRef::Runtime(runtime)],
                Vec::new(),
            );
            let started = self
                .begin(
                    &parent,
                    OperationKind::InstallRuntime,
                    OperationResource::Manager,
                    false,
                    BusyPolicy::Queue,
                    job,
                )
                .await?;
            Ok(match started {
                ChildStart::Started { operation } => ChildStart::Started { operation },
                ChildStart::AlreadyDone(_) => ChildStart::AlreadyDone(runtime),
            })
        })
    }

    fn create(
        &self,
        parent: &ChildOrigin,
        draft: CreateDraft,
        binding: EnvironmentBinding,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            self.table.push(format!("create {}", draft.name));
            let outcome = self
                .scripts
                .creates
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&draft.name)
                .cloned()
                .unwrap_or(Outcome::Ok(Duration::from_millis(20)));
            if let Outcome::Refuse(code) = outcome {
                return Err(refusal(code));
            }
            let instance = InstanceBuilder::new(&self.runtime, &self.key)
                .name(&draft.name)
                .lifecycle(InstanceLifecycle::Stopped)
                .build();
            let id = instance.id();
            // The fake create binds like the real one: the instance, its
            // member, and its lease. The member goes in through the
            // instance's own commit, read fresh by the engine's fake.
            let table = Arc::clone(&self.table);
            let name = draft.name.clone();
            let registry_binding = binding.clone();
            let job: Job = Box::new(move |mut ctx: OperationContext| {
                Box::pin(async move {
                    ctx.begin_step(StepSpec::new(
                        "run".parse().expect("the test fixture holds"),
                    ))
                    .await?;
                    tokio::time::sleep(match &outcome {
                        Outcome::Ok(delay) | Outcome::Fail(delay, _) => *delay,
                        _ => Duration::ZERO,
                    })
                    .await;
                    match &outcome {
                        Outcome::Fail(_, code) => {
                            return Err(WorkError::Failed(Box::new(ChildError {
                                code: ErrorCodeName::from_static(code),
                                items: Vec::new(),
                            })));
                        }
                        Outcome::NeedsSecret(reference) => {
                            return Err(WorkError::Failed(Box::new(ChildError {
                                code: ErrorCodeName::from_static("secret.needs_secret"),
                                items: needs_items(reference),
                            })));
                        }
                        _ => {}
                    }
                    let now = ctx.platform().clock().now();
                    let binding = registry_binding;
                    let _ = ctx
                        .commit_guarded(Box::new(move |view| {
                            let mut ops = vec![WriteOp::InsertInstance(instance)];
                            let Some(mut environment) = view
                                .environments()?
                                .into_iter()
                                .find(|environment| environment.id() == binding.environment)
                            else {
                                return Ok(GuardDecision::Write(ops));
                            };
                            let expected = environment.revision();
                            environment
                                .bind_member(
                                    EnvironmentMember::new(binding.alias, id, Ownership::Dedicated)
                                        .with_conditions(binding.conditions),
                                )
                                .expect("the alias is free");
                            ops.push(WriteOp::UpdateEnvironment {
                                environment,
                                expected,
                            });
                            ops.push(WriteOp::InsertLease(
                                Lease::new(
                                    LeaseId::generate(),
                                    id,
                                    LeaseHolder::Environment(binding.environment),
                                    None,
                                    now,
                                )
                                .expect("the test fixture holds"),
                            ));
                            Ok(GuardDecision::Write(ops))
                        }))
                        .await?;
                    table.set(id, InstanceLifecycle::Stopped, false);
                    table
                        .names
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(id, name);
                    ctx.finish_step(None).await?;
                    Ok(WorkOutcome {
                        result: OperationOutcome {
                            resources: vec![ResourceRef::Instance(id)],
                            partial: Vec::new(),
                        },
                        finalize: None,
                    })
                })
            });
            self.begin(
                &parent,
                OperationKind::CreateInstance,
                OperationResource::Instance(id),
                true,
                BusyPolicy::Reject,
                job,
            )
            .await
        })
    }

    fn start(
        &self,
        parent: &ChildOrigin,
        instance: InstanceId,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            let name = self.table.name(instance);
            self.table.push(format!("start {name}"));
            let outcome = self
                .scripts
                .starts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&name)
                .cloned()
                .unwrap_or(Outcome::Ok(Duration::from_millis(20)));
            if let Outcome::Refuse(code) = outcome {
                return Err(refusal(code));
            }
            self.table.set(instance, InstanceLifecycle::Starting, false);
            let up = Arc::clone(&self.table);
            let down = Arc::clone(&self.table);
            let job = scripted(
                outcome,
                Box::new(move || up.set(instance, InstanceLifecycle::Running, true)),
                Box::new(move || down.set(instance, InstanceLifecycle::Failed, false)),
                vec![ResourceRef::Instance(instance)],
                Vec::new(),
            );
            self.begin(
                &parent,
                OperationKind::StartInstance,
                OperationResource::Instance(instance),
                false,
                BusyPolicy::Queue,
                job,
            )
            .await
        })
    }
}

/// The read ports: plans a fixture manifest on the rig's registry.
pub struct FakeReads {
    pub rig_registry: Arc<FakeRegistry>,
    pub platform: Arc<Platform>,
    pub prepared: Arc<PreparedLayout>,
    pub probe: Arc<Probe>,
    pub table: Arc<Table>,
    /// The manifest bytes, by path.
    pub files: Mutex<HashMap<std::path::PathBuf, Vec<u8>>>,
    /// A plan to answer instead of planning, as a registry change between
    /// the plan of `accept` and its guard would leave it.
    pub frozen: Mutex<Option<PlannedUp>>,
}

impl FakeReads {
    pub fn new(rig: &Rig) -> Arc<Self> {
        Arc::new(Self {
            rig_registry: Arc::clone(&rig.registry),
            platform: Arc::clone(&rig.platform),
            prepared: Arc::clone(&rig.prepared),
            probe: Arc::clone(&rig.probe),
            table: Arc::clone(&rig.table),
            files: Mutex::new(HashMap::new()),
            frozen: Mutex::new(None),
        })
    }

    /// Plans `request` now, as the precheck does.
    pub fn plan_now(&self, request: &PlanRequest) -> PlannedUp {
        let bytes = self
            .files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&request.path)
            .cloned()
            .expect("the manifest is known");
        let catalog = catalog();
        let validation = validate(&bytes, &catalog);
        let location =
            stackly_domain::ManifestLocation::new(&request.path).expect("an absolute path");
        let state = stackly_core::catalog::CatalogState::Loaded(Arc::new(catalog.clone()));
        let host = self.platform.info().host_facts();
        let views: Vec<stackly_core::environment::InstanceView> = self
            .rig_registry
            .instances(stackly_domain::InstanceFilter {
                include_retained: true,
            })
            .expect("the registry reads")
            .value
            .iter()
            .map(|instance| {
                let now = self.table.get(instance.id());
                stackly_core::environment::InstanceView {
                    id: instance.id(),
                    name: instance.name().clone(),
                    provider: instance.provider_revision().provider().clone(),
                    runtime_version: instance.data_version().clone(),
                    runtime_line: None,
                    lifecycle: now.map_or(instance.lifecycle(), |now| now.lifecycle),
                    initialized: true,
                    readiness: if now.is_some_and(|now| now.ready) {
                        stackly_domain::ReadinessState::Ready
                    } else {
                        stackly_domain::ReadinessState::Unknown
                    },
                    health: stackly_domain::HealthState::Unknown,
                    blocker: None,
                    detail: None,
                    endpoints: Vec::new(),
                    runtime_artifact_sha256: None,
                }
            })
            .collect();
        let deps = PlanDeps {
            create: CreateDeps {
                registry: &*self.rig_registry,
                catalog: &state,
                source: None,
                platform: &self.platform,
                prepared: &self.prepared,
                probe: &*self.probe,
            },
            catalog: &catalog,
            host: &host,
            instances: &views,
        };
        let planned = plan(
            &deps,
            &PlanInputs {
                manifest: &validation,
                location: &location,
                registered: request.registered,
                lock: None,
                overrides: None,
                locked: request.locked,
            },
        )
        .expect("the plan reads");
        let blocked = planned
            .blockers
            .iter()
            .map(|blocker| BlockedItem {
                service: blocker.service.clone(),
                code: blocker.kind.code().to_owned(),
                message: "blocked".to_owned(),
            })
            .collect();
        PlannedUp {
            validation,
            location,
            plan: planned,
            blocked,
            instance_needs: Vec::new(),
            create_needs: Vec::new(),
        }
    }
}

impl UpReads for FakeReads {
    fn plan(&self, request: &PlanRequest) -> BoxFuture<'_, Result<PlannedUp, UpError>> {
        let frozen = self
            .frozen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let planned = frozen.unwrap_or_else(|| self.plan_now(request));
        Box::pin(async move { Ok(planned) })
    }

    fn instance(&self, id: InstanceId) -> BoxFuture<'_, Option<InstanceNow>> {
        let now = self.table.get(id);
        Box::pin(async move { now })
    }
}
