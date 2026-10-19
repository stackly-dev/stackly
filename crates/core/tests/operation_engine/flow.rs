//! Begin, idempotency, lock keys, the queue, progress, and stale revisions
//! (spec 0007 AC-1 to AC-6, AC-12, AC-13, AC-15).

use std::time::{Duration, Instant};

use stackly_core::operation::testing::{EffectCounter, HoldHandle, ScriptedWork};
use stackly_core::operation::{
    BeginError, BeginOutcome, BoxFuture, BusyPolicy, LockHolder, OperationContext, OperationWork,
    StepSpec, WorkOutcome, WorkResult,
};
use stackly_domain::{
    EnvironmentId, LockKey, OperationKind, OperationResource, OperationState, Progress, Registry,
    ResourceRevision, RetentionPolicy, StepState, WriteBatch, WriteOp,
};
use stackly_platform::Clock;
use stackly_testkit::EnvironmentBuilder;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, manager_request, request, user, wait_state, wait_until};

fn revision(number: u64) -> ResourceRevision {
    ResourceRevision::new(number).expect("a test revision is at least 1")
}

/// Stores an environment and returns it with its builder.
fn environment(harness: &Harness) -> (EnvironmentId, EnvironmentBuilder) {
    let builder = EnvironmentBuilder::new().no_members();
    let id = builder.id();
    harness
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            builder.clone().build(),
        )]))
        .expect("an environment can be stored");
    (id, builder)
}

