//! Down (spec 0047 AC-1, AC-4 to AC-9) on a real engine with fake child
//! ports: `read_view`, the `release` commit, the reverse dependency order,
//! partial failure, the final sweep, and cancel. The children are real
//! engine operations that only log; the guard of a real stop is tested in
//! `supervisor_start::guarded_stop` and over the socket in `stacklyd`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_core::environment::children::ChildOrigin;
use stackly_core::environment::down::{
    DownChild, DownChildren, DownDeps, DownError, DownRequest, down_work, read_view, release,
    select,
};
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BoxFuture, BusyPolicy, OperationContext, OperationDetailView,
    OperationEngine, OperationError, OperationOutcome, OperationWork, Origin, PartialItem,
    PartialOutcome, StepSpec, WorkError, WorkOutcome, WorkResult,
};
use stackly_core::secret::SecretCaller;
use stackly_core::supervisor::{ENVIRONMENT_DOWN_INPUT, INSTANCE_IN_USE};
use stackly_domain::{
    DesiredState, DetailKey, Environment, EnvironmentId, ErrorCodeName, FailureError, FailureItem,
    GuardDecision, Guarded, Instance, InstanceFilter, InstanceId, InstanceLifecycle,
    JournalPayload, Lease, LeaseFilter, LeaseHolder, LeaseState, LockKey, OperationId,
    OperationKind, OperationResource, OperationState, OperationTrigger, Ownership, Registry,
    ResourceRef, WriteBatch, WriteOp,
};
use stackly_platform::Clock;
use stackly_testkit::{EnvironmentBuilder, InstanceBuilder, LeaseBuilder};

use crate::up_fake::Rig;

/// How a scripted child stop ends.
#[derive(Debug, Clone)]
enum Script {
    /// The stop runs for `delay` and succeeds.
    Stop(Duration),
    /// The stop runs for `delay` and fails with `code`.
    Fail(Duration, &'static str),
    /// The guard of the stop finds another lease: success with the kept
    /// partial item.
    GuardKeeps,
    /// The port refuses with `code`.
    Refuse(&'static str),
}

#[derive(Debug, thiserror::Error)]
#[error("scripted stop failed with {code}")]
struct StopFailed {
    code: ErrorCodeName,
}

impl OperationError for StopFailed {
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
        Vec::new()
    }
}

type Job = Box<dyn FnOnce(OperationContext) -> BoxFuture<'static, WorkResult> + Send>;

struct FnWork(Job);

impl OperationWork for FnWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        (self.0)(ctx)
    }
}

/// The child port: real `stop_instance` operations that log `begin` and
/// `end` and follow a script by instance name.
struct FakeStops {
    engine: OperationEngine,
    registry: Arc<stackly_testkit::FakeRegistry>,
    names: HashMap<InstanceId, String>,
    scripts: Mutex<HashMap<String, Script>>,
    log: Arc<Mutex<Vec<String>>>,
}

