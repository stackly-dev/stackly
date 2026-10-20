//! Kinds that close as interrupted, `start_instance` and the other
//! instance process kinds (spec 0007 AC-10, AC-14, "Tabel resume per jenis
//! operasi"), and the feature precheck of the begin gate ("Idempotency",
//! check 3a).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use stackly_core::operation::testing::{EffectCounter, HoldHandle, ScriptedFactory, ScriptedWork};
use stackly_core::operation::{
    BeginError, BeginOutcome, BoxFuture, CancelDisposition, FinalBatch, NO_OP_STAGE, NoOpOutcome,
    NoOpReply, OperationContext, OperationOutcome, OperationWork, PrecheckOutcome, StepSpec,
    WorkError, WorkOutcome, WorkResult,
};
use stackly_domain::{
    CreatedState, InstanceId, LockKey, OperationKind, OperationResource, OperationState, Registry,
    StepState, WriteOp,
};
use stackly_testkit::EnvironmentBuilder;

use crate::common::{
    Harness, TestError, counted, fast, instance, request, user, wait_state, wait_until,
};

/// A request of `kind` on the instance `id`.
fn on_instance(
    kind: OperationKind,
    id: InstanceId,
    work: Box<dyn OperationWork>,
) -> stackly_core::operation::BeginRequest {
    request(kind, OperationResource::Instance(id), work)
}

/// A start that spawns its process in an irreversible step, then waits
/// for `hold` without looking at cancels or shutdown, as a work busy
/// stopping or watching its process would. It records what the context
/// answered after the wait in `after`.
struct BusyStart {
    hold: HoldHandle,
    after: Arc<Mutex<Option<String>>>,
}

