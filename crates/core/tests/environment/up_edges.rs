//! Edges of the Up driver (spec 0046 AC-6, AC-8, AC-11, AC-14): the
//! queue and settle limits, an instance that is `stopping` when its turn
//! comes, the install and start concurrency limits, a dependency that is
//! no longer ready right before its dependent starts, the default edge
//! timeout, and a cancel in the install and create stages.
//!
//! The 120 s and 600 s limits run on tokio's paused clock, so they take
//! no real time. Every service runs the `fake` provider; no edge stands
//! for a real dependency between databases.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_core::environment::children::{ChildOrigin, ChildStart};
use stackly_core::environment::up::{
    CreateJob, DriverDeps, Edge, Graph, InstanceNow, PlanRequest, PlannedUp, Service, UpBoard,
    UpChildren, UpError, UpReads, UpRequest, driver::Ending, up_work,
};
use stackly_core::environment::{draft_for, planned_runtime_placeholder_id};
use stackly_core::instance::create::{CreateDraft, EnvironmentBinding};
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BoxFuture, BusyPolicy, OperationContext, OperationDetailView,
    OperationEngine, OperationOutcome, OperationWork, Origin, StepSpec, WorkOutcome, WorkResult,
};
use stackly_core::secret::SecretCaller;
use stackly_domain::{
    EnvironmentId, InstanceId, InstanceLifecycle, JournalPayload, LockKey, MemberAlias,
    OperationId, OperationKind, OperationQuery, OperationResource, OperationState,
    OperationTrigger, ProviderName, Registry, RuntimeId, ServiceReport, Sha256Digest, SkipReason,
    UpPhase, UpStage, UpState,
};

use crate::up_fake::{FakeChildren, FakeReads, Outcome, Rig};

const GRAPH: &str = include_str!("../fixtures/environments/up-graph.yaml");

fn alias(text: &str) -> MemberAlias {
    text.parse().expect("the test fixture holds")
}

/// Waits up to an hour of tokio time, which the paused clock runs through
/// at once.
async fn finished(rig: &Rig, operation: OperationId) -> OperationDetailView {
    tokio::time::timeout(Duration::from_secs(3_600), rig.engine.wait_final(operation))
        .await
        .expect("the operation ends in time")
        .expect("the engine runs")
        .expect("the operation exists")
}

/// Counts the operations of `kind` that have not ended.
async fn active(engine: &OperationEngine, kind: OperationKind) -> usize {
    engine
        .list(OperationQuery {
            active_only: true,
            resource: None,
            limit: 500,
            before: None,
        })
        .await
        .expect("the engine lists")
        .items
        .iter()
        .filter(|view| view.operation.kind() == kind)
        .count()
}