impl FakeStops {
    fn log(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn refusal(code: &'static str) -> FailureError {
    FailureError::new(
        ErrorCodeName::from_static(code),
        "the child was refused",
        Vec::new(),
    )
    .expect("the test fixture holds")
}

impl DownChildren for FakeStops {
    fn stop(
        &self,
        parent: &ChildOrigin,
        instance: InstanceId,
        _environment: EnvironmentId,
    ) -> BoxFuture<'_, Result<DownChild, FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            let name = self.names.get(&instance).cloned().unwrap_or_default();
            let script = self
                .scripts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&name)
                .cloned()
                .unwrap_or(Script::Stop(Duration::from_millis(20)));
            if let Script::Refuse(code) = script {
                return Err(refusal(code));
            }
            let log = Arc::clone(&self.log);
            let registry = Arc::clone(&self.registry);
            let job: Job = Box::new(move |mut ctx: OperationContext| {
                Box::pin(async move {
                    ctx.begin_step(StepSpec::new("stop".parse().expect("a stage")))
                        .await?;
                    log.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(format!("begin {name}"));
                    let (delay, failure, kept) = match script {
                        Script::Stop(delay) => (delay, None, false),
                        Script::Fail(delay, code) => (delay, Some(code), false),
                        _ => (Duration::from_millis(5), None, true),
                    };
                    // A running stop is never cancelled by its Down.
                    tokio::time::sleep(delay).await;
                    log.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(format!("end {name}"));
                    if kept {
                        // A lease lands after the driver's check: the
                        // guard of the real stop keeps the instance.
                        registry
                            .commit(WriteBatch::new(vec![WriteOp::InsertLease(
                                LeaseBuilder::new(instance).manual(None).build(),
                            )]))
                            .expect("the lease commits");
                    }
                    if let Some(code) = failure {
                        return Err(WorkError::Failed(Box::new(StopFailed {
                            code: ErrorCodeName::from_static(code),
                        })));
                    }
                    ctx.finish_step(None).await?;
                    let partial = if kept {
                        vec![PartialItem {
                            item: name.clone(),
                            resource: Some(ResourceRef::Instance(instance)),
                            outcome: PartialOutcome::Skipped,
                            code: Some(INSTANCE_IN_USE.to_string()),
                        }]
                    } else {
                        Vec::new()
                    };
                    Ok(WorkOutcome {
                        result: OperationOutcome {
                            resources: Vec::new(),
                            partial,
                        },
                        finalize: None,
                    })
                })
            });
            let request = BeginRequest {
                kind: OperationKind::StopInstance,
                resource: OperationResource::Instance(instance),
                creates_resource: false,
                extra_locks: Vec::new(),
                busy: BusyPolicy::Queue,
                expected_revision: None,
                origin: Origin::Child(parent.operation.clone()),
                input: JournalPayload::new(
                    ENVIRONMENT_DOWN_INPUT.to_owned(),
                    stackly_domain::OperationJournal::INPUT_MAX,
                )
                .expect("the input fits"),
                work: Box::new(FnWork(job)),
                precheck: None,
            };
            match self.engine.begin(request).await {
                Ok(BeginOutcome::Created(view)) => Ok(DownChild::Started {
                    operation: view.operation.id(),
                }),
                other => panic!("a fake stop begins: {other:?}"),
            }
        })
    }
}

/// One environment on the rig, with its instances by name.
struct Site {
    rig: Rig,
    environment: EnvironmentId,
    instances: HashMap<String, Instance>,
}

