//! Coordinators and child operations (spec 0046 AC-3, AC-15, AC-24; spec
//! 0007 AC-5, AC-10, *Operasi pengatur dan operasi anak*): the coordinator
//! slot class, `Origin::Child` with its parent and the quiesce gate,
//! `wait_final`, and what `recover` does with the children of an
//! interrupted coordinator.

use std::sync::{Arc, Mutex};

use stackly_core::operation::testing::{HoldHandle, ScriptedFactory, ScriptedWork};
use stackly_core::operation::{
    BeginError, BeginOutcome, BeginRequest, BoxFuture, ChildOrigin, EngineError, EngineLimits,
    OperationContext, OperationEngine, OperationOutcome, OperationWork, Origin, StepSpec,
    WorkOutcome, WorkResult,
};
use stackly_domain::{
    EnvironmentId, OperationId, OperationKind, OperationResource, OperationState, OperationTrigger,
    Registry, RuntimeId, WriteBatch, WriteOp,
};
use stackly_testkit::EnvironmentBuilder;

use crate::common::{Harness, fast, instance, request, user, wait_state, wait_until};

/// Stores an environment with no members and returns its ID.
fn environment(harness: &Harness) -> EnvironmentId {
    let builder = EnvironmentBuilder::new().no_members();
    let id = builder.id();
    harness
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            builder.build(),
        )]))
        .expect("an environment can be stored");
    id
}

/// An environment up on a new environment, running `work`.
fn up(harness: &Harness, work: Box<dyn OperationWork>) -> BeginRequest {
    request(
        OperationKind::EnvironmentUp,
        OperationResource::Environment(environment(harness)),
        work,
    )
}

/// The request of a child `kind` on `resource` of the coordinator
/// `parent`, started with `trigger`.
fn child(
    kind: OperationKind,
    resource: OperationResource,
    parent: OperationId,
    trigger: OperationTrigger,
    work: Box<dyn OperationWork>,
) -> BeginRequest {
    let mut begin = request(kind, resource, work);
    begin.origin = Origin::Child(ChildOrigin { parent, trigger });
    begin
}

/// A coordinator that starts one child `start_instance` on its instance,
/// waits for it with `wait_final`, and records the child and the state it
/// ended in.
struct StartsOneChild {
    engine: OperationEngine,
    target: stackly_domain::InstanceId,
    seen: Arc<Mutex<Option<(OperationId, OperationState)>>>,
}

impl OperationWork for StartsOneChild {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("run".parse().expect("a stage name")).irreversible())
                .await?;
            let begin = child(
                OperationKind::StartInstance,
                OperationResource::Instance(self.target),
                ctx.id(),
                ctx.trigger().clone(),
                ScriptedWork::new().step("spawn").boxed(),
            );
            let started = match self.engine.begin(begin).await {
                Ok(BeginOutcome::Created(view)) => view.operation.id(),
                other => panic!("expected a created child, got {other:?}"),
            };
            let done = self
                .engine
                .wait_final(started)
                .await
                .expect("the engine runs")
                .expect("the child exists");
            *self.seen.lock().expect("the slot is not poisoned") =
                Some((started, done.view.operation.state()));
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

// covers: AC-3, AC-24 (spec 0046); AC-5 (spec 0007)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_coordinators_with_two_running_slots_each_wait_for_a_child_without_deadlock() {
    let harness = Harness::with_limits(fast().with_slots(2, 64)).await;
    let mut started = Vec::new();
    for _ in 0..3 {
        let seen = Arc::new(Mutex::new(None));
        let id = harness
            .begin(up(
                &harness,
                Box::new(StartsOneChild {
                    engine: harness.engine.clone(),
                    target: instance(&harness),
                    seen: Arc::clone(&seen),
                }),
            ))
            .await;
        started.push((id, seen));
    }

    for (id, seen) in started {
        let parent = wait_state(&harness.engine, id, OperationState::Succeeded).await;
        let (child_id, child_state) = seen
            .lock()
            .expect("the slot is not poisoned")
            .expect("the coordinator saw its child end");
        assert_eq!(child_state, OperationState::Succeeded);
        let child = harness.engine.get(child_id).await.unwrap().unwrap();
        assert_eq!(child.view.journal.parent(), Some(id));
        assert_eq!(child.view.journal.trigger(), parent.view.journal.trigger());
    }
    let stats = harness.engine.stats();
    assert_eq!(
        (stats.coordinators, stats.max_coordinators),
        (0, EngineLimits::default().max_coordinators)
    );
}