/// Samples, every 50 ms of tokio time until `stop` turns true, how many
/// operations of `kind` are active, and returns the most it saw.
fn sample_max(
    engine: OperationEngine,
    kind: OperationKind,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> tokio::task::JoinHandle<usize> {
    tokio::spawn(async move {
        let mut most = 0;
        while !stop.load(Ordering::SeqCst) {
            most = most.max(active(&engine, kind).await);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        most
    })
}

/// The fake ports, recording how many children of the same kind were
/// still active each time the driver asked for one more.
struct Counting {
    inner: Arc<FakeChildren>,
    engine: OperationEngine,
    installs: Mutex<Vec<usize>>,
    starts: Mutex<Vec<usize>>,
}

impl Counting {
    fn new(rig: &Rig) -> Arc<Self> {
        Arc::new(Self {
            inner: rig.children(),
            engine: rig.engine.clone(),
            installs: Mutex::new(Vec::new()),
            starts: Mutex::new(Vec::new()),
        })
    }

    fn installs(&self) -> Vec<usize> {
        self.installs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn starts(&self) -> Vec<usize> {
        self.starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl UpChildren for Counting {
    fn install(
        &self,
        parent: &ChildOrigin,
        provider: &ProviderName,
        artifact: Sha256Digest,
    ) -> BoxFuture<'_, Result<ChildStart<RuntimeId>, stackly_domain::FailureError>> {
        let parent = parent.clone();
        let provider = provider.clone();
        Box::pin(async move {
            let now = active(&self.engine, OperationKind::InstallRuntime).await;
            self.installs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(now);
            self.inner.install(&parent, &provider, artifact).await
        })
    }

    fn create(
        &self,
        parent: &ChildOrigin,
        draft: CreateDraft,
        binding: EnvironmentBinding,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, stackly_domain::FailureError>> {
        self.inner.create(parent, draft, binding)
    }

    fn start(
        &self,
        parent: &ChildOrigin,
        instance: InstanceId,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, stackly_domain::FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            let now = active(&self.engine, OperationKind::StartInstance).await;
            self.starts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(now);
            self.inner.start(&parent, instance).await
        })
    }
}

/// Where a driver run keeps how it ended and its reports.
type Seen = Arc<Mutex<Option<(Ending, Vec<ServiceReport>)>>>;

/// A work that runs the driver alone on hand made services and keeps its
/// reports and how it ended.
struct DriverWork {
    deps: DriverDeps,
    environment: EnvironmentId,
    services: Vec<Service>,
    graph: Graph,
    seen: Seen,
}

impl OperationWork for DriverWork {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(
                StepSpec::new("run".parse().expect("the test fixture holds")).irreversible(),
            )
            .await?;
            let origin = ChildOrigin {
                operation: stackly_core::operation::ChildOrigin {
                    parent: ctx.id(),
                    trigger: ctx.trigger().clone(),
                },
                caller: SecretCaller::background(),
            };
            let now = ctx.platform().clock().now();
            let mut driver = stackly_core::environment::up::driver::Driver::new(
                &mut ctx,
                self.deps,
                origin,
                self.environment,
                now,
                self.graph,
                self.services,
            );
            let ending = driver.run().await?;
            *self.seen.lock().unwrap_or_else(PoisonError::into_inner) =
                Some((ending, driver.reports()));
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

/// One driver run on the rig.
struct Run {
    operation: OperationId,
    seen: Seen,
}

impl Run {
    async fn begin(
        rig: &Rig,
        children: Arc<dyn UpChildren>,
        reads: Arc<dyn UpReads>,
        services: Vec<Service>,
        graph: Graph,
    ) -> Self {
        let environment = EnvironmentId::generate();
        let seen = Arc::new(Mutex::new(None));
        let work = DriverWork {
            deps: DriverDeps {
                engine: rig.engine.clone(),
                children,
                reads,
                board: Arc::new(UpBoard::new()),
            },
            environment,
            services,
            graph,
            seen: Arc::clone(&seen),
        };
        let request = BeginRequest {
            kind: OperationKind::EnvironmentUp,
            resource: OperationResource::Environment(environment),
            creates_resource: true,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work: Box::new(work),
            precheck: None,
        };
        let BeginOutcome::Created(view) = rig
            .engine
            .begin(request)
            .await
            .expect("the driver work begins")
        else {
            panic!("the driver work begins");
        };
        Self {
            operation: view.operation.id(),
            seen,
        }
    }

    /// Waits for the end and returns how the driver ended and its reports
    /// by alias.
    async fn end(&self, rig: &Rig) -> (Ending, BTreeMap<String, ServiceReport>) {
        finished(rig, self.operation).await;
        let (ending, reports) = self
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("the driver ran to its end");
        (
            ending,
            reports
                .into_iter()
                .map(|report| (report.alias().to_string(), report))
                .collect(),
        )
    }
}

/// A graph of `order` where each `(dependent, dependency, seconds)` is an
/// edge.
fn graph(order: &[&str], edges: &[(&str, &str, u64)]) -> Graph {
    let mut map: BTreeMap<MemberAlias, Vec<Edge>> = BTreeMap::new();
    for (dependent, dependency, seconds) in edges {
        map.entry(alias(dependent)).or_default().push(Edge {
            dependency: alias(dependency),
            timeout: Duration::from_secs(*seconds),
        });
    }
    Graph::new(order.iter().map(|name| alias(name)).collect(), map)
}

/// A service of an instance that already exists.
fn existing(name: &str, instance: InstanceId) -> Service {
    Service::new(alias(name), Some(instance), None)
}

fn script_start(rig: &Rig, name: &str, outcome: Outcome) {
    rig.scripts
        .starts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(name.to_owned(), outcome);
}

/// A work that holds its resource until it is cancelled.
struct Hold;

impl OperationWork for Hold {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "run".parse().expect("the test fixture holds"),
            ))
            .await?;
            tokio::select! {
                () = tokio::time::sleep(Duration::from_secs(100_000)) => {}
                () = ctx.stopped() => {}
            }
            ctx.check_cancel()?;
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

// covers: AC-8 (spec 0046), *Operasi anak* CHILD_QUEUE_TIMEOUT_SECONDS
#[tokio::test(start_paused = true)]
async fn a_child_start_queued_past_the_child_queue_limit_is_cancelled_and_its_service_fails_busy() {
    let rig = Rig::new().await;
    let db = rig.instance("db", InstanceLifecycle::Stopped);
    // Another operation holds the instance for a long time.
    let BeginOutcome::Created(holder) = rig
        .engine
        .begin(BeginRequest {
            kind: OperationKind::StopInstance,
            resource: OperationResource::Instance(db.id()),
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work: Box::new(Hold),
            precheck: None,
        })
        .await
        .expect("the holder begins")
    else {
        panic!("the holder begins");
    };
    let holder = holder.operation.id();
    let began = tokio::time::Instant::now();

    let run = Run::begin(
        &rig,
        rig.children(),
        FakeReads::new(&rig),
        vec![existing("db", db.id())],
        graph(&["db"], &[]),
    )
    .await;
    let (ending, reports) = run.end(&rig).await;

    assert_eq!(ending, Ending::Done);
    let report = &reports["db"];
    assert_eq!(report.phase(), UpPhase::Failed);
    assert_eq!(report.stage(), Some(UpStage::Start));
    assert_eq!(
        report.code().map(ToString::to_string).as_deref(),
        Some("resource.busy")
    );
    let child = report.operation().expect("the queued child is recorded");
    let child = rig
        .engine
        .get(child)
        .await
        .expect("the engine reads")
        .expect("the child exists");
    assert_eq!(child.view.operation.state(), OperationState::Cancelled);
    assert!(
        began.elapsed() >= Duration::from_secs(120),
        "the parent waited the whole queue limit: {:?}",
        began.elapsed()
    );
    // The holder still runs: the parent cancels only its own child.
    let holding = rig
        .engine
        .get(holder)
        .await
        .expect("the engine reads")
        .expect("the holder exists");
    assert_eq!(holding.view.operation.state(), OperationState::Running);
    rig.engine
        .cancel(holder, OperationTrigger::background())
        .await
        .expect("the holder cancels");
    finished(&rig, holder).await;
}

// covers: AC-8 (spec 0046), *Aturan graph* rule 3, SERVICE_SETTLE_TIMEOUT_SECONDS
#[tokio::test(start_paused = true)]
async fn a_running_instance_that_never_becomes_ready_fails_its_service_with_service_not_ready() {
    let rig = Rig::new().await;
    let db = rig.instance("db", InstanceLifecycle::Running);
    rig.table.set(db.id(), InstanceLifecycle::Running, false);
    let api = rig.instance("api", InstanceLifecycle::Stopped);
    let began = tokio::time::Instant::now();

    let run = Run::begin(
        &rig,
        rig.children(),
        FakeReads::new(&rig),
        vec![existing("db", db.id()), existing("api", api.id())],
        graph(&["db", "api"], &[("api", "db", 600)]),
    )
    .await;
    let (_, reports) = run.end(&rig).await;

    let report = &reports["db"];
    assert_eq!(report.phase(), UpPhase::Failed);
    assert_eq!(report.stage(), Some(UpStage::Start));
    assert_eq!(
        report.code().map(ToString::to_string).as_deref(),
        Some("env.service_not_ready")
    );
    assert_eq!(
        report.operation(),
        None,
        "an instance that runs gets no child"
    );
    assert!(began.elapsed() >= Duration::from_secs(120));
    assert!(
        began.elapsed() < Duration::from_secs(600),
        "the settle limit ends it, not the edge timeout"
    );
    // Its dependent never started.
    assert_eq!(reports["api"].phase(), UpPhase::Skipped);
    assert_eq!(reports["api"].reason(), Some(SkipReason::Failed));
    let log = rig.table.log();
    assert!(
        log.iter().all(|line| !line.starts_with("start ")),
        "{log:?}"
    );
}

// covers: AC-14 (spec 0046), *Aturan graph* rule 3 `stopping`
#[tokio::test(start_paused = true)]
async fn an_instance_that_is_stopping_when_its_turn_comes_is_waited_for_then_started_again() {
    let rig = Rig::new().await;
    let db = rig.instance("db", InstanceLifecycle::Running);
    rig.table.set(db.id(), InstanceLifecycle::Stopping, false);
    let table = Arc::clone(&rig.table);
    let id = db.id();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        table.set(id, InstanceLifecycle::Stopped, false);
    });

    let run = Run::begin(
        &rig,
        rig.children(),
        FakeReads::new(&rig),
        vec![existing("db", db.id())],
        graph(&["db"], &[]),
    )
    .await;
    let (_, reports) = run.end(&rig).await;

    assert_eq!(reports["db"].phase(), UpPhase::Ready);
    assert!(
        reports["db"].operation().is_some(),
        "it was started again through a child"
    );
    assert_eq!(rig.table.log(), ["start db"]);
    assert!(
        rig.table
            .get(db.id())
            .expect("the test fixture holds")
            .running_and_ready()
    );
}

fn fake_spec(name: &str) -> stackly_core::environment::ServiceSpec {
    let manifest = format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: x\nservices:\n  {name}:\n    provider: testing/fake\n    version: \"0.1\"\n"
    );
    let validation =
        stackly_core::environment::validate(manifest.as_bytes(), &crate::common::catalog());
    validation
        .manifest
        .expect("the test fixture holds")
        .services
        .remove(0)
}