impl Site {
    /// Commits `instances` (name, lifecycle) and an environment named
    /// `shop` whose members are `members` (alias, dependencies), each
    /// leased by it.
    async fn new(instances: &[(&str, InstanceLifecycle)], members: &[(&str, &[&str])]) -> Self {
        let rig = Rig::new().await;
        let mut by_name = HashMap::new();
        for (name, lifecycle) in instances {
            let instance = InstanceBuilder::new(&rig.runtime, &rig.key)
                .name(name)
                .lifecycle(*lifecycle)
                .desired(if *lifecycle == InstanceLifecycle::Running {
                    DesiredState::Running
                } else {
                    DesiredState::Stopped
                })
                .build();
            rig.registry
                .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
                    instance.clone(),
                )]))
                .expect("the instance commits");
            by_name.insert((*name).to_owned(), instance);
        }
        let mut builder = EnvironmentBuilder::new().name("shop");
        for (alias, on) in members {
            builder = builder.member_on(alias, by_name[*alias].id(), Ownership::Dedicated, on);
        }
        let environment = builder.build();
        let now = rig.fake.clock().now();
        let id = environment.id();
        let expected = environment.revision();
        let mut up = environment.clone();
        up.begin_up(OperationId::generate(), now);
        let mut ops = vec![
            WriteOp::InsertEnvironment(environment),
            WriteOp::UpdateEnvironment {
                environment: up,
                expected,
            },
        ];
        for (alias, _) in members {
            ops.push(WriteOp::InsertLease(
                LeaseBuilder::new(by_name[*alias].id())
                    .environment(id)
                    .build(),
            ));
        }
        rig.registry
            .commit(WriteBatch::new(ops))
            .expect("the environment commits");
        Self {
            rig,
            environment: id,
            instances: by_name,
        }
    }

    fn id(&self, name: &str) -> InstanceId {
        self.instances[name].id()
    }

    fn lease(&self, lease: Lease) {
        self.rig
            .registry
            .commit(WriteBatch::new(vec![WriteOp::InsertLease(lease)]))
            .expect("the lease commits");
    }

    fn stops(&self) -> Arc<FakeStops> {
        Arc::new(FakeStops {
            engine: self.rig.engine.clone(),
            registry: Arc::clone(&self.rig.registry),
            names: self
                .instances
                .iter()
                .map(|(name, instance)| (instance.id(), name.clone()))
                .collect(),
            scripts: Mutex::new(HashMap::new()),
            log: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// The stop set the preview would show now.
    fn preview(&self) -> Vec<InstanceId> {
        let view = read_view(self.rig.registry.as_ref(), self.environment)
            .expect("the registry reads")
            .expect("the environment exists");
        select(&view).stop_ids()
    }

    async fn begin(&self, stops: &Arc<FakeStops>, confirmed: Vec<InstanceId>) -> OperationId {
        let work = down_work(
            DownRequest {
                environment: self.environment,
                name: "shop".parse().expect("a name"),
                stop: confirmed,
                caller: SecretCaller::background(),
            },
            DownDeps {
                engine: self.rig.engine.clone(),
                children: Arc::clone(stops) as Arc<dyn DownChildren>,
            },
        );
        let request = BeginRequest {
            kind: OperationKind::EnvironmentDown,
            resource: OperationResource::Environment(self.environment),
            creates_resource: false,
            extra_locks: vec![LockKey::EnvironmentName("shop".parse().expect("a name"))],
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work,
            precheck: None,
        };
        match self.rig.engine.begin(request).await {
            Ok(BeginOutcome::Created(view)) => view.operation.id(),
            other => panic!("expected a created down, got {other:?}"),
        }
    }

    async fn finished(&self, operation: OperationId) -> OperationDetailView {
        tokio::time::timeout(
            Duration::from_secs(30),
            self.rig.engine.wait_final(operation),
        )
        .await
        .expect("the down ends in time")
        .expect("the engine runs")
        .expect("the down exists")
    }

    fn environment(&self) -> Option<Environment> {
        self.rig.environment(self.environment)
    }

    fn leases(&self, name: &str) -> Vec<Lease> {
        self.rig
            .registry
            .leases(self.id(name), LeaseFilter::All)
            .expect("the registry reads")
            .value
    }

    fn own_active(&self) -> usize {
        self.rig
            .registry
            .leases_of_environment(self.environment, LeaseFilter::Unreleased)
            .expect("the registry reads")
            .value
            .iter()
            .filter(|lease| lease.is_active())
            .count()
    }
}

fn position(log: &[String], line: &str) -> usize {
    log.iter()
        .position(|entry| entry == line)
        .unwrap_or_else(|| panic!("{line} is in {log:?}"))
}

/// The `(name, outcome, code)` of every item of `env.down_incomplete`.
fn incomplete(done: &OperationDetailView) -> Vec<(String, String, Option<String>)> {
    let error = done
        .view
        .journal
        .failure_error()
        .expect("the down failed with an error");
    assert_eq!(error.code().as_str(), "env.down_incomplete");
    error
        .items()
        .iter()
        .map(|item| {
            let field = |key: &str| {
                item.fields()
                    .iter()
                    .find(|(k, _)| k.as_str() == key)
                    .map(|(_, v)| v.clone())
            };
            (
                field("name").expect("a name"),
                field("outcome").expect("an outcome"),
                field("code"),
            )
        })
        .collect()
}

fn outcome(done: &OperationDetailView) -> OperationOutcome {
    done.view
        .journal
        .result()
        .and_then(|payload| OperationOutcome::from_payload(payload).ok())
        .expect("a result")
}

const RUNNING: InstanceLifecycle = InstanceLifecycle::Running;

// covers: AC-1 (spec 0047)
#[tokio::test]
async fn the_view_holds_members_leased_instances_and_the_lease_count() {
    let site = Site::new(
        &[("db", RUNNING), ("api", RUNNING), ("loose", RUNNING)],
        &[("db", &[]), ("api", &["db"])],
    )
    .await;
    // A lease of the environment on an instance that is no member.
    site.lease(
        LeaseBuilder::new(site.id("loose"))
            .environment(site.environment)
            .build(),
    );

    let view = read_view(site.rig.registry.as_ref(), site.environment)
        .expect("the registry reads")
        .expect("the environment exists");

    assert_eq!(view.name.as_str(), "shop");
    assert_eq!(view.lease_count, 3);
    assert!(view.held_since.is_some());
    assert_eq!(view.members.len(), 2);
    let mut names: Vec<&str> = view.instances.iter().map(|i| i.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["api", "db", "loose"]);
    let loose = view
        .instances
        .iter()
        .find(|i| i.name.as_str() == "loose")
        .expect("the leased instance");
    assert!(loose.member.is_none(), "no member row");
    assert_eq!(select(&view).stop.len(), 3);
    assert!(
        read_view(site.rig.registry.as_ref(), EnvironmentId::generate())
            .expect("the registry reads")
            .is_none()
    );
}

// covers: AC-4, AC-7 (spec 0047)
#[tokio::test]
async fn release_lets_go_of_every_lease_of_the_environment_and_nothing_else() {
    let site = Site::new(
        &[("db", RUNNING), ("cache", RUNNING), ("mine", RUNNING)],
        &[("db", &[]), ("cache", &[])],
    )
    .await;
    let other_env = EnvironmentBuilder::new().name("blog").build();
    let other = other_env.id();
    site.rig
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(other_env)]))
        .expect("the other environment commits");
    site.lease(
        LeaseBuilder::new(site.id("cache"))
            .environment(other)
            .build(),
    );
    site.lease(LeaseBuilder::new(site.id("mine")).manual(None).build());
    let before = site.environment().expect("the environment");
    let instances = site
        .rig
        .registry
        .instances(InstanceFilter {
            include_retained: true,
        })
        .expect("reads")
        .value
        .len();
    let stops = site.stops();

    let confirmed = site.preview();
    assert_eq!(
        confirmed,
        [site.id("db")],
        "cache is kept, mine is no member"
    );
    let operation = site.begin(&stops, confirmed).await;
    let done = site.finished(operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Succeeded);
    assert_eq!(
        site.own_active(),
        0,
        "every lease of the environment is released"
    );
    assert!(
        site.leases("cache")
            .iter()
            .any(|l| l.holder() == LeaseHolder::Environment(other) && l.is_active()),
        "another environment's lease stays"
    );
    assert!(
        site.leases("mine")
            .iter()
            .any(|l| l.holder() == LeaseHolder::Manual && l.is_active())
    );
    let after = site.environment().expect("the environment stays");
    assert!(after.last_up().is_none(), "last_up is cleared");
    assert!(before.last_up().is_some());
    assert_eq!(after.last_operation(), Some(operation));
    assert_eq!(
        after.members(),
        before.members(),
        "members are never written"
    );
    assert_eq!(after.revision(), before.revision().next());
    assert_eq!(
        site.rig
            .registry
            .instances(InstanceFilter {
                include_retained: true,
            })
            .expect("reads")
            .value
            .len(),
        instances
    );
    assert_eq!(
        stops.log(),
        ["begin db", "end db"],
        "only the confirmed set"
    );
    let result = outcome(&done);
    assert_eq!(result.resources, [ResourceRef::Instance(site.id("db"))]);
}