impl OperationWork for BusyStart {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(
                StepSpec::new("spawn".parse().expect("spawn is a stage name")).irreversible(),
            )
            .await?;
            self.hold.mark_started();
            self.hold.released().await;
            let answer = ctx.check_cancel();
            *self.after.lock().expect("the answer slot is not poisoned") =
                Some(format!("{answer:?}"));
            answer?;
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_start_interrupted_after_spawn_closes_interrupted_without_compensation_and_frees_its_waiter()
 {
    let harness = Harness::new().await;
    let target = instance(&harness);
    let spawned = HoldHandle::new();
    let start = harness
        .begin(on_instance(
            OperationKind::StartInstance,
            target,
            ScriptedWork::new()
                .step("prepare")
                .creates_instance_tree()
                .step("spawn")
                .irreversible()
                .hold(&spawned)
                .boxed(),
        ))
        .await;
    spawned.started().await;
    let stop = harness
        .begin(on_instance(
            OperationKind::StopInstance,
            target,
            ScriptedWork::new().step("stop").boxed(),
        ))
        .await;
    wait_until(&harness.engine, stop, |detail| {
        detail.view.journal.waiting_on() == Some(start)
    })
    .await;

    let (engine, events) = harness.restarted(fast());
    let effects = EffectCounter::new();
    engine.register_factory(
        OperationKind::StopInstance,
        Arc::new(ScriptedFactory::always(
            ScriptedWork::new().step("stop").effects(&effects),
        )),
    );
    let report = engine.recover().await.unwrap();

    assert_eq!(
        (
            report.closed_interrupted,
            report.queued,
            report.auto_compensating,
            report.needs_recovery
        ),
        (1, 1, 0, 0)
    );
    assert_eq!(engine.stats().interrupted_at_start, 1);
    let closed = engine.get(start).await.unwrap().unwrap();
    assert_eq!(closed.view.operation.state(), OperationState::Failed);
    assert_eq!(
        closed
            .view
            .operation
            .failure()
            .map(|f| (f.code().as_str(), f.retryable())),
        Some(("interrupted", true))
    );
    assert_eq!(
        closed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("operation.interrupted")
    );
    let steps: Vec<(&str, StepState)> = closed
        .steps
        .iter()
        .map(|step| (step.name().as_str(), step.state()))
        .collect();
    assert_eq!(
        steps,
        [
            ("prepare", StepState::Done),
            ("spawn", StepState::Interrupted)
        ]
    );
    assert_eq!(closed.view.journal.compensation_started_at(), None);
    assert_eq!(
        closed.created[0].state(),
        CreatedState::Created,
        "nothing is compensated"
    );
    assert_eq!(
        std::fs::read_dir(harness.layout.layout().instances_dir())
            .unwrap()
            .count(),
        1,
        "the folder the start made stays"
    );
    assert_eq!(
        engine.locks().holder(&LockKey::Instance(target)),
        None,
        "its key never enters the lock table"
    );
    let waiter = engine.get(stop).await.unwrap().unwrap();
    assert_eq!(waiter.view.operation.state(), OperationState::Queued);
    assert_eq!(waiter.view.journal.waiting_on(), None);
    assert!(
        events.of(stop).iter().any(|event| event.kind == "progress"),
        "the cleared wait is reported"
    );

    engine.start();
    wait_state(&engine, stop, OperationState::Succeeded).await;
    assert_eq!(effects.get(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_start_interrupted_while_it_winds_down_a_cancel_closes_cancelled() {
    let harness = Harness::new().await;
    let target = instance(&harness);
    let hold = HoldHandle::new();
    let start = harness
        .begin(on_instance(
            OperationKind::StartInstance,
            target,
            Box::new(BusyStart {
                hold: hold.clone(),
                after: Arc::default(),
            }),
        ))
        .await;
    hold.started().await;
    let cancel = harness.engine.cancel(start).await.unwrap();
    assert_eq!(
        cancel.disposition,
        CancelDisposition::Requested,
        "the cancel reaches the work in any step"
    );
    assert_eq!(
        cancel.view.operation.state(),
        OperationState::CancelRequested
    );

    let (engine, _) = harness.restarted(fast());
    let report = engine.recover().await.unwrap();

    assert_eq!(report.closed_interrupted, 1);
    let closed = engine.get(start).await.unwrap().unwrap();
    assert_eq!(closed.view.operation.state(), OperationState::Cancelled);
    assert_eq!(closed.view.operation.failure(), None);
    assert_eq!(closed.view.journal.compensation_started_at(), None);
    assert_eq!(engine.locks().holder(&LockKey::Instance(target)), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_leaves_a_running_start_alone_and_the_next_start_closes_it() {
    let harness = Harness::new().await;
    let target = instance(&harness);
    let hold = HoldHandle::new();
    let after = Arc::new(Mutex::new(None));
    let start = harness
        .begin(on_instance(
            OperationKind::StartInstance,
            target,
            Box::new(BusyStart {
                hold: hold.clone(),
                after: Arc::clone(&after),
            }),
        ))
        .await;
    hold.started().await;
    let before = harness.registry.operation(start).unwrap().value.unwrap();

    let started = Instant::now();
    let report = harness.engine.shutdown(Duration::from_secs(5)).await;

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "shutdown does not wait for it"
    );
    assert_eq!(
        (
            report.abandoned,
            report.parked,
            report.stopped,
            report.left_running
        ),
        (1, 0, 0, 0)
    );
    hold.release();
    let deadline = Instant::now() + Duration::from_secs(5);
    while after.lock().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "the work went on after the release"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(after.lock().unwrap().as_deref(), Some("Err(Abandoned)"));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let left = harness.registry.operation(start).unwrap().value.unwrap();
    assert_eq!(left.state(), OperationState::Running);
    assert_eq!(
        left.revision(),
        before.revision(),
        "nothing more is written for it"
    );

    let (engine, _) = harness.restarted(fast());
    let recovered = engine.recover().await.unwrap();
    assert_eq!(recovered.closed_interrupted, 1);
    let closed = engine.get(start).await.unwrap().unwrap();
    assert_eq!(
        closed
            .view
            .operation
            .failure()
            .map(|f| (f.code().as_str(), f.retryable())),
        Some(("interrupted", true))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn past_its_spawn_a_start_fails_or_cancels_without_compensation_and_never_needs_recovery() {
    let harness = Harness::new().await;
    let failing_target = instance(&harness);
    let failing = harness
        .begin(on_instance(
            OperationKind::StartInstance,
            failing_target,
            ScriptedWork::new()
                .step("prepare")
                .creates_instance_tree()
                .step("spawn")
                .irreversible()
                .step("ready")
                .fail("supervisor.ready_timeout", true)
                .boxed(),
        ))
        .await;

    let failed = wait_state(&harness.engine, failing, OperationState::Failed).await;

    assert_eq!(
        failed
            .view
            .operation
            .failure()
            .map(|f| (f.code().as_str(), f.retryable())),
        Some(("step_failed", true))
    );
    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("supervisor.ready_timeout")
    );
    assert_eq!(failed.view.journal.recovery_cause(), None);
    assert_eq!(failed.created[0].state(), CreatedState::Created);
    assert_eq!(
        harness
            .engine
            .locks()
            .holder(&LockKey::Instance(failing_target)),
        None
    );

    let cancelled_target = instance(&harness);
    let spawned = HoldHandle::new();
    let cancelled = harness
        .begin(on_instance(
            OperationKind::StartInstance,
            cancelled_target,
            ScriptedWork::new()
                .step("prepare")
                .creates_instance_tree()
                .step("spawn")
                .irreversible()
                .hold(&spawned)
                .boxed(),
        ))
        .await;
    spawned.started().await;
    let cancel = harness.engine.cancel(cancelled).await.unwrap();
    assert_eq!(cancel.disposition, CancelDisposition::Requested);
    let done = wait_state(&harness.engine, cancelled, OperationState::Cancelled).await;
    assert!(done.view.journal.point_of_no_return_at().is_some());
    assert_eq!(done.view.journal.compensation_started_at(), None);
    assert_eq!(done.created[0].state(), CreatedState::Created);
    assert_eq!(
        std::fs::read_dir(harness.layout.layout().instances_dir())
            .unwrap()
            .count(),
        2,
        "neither start's folder was compensated"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_no_op_precheck_that_writes_binds_its_key_and_a_replay_gets_the_same_answer() {
    let harness = Harness::new().await;
    let target = instance(&harness);
    let environment = EnvironmentBuilder::new().no_members();
    let written = environment.id();
    let calls = Arc::new(AtomicU32::new(0));
    let mut first = on_instance(
        OperationKind::StopInstance,
        target,
        ScriptedWork::new().step("stop").boxed(),
    );
    first.origin = user("stop-1", 0x11);
    let batch = FinalBatch {
        expected_generation: None,
        ops: vec![WriteOp::InsertEnvironment(environment.build())],
    };
    first.precheck = Some(counted(&calls, move || {
        PrecheckOutcome::NoOp(NoOpReply::writing(batch))
    }));

    let answer = harness.engine.begin(first).await.unwrap();

    let BeginOutcome::NoOp(NoOpOutcome {
        recorded: Some(view),
        replayed: false,
    }) = &answer
    else {
        panic!("expected a recorded no operation answer, got {answer:?}");
    };
    let id = view.operation.id();
    assert_eq!(view.operation.state(), OperationState::Succeeded);
    assert_eq!(view.operation.stage(), Some(&NO_OP_STAGE));
    assert!(
        harness
            .registry
            .environment(written)
            .unwrap()
            .value
            .is_some()
    );
    let detail = harness.engine.get(id).await.unwrap().unwrap();
    assert!(detail.steps.is_empty() && detail.created.is_empty());
    let shown: Vec<_> = harness
        .events
        .of(id)
        .into_iter()
        .map(|event| (event.kind, event.state))
        .collect();
    assert_eq!(shown, [("state_changed", OperationState::Succeeded)]);

    let mut again = on_instance(
        OperationKind::StopInstance,
        target,
        ScriptedWork::new().step("stop").boxed(),
    );
    again.origin = user("stop-1", 0x11);
    again.precheck = Some(counted(&calls, || PrecheckOutcome::Proceed));
    let replay = harness.engine.begin(again).await.unwrap();

    assert_eq!(
        replay,
        BeginOutcome::NoOp(NoOpOutcome {
            recorded: Some(view.clone()),
            replayed: true,
        })
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a replay never runs the precheck"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_no_op_that_writes_nothing_binds_no_key_and_a_rejection_writes_nothing() {
    let harness = Harness::new().await;
    let target = instance(&harness);
    let calls = Arc::new(AtomicU32::new(0));
    let mut idle = on_instance(
        OperationKind::StartInstance,
        target,
        ScriptedWork::new().step("spawn").boxed(),
    );
    idle.origin = user("start-1", 0x21);
    idle.precheck = Some(counted(&calls, || {
        PrecheckOutcome::NoOp(NoOpReply::nothing())
    }));

    assert_eq!(
        harness.engine.begin(idle).await.unwrap(),
        BeginOutcome::NoOp(NoOpOutcome {
            recorded: None,
            replayed: false,
        })
    );
    let key = "start-1".parse().unwrap();
    assert!(
        harness
            .registry
            .operation_by_idempotency_key(&key)
            .unwrap()
            .value
            .is_none()
    );

    let mut proceed = on_instance(
        OperationKind::StartInstance,
        target,
        ScriptedWork::new().step("spawn").boxed(),
    );
    proceed.origin = user("start-1", 0x21);
    proceed.precheck = Some(counted(&calls, || PrecheckOutcome::Proceed));
    harness.tick();
    let BeginOutcome::Created(view) = harness.engine.begin(proceed).await.unwrap() else {
        panic!("the unbound key starts a new operation");
    };
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    wait_state(
        &harness.engine,
        view.operation.id(),
        OperationState::Succeeded,
    )
    .await;

    let mut missing = on_instance(
        OperationKind::StartInstance,
        InstanceId::generate(),
        ScriptedWork::new().step("spawn").boxed(),
    );
    missing.origin = user("start-2", 0x22);
    missing.precheck = Some(Box::new(|| {
        PrecheckOutcome::Reject(TestError::boxed("instance.not_initialized", false))
    }));
    let refused = harness.engine.begin(missing).await;

    let Err(BeginError::Rejected(rejection)) = refused else {
        panic!("the precheck refuses before the resource check, got {refused:?}");
    };
    assert_eq!(rejection.code().as_str(), "instance.not_initialized");
    let key = "start-2".parse().unwrap();
    assert!(
        harness
            .registry
            .operation_by_idempotency_key(&key)
            .unwrap()
            .value
            .is_none(),
        "a rejection writes nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_work_may_not_take_the_no_op_stage() {
    let harness = Harness::new().await;
    struct Sneaky(Arc<Mutex<Option<String>>>);
    impl OperationWork for Sneaky {
        fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
            Box::pin(async move {
                let answer = ctx.set_stage(NO_OP_STAGE).await;
                *self.0.lock().expect("the answer slot is not poisoned") =
                    Some(format!("{answer:?}"));
                answer?;
                Err(WorkError::Cancelled)
            })
        }
    }
    let answer = Arc::new(Mutex::new(None));
    let id = harness
        .begin(crate::common::manager_request(
            OperationKind::UpdateManager,
            Box::new(Sneaky(Arc::clone(&answer))),
        ))
        .await;

    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    assert!(
        answer
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|text| text.contains("InvalidUse")),
        "got {answer:?}"
    );
    assert_ne!(failed.view.operation.stage(), Some(&NO_OP_STAGE));
}
