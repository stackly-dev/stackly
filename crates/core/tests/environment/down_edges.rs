//! Edges of Down (spec 0047 AC-6, AC-8) on a real engine with fake child
//! ports, next to `down`: no more than `DOWN_STOP_CONCURRENCY` stops run at
//! once and a shared dependency stops after every dependent; a dependent
//! that keeps running, at the preview or in its guard, holds nothing back;
//! and an instance the preview shows as blocked is never stopped and never
//! fails the Down.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_core::environment::children::ChildOrigin;
use stackly_core::environment::down::{
    DownChild, DownChildren, DownDeps, DownRequest, down_work, read_view, select,
};
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BoxFuture, BusyPolicy, OperationContext, OperationDetailView,
    OperationOutcome, OperationWork, Origin, PartialItem, PartialOutcome, StepSpec, WorkOutcome,
    WorkResult,
};
use stackly_core::secret::SecretCaller;
use stackly_core::supervisor::{ENVIRONMENT_DOWN_INPUT, INSTANCE_IN_USE};
use stackly_domain::{
    DesiredState, EnvironmentId, FailureError, Instance, InstanceId, InstanceLifecycle,
    JournalPayload, LockKey, OperationId, OperationKind, OperationResource, OperationState,
    Ownership, Registry, ResourceRef, WriteBatch, WriteOp,
};
use stackly_platform::Clock;
use stackly_testkit::{EnvironmentBuilder, InstanceBuilder, LeaseBuilder};

use crate::up_fake::Rig;

/// The cap of spec 0047 *Urutan* (`DOWN_STOP_CONCURRENCY`).
const CAP: usize = 4;

type Job = Box<dyn FnOnce(OperationContext) -> BoxFuture<'static, WorkResult> + Send>;

struct FnWork(Job);

impl OperationWork for FnWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        (self.0)(ctx)
    }
}

/// What the fake child port saw: a log, and how many stops ran at once.
#[derive(Default)]
struct Seen {
    log: Vec<String>,
    running: usize,
    most: usize,
}

/// The child port: real `stop_instance` operations that run for `delay`,
/// log `begin` and `end`, and count how many run at once. An instance named
/// in `keeps` ends kept, as if its guard found the manual lease the stop
/// commits.
struct CountingStops {
    engine: stackly_core::operation::OperationEngine,
    registry: Arc<stackly_testkit::FakeRegistry>,
    names: HashMap<InstanceId, String>,
    keeps: Vec<String>,
    delay: Duration,
    seen: Arc<Mutex<Seen>>,
}

impl CountingStops {
    fn log(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .log
            .clone()
    }

    fn most(&self) -> usize {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .most
    }
}