// covers: AC-6 (spec 0047)
#[tokio::test]
async fn instances_stop_in_reverse_dependency_order_and_a_failed_dependent_does_not_hold() {
    let site = Site::new(
        &[
            ("db", RUNNING),
            ("api", RUNNING),
            ("jobs", RUNNING),
            ("web", RUNNING),
        ],
        &[
            ("db", &[]),
            ("api", &["db"]),
            ("jobs", &["db"]),
            ("web", &["api", "jobs"]),
        ],
    )
    .await;
    let stops = site.stops();
    stops
        .scripts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(
            "jobs".to_owned(),
            Script::Fail(Duration::from_millis(30), "supervisor.stop_timeout"),
        );

    let operation = site.begin(&stops, site.preview()).await;
    let done = site.finished(operation).await;

    let log = stops.log();
    assert!(position(&log, "end web") < position(&log, "begin api"));
    assert!(position(&log, "end web") < position(&log, "begin jobs"));
    assert!(position(&log, "end api") < position(&log, "begin db"));
    assert!(
        position(&log, "end jobs") < position(&log, "begin db"),
        "db waits for the failed jobs to end, then stops"
    );
    assert_eq!(done.view.operation.state(), OperationState::Failed);
    assert_eq!(
        incomplete(&done),
        [(
            "jobs".to_owned(),
            "failed".to_owned(),
            Some("supervisor.stop_timeout".to_owned())
        )]
    );
    assert_eq!(site.own_active(), 0, "the leases stay released");
}