fn create_job(environment: EnvironmentId, name: &str, install: Option<Sha256Digest>) -> CreateJob {
    let spec = fake_spec(name);
    CreateJob {
        draft: draft_for(
            &spec,
            &format!("x-{name}").parse().expect("the test fixture holds"),
            planned_runtime_placeholder_id(),
        ),
        binding: EnvironmentBinding {
            environment,
            alias: alias(name),
            conditions: Vec::new(),
        },
        install: install.map(|sha| (spec.provider.clone(), sha)),
    }
}

fn digest(byte: u8) -> Sha256Digest {
    format!("{byte:02x}")
        .repeat(32)
        .parse()
        .expect("the test fixture holds")
}

// covers: AC-6 (spec 0046), UP_INSTALL_CONCURRENCY
#[tokio::test(start_paused = true)]
async fn an_up_runs_at_most_two_installs_at_once_and_does_run_two() {
    let rig = Rig::new().await;
    let environment = EnvironmentId::generate();
    let names = ["a", "b", "c", "d", "e"];
    let mut services = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let sha = digest(0x10 + u8::try_from(index).expect("five services"));
        rig.scripts
            .installs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(sha, Outcome::Ok(Duration::from_secs(3)));
        services.push(Service::new(
            alias(name),
            None,
            Some(create_job(environment, name, Some(sha))),
        ));
    }
    let counting = Counting::new(&rig);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = sample_max(
        rig.engine.clone(),
        OperationKind::InstallRuntime,
        Arc::clone(&stop),
    );

    let run = Run::begin(
        &rig,
        Arc::clone(&counting) as Arc<dyn UpChildren>,
        FakeReads::new(&rig),
        services,
        graph(&names, &[]),
    )
    .await;
    let (_, reports) = run.end(&rig).await;
    stop.store(true, Ordering::SeqCst);
    let most = sampler.await.expect("the sampler ends");

    let installs = counting.installs();
    assert_eq!(installs.len(), 5, "one install per artifact");
    assert!(
        installs.iter().all(|&running| running < 2),
        "never a third install beside two: {installs:?}"
    );
    assert_eq!(most, 2, "two installs ran at once, never more");
    for name in names {
        assert_eq!(reports[name].phase(), UpPhase::Ready, "{name}");
    }
}