// covers: AC-3 (spec 0046); AC-5 (spec 0007)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fifth_coordinator_queues_while_four_run_and_never_takes_a_running_slot() {
    let harness = Harness::with_limits(fast().with_slots(1, 64)).await;
    let holds: Vec<HoldHandle> = (0..5).map(|_| HoldHandle::new()).collect();
    let mut ids = Vec::new();
    for hold in &holds {
        ids.push(
            harness
                .begin(up(
                    &harness,
                    ScriptedWork::new().step("run").hold(hold).boxed(),
                ))
                .await,
        );
    }
    for hold in &holds[..4] {
        hold.started().await;
    }
    let stats = harness.engine.stats();
    assert_eq!(
        (stats.coordinators, stats.max_coordinators, stats.running),
        (4, 4, 0)
    );
    let fifth = harness.engine.get(ids[4]).await.unwrap().unwrap();
    assert_eq!(fifth.view.operation.state(), OperationState::Queued);

    let ordinary = harness
        .begin(request(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("spawn").boxed(),
        ))
        .await;
    wait_state(&harness.engine, ordinary, OperationState::Succeeded).await;

    holds[0].release();
    holds[4].started().await;
    for hold in &holds[1..] {
        hold.release();
    }
    for id in ids {
        wait_state(&harness.engine, id, OperationState::Succeeded).await;
    }
}

// covers: AC-24 (spec 0046)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_child_records_its_parent_and_the_parents_trigger_and_is_refused_while_quiesced() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let mut parent_request = up(
        &harness,
        ScriptedWork::new().step("run").hold(&hold).boxed(),
    );
    parent_request.origin = user("env-up-1", 7);
    let parent = harness.begin(parent_request).await;
    hold.started().await;
    let parent_view = harness.engine.get(parent).await.unwrap().unwrap().view;
    let trigger = parent_view.journal.trigger().clone();

    let first = harness
        .begin(child(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            parent,
            trigger.clone(),
            ScriptedWork::new().step("spawn").boxed(),
        ))
        .await;
    let done = wait_state(&harness.engine, first, OperationState::Succeeded).await;
    assert_eq!(done.view.journal.parent(), Some(parent));
    assert_eq!(done.view.journal.trigger(), &trigger);
    assert_eq!(done.view.journal.method(), None);
    assert_eq!(done.view.journal.fingerprint(), None);
    assert_eq!(done.view.operation.idempotency_key(), None);
    assert_eq!(parent_view.journal.parent(), None);

    harness.engine.set_quiesce(true);
    let refused = harness
        .engine
        .begin(child(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            parent,
            trigger,
            ScriptedWork::new().step("spawn").boxed(),
        ))
        .await;
    assert!(
        matches!(refused, Err(BeginError::RestartPending)),
        "{refused:?}"
    );
    let background = harness
        .engine
        .begin(request(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("spawn").boxed(),
        ))
        .await;
    assert!(matches!(background, Ok(BeginOutcome::Created(_))));
    harness.engine.set_quiesce(false);
    hold.release();
    wait_state(&harness.engine, parent, OperationState::Succeeded).await;
}

// covers: AC-24 (spec 0046)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wait_final_returns_at_a_final_state_and_at_needs_recovery() {
    let harness = Harness::new().await;
    assert_eq!(
        harness
            .engine
            .wait_final(OperationId::generate())
            .await
            .unwrap(),
        None
    );

    let hold = HoldHandle::new();
    let held = harness
        .begin(request(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("spawn").hold(&hold).boxed(),
        ))
        .await;
    hold.started().await;
    let engine = harness.engine.clone();
    let waiting = tokio::spawn(async move { engine.wait_final(held).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!waiting.is_finished(), "wait_final returned before the end");
    hold.release();
    let done = waiting.await.unwrap().unwrap().unwrap();
    assert_eq!(done.view.operation.state(), OperationState::Succeeded);

    let mut stuck = request(
        OperationKind::InstallRuntime,
        OperationResource::Runtime(RuntimeId::generate()),
        ScriptedWork::new()
            .step("fetch")
            .irreversible()
            .step("verify")
            .fail("provider.invalid_settings", false)
            .boxed(),
    );
    stuck.creates_resource = true;
    let stuck = harness.begin(stuck).await;
    let recovery = harness.engine.wait_final(stuck).await.unwrap().unwrap();
    assert_eq!(
        recovery.view.operation.state(),
        OperationState::NeedsRecovery
    );
    let failed = harness
        .begin(request(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new()
                .step("spawn")
                .fail("provider.invalid_settings", false)
                .boxed(),
        ))
        .await;
    let failed = harness.engine.wait_final(failed).await.unwrap().unwrap();
    assert_eq!(failed.view.operation.state(), OperationState::Failed);
}