// covers: AC-5, AC-8 (spec 0047)
#[tokio::test]
async fn a_stop_its_guard_keeps_ends_kept_and_down_succeeds_with_a_skipped_item() {
    let site = Site::new(
        &[("db", RUNNING), ("api", RUNNING)],
        &[("db", &[]), ("api", &[])],
    )
    .await;
    let stops = site.stops();
    stops
        .scripts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert("api".to_owned(), Script::GuardKeeps);

    let operation = site.begin(&stops, site.preview()).await;
    let done = site.finished(operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Succeeded);
    let result = outcome(&done);
    assert_eq!(result.resources, [ResourceRef::Instance(site.id("db"))]);
    assert_eq!(result.partial.len(), 1);
    assert_eq!(result.partial[0].item, "api");
    assert_eq!(result.partial[0].outcome, PartialOutcome::Skipped);
    assert_eq!(
        result.partial[0].code.as_deref(),
        Some("env.instance_in_use")
    );
}

// covers: AC-8 (spec 0047)
#[tokio::test]
async fn an_instance_outside_the_confirmed_set_with_no_demand_is_left_running() {
    let site = Site::new(
        &[("db", RUNNING), ("api", RUNNING)],
        &[("db", &[]), ("api", &[])],
    )
    .await;
    let stops = site.stops();

    // The client confirmed only db; api runs with no demand after release.
    let operation = site.begin(&stops, vec![site.id("db")]).await;
    let done = site.finished(operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Failed);
    assert_eq!(
        incomplete(&done),
        [(
            "api".to_owned(),
            "left_running".to_owned(),
            Some("env.down_changed".to_owned())
        )]
    );
    assert_eq!(stops.log(), ["begin db", "end db"], "Down never stops api");
}

// covers: AC-8 (spec 0047)
#[tokio::test]
async fn a_kept_instance_whose_other_demand_goes_away_is_left_running() {
    let site = Site::new(
        &[("db", RUNNING), ("api", RUNNING)],
        &[("db", &[]), ("api", &[])],
    )
    .await;
    let manual = LeaseBuilder::new(site.id("db")).manual(None).build();
    site.lease(manual.clone());
    let stops = site.stops();
    stops
        .scripts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert("api".to_owned(), Script::Stop(Duration::from_millis(400)));
    // The preview keeps db; the test confirms db too, so db gets its turn
    // and the driver keeps it.
    assert_eq!(site.preview(), [site.id("api")]);
    let operation = site
        .begin(&stops, vec![site.id("api"), site.id("db")])
        .await;

    // While api stops, the demand that kept db goes away.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !stops.log().contains(&"begin api".to_owned()) {
        assert!(tokio::time::Instant::now() < deadline, "api begins");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut released = manual;
    released
        .release(site.rig.fake.clock().now())
        .expect("an active lease releases");
    site.rig
        .registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateLease(released)]))
        .expect("the release commits");
    let done = site.finished(operation).await;

    assert_eq!(
        incomplete(&done),
        [(
            "db".to_owned(),
            "left_running".to_owned(),
            Some("env.down_changed".to_owned())
        )],
        "kept at its turn, without demand at the sweep"
    );
    assert_eq!(stops.log(), ["begin api", "end api"], "db is never stopped");
}

// covers: AC-8 (spec 0047)
#[tokio::test]
async fn a_refused_child_fails_with_its_code_and_an_idle_one_is_already_stopped() {
    let site = Site::new(
        &[
            ("db", RUNNING),
            ("api", RUNNING),
            ("old", InstanceLifecycle::Stopped),
        ],
        &[("db", &[]), ("api", &[]), ("old", &[])],
    )
    .await;
    let stops = site.stops();
    stops
        .scripts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(
            "api".to_owned(),
            Script::Refuse("supervisor.identity_unproven"),
        );

    let operation = site
        .begin(&stops, vec![site.id("db"), site.id("api"), site.id("old")])
        .await;
    let done = site.finished(operation).await;

    assert_eq!(
        incomplete(&done),
        [(
            "api".to_owned(),
            "failed".to_owned(),
            Some("supervisor.identity_unproven".to_owned())
        )],
        "old is already stopped, db stopped"
    );
    let details: Vec<(String, String)> = done
        .view
        .journal
        .failure_error()
        .expect("an error")
        .details()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.clone()))
        .collect();
    assert!(details.contains(&("stopped".to_owned(), "1".to_owned())));
    assert!(details.contains(&("total".to_owned(), "3".to_owned())));
}