// covers: AC-8 (spec 0046), UP_START_CONCURRENCY and `order`
#[tokio::test(start_paused = true)]
async fn an_up_runs_at_most_four_starts_at_once_picked_in_plan_order() {
    let rig = Rig::new().await;
    let names = ["s1", "s2", "s3", "s4", "s5", "s6"];
    let mut services = Vec::new();
    for name in names {
        let instance = rig.instance(name, InstanceLifecycle::Stopped);
        script_start(&rig, name, Outcome::Ok(Duration::from_secs(2)));
        services.push(existing(name, instance.id()));
    }
    let counting = Counting::new(&rig);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = sample_max(
        rig.engine.clone(),
        OperationKind::StartInstance,
        Arc::clone(&stop),
    );

    let run = Run::begin(
        &rig,
        Arc::clone(&counting) as Arc<dyn UpChildren>,
        FakeReads::new(&rig),
        services,
        graph(&names, &[]),
    )
    .await;
    let (_, reports) = run.end(&rig).await;
    stop.store(true, Ordering::SeqCst);
    let most = sampler.await.expect("the sampler ends");

    let starts = counting.starts();
    assert_eq!(starts.len(), 6);
    assert!(
        starts.iter().all(|&running| running < 4),
        "never a fifth start beside four: {starts:?}"
    );
    assert_eq!(most, 4, "four starts ran at once, never more");
    let mut first: Vec<String> = rig.table.log()[..4].to_vec();
    first.sort();
    assert_eq!(
        first,
        ["start s1", "start s2", "start s3", "start s4"],
        "the first four are picked in plan order"
    );
    for name in names {
        assert_eq!(reports[name].phase(), UpPhase::Ready, "{name}");
    }
}