// covers: AC-24 (spec 0046)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wait_final_ends_with_shutting_down_when_the_engine_stops() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let held = harness
        .begin(request(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("spawn").hold(&hold).boxed(),
        ))
        .await;
    hold.started().await;
    let engine = harness.engine.clone();
    let waiting = tokio::spawn(async move { engine.wait_final(held).await });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    harness
        .engine
        .shutdown(std::time::Duration::from_millis(50))
        .await;
    assert_eq!(
        waiting.await.unwrap().map(|detail| detail.is_some()),
        Err(EngineError::ShuttingDown)
    );
    hold.release();
}

// covers: AC-15, AC-24 (spec 0046); AC-10 (spec 0007)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recover_closes_an_interrupted_coordinator_and_its_queued_create_and_start_but_rebuilds_its_install()
 {
    let harness = Harness::with_limits(fast().with_slots(1, 64)).await;
    let coordinator_hold = HoldHandle::new();
    let coordinator = harness
        .begin(up(
            &harness,
            ScriptedWork::new()
                .step("run")
                .irreversible()
                .hold(&coordinator_hold)
                .boxed(),
        ))
        .await;
    coordinator_hold.started().await;
    let slot_hold = HoldHandle::new();
    let blocker = harness
        .begin(request(
            OperationKind::StopInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("stop").hold(&slot_hold).boxed(),
        ))
        .await;
    slot_hold.started().await;
    let trigger = OperationTrigger::background();
    let create = harness
        .begin(child(
            OperationKind::CreateInstance,
            OperationResource::Instance(instance(&harness)),
            coordinator,
            trigger.clone(),
            ScriptedWork::new().step("register").boxed(),
        ))
        .await;
    let start = harness
        .begin(child(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            coordinator,
            trigger.clone(),
            ScriptedWork::new().step("spawn").boxed(),
        ))
        .await;
    let install = harness
        .begin(child(
            OperationKind::InstallRuntime,
            OperationResource::Manager,
            coordinator,
            trigger,
            ScriptedWork::new().step("fetch").boxed(),
        ))
        .await;
    for queued in [create, start, install] {
        let detail = harness.engine.get(queued).await.unwrap().unwrap();
        assert_eq!(detail.view.operation.state(), OperationState::Queued);
    }

    let (engine, _) = harness.restarted(fast());
    for kind in [OperationKind::CreateInstance, OperationKind::InstallRuntime] {
        engine.register_factory(
            kind,
            Arc::new(ScriptedFactory::always(ScriptedWork::new().step("again"))),
        );
    }
    let report = engine.recover().await.unwrap();
    assert_eq!((report.queued, report.rebuild_failed), (1, 0));

    for closed in [coordinator, blocker, create, start] {
        let detail = engine.get(closed).await.unwrap().unwrap();
        assert_eq!(
            detail.view.operation.state(),
            OperationState::Failed,
            "{closed}"
        );
        assert_eq!(
            detail
                .view
                .operation
                .failure()
                .map(|f| (f.code().as_str(), f.retryable())),
            Some(("interrupted", true)),
            "{closed}"
        );
        assert_eq!(
            detail
                .view
                .journal
                .failure_error()
                .map(|e| e.code().as_str()),
            Some("operation.interrupted"),
            "{closed}"
        );
    }
    engine.start();
    wait_until(&engine, install, |detail| {
        detail.view.operation.state() == OperationState::Succeeded
            && detail.view.journal.parent() == Some(coordinator)
    })
    .await;
    assert!(
        harness
            .registry
            .operation(coordinator)
            .unwrap()
            .value
            .is_some()
    );
}

