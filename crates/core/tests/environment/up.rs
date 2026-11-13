//! Up (spec 0046 AC-4, AC-6, AC-8 to AC-11) on a real engine with fake
//! ports: the `accept` commit, the graph, partial failure, cancel, and the
//! closing commit. Every service runs the `fake` provider; no edge stands
//! for a real dependency between databases.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use stackly_core::environment::children::ChildOrigin;
use stackly_core::environment::up::{
    CreateJob, DriverDeps, Graph, PlanRequest, Service, UpBoard, UpRequest, up_work,
};
use stackly_core::environment::{draft_for, planned_runtime_placeholder_id};
use stackly_core::instance::create::EnvironmentBinding;
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BoxFuture, BusyPolicy, OperationContext, OperationOutcome,
    OperationWork, Origin, StepSpec, WorkOutcome, WorkResult,
};
use stackly_core::secret::SecretCaller;
use stackly_domain::{
    EnvironmentId, InstanceLifecycle, JournalPayload, LeaseFilter, LeaseHolder, LeaseState,
    LockKey, MemberAlias, OperationId, OperationKind, OperationResource, OperationState,
    OperationTrigger, Ownership, Registry, ServiceReport, Sha256Digest, SkipReason, UpPhase,
    UpStage, UpState,
};
use stackly_platform::Clock;

use crate::up_fake::{FakeReads, Outcome, Rig};

const GRAPH: &str = include_str!("../fixtures/environments/up-graph.yaml");

fn alias(text: &str) -> MemberAlias {
    text.parse().expect("the test fixture holds")
}

/// One Up on the rig.
struct Up {
    reads: Arc<FakeReads>,
    path: PathBuf,
    environment: EnvironmentId,
    sha256: Sha256Digest,
}

impl Up {
    fn new(rig: &Rig, manifest: &str) -> Self {
        let reads = FakeReads::new(rig);
        let path = rig.root.path().join("project").join("stackly.yaml");
        reads
            .files
            .lock()
            .expect("the test fixture holds")
            .insert(path.clone(), manifest.as_bytes().to_vec());
        let sha256 = stackly_core::environment::manifest_digest(manifest.as_bytes());
        Self {
            reads,
            path,
            environment: EnvironmentId::generate(),
            sha256,
        }
    }

    fn request(&self) -> PlanRequest {
        PlanRequest {
            path: self.path.clone(),
            registered: None,
            locked: false,
        }
    }