/// The fake reads, where `name`'s instance reads ready only `ready_reads`
/// times and `failed` after that, as a dependency that crashed right after
/// it became ready.
struct Flaky {
    inner: Arc<FakeReads>,
    target: InstanceId,
    ready_reads: AtomicUsize,
}

impl UpReads for Flaky {
    fn plan(&self, request: &PlanRequest) -> BoxFuture<'_, Result<PlannedUp, UpError>> {
        self.inner.plan(request)
    }

    fn instance(&self, id: InstanceId) -> BoxFuture<'_, Option<InstanceNow>> {
        Box::pin(async move {
            let now = self.inner.instance(id).await;
            if id != self.target || !now.is_some_and(|now| now.running_and_ready()) {
                return now;
            }
            let left = self.ready_reads.load(Ordering::SeqCst);
            if left > 0 {
                self.ready_reads.store(left - 1, Ordering::SeqCst);
                return now;
            }
            Some(InstanceNow {
                lifecycle: InstanceLifecycle::Failed,
                ready: false,
                refuses_start: false,
            })
        })
    }
}

// covers: AC-8 (spec 0046), *Aturan graph* rule 2
#[tokio::test]
async fn a_dependency_that_is_no_longer_ready_when_its_dependent_starts_skips_it_unstarted() {
    let rig = Rig::new().await;
    let db = rig.instance("db", InstanceLifecycle::Stopped);
    let api = rig.instance("api", InstanceLifecycle::Stopped);
    let web = rig.instance("web", InstanceLifecycle::Stopped);
    let reads = Arc::new(Flaky {
        inner: FakeReads::new(&rig),
        target: db.id(),
        ready_reads: AtomicUsize::new(1),
    });

    let run = Run::begin(
        &rig,
        rig.children(),
        reads,
        vec![
            existing("db", db.id()),
            existing("api", api.id()),
            existing("web", web.id()),
        ],
        graph(
            &["db", "api", "web"],
            &[("api", "db", 600), ("web", "api", 600)],
        ),
    )
    .await;
    let (_, reports) = run.end(&rig).await;

    // The dependency stays as it was in the report.
    assert_eq!(reports["db"].phase(), UpPhase::Ready);
    assert_eq!(reports["api"].phase(), UpPhase::Skipped);
    assert_eq!(reports["api"].dependency(), Some(&alias("db")));
    assert_eq!(reports["api"].reason(), Some(SkipReason::NotReady));
    assert_eq!(reports["web"].phase(), UpPhase::Skipped);
    assert_eq!(reports["web"].dependency(), Some(&alias("api")));
    assert_eq!(rig.table.log(), ["start db"], "no dependent was spawned");
}