// covers: AC-15 (spec 0046)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_queued_coordinator_closes_as_interrupted_with_its_queued_child() {
    let harness = Harness::with_limits(fast().with_slots(1, 64).with_coordinators(1)).await;
    let first_hold = HoldHandle::new();
    let first = harness
        .begin(up(
            &harness,
            ScriptedWork::new().step("run").hold(&first_hold).boxed(),
        ))
        .await;
    first_hold.started().await;
    let queued = harness
        .begin(up(&harness, ScriptedWork::new().step("run").boxed()))
        .await;
    let slot_hold = HoldHandle::new();
    harness
        .begin(request(
            OperationKind::StopInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("stop").hold(&slot_hold).boxed(),
        ))
        .await;
    slot_hold.started().await;
    let orphan = harness
        .begin(child(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            queued,
            OperationTrigger::background(),
            ScriptedWork::new().step("spawn").boxed(),
        ))
        .await;
    assert_eq!(
        harness
            .engine
            .get(queued)
            .await
            .unwrap()
            .unwrap()
            .view
            .operation
            .state(),
        OperationState::Queued
    );

    let (engine, _) = harness.restarted(fast());
    engine.recover().await.unwrap();
    for closed in [first, queued, orphan] {
        let detail = engine.get(closed).await.unwrap().unwrap();
        assert_eq!(
            detail
                .view
                .operation
                .failure()
                .map(|f| (detail.view.operation.state(), f.code().as_str())),
            Some((OperationState::Failed, "interrupted")),
            "{closed}"
        );
    }
}

// covers: AC-10, AC-25 (spec 0047); AC-10 (spec 0007)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_interrupted_environment_down_closes_with_its_queued_child_stop_and_no_rebuild() {
    let harness = Harness::with_limits(fast().with_slots(1, 64)).await;
    let down_hold = HoldHandle::new();
    let down = harness
        .begin(request(
            OperationKind::EnvironmentDown,
            OperationResource::Environment(environment(&harness)),
            ScriptedWork::new()
                .step("release")
                .irreversible()
                .step("stop")
                .irreversible()
                .hold(&down_hold)
                .boxed(),
        ))
        .await;
    down_hold.started().await;
    let slot_hold = HoldHandle::new();
    let blocker = harness
        .begin(request(
            OperationKind::StopInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("stop").hold(&slot_hold).boxed(),
        ))
        .await;
    slot_hold.started().await;
    let stop = harness
        .begin(child(
            OperationKind::StopInstance,
            OperationResource::Instance(instance(&harness)),
            down,
            OperationTrigger::background(),
            ScriptedWork::new().step("stop").boxed(),
        ))
        .await;
    assert_eq!(
        harness
            .engine
            .get(stop)
            .await
            .unwrap()
            .unwrap()
            .view
            .operation
            .state(),
        OperationState::Queued
    );

    let (engine, _) = harness.restarted(fast());
    let report = engine.recover().await.unwrap();
    // `stop_instance` has no work factory, so the existing rule closes the
    // queued child as a rebuild that failed; no branch is new for Down.
    assert_eq!((report.queued, report.rebuild_failed), (0, 1));
    for closed in [down, blocker, stop] {
        let detail = engine.get(closed).await.unwrap().unwrap();
        assert_eq!(
            detail
                .view
                .operation
                .failure()
                .map(|f| (detail.view.operation.state(), f.code().as_str())),
            Some((OperationState::Failed, "interrupted")),
            "{closed}"
        );
        assert_eq!(detail.view.operation.recovery_hint(), None, "{closed}");
    }
    assert_eq!(
        engine
            .get(stop)
            .await
            .unwrap()
            .unwrap()
            .view
            .journal
            .parent(),
        Some(down)
    );
}

// covers: AC-3, AC-25 (spec 0047)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_environment_down_runs_in_the_coordinator_slot_class_beside_a_busy_running_slot() {
    let harness = Harness::with_limits(fast().with_slots(1, 64)).await;
    let slot_hold = HoldHandle::new();
    harness
        .begin(request(
            OperationKind::StopInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("stop").hold(&slot_hold).boxed(),
        ))
        .await;
    slot_hold.started().await;

    let down = harness
        .begin(request(
            OperationKind::EnvironmentDown,
            OperationResource::Environment(environment(&harness)),
            ScriptedWork::new().step("release").irreversible().boxed(),
        ))
        .await;

    wait_state(&harness.engine, down, OperationState::Succeeded).await;
    slot_hold.release();
}