// covers: AC-2, AC-4 (spec 0047)
#[tokio::test]
async fn a_down_with_nothing_to_stop_succeeds_and_one_of_a_gone_environment_fails() {
    let site = Site::new(&[("old", InstanceLifecycle::Stopped)], &[("old", &[])]).await;
    let stops = site.stops();
    assert_eq!(site.preview(), Vec::<InstanceId>::new());

    let operation = site.begin(&stops, Vec::new()).await;
    let done = site.finished(operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Succeeded);
    assert_eq!(site.own_active(), 0);
    assert!(stops.log().is_empty());

    // The environment is gone when the release guard reads it.
    let refused = site
        .rig
        .registry
        .commit_guarded(Box::new(|view| {
            Ok(
                match release(
                    view,
                    EnvironmentId::generate(),
                    OperationId::generate(),
                    jiff::Timestamp::now(),
                )? {
                    Ok(release) => GuardDecision::Write(release.ops),
                    Err(error) => GuardDecision::Refuse(Box::new(error)),
                },
            )
        }))
        .expect("the guard runs");
    let Guarded::Refused(reason) = refused else {
        panic!("a gone environment writes nothing");
    };
    let error = reason.downcast::<DownError>().expect("a down error");
    assert_eq!(error.code().as_str(), "env.not_found");
}

// covers: AC-9 (spec 0047)
#[tokio::test]
async fn a_cancel_after_release_waits_for_the_running_stop_and_cancels_the_rest() {
    let site = Site::new(
        &[("db", RUNNING), ("api", RUNNING)],
        &[("db", &[]), ("api", &["db"])],
    )
    .await;
    let stops = site.stops();
    stops
        .scripts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert("api".to_owned(), Script::Stop(Duration::from_millis(600)));

    let operation = site.begin(&stops, site.preview()).await;
    // Wait until api's stop runs, then cancel the Down.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !stops.log().contains(&"begin api".to_owned()) {
        assert!(tokio::time::Instant::now() < deadline, "api begins");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    site.rig
        .engine
        .cancel(operation, OperationTrigger::background())
        .await
        .expect("the cancel lands");
    let done = site.finished(operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Cancelled);
    assert_eq!(
        stops.log(),
        ["begin api", "end api"],
        "the running stop ends, db never begins"
    );
    assert_eq!(site.own_active(), 0, "released leases stay released");
}

// covers: AC-9 (spec 0047)
#[tokio::test]
async fn a_cancel_while_queued_releases_no_lease() {
    let site = Site::new(&[("db", RUNNING)], &[("db", &[])]).await;
    // Another operation holds the environment, so the Down queues.
    let (release, held) = tokio::sync::oneshot::channel::<()>();
    let holder: Job = Box::new(move |_ctx| {
        Box::pin(async move {
            let _ = held.await;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    });
    let blocker = match site
        .rig
        .engine
        .begin(BeginRequest {
            kind: OperationKind::ImportEnvironment,
            resource: OperationResource::Environment(site.environment),
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Reject,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work: Box::new(FnWork(holder)),
            precheck: None,
        })
        .await
    {
        Ok(BeginOutcome::Created(view)) => view.operation.id(),
        other => panic!("the holder begins: {other:?}"),
    };
    let stops = site.stops();

    let operation = site.begin(&stops, site.preview()).await;
    site.rig
        .engine
        .cancel(operation, OperationTrigger::background())
        .await
        .expect("the cancel lands");
    let done = site.finished(operation).await;
    let _ = release.send(());
    let _ = site.finished(blocker).await;

    assert_eq!(done.view.operation.state(), OperationState::Cancelled);
    assert_eq!(site.own_active(), 1, "no lease was released");
    assert!(stops.log().is_empty());
    assert!(
        site.leases("db")
            .iter()
            .all(|lease| lease.state() == LeaseState::Active)
    );
}