// covers: AC-8 (spec 0046), DEFAULT_DEPENDENCY_TIMEOUT_SECONDS
#[tokio::test(start_paused = true)]
async fn an_edge_with_no_timeout_waits_six_hundred_seconds_then_skips_its_dependent() {
    let rig = Rig::new().await;
    let up = FullUp::new(
        &rig,
        "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: slow
services:
  api:
    provider: testing/fake
    version: \"0.1\"
    dependsOn:
      - {service: db}
  db:
    provider: testing/fake
    version: \"0.1\"
",
    );
    let names = up.names();
    script_start(&rig, &names["db"], Outcome::Ok(Duration::from_secs(700)));
    let began = tokio::time::Instant::now();

    let operation = up.begin(&rig).await;
    finished(&rig, operation).await;

    let reports = up.reports(&rig);
    assert_eq!(reports["api"].phase(), UpPhase::Skipped);
    assert_eq!(reports["api"].reason(), Some(SkipReason::Timeout));
    assert_eq!(
        reports["db"].phase(),
        UpPhase::Ready,
        "the dependency went on"
    );
    assert!(began.elapsed() >= Duration::from_secs(700));
    let member = rig
        .environment(up.environment)
        .expect("accept registered it")
        .member(&alias("api"))
        .expect("the member exists")
        .clone();
    assert_eq!(member.dependency_conditions()[0].timeout_seconds(), 600);
    assert!(
        !rig.table.log().contains(&format!("start {}", names["api"])),
        "{:?}",
        rig.table.log()
    );
}

// covers: AC-11 (spec 0046), cancel in the install stage
#[tokio::test]
async fn a_cancel_in_the_install_stage_cancels_the_install_and_creates_and_starts_nothing() {
    let rig = Rig::new().await;
    let environment = EnvironmentId::generate();
    let sha = digest(0x41);
    rig.scripts
        .installs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(sha, Outcome::Ok(Duration::from_secs(30)));
    let run = Run::begin(
        &rig,
        rig.children(),
        FakeReads::new(&rig),
        vec![
            Service::new(
                alias("a"),
                None,
                Some(create_job(environment, "a", Some(sha))),
            ),
            Service::new(alias("b"), None, Some(create_job(environment, "b", None))),
        ],
        graph(&["a", "b"], &[("b", "a", 600)]),
    )
    .await;
    wait_for(&rig, &format!("install {sha}")).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    rig.engine
        .cancel(run.operation, OperationTrigger::background())
        .await
        .expect("the cancel lands");
    let (ending, reports) = run.end(&rig).await;

    assert_eq!(ending, Ending::Cancelled);
    assert_eq!(reports["a"].phase(), UpPhase::Cancelled);
    assert_eq!(reports["b"].phase(), UpPhase::Cancelled);
    let log = rig.table.log();
    assert!(
        log.iter()
            .all(|line| !line.starts_with("create ") && !line.starts_with("start ")),
        "{log:?}"
    );
    let installs = rig
        .engine
        .list(OperationQuery {
            active_only: false,
            resource: None,
            limit: 500,
            before: None,
        })
        .await
        .expect("the engine lists")
        .items
        .into_iter()
        .filter(|view| view.operation.kind() == OperationKind::InstallRuntime)
        .collect::<Vec<_>>();
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].operation.state(), OperationState::Cancelled);
}