fn on_environment(id: EnvironmentId, work: ScriptedWork) -> stackly_core::operation::BeginRequest {
    request(
        OperationKind::EnvironmentDown,
        OperationResource::Environment(id),
        work.boxed(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_begun_operation_runs_its_steps_and_reports_every_change_in_revision_order() {
    let harness = Harness::new().await;
    let work = ScriptedWork::new()
        .step("download")
        .progress(1, 3)
        .step("extract")
        .progress(2, 3)
        .step("commit")
        .progress(3, 3);

    let id = harness
        .begin(manager_request(OperationKind::UpdateManager, work.boxed()))
        .await;
    let done = wait_state(&harness.engine, id, OperationState::Succeeded).await;

    let steps: Vec<(&str, StepState)> = done
        .steps
        .iter()
        .map(|step| (step.name().as_str(), step.state()))
        .collect();
    assert_eq!(
        steps,
        [
            ("download", StepState::Done),
            ("extract", StepState::Done),
            ("commit", StepState::Done)
        ]
    );
    assert!(done.view.journal.result().is_some());
    let events = harness.events.of(id);
    assert_eq!(
        (events[0].kind, events[0].state),
        ("state_changed", OperationState::Queued)
    );
    assert!(
        events
            .iter()
            .any(|event| event.state == OperationState::Running)
    );
    let last = events.last().expect("the operation reported events");
    assert_eq!(
        (last.kind, last.state),
        ("state_changed", OperationState::Succeeded)
    );
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].revision < pair[1].revision),
        "revisions rise with every event: {events:?}"
    );
    assert_eq!(last.revision, done.view.operation.revision());
    let stored = harness
        .registry
        .operation_journal(id)
        .unwrap()
        .value
        .unwrap();
    assert!(
        stored.locks.is_empty(),
        "the final commit deletes the lock rows"
    );
    assert_eq!(harness.engine.locks().holder(&LockKey::Manager), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sixteen_begins_with_one_key_create_one_operation_and_run_its_work_once() {
    let ev = evidence!(
        covers = ["REQ-048", "AC-048", "NFR-005"],
        kind = Kind::Fixture,
        expected = "16 concurrent begins with one idempotency key and one fingerprint give one Created and 15 Replayed with the same operation ID, the work runs exactly once, a replay after the final state runs nothing, the key with another request is refused, and the key is free again after prune",
    );
    let harness = Harness::new().await;
    let effects = EffectCounter::new();
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let engine = harness.engine.clone();
        let mut begin = manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").effects(&effects).boxed(),
        );
        begin.origin = user("gui-click-1", 0xaa);
        tasks.push(tokio::spawn(async move { engine.begin(begin).await }));
    }
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await.unwrap().unwrap());
    }

    let created = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, BeginOutcome::Created(_)))
        .count();
    let id = outcomes[0]
        .view()
        .expect("every begin names the operation")
        .operation
        .id();
    assert_eq!(created, 1);
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.view().is_some_and(|view| view.operation.id() == id))
    );
    wait_state(&harness.engine, id, OperationState::Succeeded).await;
    assert_eq!(effects.get(), 1, "the work ran exactly once");

    let mut again = manager_request(
        OperationKind::UpdateManager,
        ScriptedWork::new().step("swap").effects(&effects).boxed(),
    );
    again.origin = user("gui-click-1", 0xaa);
    let replayed = harness.engine.begin(again).await.unwrap();
    assert!(matches!(&replayed, BeginOutcome::Replayed(view)
        if view.operation.id() == id && view.operation.state() == OperationState::Succeeded));
    let mut other = manager_request(
        OperationKind::UpdateManager,
        ScriptedWork::new().step("swap").boxed(),
    );
    other.origin = user("gui-click-1", 0xbb);
    assert_eq!(
        harness.engine.begin(other).await,
        Err(BeginError::KeyReused { existing: id })
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(effects.get(), 1, "a replay runs nothing");

    harness.tick();
    let policy = RetentionPolicy {
        operation_max_age: Duration::ZERO,
        ..RetentionPolicy::DEFAULT
    };
    harness
        .registry
        .prune(&policy, harness.fake.clock().now())
        .unwrap();
    let mut after_prune = manager_request(
        OperationKind::UpdateManager,
        ScriptedWork::new().step("swap").boxed(),
    );
    after_prune.origin = user("gui-click-1", 0xaa);
    let fresh = harness.engine.begin(after_prune).await.unwrap();
    assert!(matches!(&fresh, BeginOutcome::Created(view) if view.operation.id() != id));
    ev.pass();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn operations_sharing_a_key_wait_in_order_and_others_run_in_parallel() {
    let harness = Harness::new().await;
    let (x, _) = environment(&harness);
    let (y, _) = environment(&harness);
    let (hold_a, hold_c) = (HoldHandle::new(), HoldHandle::new());
    let a = harness
        .begin(on_environment(
            x,
            ScriptedWork::new().step("drain").hold(&hold_a),
        ))
        .await;
    hold_a.started().await;
    let b = harness
        .begin(on_environment(x, ScriptedWork::new().step("drain")))
        .await;
    let c = harness
        .begin(on_environment(
            y,
            ScriptedWork::new().step("drain").hold(&hold_c),
        ))
        .await;
    hold_c.started().await;

    let waiting = wait_until(&harness.engine, b, |detail| {
        detail.view.journal.waiting_on() == Some(a)
    })
    .await;
    assert_eq!(waiting.view.operation.state(), OperationState::Queued);
    assert_eq!(
        harness.engine.locks().holder(&LockKey::Environment(x)),
        Some(LockHolder::Operation {
            id: a,
            state: OperationState::Running
        })
    );
    let mut reject = on_environment(x, ScriptedWork::new().step("drain"));
    reject.busy = BusyPolicy::Reject;
    assert!(matches!(
        harness.engine.begin(reject).await,
        Err(BeginError::Busy { holder: LockHolder::Operation { id, .. } }) if id == a
    ));
    assert!(
        harness
            .engine
            .locks()
            .try_lock_internal(&[LockKey::Environment(x)], "lifecycle_write")
            .is_err()
    );

    hold_a.release();
    let first = wait_state(&harness.engine, a, OperationState::Succeeded).await;
    let second = wait_state(&harness.engine, b, OperationState::Succeeded).await;
    assert!(second.view.operation.started_at() >= first.view.operation.updated_at().into());
    assert_eq!(
        harness
            .engine
            .get(c)
            .await
            .unwrap()
            .unwrap()
            .view
            .operation
            .state(),
        OperationState::Running,
        "an operation on another key ran in parallel"
    );
    hold_c.release();
    wait_state(&harness.engine, c, OperationState::Succeeded).await;

    let guard = harness
        .engine
        .locks()
        .try_lock_internal(&[LockKey::Environment(x)], "lifecycle_write")
        .unwrap();
    let held_back = harness
        .begin(on_environment(x, ScriptedWork::new().step("drain")))
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        harness
            .engine
            .get(held_back)
            .await
            .unwrap()
            .unwrap()
            .view
            .operation
            .state(),
        OperationState::Queued,
        "a queued operation never gets a key an internal lock holds"
    );
    drop(guard);
    wait_state(&harness.engine, held_back, OperationState::Succeeded).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_queued_waiter_fails_when_its_holder_enters_needs_recovery() {
    let harness = Harness::new().await;
    let (x, _) = environment(&harness);
    let verifying = HoldHandle::new();
    let holder = harness
        .begin(on_environment(
            x,
            ScriptedWork::new()
                .step("swap")
                .irreversible()
                .step("verify")
                .hold(&verifying)
                .fail("environment.verify_failed", false),
        ))
        .await;
    verifying.started().await;
    let waiter = harness
        .begin(on_environment(x, ScriptedWork::new().step("drain")))
        .await;
    wait_until(&harness.engine, waiter, |detail| {
        detail.view.journal.waiting_on() == Some(holder)
    })
    .await;

    verifying.release();
    let closed = wait_state(&harness.engine, waiter, OperationState::Failed).await;

    assert_eq!(
        closed
            .view
            .operation
            .failure()
            .map(|f| (f.code().as_str(), f.retryable())),
        Some(("precondition_failed", false))
    );
    let error = closed.view.journal.failure_error().unwrap();
    assert_eq!(error.code().as_str(), "operation.recovery_required");
    assert!(
        error
            .details()
            .iter()
            .any(|(key, value)| key.as_str() == "operationId" && *value == holder.to_string())
    );
    assert_eq!(closed.view.journal.waiting_on(), None);
    assert!(closed.steps.is_empty(), "the waiter never ran");
    let stuck = harness.engine.get(holder).await.unwrap().unwrap();
    assert_eq!(stuck.view.operation.state(), OperationState::NeedsRecovery);
    assert!(matches!(
        harness.engine.locks().holder(&LockKey::Environment(x)),
        Some(LockHolder::Operation { id, state: OperationState::NeedsRecovery }) if id == holder
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn at_most_the_running_limit_runs_and_a_full_queue_refuses() {
    let harness = Harness::with_limits(crate::common::fast().with_slots(2, 3)).await;
    let holds: Vec<HoldHandle> = (0..5).map(|_| HoldHandle::new()).collect();
    let mut ids = Vec::new();
    for hold in &holds {
        let (env, _) = environment(&harness);
        ids.push(
            harness
                .begin(on_environment(
                    env,
                    ScriptedWork::new().step("drain").hold(hold),
                ))
                .await,
        );
    }
    holds[0].started().await;
    holds[1].started().await;
    let (extra, _) = environment(&harness);

    let refused = harness
        .engine
        .begin(on_environment(extra, ScriptedWork::new().step("drain")))
        .await;

    assert_eq!(refused, Err(BeginError::QueueFull { max: 3 }));
    let stats = harness.engine.stats();
    assert_eq!(
        (stats.running, stats.queued, stats.rejected_queue_full),
        (2, 3, 1)
    );
    assert_eq!((stats.max_running, stats.max_queued), (2, 3));
    for hold in &holds {
        hold.release();
    }
    for id in ids {
        wait_state(&harness.engine, id, OperationState::Succeeded).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_task_that_goes_away_never_leaves_a_stuck_or_cancelled_operation() {
    let harness = Harness::new().await;
    let effects = EffectCounter::new();
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let engine = harness.engine.clone();
        let begin = manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").effects(&effects).boxed(),
        );
        tasks.push(tokio::spawn(async move { engine.begin(begin).await }));
    }
    for task in &tasks {
        task.abort();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    let page = harness
        .engine
        .list(stackly_domain::OperationQuery {
            active_only: false,
            resource: None,
            limit: 50,
            before: None,
        })
        .await
        .unwrap();
    for view in &page.items {
        let done = wait_state(
            &harness.engine,
            view.operation.id(),
            OperationState::Succeeded,
        )
        .await;
        assert_eq!(done.view.journal.cancel_requested_at(), None);
    }
    assert_eq!(
        effects.get(),
        u64::try_from(page.items.len()).unwrap(),
        "every recorded operation ran, and nothing else did"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_step_that_waits_holds_no_registry_transaction() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("download").hold(&hold).boxed(),
        ))
        .await;
    hold.started().await;

    for _ in 0..50 {
        let started = Instant::now();
        harness
            .registry
            .commit(WriteBatch::new(Vec::new()))
            .unwrap();
        harness.registry.operation(id).unwrap();
        assert!(started.elapsed() < Duration::from_millis(50));
    }
    hold.release();
    wait_state(&harness.engine, id, OperationState::Succeeded).await;
}

/// A work that reports progress 100 times at once, then waits.
struct Chatty;

impl OperationWork for Chatty {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "download".parse().expect("download step name is valid"),
            ))
            .await?;
            for done in 1..=100 {
                ctx.set_progress(Progress::Counted { done, total: 100 });
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: stackly_core::operation::OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_burst_of_progress_commits_at_most_twice_and_keeps_the_last_value() {
    let harness = Harness::new().await;

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(Chatty),
        ))
        .await;
    wait_state(&harness.engine, id, OperationState::Succeeded).await;

    let views = harness.events.views(id);
    let progress: Vec<Progress> = views
        .iter()
        .zip(harness.events.of(id))
        .filter(|(_, event)| event.kind == "progress")
        .map(|(view, _)| view.operation.progress())
        .collect();
    assert!(
        (1..=2).contains(&progress.len()),
        "100 reports in one window commit at most twice: {progress:?}"
    );
    assert_eq!(
        progress.last(),
        Some(&Progress::Counted {
            done: 100,
            total: 100
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resource_that_changed_while_queued_fails_the_operation_before_any_effect() {
    let ev = evidence!(
        covers = ["VAL-015"],
        kind = Kind::Fixture,
        expected = "a begin with a stale expected revision is refused with RevisionMismatch, and an operation whose resource changed while it waited in the queue fails from queued with precondition_failed and resource.revision_mismatch before its work runs",
    );
    let harness = Harness::new().await;
    let (env, builder) = environment(&harness);
    let effects = EffectCounter::new();
    let blocker = HoldHandle::new();
    let first = harness
        .begin(on_environment(
            env,
            ScriptedWork::new().step("drain").hold(&blocker),
        ))
        .await;
    blocker.started().await;
    let mut stale = on_environment(env, ScriptedWork::new().step("drain").effects(&effects));
    stale.expected_revision = Some(revision(1));
    let queued = harness.begin(stale).await;

    harness
        .registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateEnvironment {
            environment: builder.clone().revision(2).build(),
            expected: revision(1),
        }]))
        .unwrap();
    blocker.release();
    wait_state(&harness.engine, first, OperationState::Succeeded).await;
    let failed = wait_state(&harness.engine, queued, OperationState::Failed).await;

    assert_eq!(
        failed
            .view
            .operation
            .failure()
            .map(|failure| failure.code().as_str()),
        Some("precondition_failed")
    );
    let error = failed.view.journal.failure_error().unwrap();
    assert_eq!(error.code().as_str(), "resource.revision_mismatch");
    let details: Vec<(&str, &str)> = error
        .details()
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    assert_eq!(details, [("expected", "1"), ("actual", "2")]);
    assert_eq!(failed.view.operation.started_at(), None);
    assert_eq!(effects.get(), 0, "the work never ran");

    let mut late = on_environment(env, ScriptedWork::new().step("drain"));
    late.expected_revision = Some(revision(1));
    assert_eq!(
        harness.engine.begin(late).await,
        Err(BeginError::RevisionMismatch {
            expected: revision(1),
            actual: revision(2)
        })
    );
    let mut invented = on_environment(env, ScriptedWork::new().step("drain"));
    invented.creates_resource = true;
    invented.expected_revision = Some(revision(1));
    assert!(matches!(
        harness.engine.begin(invented).await,
        Err(BeginError::InvalidRequest { .. })
    ));
    let missing = on_environment(EnvironmentId::generate(), ScriptedWork::new().step("drain"));
    assert_eq!(
        harness.engine.begin(missing).await,
        Err(BeginError::ResourceNotFound)
    );
    ev.pass();
}

/// A work whose finalize updates an environment, losing a generation race
/// on its first call when `race` is set.
struct Finalizing {
    env: EnvironmentBuilder,
    expected: ResourceRevision,
    race: bool,
    calls: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl OperationWork for Finalizing {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "apply".parse().expect("apply step name is valid"),
            ))
            .await?;
            ctx.finish_step(None).await?;
            let Self {
                env,
                expected,
                race,
                calls,
            } = *self;
            let finalize: stackly_core::operation::Finalize = Box::new(move |registry| {
                let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let generation = registry
                    .known_instance_ids()
                    .expect("registry instance IDs can be read")
                    .generation;
                if race && call == 0 {
                    registry
                        .commit(WriteBatch::new(Vec::new()))
                        .expect("empty registry batch is valid");
                }
                Ok(stackly_core::operation::FinalBatch {
                    expected_generation: Some(generation),
                    ops: vec![WriteOp::UpdateEnvironment {
                        environment: env.clone().revision(expected.get() + 1).build(),
                        expected,
                    }],
                })
            });
            Ok(WorkOutcome {
                result: stackly_core::operation::OperationOutcome::default(),
                finalize: Some(finalize),
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finalize_runs_again_after_a_generation_conflict_and_a_stale_revision_fails_the_step() {
    let harness = Harness::new().await;
    let (env, builder) = environment(&harness);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));

    let raced = harness
        .begin(request(
            OperationKind::EnvironmentUp,
            OperationResource::Environment(env),
            Box::new(Finalizing {
                env: builder.clone(),
                expected: revision(1),
                race: true,
                calls: std::sync::Arc::clone(&calls),
            }),
        ))
        .await;
    wait_state(&harness.engine, raced, OperationState::Succeeded).await;

    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        harness
            .registry
            .environment(env)
            .unwrap()
            .value
            .map(|stored| stored.revision()),
        Some(revision(2))
    );

    let stale = harness
        .begin(request(
            OperationKind::EnvironmentUp,
            OperationResource::Environment(env),
            Box::new(Finalizing {
                env: builder,
                expected: revision(1),
                race: false,
                calls: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            }),
        ))
        .await;
    let failed = wait_state(&harness.engine, stale, OperationState::Failed).await;
    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("resource.revision_mismatch")
    );
}

#[tokio::test]
async fn a_background_operation_records_no_key_method_or_fingerprint_and_a_request_records_all_three()
 {
    // Spec 0007 AC-3: an operation the daemon starts itself carries no
    // idempotency key; one a client starts carries its key, its method, and
    // its fingerprint.
    let harness = Harness::new().await;
    let background = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().boxed(),
        ))
        .await;
    let mut from_client =
        manager_request(OperationKind::UpdateManager, ScriptedWork::new().boxed());
    from_client.origin = user("cli-run-7", 0x5a);
    let requested = harness.begin(from_client).await;

    let background = harness.engine.get(background).await.unwrap().unwrap().view;
    let requested = harness.engine.get(requested).await.unwrap().unwrap().view;

    assert_eq!(background.operation.idempotency_key(), None);
    assert_eq!(background.journal.method(), None);
    assert_eq!(background.journal.fingerprint(), None);
    assert_eq!(
        background.journal.trigger().origin(),
        stackly_domain::TriggerOrigin::Background
    );
    assert_eq!(
        requested
            .operation
            .idempotency_key()
            .map(|key| key.as_str().to_owned()),
        Some("cli-run-7".to_owned())
    );
    assert_eq!(
        requested
            .journal
            .method()
            .map(|method| method.as_str().to_owned()),
        Some("manager.update".to_owned())
    );
    assert!(requested.journal.fingerprint().is_some());
    assert_eq!(
        requested.journal.trigger().origin(),
        stackly_domain::TriggerOrigin::User
    );
}