impl DownChildren for CountingStops {
    fn stop(
        &self,
        parent: &ChildOrigin,
        instance: InstanceId,
        _environment: EnvironmentId,
    ) -> BoxFuture<'_, Result<DownChild, FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            let name = self.names.get(&instance).cloned().unwrap_or_default();
            let kept = self.keeps.contains(&name);
            let seen = Arc::clone(&self.seen);
            let registry = Arc::clone(&self.registry);
            let delay = self.delay;
            let job: Job = Box::new(move |mut ctx: OperationContext| {
                Box::pin(async move {
                    ctx.begin_step(StepSpec::new("stop".parse().expect("a stage")))
                        .await?;
                    {
                        let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
                        seen.log.push(format!("begin {name}"));
                        seen.running += 1;
                        seen.most = seen.most.max(seen.running);
                    }
                    tokio::time::sleep(delay).await;
                    {
                        let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
                        seen.log.push(format!("end {name}"));
                        seen.running -= 1;
                    }
                    if kept {
                        // The lease the guard found: the final sweep keeps
                        // seeing it.
                        registry
                            .commit(WriteBatch::new(vec![WriteOp::InsertLease(
                                LeaseBuilder::new(instance).manual(None).build(),
                            )]))
                            .expect("the lease commits");
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

/// One environment `shop` on the rig whose members are each leased by it.
struct Shop {
    rig: Rig,
    environment: EnvironmentId,
    instances: HashMap<String, Instance>,
}

impl Shop {
    /// Commits `instances` (name, lifecycle) and `shop` with `members`
    /// (alias, dependencies), each member leased by `shop`.
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

    fn stops(&self, keeps: &[&str], delay: Duration) -> Arc<CountingStops> {
        Arc::new(CountingStops {
            engine: self.rig.engine.clone(),
            registry: Arc::clone(&self.rig.registry),
            names: self
                .instances
                .iter()
                .map(|(name, instance)| (instance.id(), name.clone()))
                .collect(),
            keeps: keeps.iter().map(|name| (*name).to_owned()).collect(),
            delay,
            seen: Arc::new(Mutex::new(Seen::default())),
        })
    }

    fn selection(&self) -> stackly_core::environment::down::DownSelection {
        let view = read_view(self.rig.registry.as_ref(), self.environment)
            .expect("the registry reads")
            .expect("the environment exists");
        select(&view)
    }

    async fn down(&self, stops: &Arc<CountingStops>, confirmed: Vec<InstanceId>) -> OperationId {
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
            Duration::from_secs(60),
            self.rig.engine.wait_final(operation),
        )
        .await
        .expect("the down ends in time")
        .expect("the engine runs")
        .expect("the down exists")
    }
}

fn position(log: &[String], line: &str) -> usize {
    log.iter()
        .position(|entry| entry == line)
        .unwrap_or_else(|| panic!("{line} is in {log:?}"))
}

fn outcome(done: &OperationDetailView) -> OperationOutcome {
    done.view
        .journal
        .result()
        .and_then(|payload| OperationOutcome::from_payload(payload).ok())
        .expect("a result")
}

const RUNNING: InstanceLifecycle = InstanceLifecycle::Running;

// covers: AC-6 (spec 0047)
#[tokio::test]
async fn no_more_than_four_stops_run_at_once_and_the_shared_dependency_stops_last() {
    let dependents = ["s1", "s2", "s3", "s4", "s5", "s6"];
    let mut instances = vec![("db", RUNNING)];
    instances.extend(dependents.iter().map(|name| (*name, RUNNING)));
    let mut members: Vec<(&str, &[&str])> = vec![("db", &[])];
    members.extend(dependents.iter().map(|name| (*name, &["db"][..])));
    let shop = Shop::new(&instances, &members).await;
    let stops = shop.stops(&[], Duration::from_millis(150));
    let confirmed = shop.selection().stop_ids();
    assert_eq!(confirmed.len(), 7);

    let operation = shop.down(&stops, confirmed).await;
    let done = shop.finished(operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Succeeded);
    assert_eq!(stops.most(), CAP, "four at once, never five");
    let log = stops.log();
    let db = position(&log, "begin db");
    for name in dependents {
        assert!(
            position(&log, &format!("end {name}")) < db,
            "{name} ends before db begins: {log:?}"
        );
    }
    // Ties are broken by alias: the first four in alias order begin first.
    let mut first: Vec<&str> = log
        .iter()
        .take(CAP)
        .map(|line| line.trim_start_matches("begin "))
        .collect();
    first.sort_unstable();
    assert_eq!(first, ["s1", "s2", "s3", "s4"], "{log:?}");
    assert_eq!(outcome(&done).resources.len(), 7);
}

// covers: AC-6, AC-8 (spec 0047)
#[tokio::test]
async fn a_dependent_that_keeps_running_holds_its_dependency_back_neither_at_the_preview_nor_in_its_guard()
 {
    let shop = Shop::new(
        &[
            ("db", RUNNING),
            ("api", RUNNING),
            ("mine", RUNNING),
            ("web", RUNNING),
        ],
        &[
            ("db", &[]),
            ("api", &["db"]),
            ("mine", &["db"]),
            ("web", &["api"]),
        ],
    )
    .await;
    // You started `mine` yourself: the preview keeps it running.
    shop.rig
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertLease(
            LeaseBuilder::new(shop.id("mine")).manual(None).build(),
        )]))
        .expect("the lease commits");
    let selection = shop.selection();
    assert_eq!(
        selection
            .keep
            .iter()
            .map(|kept| kept.entry.name.as_str())
            .collect::<Vec<_>>(),
        ["mine"]
    );
    // `api` ends kept in its guard, as if a lease landed after the preview.
    let stops = shop.stops(&["api"], Duration::from_millis(30));

    let operation = shop.down(&stops, selection.stop_ids()).await;
    let done = shop.finished(operation).await;

    assert_eq!(done.view.operation.state(), OperationState::Succeeded);
    let log = stops.log();
    assert!(position(&log, "end web") < position(&log, "begin api"));
    assert!(
        position(&log, "end api") < position(&log, "begin db"),
        "the kept dependent ended, so db may stop: {log:?}"
    );
    assert!(
        !log.iter().any(|line| line.ends_with(" mine")),
        "mine is no child: {log:?}"
    );
    let result = outcome(&done);
    assert!(
        result.partial.iter().any(
            |item| item.code.as_deref() == Some(INSTANCE_IN_USE.as_str())
                && item.resource == Some(ResourceRef::Instance(shop.id("api")))
        ),
        "{result:?}"
    );
}

// covers: AC-1, AC-8 (spec 0047): *Sapuan akhir* never changes a blocked
// instance
#[tokio::test]
async fn an_instance_the_preview_shows_blocked_is_never_stopped_and_never_fails_the_down() {
    let shop = Shop::new(
        &[("db", InstanceLifecycle::Unknown), ("api", RUNNING)],
        &[("db", &[]), ("api", &["db"])],
    )
    .await;
    let selection = shop.selection();
    assert_eq!(selection.blocked.len(), 1);
    assert_eq!(
        selection.blocked[0].code.as_str(),
        "supervisor.identity_unproven"
    );
    assert_eq!(selection.stop_ids(), [shop.id("api")]);
    let stops = shop.stops(&[], Duration::from_millis(10));

    let operation = shop.down(&stops, selection.stop_ids()).await;
    let done = shop.finished(operation).await;

    assert_eq!(
        done.view.operation.state(),
        OperationState::Succeeded,
        "a blocked instance shown at the preview does not fail the Down"
    );
    assert_eq!(stops.log(), ["begin api", "end api"]);
    assert_eq!(
        outcome(&done).resources,
        [ResourceRef::Instance(shop.id("api"))]
    );
}