async fn wait_for(rig: &Rig, line: &str) {
    for _ in 0..500 {
        if rig.table.log().iter().any(|entry| entry == line) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{line} never came: {:?}", rig.table.log());
}

/// A whole Up through `up_work` on the rig, as `up.rs` runs it.
struct FullUp {
    reads: Arc<FakeReads>,
    path: PathBuf,
    environment: EnvironmentId,
    sha256: Sha256Digest,
}

impl FullUp {
    fn new(rig: &Rig, manifest: &str) -> Self {
        let reads = FakeReads::new(rig);
        let path = rig.root.path().join("project").join("stackly.yaml");
        reads
            .files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone(), manifest.as_bytes().to_vec());
        Self {
            reads,
            path,
            environment: EnvironmentId::generate(),
            sha256: stackly_core::environment::manifest_digest(manifest.as_bytes()),
        }
    }

    fn request(&self) -> PlanRequest {
        PlanRequest {
            path: self.path.clone(),
            registered: None,
            locked: false,
        }
    }

    fn names(&self) -> BTreeMap<String, String> {
        self.reads
            .plan_now(&self.request())
            .plan
            .items
            .iter()
            .map(|item| (item.alias.to_string(), item.instance_name.to_string()))
            .collect()
    }

    async fn begin(&self, rig: &Rig) -> OperationId {
        let planned = self.reads.plan_now(&self.request());
        assert!(planned.plan.applicable, "{:?}", planned.plan.blockers);
        let name = planned
            .validation
            .name
            .clone()
            .expect("the test fixture holds");
        let work = up_work(
            UpRequest {
                environment: self.environment,
                name: name.clone(),
                plan: self.request(),
                manifest_sha256: self.sha256,
                caller: SecretCaller::background(),
                home: stackly_core::diagnostics::HomeMask::default(),
            },
            DriverDeps {
                engine: rig.engine.clone(),
                children: rig.children(),
                reads: Arc::clone(&self.reads) as Arc<dyn UpReads>,
                board: Arc::clone(&rig.board),
            },
        );
        let request = BeginRequest {
            kind: OperationKind::EnvironmentUp,
            resource: OperationResource::Environment(self.environment),
            creates_resource: true,
            extra_locks: vec![LockKey::EnvironmentName(name)],
            busy: BusyPolicy::Reject,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work,
            precheck: None,
        };
        match rig.engine.begin(request).await {
            Ok(BeginOutcome::Created(view)) => view.operation.id(),
            other => panic!("expected a created up, got {other:?}"),
        }
    }

    fn reports(&self, rig: &Rig) -> BTreeMap<String, ServiceReport> {
        rig.environment(self.environment)
            .and_then(|environment| environment.last_up().cloned())
            .expect("the closing commit wrote last_up")
            .services()
            .iter()
            .map(|report| (report.alias().to_string(), report.clone()))
            .collect()
    }
}

// covers: AC-11, AC-13 (spec 0046), cancel in the create stage
#[tokio::test]
async fn a_cancel_in_the_create_stage_starts_nothing_and_keeps_the_members_it_made() {
    let rig = Rig::new().await;
    let up = FullUp::new(&rig, GRAPH);
    let names = up.names();
    for name in names.values() {
        rig.scripts
            .creates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.clone(), Outcome::Ok(Duration::from_millis(400)));
    }

    let operation = up.begin(&rig).await;
    // The creates run one after the other in manifest order; cancel while
    // the second one runs.
    wait_for(&rig, &format!("create {}", names["api"])).await;
    rig.engine
        .cancel(operation, OperationTrigger::background())
        .await
        .expect("the cancel lands");
    let done = finished(&rig, operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Cancelled);
    let environment = rig
        .environment(up.environment)
        .expect("accept registered it");
    assert_eq!(
        environment
            .last_up()
            .expect("the closing commit ran")
            .state(),
        UpState::Cancelled
    );
    let reports = up.reports(&rig);
    for (alias, report) in &reports {
        assert_eq!(report.phase(), UpPhase::Cancelled, "{alias}");
    }
    let log = rig.table.log();
    assert!(
        log.iter().all(|line| !line.starts_with("start ")),
        "{log:?}"
    );
    assert!(
        !log.contains(&format!("create {}", names["db"])),
        "no create began after the cancel: {log:?}"
    );
    // The member the first create made keeps its lease.
    let web = environment
        .member(&alias("web"))
        .expect("the first create bound its member");
    let leases = rig
        .registry
        .leases(web.instance_id(), stackly_domain::LeaseFilter::Unreleased)
        .expect("the registry reads")
        .value;
    assert_eq!(leases.len(), 1);
}