    /// The instance name of each service, from the plan.
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
        let deps = DriverDeps {
            engine: rig.engine.clone(),
            children: rig.children(),
            reads: Arc::clone(&self.reads) as Arc<dyn stackly_core::environment::up::UpReads>,
            board: Arc::clone(&rig.board),
        };
        let existing = rig
            .registry
            .environments()
            .expect("the test fixture holds")
            .value
            .into_iter()
            .find(|environment| environment.name() == &name);
        let environment = existing.as_ref().map_or(self.environment, |env| env.id());
        let work = up_work(
            UpRequest {
                environment,
                name: name.clone(),
                plan: self.request(),
                manifest_sha256: self.sha256,
                caller: SecretCaller::background(),
                home: stackly_core::diagnostics::HomeMask::default(),
            },
            deps,
        );
        let request = BeginRequest {
            kind: OperationKind::EnvironmentUp,
            resource: OperationResource::Environment(environment),
            creates_resource: existing.is_none(),
            extra_locks: vec![LockKey::EnvironmentName(name)],
            busy: BusyPolicy::Reject,
            expected_revision: existing.map(|env| env.revision()),
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
}

async fn finished(
    rig: &Rig,
    operation: OperationId,
) -> stackly_core::operation::OperationDetailView {
    tokio::time::timeout(Duration::from_secs(30), rig.engine.wait_final(operation))
        .await
        .expect("the up ends in time")
        .expect("the engine runs")
        .expect("the up exists")
}

fn phases(rig: &Rig, environment: EnvironmentId) -> BTreeMap<String, ServiceReport> {
    rig.environment(environment)
        .and_then(|environment| environment.last_up().cloned())
        .expect("the closing commit wrote last_up")
        .services()
        .iter()
        .map(|report| (report.alias().to_string(), report.clone()))
        .collect()
}

fn position(log: &[String], line: &str) -> usize {
    log.iter()
        .position(|entry| entry == line)
        .unwrap_or_else(|| panic!("{line} is in {log:?}"))
}

#[tokio::test]
async fn up_registers_creates_leases_and_starts_every_service_in_dependency_order() {
    let rig = Rig::new().await;
    let up = Up::new(&rig, GRAPH);
    let names = up.names();

    let operation = up.begin(&rig).await;
    let done = finished(&rig, operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Succeeded);
    let environment = rig
        .environment(up.environment)
        .expect("accept registered it");
    let last = environment.last_up().expect("last_up is written");
    assert_eq!(last.state(), UpState::Succeeded);
    assert_eq!(environment.last_operation(), Some(operation));
    assert_eq!(environment.members().len(), 4);
    for member in environment.members() {
        assert_eq!(member.ownership(), Ownership::Dedicated);
        let leases = rig
            .registry
            .leases(member.instance_id(), LeaseFilter::Unreleased)
            .expect("the test fixture holds")
            .value;
        assert_eq!(
            leases
                .iter()
                .filter(
                    |lease| lease.holder() == LeaseHolder::Environment(up.environment)
                        && lease.state() == LeaseState::Active
                )
                .count(),
            1,
            "{}",
            member.alias()
        );
    }
    let api = environment
        .member(&alias("api"))
        .expect("the test fixture holds");
    assert_eq!(api.dependency_conditions().len(), 2);
    assert_eq!(api.dependency_conditions()[0].timeout_seconds(), 600);

    let log = rig.table.log();
    let start = |service: &str| position(&log, &format!("start {}", names[service]));
    // Every create comes before the first start (rule 1).
    let last_create = log
        .iter()
        .rposition(|line| line.starts_with("create "))
        .expect("the test fixture holds");
    let first_start = log
        .iter()
        .position(|line| line.starts_with("start "))
        .expect("the test fixture holds");
    assert!(last_create < first_start, "{log:?}");
    assert!(
        start("db") < start("api") && start("cache") < start("api"),
        "{log:?}"
    );
    assert!(start("api") < start("web"), "{log:?}");
    assert!(
        rig.board.get(up.environment).is_none(),
        "the board entry is gone"
    );
}

#[tokio::test]
async fn a_dependency_that_fails_skips_its_dependents_and_the_up_ends_incomplete() {
    let rig = Rig::new().await;
    let up = Up::new(&rig, GRAPH);
    let names = up.names();
    rig.scripts
        .starts
        .lock()
        .expect("the test fixture holds")
        .insert(
            names["db"].clone(),
            Outcome::Fail(Duration::from_millis(10), "supervisor.readiness_timeout"),
        );

    let operation = up.begin(&rig).await;
    let done = finished(&rig, operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Failed);
    let error = done
        .view
        .journal
        .failure_error()
        .expect("the test fixture holds");
    assert_eq!(error.code().as_str(), "env.up_incomplete");
    assert!(
        error
            .details()
            .iter()
            .any(|(key, value)| key.as_str() == "ready" && value == "1")
    );
    assert_eq!(
        done.view.operation.stage().map(|stage| stage.to_string()),
        Some("start".to_owned())
    );
    let reports = phases(&rig, up.environment);
    assert_eq!(reports["db"].phase(), UpPhase::Failed);
    assert_eq!(reports["db"].stage(), Some(UpStage::Start));
    assert_eq!(
        reports["db"].code().map(ToString::to_string).as_deref(),
        Some("supervisor.readiness_timeout")
    );
    assert_eq!(reports["cache"].phase(), UpPhase::Ready);
    assert_eq!(reports["api"].phase(), UpPhase::Skipped);
    assert_eq!(reports["api"].dependency(), Some(&alias("db")));
    assert_eq!(reports["api"].reason(), Some(SkipReason::Failed));
    assert_eq!(reports["web"].phase(), UpPhase::Skipped);
    assert_eq!(reports["web"].dependency(), Some(&alias("api")));
    // A skipped service names no operation, not even the create that made
    // its instance: none of its own failed.
    assert_eq!(reports["api"].operation(), None);
    assert_eq!(reports["web"].operation(), None);
    let log = rig.table.log();
    assert!(!log.contains(&format!("start {}", names["api"])), "{log:?}");
    assert!(!log.contains(&format!("start {}", names["web"])), "{log:?}");
    // The cache that is ready keeps running, with its lease.
    let cache = rig
        .environment(up.environment)
        .expect("the test fixture holds")
        .member(&alias("cache"))
        .expect("the test fixture holds")
        .instance_id();
    assert!(
        rig.table
            .get(cache)
            .expect("the test fixture holds")
            .running_and_ready()
    );
}

#[tokio::test]
async fn a_second_up_starts_only_what_is_not_ready_and_writes_no_second_lease() {
    let rig = Rig::new().await;
    let up = Up::new(&rig, GRAPH);
    let names = up.names();
    rig.scripts
        .starts
        .lock()
        .expect("the test fixture holds")
        .insert(
            names["db"].clone(),
            Outcome::Fail(Duration::from_millis(10), "supervisor.readiness_timeout"),
        );
    let first = up.begin(&rig).await;
    finished(&rig, first).await;
    rig.scripts
        .starts
        .lock()
        .expect("the test fixture holds")
        .clear();
    rig.table
        .log
        .lock()
        .expect("the test fixture holds")
        .clear();

    let second = up.begin(&rig).await;
    let done = finished(&rig, second).await;

    assert_eq!(done.view.operation.state(), OperationState::Succeeded);
    let log = rig.table.log();
    assert!(
        log.iter().all(|line| !line.starts_with("create ")),
        "{log:?}"
    );
    assert!(
        !log.contains(&format!("start {}", names["cache"])),
        "{log:?}"
    );
    let environment = rig
        .environment(up.environment)
        .expect("the test fixture holds");
    for member in environment.members() {
        let active = rig
            .registry
            .leases(member.instance_id(), LeaseFilter::Unreleased)
            .expect("the test fixture holds")
            .value
            .into_iter()
            .filter(|lease| lease.state() == LeaseState::Active)
            .count();
        assert_eq!(active, 1, "{}", member.alias());
    }
}

#[tokio::test]
async fn a_dependency_slower_than_the_edge_timeout_skips_its_dependent_and_still_ends_ready() {
    let rig = Rig::new().await;
    let up = Up::new(
        &rig,
        &GRAPH.replace("timeoutSeconds: 30", "timeoutSeconds: 1"),
    );
    let names = up.names();
    rig.scripts
        .starts
        .lock()
        .expect("the test fixture holds")
        .insert(
            names["api"].clone(),
            Outcome::Ok(Duration::from_millis(2_500)),
        );

    let operation = up.begin(&rig).await;
    finished(&rig, operation).await;

    let reports = phases(&rig, up.environment);
    assert_eq!(reports["api"].phase(), UpPhase::Ready);
    assert_eq!(reports["web"].phase(), UpPhase::Skipped);
    assert_eq!(reports["web"].reason(), Some(SkipReason::Timeout));
    assert!(!rig.table.log().contains(&format!("start {}", names["web"])));
}

#[tokio::test]
async fn a_cancelled_up_cancels_its_child_and_records_the_rest_cancelled() {
    let rig = Rig::new().await;
    let up = Up::new(&rig, GRAPH);
    let names = up.names();
    rig.scripts
        .starts
        .lock()
        .expect("the test fixture holds")
        .insert(names["db"].clone(), Outcome::Ok(Duration::from_secs(20)));

    let operation = up.begin(&rig).await;
    // Wait until the slow start runs.
    let db_start = format!("start {}", names["db"]);
    for _ in 0..200 {
        if rig.table.log().contains(&db_start) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    rig.engine
        .cancel(operation, OperationTrigger::background())
        .await
        .expect("the cancel lands");
    let done = finished(&rig, operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Cancelled);
    let environment = rig
        .environment(up.environment)
        .expect("the test fixture holds");
    assert_eq!(
        environment
            .last_up()
            .expect("the test fixture holds")
            .state(),
        UpState::Cancelled
    );
    let reports = phases(&rig, up.environment);
    assert_eq!(reports["db"].phase(), UpPhase::Cancelled);
    assert!(reports["db"].operation().is_some(), "the child is recorded");
    assert_eq!(reports["web"].phase(), UpPhase::Cancelled);
    // The members and leases stay.
    assert_eq!(environment.members().len(), 4);
}

#[tokio::test]
async fn a_borrowed_running_instance_is_ready_without_a_child_and_gets_one_shared_lease() {
    let rig = Rig::new().await;
    let shared = rig.instance("shared-cache", InstanceLifecycle::Running);
    let manifest = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: borrow
services:
  api:
    provider: testing/fake
    version: \"0.1\"
    dependsOn:
      - {service: cache}
  cache:
    provider: testing/fake
    instanceRef: shared-cache
";
    let up = Up::new(&rig, manifest);
    let before = rig
        .registry
        .instance(shared.id())
        .expect("the test fixture holds")
        .value;

    let first = up.begin(&rig).await;
    assert_eq!(
        finished(&rig, first).await.view.operation.state(),
        OperationState::Succeeded
    );
    let second = up.begin(&rig).await;
    finished(&rig, second).await;

    assert!(
        !rig.table.log().contains(&"start shared-cache".to_owned()),
        "{:?}",
        rig.table.log()
    );
    let environment = rig
        .environment(up.environment)
        .expect("the test fixture holds");
    let member = environment
        .member(&alias("cache"))
        .expect("the test fixture holds");
    assert_eq!(member.ownership(), Ownership::Shared);
    assert_eq!(member.instance_id(), shared.id());
    let leases = rig
        .registry
        .leases(shared.id(), LeaseFilter::Unreleased)
        .expect("the test fixture holds")
        .value;
    assert_eq!(leases.len(), 1);
    assert_eq!(
        rig.registry
            .instance(shared.id())
            .expect("the test fixture holds")
            .value,
        before,
        "Up never changes a borrowed instance"
    );
}

#[tokio::test]
async fn accept_fails_with_plan_changed_and_writes_nothing_when_a_member_instance_is_gone() {
    let rig = Rig::new().await;
    let shared = rig.instance("shared-cache", InstanceLifecycle::Stopped);
    let manifest = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: gone
services:
  cache:
    provider: testing/fake
    instanceRef: shared-cache
";
    let up = Up::new(&rig, manifest);
    // The plan `accept` reads still saw it; the registry retained it since.
    let planned = up.reads.plan_now(&up.request());
    assert!(planned.plan.applicable);
    *up.reads.frozen.lock().expect("the test fixture holds") = Some(planned);
    let mut retained = rig
        .registry
        .instance(shared.id())
        .expect("the test fixture holds")
        .value
        .expect("the test fixture holds");
    let expected = retained.revision();
    retained
        .apply_lifecycle(
            stackly_domain::LifecycleEvent::Retain {
                by: OperationId::generate(),
            },
            rig.fake.clock().now(),
        )
        .expect("the test fixture holds");
    rig.registry
        .commit(stackly_domain::WriteBatch::new(vec![
            stackly_domain::WriteOp::UpdateInstance {
                instance: retained,
                expected,
            },
        ]))
        .expect("the instance is retained");
    let operation = up.begin(&rig).await;

    let done = finished(&rig, operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Failed);
    assert_eq!(
        done.view
            .journal
            .failure_error()
            .expect("the test fixture holds")
            .code()
            .as_str(),
        "env.plan_changed"
    );
    assert!(
        rig.environment(up.environment).is_none(),
        "accept wrote nothing"
    );
}

#[tokio::test]
async fn a_create_that_needs_a_secret_ends_needs_secret_with_its_reference() {
    let rig = Rig::new().await;
    let up = Up::new(&rig, GRAPH);
    let names = up.names();
    rig.scripts
        .creates
        .lock()
        .expect("the test fixture holds")
        .insert(
            names["cache"].clone(),
            Outcome::NeedsSecret("local:graph/cache-token"),
        );

    let operation = up.begin(&rig).await;
    let done = finished(&rig, operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Failed);
    let reports = phases(&rig, up.environment);
    assert_eq!(reports["cache"].phase(), UpPhase::NeedsSecret);
    assert_eq!(reports["cache"].stage(), Some(UpStage::Create));
    assert_eq!(
        reports["cache"]
            .references()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["local:graph/cache-token"]
    );
    assert_eq!(reports["db"].phase(), UpPhase::Ready);
    assert_eq!(reports["api"].phase(), UpPhase::Skipped);
    // The create that failed left no member.
    let environment = rig
        .environment(up.environment)
        .expect("the test fixture holds");
    assert!(environment.member(&alias("cache")).is_none());
}

#[tokio::test]
async fn a_child_refused_by_the_restart_quiesce_stops_new_children_and_cancels_the_rest() {
    let rig = Rig::new().await;
    let up = Up::new(&rig, GRAPH);
    let names = up.names();
    rig.scripts
        .starts
        .lock()
        .expect("the test fixture holds")
        .insert(
            names["api"].clone(),
            Outcome::Refuse("daemon.restart_pending"),
        );

    let operation = up.begin(&rig).await;
    let done = finished(&rig, operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Failed);
    assert_eq!(
        done.view
            .journal
            .failure_error()
            .expect("the test fixture holds")
            .code()
            .as_str(),
        "env.up_incomplete"
    );
    let reports = phases(&rig, up.environment);
    assert_eq!(reports["api"].phase(), UpPhase::Cancelled);
    assert_eq!(reports["web"].phase(), UpPhase::Cancelled);
    assert!(!rig.table.log().contains(&format!("start {}", names["web"])));
}

/// A work that runs the driver alone on hand made services.
struct DriverWork {
    deps: DriverDeps,
    environment: EnvironmentId,
    services: Vec<Service>,
    graph: Graph,
    seen: Arc<std::sync::Mutex<Vec<ServiceReport>>>,
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
            driver.run().await?;
            *self.seen.lock().expect("the test fixture holds") = driver.reports();
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test]
async fn two_services_of_one_artifact_install_once_and_a_failed_install_ends_both_in_install() {
    let rig = Rig::new().await;
    let environment = EnvironmentId::generate();
    let shared: Sha256Digest = "ab".repeat(32).parse().expect("the test fixture holds");
    let alone: Sha256Digest = "cd".repeat(32).parse().expect("the test fixture holds");
    rig.scripts
        .installs
        .lock()
        .expect("the test fixture holds")
        .insert(
            shared,
            Outcome::Fail(Duration::from_millis(10), "runtime.download_failed"),
        );
    let fake_spec = |name: &str| {
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
    };
    let job = |name: &str, artifact: Sha256Digest| {
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
            install: Some((spec.provider.clone(), artifact)),
        }
    };
    let services = vec![
        Service::new(alias("a"), None, Some(job("a", shared))),
        Service::new(alias("b"), None, Some(job("b", shared))),
        Service::new(alias("c"), None, Some(job("c", alone))),
    ];
    let graph = Graph::new(vec![alias("a"), alias("b"), alias("c")], BTreeMap::new());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let reads = FakeReads::new(&rig);
    let work = DriverWork {
        deps: DriverDeps {
            engine: rig.engine.clone(),
            children: rig.children(),
            reads: reads as Arc<dyn stackly_core::environment::up::UpReads>,
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
        .expect("the test fixture holds")
    else {
        panic!("the driver work begins");
    };
    finished(&rig, view.operation.id()).await;

    let log = rig.table.log();
    assert_eq!(
        log.iter()
            .filter(|line| line.starts_with("install "))
            .count(),
        2,
        "{log:?}"
    );
    let reports = seen.lock().expect("the test fixture holds").clone();
    assert_eq!(reports[0].phase(), UpPhase::Failed);
    assert_eq!(reports[0].stage(), Some(UpStage::Install));
    assert_eq!(reports[1].phase(), UpPhase::Failed);
    assert_eq!(reports[1].stage(), Some(UpStage::Install));
    // The other artifact installed; its service was created and started.
    assert_eq!(reports[2].phase(), UpPhase::Ready);
}

#[tokio::test]
async fn the_precheck_refuses_an_invalid_manifest_a_changed_digest_and_missing_references() {
    use stackly_core::environment::up::{UpError, check};
    use stackly_core::operation::OperationError;
    use stackly_domain::{NeedsSecretItem, NeedsSecretReason};

    let rig = Rig::new().await;
    let up = Up::new(&rig, GRAPH);
    let mut planned = up.reads.plan_now(&up.request());
    let digest = planned
        .plan
        .digest
        .expect("an applicable plan has a digest");
    assert_eq!(check(&planned, digest), Ok(()));

    let other: Sha256Digest = "00".repeat(32).parse().expect("the test fixture holds");
    assert_eq!(check(&planned, other), Err(UpError::PlanChanged));

    planned.create_needs = vec![
        NeedsSecretItem::new(
            "local:graph/z".parse().expect("the test fixture holds"),
            NeedsSecretReason::Missing,
            true,
        ),
        NeedsSecretItem::new(
            "local:graph/a".parse().expect("the test fixture holds"),
            NeedsSecretReason::Missing,
            true,
        ),
    ];
    let refused = check(&planned, digest).unwrap_err();
    assert_eq!(refused.code().as_str(), "secret.needs_secret");
    let references: Vec<String> = refused
        .items()
        .iter()
        .map(|item| item.fields()[0].1.clone())
        .collect();
    assert_eq!(references, ["local:graph/a", "local:graph/z"]);

    let invalid = Up::new(&rig, "apiVersion: stackly/v1alpha1\nkind: Environment\n");
    let planned = invalid.reads.plan_now(&invalid.request());
    assert_eq!(
        check(&planned, digest).unwrap_err().code().as_str(),
        "env.manifest_invalid"
    );
}
